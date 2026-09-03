use crate::crypto::{compute_file_sha256, compute_sas_pin, TlsCertificateBundle};
use crate::protocol::{
    DeviceInfo, PairRequest, PairResponse, PrepareResponse,
    TransferManifest, TransferProgress,
};
use crate::security::{resolve_conflict_path, resolve_safe_path, sanitize_filename};
use crate::storage::{StorageManager, TransferDirection, TransferRecord};
use crate::transfer::SpeedTracker;
use anyhow::Result;
use axum::{
    body::Body,
    extract::{ConnectInfo, Path as AxPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::net::SocketAddr;
use chrono::Utc;
use futures_util::StreamExt;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{broadcast, oneshot};
use tracing::{error, info};
use uuid::Uuid;

#[derive(Clone)]
pub struct IncomingTransferPrompt {
    pub manifest: TransferManifest,
    pub sas_pin: String,
    pub reply_tx: Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>,
}

pub struct ServerState {
    pub local_device: Arc<RwLock<DeviceInfo>>,
    pub download_dir: Arc<RwLock<PathBuf>>,
    pub storage: Arc<StorageManager>,
    pub cert_bundle: Arc<TlsCertificateBundle>,
    pub active_manifest: Arc<RwLock<Option<TransferManifest>>>,
    pub active_progress: Arc<RwLock<Option<TransferProgress>>>,
    pub speed_tracker: Arc<RwLock<SpeedTracker>>,
    pub is_cancelled: Arc<AtomicBool>,
    pub prompt_tx: broadcast::Sender<IncomingTransferPrompt>,
}

pub struct TransferServer {
    state: Arc<ServerState>,
    shutdown_tx: broadcast::Sender<()>,
}

impl TransferServer {
    pub fn new(
        local_device: DeviceInfo,
        download_dir: PathBuf,
        storage: Arc<StorageManager>,
        cert_bundle: TlsCertificateBundle,
    ) -> (Self, broadcast::Receiver<IncomingTransferPrompt>) {
        let (prompt_tx, prompt_rx) = broadcast::channel(16);
        let (shutdown_tx, _) = broadcast::channel(1);

        let state = Arc::new(ServerState {
            local_device: Arc::new(RwLock::new(local_device)),
            download_dir: Arc::new(RwLock::new(download_dir)),
            storage,
            cert_bundle: Arc::new(cert_bundle),
            active_manifest: Arc::new(RwLock::new(None)),
            active_progress: Arc::new(RwLock::new(None)),
            speed_tracker: Arc::new(RwLock::new(SpeedTracker::new(2))),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            prompt_tx,
        });

        (Self { state, shutdown_tx }, prompt_rx)
    }

    pub fn set_download_dir(&self, dir: PathBuf) {
        *self.state.download_dir.write() = dir;
    }

    pub fn get_active_progress(&self) -> Option<TransferProgress> {
        self.state.active_progress.read().clone()
    }

    pub async fn start(&self, bind_port: u16) -> Result<u16> {
        let app = Router::new()
            .route("/api/v1/ping", get(handle_ping))
            .route("/api/v1/pair", post(handle_pair))
            .route("/api/v1/transfer/prepare", post(handle_prepare))
            .route("/api/v1/transfer/upload/{file_id}", post(handle_upload))
            .route("/api/v1/transfer/status/{session_id}", get(handle_status))
            .route("/api/v1/transfer/cancel/{session_id}", post(handle_cancel))
            .with_state(Arc::clone(&self.state));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", bind_port)).await?;
        let actual_port = listener.local_addr()?.port();
        info!("Transfer server listening on port {}", actual_port);

        // Update local device port
        self.state.local_device.write().port = actual_port;

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.recv().await;
                })
                .await
                .unwrap_or_else(|e| error!("Transfer server error: {}", e));
        });

        Ok(actual_port)
    }

    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn handle_ping(State(state): State<Arc<ServerState>>) -> Json<DeviceInfo> {
    Json(state.local_device.read().clone())
}

async fn handle_pair(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PairRequest>,
) -> impl IntoResponse {
    let local_fp = &state.cert_bundle.fingerprint;
    let expected_sas = compute_sas_pin(&req.device.fingerprint, local_fp);

    let auto_accept = state.storage.is_auto_accept(&req.device.fingerprint);

    if auto_accept {
        return (StatusCode::OK, Json(PairResponse {
            accepted: true,
            message: Some("Device is trusted. Auto-accepted.".to_string()),
            session_token: Some(Uuid::new_v4().to_string()),
        }));
    }

    // Verify SAS PIN if provided
    if !req.sas_pin.is_empty() && req.sas_pin == expected_sas {
        return (StatusCode::OK, Json(PairResponse {
            accepted: true,
            message: Some("SAS PIN verified successfully.".to_string()),
            session_token: Some(Uuid::new_v4().to_string()),
        }));
    }

    (StatusCode::FORBIDDEN, Json(PairResponse {
        accepted: false,
        message: Some("SAS PIN mismatch or device not trusted.".to_string()),
        session_token: None,
    }))
}

async fn handle_prepare(
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<ServerState>>,
    Json(mut manifest): Json<TransferManifest>,
) -> impl IntoResponse {
    let peer_ip = peer_addr.ip().to_string();
    if manifest.sender.address.as_ref().map(|a| a.is_empty() || a == "127.0.0.1" || a == "localhost").unwrap_or(true) {
        manifest.sender.address = Some(peer_ip);
    }
    let local_fp = &state.cert_bundle.fingerprint;
    let sas_pin = compute_sas_pin(&manifest.sender.fingerprint, local_fp);

    let is_trusted = state.storage.is_auto_accept(&manifest.sender.fingerprint);

    if !is_trusted {
        let (reply_tx, reply_rx) = oneshot::channel();
        let prompt = IncomingTransferPrompt {
            manifest: manifest.clone(),
            sas_pin: sas_pin.clone(),
            reply_tx: Arc::new(parking_lot::Mutex::new(Some(reply_tx))),
        };

        if state.prompt_tx.send(prompt).is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(PrepareResponse {
                accepted: false,
                reason: Some("Receiver UI is unavailable.".to_string()),
                resume_offsets: HashMap::new(),
            }));
        }

        // Wait for user prompt response (30s timeout)
        match tokio::time::timeout(std::time::Duration::from_secs(30), reply_rx).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                return (StatusCode::FORBIDDEN, Json(PrepareResponse {
                    accepted: false,
                    reason: Some("Transfer declined by receiver.".to_string()),
                    resume_offsets: HashMap::new(),
                }));
            }
            _ => {
                return (StatusCode::REQUEST_TIMEOUT, Json(PrepareResponse {
                    accepted: false,
                    reason: Some("Transfer request timed out.".to_string()),
                    resume_offsets: HashMap::new(),
                }));
            }
        }
    }

    // Compute resume offsets for each file
    let mut resume_offsets = HashMap::new();
    let download_dir = state.download_dir.read().clone();

    for file in &manifest.files {
        let clean_name = sanitize_filename(&file.name);
        let part_file = download_dir.join(format!("{}.droplink_part", clean_name));

        if part_file.exists() {
            if let Ok(meta) = tokio::fs::metadata(&part_file).await {
                if meta.len() < file.size {
                    resume_offsets.insert(file.id.clone(), meta.len());
                } else if meta.len() > file.size {
                    let _ = tokio::fs::remove_file(&part_file).await;
                    resume_offsets.insert(file.id.clone(), 0);
                }
            }
        } else {
            resume_offsets.insert(file.id.clone(), 0);
        }
    }

    *state.active_manifest.write() = Some(manifest.clone());
    state.is_cancelled.store(false, Ordering::SeqCst);

    (StatusCode::OK, Json(PrepareResponse {
        accepted: true,
        reason: None,
        resume_offsets,
    }))
}

async fn handle_upload(
    AxPath(file_id): AxPath<String>,
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if state.is_cancelled.load(Ordering::Relaxed) {
        return Err((StatusCode::CONFLICT, "Transfer cancelled".to_string()));
    }

    let manifest = state.active_manifest.read().clone()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "No active transfer session".to_string()))?;

    let file_meta = manifest.files.iter().find(|f| f.id == file_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File ID not in manifest".to_string()))?
        .clone();

    let download_dir = state.download_dir.read().clone();
    let safe_final_path = resolve_safe_path(&download_dir, &file_meta.name)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let part_path = download_dir.join(format!("{}.droplink_part", sanitize_filename(&file_meta.name)));

    // Parse Range header for resume
    let start_offset = headers
        .get("x-droplink-offset")
        .or_else(|| headers.get("content-range"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut dest_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if start_offset > 0 {
        dest_file.seek(SeekFrom::Start(start_offset)).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        dest_file.set_len(0).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let mut stream = body.into_data_stream();
    let mut written = start_offset;

    while let Some(chunk_res) = stream.next().await {
        if state.is_cancelled.load(Ordering::Relaxed) {
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err((StatusCode::CONFLICT, "Transfer cancelled by user".to_string()));
        }

        let chunk = chunk_res.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        dest_file.write_all(&chunk).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        written += chunk.len() as u64;
        state.speed_tracker.write().record_bytes(written);
    }

    dest_file.flush().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(dest_file);

    // Verify SHA-256 integrity
    let computed_hash = compute_file_sha256(&part_path).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !file_meta.sha256.is_empty() && !computed_hash.eq_ignore_ascii_case(&file_meta.sha256) {
        let _ = tokio::fs::remove_file(&part_path).await;
        error!("Integrity hash mismatch for {}: expected {}, got {}", file_meta.name, file_meta.sha256, computed_hash);
        return Err((StatusCode::BAD_REQUEST, "File integrity hash verification failed".to_string()));
    }

    // Conflict resolution & atomic rename
    let final_dest = resolve_conflict_path(&safe_final_path);
    tokio::fs::rename(&part_path, &final_dest).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to finalize file: {}", e)))?;

    info!("Successfully received and verified: {:?}", final_dest);

    // Record in storage history
    let record = TransferRecord {
        id: Uuid::new_v4().to_string(),
        session_id: manifest.session_id,
        direction: TransferDirection::Received,
        peer_name: manifest.sender.name.clone(),
        peer_platform: manifest.sender.platform,
        file_names: vec![file_meta.name.clone()],
        total_size: file_meta.size,
        timestamp: Utc::now(),
        status: "completed".to_string(),
        save_path: Some(final_dest.to_string_lossy().to_string()),
    };
    let _ = state.storage.add_history(record);

    Ok((StatusCode::OK, "File received and verified"))
}

async fn handle_status(
    AxPath(_session_id): AxPath<String>,
    State(state): State<Arc<ServerState>>,
) -> Json<Option<TransferProgress>> {
    Json(state.active_progress.read().clone())
}

async fn handle_cancel(
    AxPath(_session_id): AxPath<String>,
    State(state): State<Arc<ServerState>>,
) -> StatusCode {
    state.is_cancelled.store(true, Ordering::SeqCst);
    StatusCode::OK
}
