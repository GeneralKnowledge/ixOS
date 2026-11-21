package org.forgemobile.pocketlibrary.host

import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import kotlin.concurrent.thread
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.pocket_library_core.HttpHeader
import uniffi.pocket_library_core.HttpRequest
import uniffi.pocket_library_core.HttpStreamBody
import uniffi.pocket_library_core.NoHandle
import uniffi.pocket_library_core.HttpTransportException

/**
 * The Android HTTP transport against a small local server (Android's unit-test classpath has no
 * JDK HTTP server): no network needed.
 */
class ForgeHttpTransportTest {
    private val transport = ForgeHttpTransport()
    private val release = CountDownLatch(1)
    private val arrived = CountDownLatch(1)
    private val socket = ServerSocket(0, 50, InetAddress.getLoopbackAddress())
    private val port get() = socket.localPort

    /** The paths the server was asked for, in order. */
    private val paths = java.util.concurrent.CopyOnWriteArrayList<String>()

    /** What the server answers for a path: status, body, extra headers. */
    private fun answer(method: String, path: String, body: ByteArray): Triple<Int, ByteArray, List<Pair<String, String>>> =
        when (path) {
            "/hello" -> Triple(200, "hello $method".toByteArray(), listOf("X-Test" to "yes"))
            "/echo" -> Triple(201, body, emptyList())
            "/missing" -> Triple(404, "not here".toByteArray(), emptyList())
            "/big" -> Triple(200, ByteArray(10_000), emptyList())
            "/moved" -> Triple(302, ByteArray(0), listOf("Location" to "/hello"))
            "/slow" -> {
                arrived.countDown()
                release.await(10, TimeUnit.SECONDS)
                Triple(200, "late".toByteArray(), emptyList())
            }
            else -> Triple(404, ByteArray(0), emptyList())
        }

    @Before
    fun start() {
        thread(isDaemon = true) {
            while (!socket.isClosed) {
                val client = try { socket.accept() } catch (e: IOException) { break }
                thread(isDaemon = true) { client.use(::serve) }
            }
        }
    }

    /** One request per connection, HTTP/1.1 with `Connection: close`. */
    private fun serve(client: Socket) {
        try {
            val input = client.getInputStream().buffered()
            fun line(): String {
                val out = StringBuilder()
                while (true) {
                    val c = input.read()
                    if (c < 0 || c == '\n'.code) break
                    if (c != '\r'.code) out.append(c.toChar())
                }
                return out.toString()
            }
            val (method, path) = line().split(" ").let { it[0] to it[1] }
            var length = 0
            while (true) {
                val header = line()
                if (header.isEmpty()) break
                val (name, value) = header.split(":", limit = 2).map { it.trim() }
                if (name.equals("Content-Length", ignoreCase = true)) length = value.toInt()
            }
            val body = ByteArray(length).also { var read = 0; while (read < length) read += input.read(it, read, length - read) }
            paths.add(path)
            val (status, reply, headers) = answer(method, path, body)
            val head = StringBuilder("HTTP/1.1 $status X\r\nContent-Length: ${reply.size}\r\nConnection: close\r\n")
            for ((name, value) in headers) head.append("$name: $value\r\n")
            head.append("\r\n")
            client.getOutputStream().apply {
                write(head.toString().toByteArray())
                write(reply)
                flush()
            }
        } catch (e: IOException) {
            // The client went away (a cancelled or timed-out request).
        }
    }

    @After
    fun stop() {
        release.countDown()
        socket.close()
    }

    private var nextId = 1uL

    private fun request(
        path: String,
        method: String = "GET",
        body: ByteArray = ByteArray(0),
        timeoutMs: ULong = 5_000u,
        maxBodyBytes: ULong = 1_000_000u,
        followRedirects: Boolean = true,
        url: String = "http://127.0.0.1:$port$path",
    ) = HttpRequest(
        id = nextId++,
        method = method,
        url = url,
        headers = listOf(HttpHeader("User-Agent", "ForgeTest")),
        body = body,
        bodyFile = null,
        timeoutMs = timeoutMs,
        maxBodyBytes = maxBodyBytes,
        followRedirects = followRedirects,
    )

    private inline fun <reified E : HttpTransportException> assertFails(block: () -> Unit): E {
        val error = runCatching(block).exceptionOrNull()
        if (error !is E) throw AssertionError("expected ${E::class.simpleName}, got $error", error)
        return error as E
    }

    @Test
    fun sendsAndReturnsStatusHeadersAndBody() {
        val response = transport.send(request("/hello"))
        assertEquals(200.toUShort(), response.status)
        assertEquals("hello GET", response.body.decodeToString())
        assertTrue(response.headers.any { it.name.equals("X-Test", ignoreCase = true) && it.value == "yes" })

        val posted = transport.send(request("/echo", method = "POST", body = "a body".toByteArray()))
        assertEquals(201.toUShort(), posted.status)
        assertArrayEquals("a body".toByteArray(), posted.body)
    }

    @Test
    fun anErrorStatusIsAResponse() {
        val response = transport.send(request("/missing"))
        assertEquals(404.toUShort(), response.status)
        assertEquals("not here", response.body.decodeToString())
    }

    @Test
    fun aBodyPastTheLimitFails() {
        val error = assertFails<HttpTransportException.Transport> { transport.send(request("/big", maxBodyBytes = 100u)) }
        assertTrue(error.reason.contains("larger than 100 bytes"))
    }

    @Test
    fun redirectsAreFollowedUnlessTheRequestSaysNot() {
        assertEquals("hello GET", transport.send(request("/moved")).body.decodeToString())
        val answer = transport.send(request("/moved", followRedirects = false))
        assertEquals(302.toUShort(), answer.status)
        assertEquals("/hello", answer.headers.first { it.name.equals("Location", ignoreCase = true) }.value)
    }

    @Test
    fun aRequestWithABodyReturnsItsRedirect() {
        // Its streamed body can't be sent again, so the 3xx is the answer.
        val answer = transport.send(request("/moved", method = "POST", body = "a body".toByteArray()))
        assertEquals(302.toUShort(), answer.status)
        assertEquals(listOf("/moved"), paths.toList())
    }

    @Test
    fun failuresAreTyped() {
        // A port nothing listens on: refused, so offline.
        val closed = ServerSocket(0).use { it.localPort }
        assertFails<HttpTransportException.Offline> { transport.send(request("/", url = "http://127.0.0.1:$closed/")) }
        assertFails<HttpTransportException.InvalidRequest> { transport.send(request("/", url = "ftp://example.com/")) }
        assertFails<HttpTransportException.Timeout> { transport.send(request("/slow", timeoutMs = 300u)) }
    }

    @Test
    fun cancellingFromAnotherThreadStopsTheRequest() {
        val slow = request("/slow")
        Thread {
            arrived.await(5, TimeUnit.SECONDS)
            transport.cancel(slow.id)
        }.start()
        assertFails<HttpTransportException.Cancelled> { transport.send(slow) }

        // Cancelled after it was registered but before it connected: never sent.
        val between = request("/hello")
        transport.onRegistered = { if (it == between.id) transport.cancel(it) }
        assertFails<HttpTransportException.Cancelled> { transport.send(between) }
        transport.onRegistered = null
        assertEquals(listOf("/slow"), paths.toList())

        // Cancelled before it began: never sent.
        val early = request("/hello")
        transport.cancel(early.id)
        assertFails<HttpTransportException.Cancelled> { transport.send(early) }
    }

    @Test
    fun cancellingAStreamBeforeItsHeadersReadsAsCancelled() {
        // The cancel closes the connection while it waits for the status line; that failure
        // must still read as Cancelled, not as a broken connection.
        val stream = request("/slow")
        Thread {
            arrived.await(5, TimeUnit.SECONDS)
            transport.cancel(stream.id)
        }.start()
        // Never used: the stream fails before it has a body.
        val body = HttpStreamBody(NoHandle)
        assertFails<HttpTransportException.Cancelled> { transport.openStream(stream, body) }
    }
}
