package com.xent.app

import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.EOFException
import java.io.InputStream
import java.net.InetAddress
import java.net.URLDecoder
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket

/**
 * Pure-Kotlin HTTPS/1.1 server on SSLServerSocket compatible with Xent's desktop REST & chunked transfer endpoints.
 */
class XentHttpServer(
    private val sslContext: SSLContext,
    private val router: (method: String, path: String, query: Map<String, String>, remoteIp: String, body: ByteArray) -> HttpResponse
) {
    data class HttpResponse(
        val statusCode: Int = 200,
        val statusText: String = "OK",
        val contentType: String = "application/json; charset=utf-8",
        val body: ByteArray = ByteArray(0)
    ) {
        companion object {
            fun okJson(obj: JSONObject): HttpResponse = HttpResponse(
                200, "OK", "application/json; charset=utf-8", obj.toString().toByteArray(Charsets.UTF_8)
            )

            fun text(code: Int, statusText: String, text: String): HttpResponse = HttpResponse(
                code, statusText, "text/plain; charset=utf-8", text.toByteArray(Charsets.UTF_8)
            )

            fun error(code: Int, statusText: String, msg: String): HttpResponse = HttpResponse(
                code, statusText, "application/json; charset=utf-8",
                JSONObject().put("success", false).put("error", msg).toString().toByteArray(Charsets.UTF_8)
            )
        }
    }

    private var serverSocket: SSLServerSocket? = null
    private val running = AtomicBoolean(false)
    private var acceptThread: Thread? = null
    private val workers: ExecutorService = Executors.newCachedThreadPool()

    var boundPort: Int = 0
        private set

    fun start(preferredPort: Int = 53317): Int {
        val factory = sslContext.serverSocketFactory
        val candidates = (listOf(preferredPort) + (53317..53326).toList()).distinct()
        var bound: SSLServerSocket? = null
        for (p in candidates) {
            val attempt = runCatching {
                (factory.createServerSocket(p, 64, InetAddress.getByName("0.0.0.0")) as SSLServerSocket).apply {
                    reuseAddress = true
                    soTimeout = 0
                }
            }.getOrNull()
            if (attempt != null) {
                bound = attempt
                boundPort = p
                break
            }
        }
        if (bound == null) {
            // Ephemeral fallback if all 53317..53326 are occupied (helpful in tests)
            bound = (factory.createServerSocket(0, 64, InetAddress.getByName("0.0.0.0")) as SSLServerSocket).apply {
                reuseAddress = true
            }
            boundPort = bound.localPort
        }

        serverSocket = bound
        running.set(true)
        acceptThread = Thread({
            while (running.get()) {
                val sock = runCatching { bound.accept() }.getOrNull() ?: break
                workers.submit { handleClient(sock) }
            }
        }, "xent-https-accept").apply {
            isDaemon = true
            start()
        }
        return boundPort
    }

    fun stop() {
        running.set(false)
        runCatching { serverSocket?.close() }
        serverSocket = null
        runCatching { workers.shutdownNow() }
    }

    private fun handleClient(sock: java.net.Socket) {
        sock.use { socket ->
            socket.soTimeout = 15_000
            val remoteIp = socket.inetAddress?.hostAddress?.removePrefix("::ffff:") ?: ""
            val input = BufferedInputStream(socket.getInputStream(), 65_536)
            val out = socket.getOutputStream()

            val requestLine = readAsciiLine(input) ?: return
            val parts = requestLine.split(' ')
            if (parts.size < 2) return
            val method = parts[0].uppercase()
            val rawTarget = parts[1]

            val headers = mutableMapOf<String, String>()
            while (true) {
                val line = readAsciiLine(input) ?: break
                if (line.isEmpty()) break
                val colon = line.indexOf(':')
                if (colon > 0) {
                    headers[line.substring(0, colon).trim().lowercase()] = line.substring(colon + 1).trim()
                }
            }

            val bodyBytes = when {
                headers["transfer-encoding"]?.lowercase()?.contains("chunked") == true -> readChunkedBody(input)
                headers.containsKey("content-length") -> {
                    val len = headers["content-length"]?.toIntOrNull() ?: 0
                    if (len > 0) readExactBytes(input, len) else ByteArray(0)
                }
                else -> ByteArray(0)
            }

            val qIdx = rawTarget.indexOf('?')
            val path = if (qIdx >= 0) rawTarget.substring(0, qIdx) else rawTarget
            val queryStr = if (qIdx >= 0) rawTarget.substring(qIdx + 1) else ""
            val query = parseQuery(queryStr)

            val resp = runCatching { router(method, path, query, remoteIp, bodyBytes) }.getOrElse { e ->
                HttpResponse.error(500, "Internal Server Error", e.message ?: "Server error")
            }

            val headerText = buildString {
                append("HTTP/1.1 ").append(resp.statusCode).append(' ').append(resp.statusText).append("\r\n")
                append("Content-Type: ").append(resp.contentType).append("\r\n")
                append("Content-Length: ").append(resp.body.size).append("\r\n")
                append("Connection: close\r\n\r\n")
            }
            out.write(headerText.toByteArray(Charsets.US_ASCII))
            if (resp.body.isNotEmpty()) out.write(resp.body)
            out.flush()
        }
    }

    private fun readAsciiLine(input: InputStream): String? {
        val baos = ByteArrayOutputStream(128)
        while (true) {
            val b = input.read()
            if (b == -1) return if (baos.size() == 0) null else baos.toString("ISO-8859-1")
            if (b == '\n'.code) break
            if (b != '\r'.code) baos.write(b)
        }
        return baos.toString("ISO-8859-1")
    }

    private fun readExactBytes(input: InputStream, len: Int): ByteArray {
        val buf = ByteArray(len)
        var off = 0
        while (off < len) {
            val n = input.read(buf, off, len - off)
            if (n == -1) throw EOFException("Unexpected EOF after $off/$len bytes")
            off += n
        }
        return buf
    }

    private fun readChunkedBody(input: InputStream): ByteArray {
        val baos = ByteArrayOutputStream()
        while (true) {
            val sizeLine = readAsciiLine(input) ?: break
            val semi = sizeLine.indexOf(';')
            val hex = (if (semi >= 0) sizeLine.substring(0, semi) else sizeLine).trim()
            val chunkLen = hex.toIntOrNull(16) ?: 0
            if (chunkLen <= 0) {
                // consume trailing CRLF
                readAsciiLine(input)
                break
            }
            baos.write(readExactBytes(input, chunkLen))
            readAsciiLine(input) // CRLF after chunk
        }
        return baos.toByteArray()
    }

    private fun parseQuery(q: String): Map<String, String> {
        if (q.isEmpty()) return emptyMap()
        val map = mutableMapOf<String, String>()
        for (pair in q.split('&')) {
            val eq = pair.indexOf('=')
            if (eq > 0) {
                val k = URLDecoder.decode(pair.substring(0, eq), "UTF-8")
                val v = URLDecoder.decode(pair.substring(eq + 1), "UTF-8")
                map[k] = v
            }
        }
        return map
    }
}
