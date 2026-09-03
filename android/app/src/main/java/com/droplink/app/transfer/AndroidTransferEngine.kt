package com.droplink.app.transfer

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import com.droplink.app.crypto.AndroidCrypto
import com.droplink.app.protocol.*
import com.droplink.app.service.DropLinkForegroundService
import com.google.gson.Gson
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import okhttp3.*
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody.Companion.toRequestBody
import okio.BufferedSink
import java.io.IOException
import java.util.UUID
import java.util.concurrent.TimeUnit

class AndroidTransferEngine(
    private val context: Context,
    private val localDevice: DeviceInfo
) {
    private val gson = Gson()
    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(300, TimeUnit.SECONDS)
        .writeTimeout(300, TimeUnit.SECONDS)
        .build()

    private val _progressFlow = MutableStateFlow<LiveTransferProgress?>(null)
    val progressFlow: StateFlow<LiveTransferProgress?> = _progressFlow.asStateFlow()

    private var isCancelled = false
    private var isPaused = false

    fun cancelTransfer() {
        isCancelled = true
    }

    fun togglePause(): Boolean {
        isPaused = !isPaused
        return isPaused
    }

    suspend fun sendUris(
        host: String,
        port: Int,
        uris: List<Uri>
    ): Boolean = withContext(Dispatchers.IO) {
        isCancelled = false
        isPaused = false
        val sessionId = UUID.randomUUID().toString()

        // 1. Gather file metadata
        val fileMetas = mutableListOf<FileMetadata>()
        var totalBytes = 0L

        for (uri in uris) {
            val (name, size) = queryUriMetadata(uri)
            val sha256 = context.contentResolver.openInputStream(uri)?.use {
                AndroidCrypto.computeSha256(it)
            } ?: ""

            fileMetas.add(
                FileMetadata(
                    id = UUID.randomUUID().toString(),
                    name = name,
                    size = size,
                    mime_type = context.contentResolver.getType(uri) ?: "application/octet-stream",
                    sha256 = sha256
                )
            )
            totalBytes += size
        }

        val manifest = TransferManifest(
            session_id = sessionId,
            sender = localDevice,
            files = fileMetas,
            total_size = totalBytes,
            total_files = fileMetas.size
        )

        DropLinkForegroundService.startService(context, fileMetas.firstOrNull()?.name ?: "Transfer")

        // 2. Prepare request
        val prepareUrl = "http://$host:$port/api/v1/transfer/prepare"
        val prepBody = gson.toJson(manifest).toRequestBody("application/json".toMediaTypeOrNull())
        val prepReq = Request.Builder().url(prepareUrl).post(prepBody).build()

        try {
            val prepResp = httpClient.newCall(prepReq).execute()
            if (!prepResp.isSuccessful) {
                stopProgress("Receiver declined transfer")
                return@withContext false
            }

            val prepJson = prepResp.body?.string() ?: ""
            val prepData = gson.fromJson(prepJson, PrepareResponse::class.java)
            if (!prepData.accepted) {
                stopProgress(prepData.reason ?: "Declined by receiver")
                return@withContext false
            }

            // 3. Stream each file
            var cumulativeBytes = 0L
            val startTime = System.currentTimeMillis()

            for ((idx, meta) in fileMetas.withIndex()) {
                if (isCancelled) break
                val uri = uris[idx]
                val startOffset = prepData.resume_offsets[meta.id] ?: 0L
                cumulativeBytes += startOffset

                val uploadUrl = "http://$host:$port/api/v1/transfer/upload/${meta.id}"

                val streamingBody = object : RequestBody() {
                    override fun contentType(): MediaType? = "application/octet-stream".toMediaTypeOrNull()
                    override fun contentLength(): Long = meta.size - startOffset

                    override fun writeTo(sink: BufferedSink) {
                        context.contentResolver.openInputStream(uri)?.use { stream ->
                            if (startOffset > 0) {
                                stream.skip(startOffset)
                            }
                            val buffer = ByteArray(1024 * 1024) // 1 MB turbo buffer
                            var read: Int
                            while (stream.read(buffer).also { read = it } != -1) {
                                if (isCancelled) throw IOException("Cancelled by user")
                                while (isPaused) {
                                    Thread.sleep(100)
                                }
                                sink.write(buffer, 0, read)
                                cumulativeBytes += read

                                val elapsedSec = (System.currentTimeMillis() - startTime) / 1000.0
                                val speed = if (elapsedSec > 0) cumulativeBytes / elapsedSec else 0.0
                                val remainingBytes = totalBytes - cumulativeBytes
                                val eta = if (speed > 1024) (remainingBytes / speed).toLong() else null

                                val progressPercent = if (totalBytes > 0) ((cumulativeBytes * 100) / totalBytes).toInt() else 0

                                _progressFlow.value = LiveTransferProgress(
                                    sessionId = sessionId,
                                    status = TransferStatus.TRANSFERRING,
                                    currentFileName = meta.name,
                                    currentFileIndex = idx,
                                    totalFiles = fileMetas.size,
                                    transferredBytes = cumulativeBytes,
                                    totalBytes = totalBytes,
                                    speedBytesPerSec = speed,
                                    estimatedSecondsRemaining = eta
                                )

                                val speedStr = "%.1f MB/s".format(speed / (1024 * 1024))
                                DropLinkForegroundService.updateProgress(context, meta.name, progressPercent, speedStr)
                            }
                        }
                    }
                }

                val uploadReq = Request.Builder()
                    .url(uploadUrl)
                    .addHeader("x-droplink-offset", startOffset.toString())
                    .post(streamingBody)
                    .build()

                val uploadResp = httpClient.newCall(uploadReq).execute()
                if (!uploadResp.isSuccessful) {
                    stopProgress("File upload failed: ${uploadResp.code}")
                    return@withContext false
                }
            }

            _progressFlow.value = _progressFlow.value?.copy(status = TransferStatus.COMPLETED)
            DropLinkForegroundService.stopService(context)
            return@withContext true

        } catch (e: Exception) {
            stopProgress(e.message ?: "Transfer error")
            return@withContext false
        }
    }

    private fun stopProgress(errorMsg: String) {
        _progressFlow.value = _progressFlow.value?.copy(
            status = TransferStatus.FAILED,
            errorMessage = errorMsg
        )
        DropLinkForegroundService.stopService(context)
    }

    private fun queryUriMetadata(uri: Uri): Pair<String, Long> {
        var name = "file"
        var size = 0L

        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)
            if (cursor.moveToFirst()) {
                if (nameIndex != -1) name = cursor.getString(nameIndex)
                if (sizeIndex != -1) size = cursor.getLong(sizeIndex)
            }
        }
        return Pair(name, size)
    }
}
