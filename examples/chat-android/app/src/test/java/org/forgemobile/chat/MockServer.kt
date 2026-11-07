package org.forgemobile.chat

import java.io.File
import java.net.HttpURLConnection
import java.net.ServerSocket
import java.net.URI
import java.nio.file.Files
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.delay
import org.junit.Assert.fail
import org.junit.Assume.assumeTrue

/**
 * Runs the chat server (`examples/chat-server`) for a test, in memory and accepting posts without an
 * account (this frozen example has no accounts), on a free port (or on [port], to restart a server
 * where a killed one was).
 *
 * The path comes from the `chat.mockServer` system property, set by `app/build.gradle.kts`
 * (`target/debug/chat-server`, or `CHAT_MOCK_SERVER`). If the binary has not been built,
 * the test is skipped; CI always builds it (`cargo build -p chat-server`).
 */
class MockServer private constructor(val url: String, private val process: Process) {
    val port: Int get() = url.substringAfterLast(':').toInt()

    /** Kills the server, like Ctrl-C, and waits for it to exit. */
    fun stop() {
        if (!process.isAlive) return
        process.destroy()
        process.waitFor(5, TimeUnit.SECONDS)
    }

    /** Posts as someone else, like the README's curl command. */
    fun post(room: String, author: String, body: String) {
        val connection = URI("$url/rooms/$room/messages").toURL().openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.doOutput = true
        connection.setRequestProperty("Content-Type", "application/json")
        connection.outputStream.use {
            it.write("""{"author":"$author","body":"$body"}""".toByteArray())
        }
        check(connection.responseCode in 200..201) { "post failed: ${connection.responseCode}" }
        connection.disconnect()
    }

    companion object {
        fun binary(): File {
            val path = System.getProperty("chat.mockServer").orEmpty()
            val file = File(path)
            assumeTrue(
                "chat server not built at $path; run cargo build -p chat-server",
                path.isNotEmpty() && file.canExecute(),
            )
            return file
        }

        fun start(port: Int = 0, heartbeatMs: Int = 200): MockServer {
            val binary = binary()
            // Right after a kill the port can still be in use for a moment.
            repeat(50) {
                val process =
                    ProcessBuilder(
                            binary.absolutePath,
                            "--bind",
                            "127.0.0.1:$port",
                            "--heartbeat-ms",
                            heartbeatMs.toString(),
                            "--in-memory",
                            "--allow-anonymous",
                        )
                        .redirectErrorStream(true)
                        .start()
                // The first line is "chat server on http://127.0.0.1:PORT (...".
                val line = process.inputStream.bufferedReader().readLine().orEmpty()
                val start = line.indexOf("http://")
                if (start >= 0) {
                    // Keep draining output so the server never blocks on a full pipe.
                    Thread { process.inputStream.copyTo(java.io.OutputStream.nullOutputStream()) }
                        .apply { isDaemon = true }
                        .start()
                    return MockServer(line.substring(start, line.indexOf(' ', start)), process)
                }
                process.destroy()
                Thread.sleep(100)
            }
            error("chat server did not start on port $port")
        }

        /** A port nothing listens on: "server down". */
        fun unusedPort(): Int = ServerSocket(0).use { it.localPort }
    }
}

/** Polls [condition] until it holds or [timeoutMs] passes. */
suspend fun waitUntil(what: String, timeoutMs: Long = 10_000, condition: () -> Boolean) {
    val deadline = System.currentTimeMillis() + timeoutMs
    while (!condition()) {
        if (System.currentTimeMillis() > deadline) fail("timed out waiting until $what")
        delay(20)
    }
}

fun temporaryDirectory(): File = Files.createTempDirectory("forge-chat-test").toFile()
