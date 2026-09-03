use crate::crypto::{compute_file_sha256, compute_sas_pin};
use crate::protocol::{
    DeviceInfo, FileMetadata, PairRequest, PairResponse, PrepareResponse,
    TransferManifest, TransferProgress, TransferStatus,
};
use crate::storage::{StorageManager, TransferDirection, TransferRecord};
use crate::transfer::ActiveTransferSession;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::broadcast;
use tracing::info;
use uuid::Uuid;

pub struct TransferClient {
    http: Client,
    storage: Arc<StorageManager>,
    local_device: DeviceInfo,
}

impl TransferClient {
    pub fn new(local_device: DeviceInfo, storage: Arc<StorageManager>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(600))
            .tcp_nodelay(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .danger_accept_invalid_certs(true) // DropLink uses peer-verified self-signed ephemeral certificates
            .build()
            .unwrap_or_default();

        Self {
            http,
            storage,
            local_device,
        }
    }

    pub async fn ping_peer(&self, host: &str, port: u16) -> Result<DeviceInfo> {
        let url = format!("http://{}:{}/api/v1/ping", host, port);
        let resp = self.http.get(&url).send().await?;
        let info = resp.json::<DeviceInfo>().await?;
        Ok(info)
    }

    pub async fn pair(&self, host: &str, port: u16, target_fp: &str) -> Result<PairResponse> {
        let sas_pin = compute_sas_pin(&self.local_device.fingerprint, target_fp);
        let req = PairRequest {
            device: self.local_device.clone(),
            session_id: Uuid::new_v4(),
            sas_pin,
        };

        let url = format!("http://{}:{}/api/v1/pair", host, port);
        let resp = self.http.post(&url).json(&req).send().await?;
        let pair_resp = resp.json::<PairResponse>().await?;
        Ok(pair_resp)
    }

    pub async fn send_files(
        &self,
        host: &str,
        port: u16,
        file_paths: Vec<PathBuf>,
        progress_tx: Option<broadcast::Sender<TransferProgress>>,
    ) -> Result<()> {
        let session_id = Uuid::new_v4();

        // 1. Build FileMetadata for all files
        let mut files = Vec::new();
        let mut total_size = 0u64;

        for path in &file_paths {
            let meta = tokio::fs::metadata(path).await
                .with_context(|| format!("Failed to read metadata for file: {:?}", path))?;

            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unnamed_file")
                .to_string();

            let sha256 = compute_file_sha256(path).await?;
            let size = meta.len();
            total_size += size;

            files.push(FileMetadata {
                id: Uuid::new_v4().to_string(),
                name,
                size,
                mime_type: "application/octet-stream".to_string(),
                sha256,
                relative_path: None,
            });
        }

        let manifest = TransferManifest {
            session_id,
            sender: self.local_device.clone(),
            files: files.clone(),
            total_size,
            total_files: files.len(),
        };

        let session = Arc::new(parking_lot::Mutex::new(ActiveTransferSession::new(manifest.clone())));

        // 2. Send Prepare request
        let prepare_url = format!("http://{}:{}/api/v1/transfer/prepare", host, port);
        let prep_resp: PrepareResponse = self.http.post(&prepare_url)
            .json(&manifest)
            .send()
            .await
            .context("Failed to connect to receiver for prepare phase")?
            .json()
            .await
            .context("Failed to parse prepare response from receiver")?;

        if !prep_resp.accepted {
            let reason = prep_resp.reason.unwrap_or_else(|| "Declined by receiver".into());
            session.lock().status = TransferStatus::Failed;
            if let Some(tx) = &progress_tx {
                let _ = tx.send(session.lock().snapshot_progress(Some(reason.clone())));
            }
            bail!("Transfer was declined: {}", reason);
        }

        // 3. Stream each file
        let mut cumulative_bytes = 0u64;
        session.lock().status = TransferStatus::Transferring;

        for (idx, (file_meta, path)) in files.iter().zip(file_paths.iter()).enumerate() {
            if session.lock().is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                session.lock().status = TransferStatus::Failed;
                if let Some(tx) = &progress_tx {
                    let _ = tx.send(session.lock().snapshot_progress(Some("Transfer cancelled by user".into())));
                }
                break;
            }

            let start_offset = prep_resp.resume_offsets.get(&file_meta.id).copied().unwrap_or(0);
            session.lock().current_file_index = idx;
            cumulative_bytes += start_offset;

            let mut file = File::open(path).await
                .with_context(|| format!("Failed to open file for streaming: {:?}", path))?;

            if start_offset > 0 {
                file.seek(SeekFrom::Start(start_offset)).await?;
            }

            let upload_url = format!("http://{}:{}/api/v1/transfer/upload/{}", host, port, file_meta.id);
            let chunk_size = 1024 * 1024; // 1 MB high-throughput turbo chunks
            let mut buffer = vec![0u8; chunk_size];

            // Stream chunks in HTTP body with live progress reporting
            let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(16);
            let mut file_handle = file;
            let cancelled_flag = session.lock().is_cancelled.clone();
            let paused_flag = session.lock().is_paused.clone();

            let bytes_streamed = Arc::new(std::sync::atomic::AtomicU64::new(cumulative_bytes));
            let bytes_streamed_worker = bytes_streamed.clone();

            tokio::spawn(async move {
                loop {
                    if cancelled_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    while paused_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }

                    match file_handle.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = bytes::Bytes::copy_from_slice(&buffer[..n]);
                            bytes_streamed_worker.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
                            if body_tx.send(Ok(chunk)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = body_tx.send(Err(e)).await;
                            break;
                        }
                    }
                }
            });

            // Monitor progress periodically while uploading
            let monitor_bytes = bytes_streamed.clone();
            let monitor_tx = progress_tx.clone();
            let monitor_file_size = file_meta.size;
            let monitor_cancelled = session.lock().is_cancelled.clone();
            let session_monitor = session.clone();

            let monitor_handle = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(250));
                while !monitor_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    ticker.tick().await;
                    let current = monitor_bytes.load(std::sync::atomic::Ordering::Relaxed);
                    let mut s = session_monitor.lock();
                    s.update_progress(monitor_file_size, current);
                    if let Some(tx) = &monitor_tx {
                        let _ = tx.send(s.snapshot_progress(None));
                    }
                }
            });

            let remaining_bytes = file_meta.size.saturating_sub(start_offset);
            let stream_body = reqwest::Body::wrap_stream(tokio_stream::wrappers::ReceiverStream::new(body_rx));
            let upload_req = self.http.post(&upload_url)
                .header("content-length", remaining_bytes.to_string())
                .header("x-droplink-offset", start_offset.to_string())
                .body(stream_body);

            let upload_resp = upload_req.send().await
                .with_context(|| format!("Failed to stream upload for file: {}", file_meta.name))?;

            monitor_handle.abort();

            if !upload_resp.status().is_success() {
                let err_msg = upload_resp.text().await.unwrap_or_default();
                session.lock().status = TransferStatus::Failed;
                if let Some(tx) = &progress_tx {
                    let _ = tx.send(session.lock().snapshot_progress(Some(err_msg.clone())));
                }
                bail!("File upload failed: {}", err_msg);
            }

            cumulative_bytes += file_meta.size.saturating_sub(start_offset);
            session.lock().update_progress(file_meta.size, cumulative_bytes);

            if let Some(tx) = &progress_tx {
                let _ = tx.send(session.lock().snapshot_progress(None));
            }
        }

        session.lock().status = TransferStatus::Completed;
        if let Some(tx) = &progress_tx {
            let _ = tx.send(session.lock().snapshot_progress(None));
        }

        // Record in history
        let record = TransferRecord {
            id: Uuid::new_v4().to_string(),
            session_id,
            direction: TransferDirection::Sent,
            peer_name: format!("{}:{}", host, port),
            peer_platform: crate::protocol::Platform::Unknown,
            file_names: files.iter().map(|f| f.name.clone()).collect(),
            total_size,
            timestamp: Utc::now(),
            status: "completed".to_string(),
            save_path: None,
        };
        let _ = self.storage.add_history(record);

        info!("All files successfully transferred in session {}", session_id);
        Ok(())
    }
}
