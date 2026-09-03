pub mod client;
pub mod crypto;
pub mod discovery;
pub mod protocol;
pub mod security;
pub mod server;
pub mod storage;
pub mod transfer;

// Re-exports of core types
pub use client::TransferClient;
pub use crypto::{compute_file_sha256, compute_sas_pin, generate_ephemeral_cert, TlsCertificateBundle};
pub use discovery::{DiscoveryEvent, DiscoveryManager, get_local_ip};
pub use protocol::{
    DeviceInfo, DiscoveryBeacon, FileMetadata, PairRequest, PairResponse, Platform,
    PrepareResponse, TransferManifest, TransferProgress, TransferStatus,
};
pub use security::{resolve_conflict_path, resolve_safe_path, sanitize_filename};
pub use server::{IncomingTransferPrompt, ServerState, TransferServer};
pub use storage::{StorageManager, TransferDirection, TransferRecord, TrustedPeer};
pub use transfer::{ActiveTransferSession, SpeedTracker};
