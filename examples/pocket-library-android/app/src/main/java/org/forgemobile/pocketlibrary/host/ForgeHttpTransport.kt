// The Android counterpart of ForgeServices' URLSession transport
// (crates/forge-services/swift/optional/ForgeServices+Http.swift): Rust's ordinary HTTP requests
// go through the platform's own stack, `HttpURLConnection`, so they get the system's proxy
// settings, trust store and network security config instead of Rust's bundled TLS.
//
// Plain Kotlin and java.net, no Android UI, so the JVM unit tests run it against a local
// server. It lives in this app until a second Android app needs it, as `ForgeHost` does.
package org.forgemobile.pocketlibrary.host

import java.io.File
import java.io.IOException
import java.io.InputStream
import java.net.ConnectException
import java.net.HttpURLConnection
import java.net.MalformedURLException
import java.net.NoRouteToHostException
import java.net.ProtocolException
import java.net.SocketTimeoutException
import java.net.URL
import java.net.UnknownHostException
import java.security.cert.CertificateException
import javax.net.ssl.SSLHandshakeException
import javax.net.ssl.SSLPeerUnverifiedException
import uniffi.pocket_library_core.HttpHeader
import uniffi.pocket_library_core.HttpRequest
import uniffi.pocket_library_core.HttpResponse
import uniffi.pocket_library_core.HttpStreamBody
import uniffi.pocket_library_core.HttpStreamHead
import uniffi.pocket_library_core.HttpTransport
import uniffi.pocket_library_core.HttpTransportException
import uniffi.pocket_library_core.installNativeHttp

/**
 * `HttpTransport` backed by `HttpURLConnection`. Rust calls [send] and [openStream] on its own
 * worker threads and they block there; [cancel] comes from another thread and disconnects the
 * request, which then fails with `Cancelled`.
 *
 * Not the same as iOS yet: `PATCH` is refused where the platform's `HttpURLConnection` doesn't
 * take it (the JVM's; Android's does), a request with a body doesn't follow redirects (its 3xx is
 * the response), and HTTP/3 isn't used.
 */
class ForgeHttpTransport : HttpTransport {
    private val lock = Any()

    /** Requests in flight, by Rust's request id. */
    private val connections = HashMap<ULong, HttpURLConnection>()

    /** Ids cancelled: in flight (so their failure reads as `Cancelled`), or before they began. */
    private val cancelled = HashSet<ULong>()

    override fun send(request: HttpRequest): HttpResponse {
        val connection = start(request)
        try {
            val status = connection.responseCode
            if (connection.contentLengthLong > 0 && connection.contentLengthLong.toULong() > request.maxBodyBytes) {
                throw tooLarge(request.maxBodyBytes)
            }
            val body = body(connection, status)?.use { read(it, request.maxBodyBytes) } ?: ByteArray(0)
            return HttpResponse(status.toUShort(), headers(connection), body)
        } catch (e: IOException) {
            throw failure(request.id, e)
        } finally {
            connection.disconnect()
            finished(request.id)
        }
    }

    override fun openStream(request: HttpRequest, body: HttpStreamBody): HttpStreamHead {
        val connection = start(request)
        val head = try {
            val status = connection.responseCode
            HttpStreamHead(status.toUShort(), headers(connection))
        } catch (e: IOException) {
            throw closed(connection, request.id, e)
        }
        // The headers came within the request's timeout; a stream may then be quiet for long
        // stretches (Server-Sent Events), so its body has no idle timeout.
        connection.readTimeout = 0
        Thread({
            try {
                body(connection, head.status.toInt())?.use { input ->
                    val buffer = ByteArray(16 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        if (count > 0) body.receive(buffer.copyOf(count))
                    }
                }
                body.finish()
            } catch (e: IOException) {
                body.fail(failure(request.id, e))
            } finally {
                connection.disconnect()
                finished(request.id)
            }
        }, "forge-http-stream-${request.id}").apply { isDaemon = true }.start()
        return head
    }

    override fun cancel(id: ULong) {
        val connection = synchronized(lock) {
            cancelled.add(id)
            connections[id]
        }
        // From another thread: the blocked read or connect fails at once.
        connection?.disconnect()
    }

    /** Opens [request]'s connection and sends its body, registered for [cancel]. */
    private fun start(request: HttpRequest): HttpURLConnection {
        val url = try {
            URL(request.url)
        } catch (e: MalformedURLException) {
            throw HttpTransportException.InvalidRequest("not a URL")
        }
        if (url.protocol != "https" && url.protocol != "http") {
            throw HttpTransportException.InvalidRequest("not an http(s) URL")
        }
        val connection = url.openConnection() as HttpURLConnection
        try {
            connection.requestMethod = request.method
        } catch (e: ProtocolException) {
            throw HttpTransportException.InvalidRequest("this platform can't send ${request.method}")
        }
        val timeout = request.timeoutMs.coerceIn(1uL, Int.MAX_VALUE.toULong()).toInt()
        connection.connectTimeout = timeout
        connection.readTimeout = timeout
        val file = request.bodyFile?.let(::File)
        // A request with a body streams it (a file can be large), and HttpURLConnection can't
        // resend a streamed body, so it doesn't follow that request's redirects: it returns the
        // 3xx as the response. Requests without a body follow them as Rust asks.
        val hasBody = file != null || request.body.isNotEmpty()
        connection.instanceFollowRedirects = request.followRedirects && !hasBody
        connection.useCaches = false
        for (header in request.headers) connection.addRequestProperty(header.name, header.value)

        synchronized(lock) {
            if (cancelled.remove(request.id)) throw HttpTransportException.Cancelled()
            connections[request.id] = connection
            if (!announced) {
                announced = true
                // Once per launch, so the log shows which transport ran.
                System.err.println("[forge] http: requests go through HttpURLConnection")
            }
        }
        when {
            file != null -> {
                connection.doOutput = true
                connection.setFixedLengthStreamingMode(file.length())
            }
            request.body.isNotEmpty() -> {
                connection.doOutput = true
                connection.setFixedLengthStreamingMode(request.body.size)
            }
        }
        onRegistered?.invoke(request.id)
        try {
            // Connect now, then look again: a cancel that came before the connection existed
            // could not disconnect it (it is a no-op until then), so it is caught here, and one
            // that comes later disconnects it.
            connection.connect()
            if (synchronized(lock) { request.id in cancelled }) {
                connection.disconnect()
                finished(request.id)
                throw HttpTransportException.Cancelled()
            }
            when {
                file != null -> connection.outputStream.use { out -> file.inputStream().use { it.copyTo(out) } }
                request.body.isNotEmpty() -> connection.outputStream.use { it.write(request.body) }
            }
        } catch (e: IOException) {
            throw closed(connection, request.id, e)
        }
        return connection
    }

    private var announced = false

    /** Tests only: runs once a request is registered for [cancel], before it connects. */
    internal var onRegistered: ((ULong) -> Unit)? = null

    /**
     * Closes a request that failed and says why. The failure is read first: [finished] forgets
     * that the request was cancelled.
     */
    private fun closed(connection: HttpURLConnection, id: ULong, e: IOException): HttpTransportException {
        val error = failure(id, e)
        connection.disconnect()
        finished(id)
        return error
    }

    private fun finished(id: ULong) {
        synchronized(lock) {
            connections.remove(id)
            cancelled.remove(id)
        }
    }

    /** What went wrong, in Rust's terms: a cancelled request is `Cancelled` however it failed. */
    private fun failure(id: ULong, e: IOException): HttpTransportException {
        if (synchronized(lock) { id in cancelled }) return HttpTransportException.Cancelled()
        return classify(e)
    }

    companion object {
        /** Installs this transport for Rust's `HttpService` (iOS: `ForgeServices.install()`). */
        fun install() = installNativeHttp(ForgeHttpTransport())

        internal fun classify(e: IOException): HttpTransportException = when {
            e is SocketTimeoutException -> HttpTransportException.Timeout()
            e is UnknownHostException || e is ConnectException || e is NoRouteToHostException ->
                HttpTransportException.Offline(e.message ?: e.toString())
            e is SSLPeerUnverifiedException || (e is SSLHandshakeException && e.hasCause<CertificateException>()) ->
                HttpTransportException.Certificate(e.message ?: e.toString())
            else -> HttpTransportException.Transport(e.message ?: e.toString())
        }

        private inline fun <reified T : Throwable> Throwable.hasCause(): Boolean {
            var cause: Throwable? = this
            while (cause != null) {
                if (cause is T) return true
                cause = cause.cause
            }
            return false
        }

        /** The body, which `HttpURLConnection` keeps apart for an error status. */
        private fun body(connection: HttpURLConnection, status: Int): InputStream? =
            if (status >= 400) connection.errorStream else connection.inputStream

        private fun read(input: InputStream, limit: ULong): ByteArray {
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            var total = 0uL
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                total += count.toULong()
                if (total > limit) throw tooLarge(limit)
                out.write(buffer, 0, count)
            }
            return out.toByteArray()
        }

        private fun tooLarge(limit: ULong) =
            HttpTransportException.Transport("the response body is larger than $limit bytes")

        /** Every header, one entry per value; the status line (a null name) is left out. */
        private fun headers(connection: HttpURLConnection): List<HttpHeader> =
            connection.headerFields.flatMap { (name, values) ->
                if (name == null) emptyList() else values.map { HttpHeader(name, it) }
            }
    }
}
