package com.droplink.app.protocol

import com.google.gson.annotations.SerializedName

enum class Platform {
    @SerializedName("windows") WINDOWS,
    @SerializedName("android") ANDROID,
    @SerializedName("ios") IOS,
    @SerializedName("macos") MACOS,
    @SerializedName("linux") LINUX,
    @SerializedName("unknown") UNKNOWN;

    override fun toString(): String = when (this) {
        WINDOWS -> "Windows"
        ANDROID -> "Android"
        IOS -> "iOS"
        MACOS -> "macOS"
        LINUX -> "Linux"
        UNKNOWN -> "Unknown"
    }
}

data class DeviceInfo(
    val id: String,
    val name: String,
    val platform: Platform,
    val version: String,
    val port: Int,
    val fingerprint: String,
    var address: String? = null
)

data class DiscoveryBeacon(
    val magic: String = "DROPLINK_BEACON",
    val device: DeviceInfo,
    val timestamp: Long
)

data class FileMetadata(
    val id: String,
    val name: String,
    val size: Long,
    val mime_type: String,
    val sha256: String,
    val relative_path: String? = null
)

data class TransferManifest(
    val session_id: String,
    val sender: DeviceInfo,
    val files: List<FileMetadata>,
    val total_size: Long,
    val total_files: Int
)

data class PrepareResponse(
    val accepted: Boolean,
    val reason: String?,
    val resume_offsets: Map<String, Long> = emptyMap()
)

data class PairRequest(
    val device: DeviceInfo,
    val session_id: String,
    val sas_pin: String
)

data class PairResponse(
    val accepted: Boolean,
    val message: String?,
    val session_token: String?
)

enum class TransferStatus {
    PENDING,
    CONNECTING,
    TRANSFERRING,
    PAUSED,
    VERIFYING,
    COMPLETED,
    CANCELLED,
    FAILED
}

data class LiveTransferProgress(
    val sessionId: String,
    val status: TransferStatus,
    val currentFileName: String,
    val currentFileIndex: Int,
    val totalFiles: Int,
    val transferredBytes: Long,
    val totalBytes: Long,
    val speedBytesPerSec: Double,
    val estimatedSecondsRemaining: Long?,
    val errorMessage: String? = null
)
