use crate::protocol::Platform;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Sent,
    Received,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    pub id: String,
    pub session_id: Uuid,
    pub direction: TransferDirection,
    pub peer_name: String,
    pub peer_platform: Platform,
    pub file_names: Vec<String>,
    pub total_size: u64,
    pub timestamp: DateTime<Utc>,
    pub status: String, // "completed", "failed", "cancelled"
    pub save_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
    pub auto_accept: bool,
    pub trusted_at: DateTime<Utc>,
}

pub struct StorageManager {
    data_dir: PathBuf,
    history: Arc<RwLock<Vec<TransferRecord>>>,
    trusted_peers: Arc<RwLock<HashMap<String, TrustedPeer>>>,
}

impl StorageManager {
    pub fn new(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;

        let history_file = data_dir.join("history.json");
        let peers_file = data_dir.join("trusted_peers.json");

        let history: Vec<TransferRecord> = if history_file.exists() {
            fs::read_to_string(&history_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let trusted_peers: HashMap<String, TrustedPeer> = if peers_file.exists() {
            fs::read_to_string(&peers_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            history: Arc::new(RwLock::new(history)),
            trusted_peers: Arc::new(RwLock::new(trusted_peers)),
        })
    }

    pub fn add_history(&self, record: TransferRecord) -> Result<()> {
        let mut lock = self.history.write();
        lock.insert(0, record); // Most recent first
        if lock.len() > 500 {
            lock.truncate(500);
        }
        self.save_history(&lock)
    }

    pub fn get_history(&self) -> Vec<TransferRecord> {
        self.history.read().clone()
    }

    pub fn clear_history(&self) -> Result<()> {
        let mut lock = self.history.write();
        lock.clear();
        self.save_history(&lock)
    }

    pub fn trust_peer(&self, peer: TrustedPeer) -> Result<()> {
        let mut lock = self.trusted_peers.write();
        lock.insert(peer.fingerprint.clone(), peer);
        self.save_peers(&lock)
    }

    pub fn is_trusted(&self, fingerprint: &str) -> bool {
        self.trusted_peers.read().contains_key(fingerprint)
    }

    pub fn is_auto_accept(&self, fingerprint: &str) -> bool {
        self.trusted_peers
            .read()
            .get(fingerprint)
            .map(|p| p.auto_accept)
            .unwrap_or(false)
    }

    pub fn remove_trusted_peer(&self, fingerprint: &str) -> Result<()> {
        let mut lock = self.trusted_peers.write();
        lock.remove(fingerprint);
        self.save_peers(&lock)
    }

    pub fn get_trusted_peers(&self) -> Vec<TrustedPeer> {
        self.trusted_peers.read().values().cloned().collect()
    }

    fn save_history(&self, records: &[TransferRecord]) -> Result<()> {
        let json = serde_json::to_string_pretty(records)?;
        fs::write(self.data_dir.join("history.json"), json)?;
        Ok(())
    }

    fn save_peers(&self, peers: &HashMap<String, TrustedPeer>) -> Result<()> {
        let json = serde_json::to_string_pretty(peers)?;
        fs::write(self.data_dir.join("trusted_peers.json"), json)?;
        Ok(())
    }
}
