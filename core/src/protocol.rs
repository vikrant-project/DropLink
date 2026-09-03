use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Platform type of the DropLink node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Android,
    Ios,
    Macos,
    Linux,
    Unknown,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Windows => write!(f, "Windows"),
            Platform::Android => write!(f, "Android"),
            Platform::Ios => write!(f, "iOS"),
            Platform::Macos => write!(f, "macOS"),
            Platform::Linux => write!(f, "Linux"),
            Platform::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Device identification and capabilities announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub version: String,
    pub port: u16,
    pub fingerprint: String, // SHA-256 fingerprint of peer's TLS certificate
    pub address: Option<String>,
}

/// Discovery beacon packet sent over UDP multicast/broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBeacon {
    pub magic: String, // "DROPLINK_BEACON"
    pub device: DeviceInfo,
    pub timestamp: u64,
}

impl DiscoveryBeacon {
    pub const MAGIC: &'static str = "DROPLINK_BEACON";
    pub const DEFAULT_PORT: u16 = 52520;
}

/// Pairing request sent to establish trust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    pub device: DeviceInfo,
    pub session_id: Uuid,
    pub sas_pin: String, // 6-digit numeric Short Authentication String
}

/// Pairing response acknowledging trust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    pub accepted: bool,
    pub message: Option<String>,
    pub session_token: Option<String>,
}

/// Metadata for an individual file in a transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
    pub sha256: String,
    pub relative_path: Option<String>,
}

/// Transfer manifest containing all files to be transmitted in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferManifest {
    pub session_id: Uuid,
    pub sender: DeviceInfo,
    pub files: Vec<FileMetadata>,
    pub total_size: u64,
    pub total_files: usize,
}

/// Response from receiver indicating acceptance and resume offsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareResponse {
    pub accepted: bool,
    pub reason: Option<String>,
    /// Maps file_id to starting byte offset for resume (0 for new transfer)
    pub resume_offsets: std::collections::HashMap<String, u64>,
}

/// Transfer state for progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Pending,
    Connecting,
    Pairing,
    Transferring,
    Paused,
    Verifying,
    Completed,
    Cancelled,
    Failed,
}

/// Live progress telemetry for active transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub session_id: Uuid,
    pub status: TransferStatus,
    pub current_file_index: usize,
    pub current_file_name: String,
    pub current_file_bytes: u64,
    pub current_file_total: u64,
    pub total_bytes_transferred: u64,
    pub total_bytes_overall: u64,
    pub speed_bytes_per_sec: f64,
    pub estimated_seconds_remaining: Option<u64>,
    pub error_message: Option<String>,
}
