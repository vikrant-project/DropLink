package com.droplink.app.server

import android.content.Context
import android.os.Environment
import com.droplink.app.protocol.*
import com.google.gson.Gson
import kotlinx.coroutines.*
import java.io.*
import java.net.ServerSocket
import java.net.Socket
import java.util.UUID

class AndroidReceiverServer(
    private val context: Context,
    private val localDevice: DeviceInfo,
    private val onIncomingTransfer: (TransferManifest, (Boolean) -> Unit) -> Unit,
    private val onReceiveProgress: (String, Float, Long, Long) -> Unit
) {
    private val gson = Gson()
    private var serverSocket: ServerSocket? = null
    private var isRunning = false
    private var activeManifest: TransferManifest? = null
    private var serverScope: CoroutineScope? = null

    fun start() {
        if (isRunning) return
        isRunning = true
        serverScope = CoroutineScope(Dispatchers.IO + SupervisorJob())

        serverScope?.launch {
            try {
                serverSocket = ServerSocket(52520).apply {
                    reuseAddress = true
                    receiveBufferSize = 2 * 1024 * 1024
                }
                println("[AndroidReceiverServer] Listening on port 52520")

                while (isRunning) {
                    val client = serverSocket?.accept() ?: break
                    client.tcpNoDelay = true
                    client.receiveBufferSize = 2 * 1024 * 1024
                    client.sendBufferSize = 2 * 1024 * 1024
                    launch {
                        handleClient(client)
                    }
                }
            } catch (e: Exception) {
                if (isRunning) e.printStackTrace()
            }
        }
    }

    fun stop() {
        isRunning = false
        serverScope?.cancel()
        serverScope = null
        try {
            serverSocket?.close()
        } catch (_: Exception) {}
        serverSocket = null
    }

    private suspend fun handleClient(socket: Socket) = withContext(Dispatchers.IO) {
        try {
            val input = BufferedInputStream(socket.getInputStream())
            val output = BufferedOutputStream(socket.getOutputStream())

            // Read HTTP headers until \r\n\r\n
            val headerBuffer = ByteArrayOutputStream()
            val separator = byteArrayOf(13, 10, 13, 10) // \r\n\r\n
            var sepMatchIndex = 0

            while (true) {
                val b = input.read()
                if (b == -1) break
                headerBuffer.write(b)
                if (b.toByte() == separator[sepMatchIndex]) {
                    sepMatchIndex++
                    if (sepMatchIndex == separator.size) break
                } else {
                    sepMatchIndex = if (b.toByte() == separator[0]) 1 else 0
                }
            }

            val headerStr = headerBuffer.toString("UTF-8")
            val lines = headerStr.split("\r\n")
            val requestLine = lines.firstOrNull() ?: return@withContext
            val parts = requestLine.split(" ")
            if (parts.size < 2) return@withContext

            val method = parts[0]
            val path = parts[1]

            // Find Content-Length
            var contentLength = 0L
            for (line in lines) {
                if (line.lowercase().startsWith("content-length:")) {
                    contentLength = line.substringAfter(":").trim().toLongOrNull() ?: 0L
                }
            }

            when {
                method == "GET" && path == "/api/v1/ping" -> {
                    val json = gson.toJson(localDevice)
                    sendHttpResponse(output, 200, "application/json", json.toByteArray(Charsets.UTF_8))
                }
                method == "POST" && path == "/api/v1/transfer/prepare" -> {
                    handlePrepare(input, output, contentLength)
                }
                method == "POST" && path.startsWith("/api/v1/transfer/upload/") -> {
                    val fileId = path.removePrefix("/api/v1/transfer/upload/")
                    handleUpload(input, output, fileId, contentLength)
                }
                else -> {
                    sendHttpResponse(output, 404, "text/plain", "Not Found".toByteArray())
                }
            }
        } catch (e: Exception) {
            e.printStackTrace()
        } finally {
            try { socket.close() } catch (_: Exception) {}
        }
    }

    private suspend fun handlePrepare(input: InputStream, output: OutputStream, contentLength: Long) {
        val bodyBytes = ByteArray(contentLength.toInt().coerceAtLeast(0))
        var readTotal = 0
        while (readTotal < bodyBytes.size) {
            val n = input.read(bodyBytes, readTotal, bodyBytes.size - readTotal)
            if (n <= 0) break
            readTotal += n
        }

        val json = String(bodyBytes, 0, readTotal, Charsets.UTF_8)
        val manifest = try {
            gson.fromJson(json, TransferManifest::class.java)
        } catch (_: Exception) { null }

        if (manifest == null) {
            sendHttpResponse(output, 400, "application/json", "{\"accepted\":false,\"reason\":\"Invalid manifest\"}".toByteArray())
            return
        }

        activeManifest = manifest

        // Await user approval via callback
        val accepted = CompletableDeferred<Boolean>()
        withContext(Dispatchers.Main) {
            onIncomingTransfer(manifest) { isAccepted ->
                accepted.complete(isAccepted)
            }
        }

        val userAccepted = accepted.await()
        if (userAccepted) {
            val response = PrepareResponse(accepted = true, reason = null)
            sendHttpResponse(output, 200, "application/json", gson.toJson(response).toByteArray(Charsets.UTF_8))
        } else {
            val response = PrepareResponse(accepted = false, reason = "Declined by user")
            sendHttpResponse(output, 200, "application/json", gson.toJson(response).toByteArray(Charsets.UTF_8))
        }
    }

    private suspend fun handleUpload(input: InputStream, output: OutputStream, fileId: String, contentLength: Long) {
        val fileMeta = activeManifest?.files?.firstOrNull { it.id == fileId }
        val fileName = fileMeta?.name ?: "received_file_${UUID.randomUUID()}"
        val targetSize = if (fileMeta != null && fileMeta.size > 0) fileMeta.size else contentLength

        val dropLinkDir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "DropLink")
        if (!dropLinkDir.exists()) dropLinkDir.mkdirs()

        val destFile = File(dropLinkDir, fileName)
        val fileOut = FileOutputStream(destFile)

        var bytesWritten = 0L
        val buffer = ByteArray(1024 * 1024) // 1 MB turbo buffer for maximum throughput
        var lastUIUpdate = System.currentTimeMillis()

        try {
            while (isRunning) {
                if (targetSize > 0 && bytesWritten >= targetSize) break
                val toRead = if (targetSize > 0) {
                    Math.min(buffer.size.toLong(), targetSize - bytesWritten).toInt()
                } else {
                    buffer.size
                }

                val n = input.read(buffer, 0, toRead)
                if (n <= 0) break
                fileOut.write(buffer, 0, n)
                bytesWritten += n

                val now = System.currentTimeMillis()
                if (now - lastUIUpdate > 200) {
                    lastUIUpdate = now
                    val pct = if (targetSize > 0) bytesWritten.toFloat() / targetSize.toFloat() else 0f
                    withContext(Dispatchers.Main) {
                        onReceiveProgress(fileName, pct, bytesWritten, targetSize)
                    }
                }
            }
            fileOut.flush()
            fileOut.close()

            withContext(Dispatchers.Main) {
                onReceiveProgress(fileName, 1.0f, bytesWritten, targetSize)
            }
            sendHttpResponse(output, 200, "application/json", "{\"status\":\"ok\"}".toByteArray())
        } catch (e: Exception) {
            try { fileOut.close() } catch (_: Exception) {}
            sendHttpResponse(output, 500, "application/json", "{\"error\":\"Upload failed\"}".toByteArray())
        }
    }

    private fun sendHttpResponse(output: OutputStream, statusCode: Int, contentType: String, body: ByteArray) {
        val statusText = when (statusCode) {
            200 -> "OK"
            400 -> "Bad Request"
            404 -> "Not Found"
            else -> "Internal Server Error"
        }
        val header = "HTTP/1.1 $statusCode $statusText\r\n" +
                "Content-Type: $contentType\r\n" +
                "Content-Length: ${body.size}\r\n" +
                "Connection: close\r\n\r\n"
        output.write(header.toByteArray(Charsets.UTF_8))
        output.write(body)
        output.flush()
    }
}
