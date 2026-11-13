use crate::db::Database;
use crate::discovery::DiscoveryManager;
use crate::settings::SettingsState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    Pending,
    Accepted,
    Declined,
    Transferring,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferInfo {
    pub transfer_id: String,
    pub peer_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub receiver_id: String,
    pub receiver_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub transferred_bytes: u64,
    pub speed_mbps: f64,
    pub status: TransferStatus,
    pub is_incoming: bool,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferRequest {
    pub transfer_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub receiver_id: String,
    pub receiver_name: String,
    pub file_name: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferResponse {
    pub transfer_id: String,
    pub accepted: bool,
}

pub struct TransferManager {
    transfers: Arc<RwLock<HashMap<String, FileTransferInfo>>>,
    cancellations: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
    response_notifiers: Arc<RwLock<HashMap<String, Arc<Notify>>>>,
    response_results: Arc<RwLock<HashMap<String, bool>>>,
    app_handle: Option<AppHandle>,
    db: Option<Arc<Database>>,
}

impl TransferManager {
    pub fn new(app_handle: Option<AppHandle>, db: Option<Arc<Database>>) -> Self {
        Self {
            transfers: Arc::new(RwLock::new(HashMap::new())),
            cancellations: Arc::new(RwLock::new(HashMap::new())),
            response_notifiers: Arc::new(RwLock::new(HashMap::new())),
            response_results: Arc::new(RwLock::new(HashMap::new())),
            app_handle,
            db,
        }
    }

    fn maybe_record_history(&self, t: &FileTransferInfo) {
        if let Some(ref db) = self.db {
            let status_str = match &t.status {
                TransferStatus::Completed => "completed",
                TransferStatus::Declined => "declined",
                TransferStatus::Cancelled => "cancelled",
                TransferStatus::Failed(_) => "failed",
                _ => return,
            };
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let record = crate::db::TransferRecord {
                id: t.transfer_id.clone(),
                peer_id: t.peer_id.clone(),
                sender_name: t.sender_name.clone(),
                receiver_name: t.receiver_name.clone(),
                file_name: t.file_name.clone(),
                file_size: t.file_size,
                status: status_str.to_string(),
                created_at: now_ms,
            };
            let _ = db.save_transfer(&record);
        }
    }

    pub fn register_cancellation(&self, transfer_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancellations
            .write()
            .unwrap()
            .insert(transfer_id.to_string(), flag.clone());
        flag
    }

    pub fn cancel(&self, transfer_id: &str) {
        if let Some(flag) = self.cancellations.read().unwrap().get(transfer_id) {
            flag.store(true, Ordering::SeqCst);
        }
        let mut lock = self.transfers.write().unwrap();
        if let Some(t) = lock.get_mut(transfer_id) {
            t.status = TransferStatus::Cancelled;
            if let Some(ref handle) = self.app_handle {
                let _ = handle.emit("file_transfer_progress", t.clone());
            }
            self.maybe_record_history(t);
        }
    }

    pub fn register_response_waiter(&self, transfer_id: &str) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.response_notifiers
            .write()
            .unwrap()
            .insert(transfer_id.to_string(), notify.clone());
        notify
    }

    pub fn handle_response(&self, transfer_id: &str, accepted: bool) {
        self.response_results
            .write()
            .unwrap()
            .insert(transfer_id.to_string(), accepted);

        let mut lock = self.transfers.write().unwrap();
        if let Some(t) = lock.get_mut(transfer_id) {
            t.status = if accepted {
                TransferStatus::Accepted
            } else {
                TransferStatus::Declined
            };
            if let Some(ref handle) = self.app_handle {
                let _ = handle.emit("file_transfer_progress", t.clone());
            }
            if !accepted {
                self.maybe_record_history(t);
            }
        }

        if let Some(notify) = self.response_notifiers.read().unwrap().get(transfer_id) {
            notify.notify_waiters();
        }
    }

    pub fn get_response(&self, transfer_id: &str) -> Option<bool> {
        self.response_results.read().unwrap().get(transfer_id).copied()
    }

    pub fn add_transfer(&self, info: FileTransferInfo) {
        let mut lock = self.transfers.write().unwrap();
        lock.insert(info.transfer_id.clone(), info.clone());
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("file_transfer_progress", info);
        }
    }

    pub fn update_progress(
        &self,
        transfer_id: &str,
        transferred_bytes: u64,
        speed_mbps: f64,
        status: TransferStatus,
    ) {
        let mut lock = self.transfers.write().unwrap();
        if let Some(t) = lock.get_mut(transfer_id) {
            t.transferred_bytes = transferred_bytes;
            t.speed_mbps = speed_mbps;
            t.status = status;
            if let Some(ref handle) = self.app_handle {
                let _ = handle.emit("file_transfer_progress", t.clone());
            }
            self.maybe_record_history(t);
        }
    }

    pub fn get_transfer(&self, transfer_id: &str) -> Option<FileTransferInfo> {
        self.transfers.read().unwrap().get(transfer_id).cloned()
    }

    pub fn write_incoming_chunk(
        &self,
        transfer_id: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<FileTransferInfo, String> {
        let path_str = {
            let mut lock = self.transfers.write().unwrap();
            let t = lock
                .get_mut(transfer_id)
                .ok_or_else(|| "Transfer not found".to_string())?;

            if t.status == TransferStatus::Cancelled {
                return Err("Transfer cancelled".to_string());
            }

            let p = t
                .file_path
                .as_ref()
                .ok_or_else(|| "Destination file path not set".to_string())?
                .clone();

            t.transferred_bytes = offset + data.len() as u64;
            t.status = if t.transferred_bytes >= t.file_size {
                TransferStatus::Completed
            } else {
                TransferStatus::Transferring
            };

            p
        };

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path_str)
            .map_err(|e| format!("Failed to open destination file: {}", e))?;

        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("Failed to seek in destination file: {}", e))?;

        file.write_all(data)
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        let updated = self.get_transfer(transfer_id).unwrap();
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("file_transfer_progress", updated.clone());
        }

        Ok(updated)
    }
}

// Commands
#[tauri::command]
pub async fn pick_file_to_send() -> Option<String> {
    let file = rfd::AsyncFileDialog::new().pick_file().await;
    file.map(|f| f.path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn send_file_request(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    settings_state: State<'_, Arc<SettingsState>>,
    peer_id: String,
    file_path: String,
) -> Result<FileTransferInfo, String> {
    let path = PathBuf::from(&file_path);
    if !path.exists() || !path.is_file() {
        return Err("Selected file does not exist or is not a valid file".to_string());
    }

    let file_meta = std::fs::metadata(&path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    let file_size = file_meta.len();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let my_settings = settings_state.get_settings();
    let peers = discovery_mgr.get_peers();
    let peer = peers
        .iter()
        .find(|p| p.device_id == peer_id)
        .ok_or_else(|| "Peer not found or offline".to_string())?
        .clone();

    let transfer_id = Uuid::new_v4().to_string();

    let transfer_info = FileTransferInfo {
        transfer_id: transfer_id.clone(),
        peer_id: peer.device_id.clone(),
        sender_id: my_settings.device_id.clone(),
        sender_name: my_settings.alias.clone(),
        receiver_id: peer.device_id.clone(),
        receiver_name: peer.alias.clone(),
        file_name: file_name.clone(),
        file_size,
        transferred_bytes: 0,
        speed_mbps: 0.0,
        status: TransferStatus::Pending,
        is_incoming: false,
        file_path: Some(file_path),
    };

    transfer_mgr.add_transfer(transfer_info.clone());

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let req_body = FileTransferRequest {
        transfer_id: transfer_id.clone(),
        sender_id: my_settings.device_id,
        sender_name: my_settings.alias,
        receiver_id: peer.device_id.clone(),
        receiver_name: peer.alias.clone(),
        file_name,
        file_size,
    };

    let url = format!("https://{}:{}/api/files/request", peer.ip, peer.port);
    let res = client
        .post(&url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send file request to peer: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Peer rejected request with status: {}", res.status()));
    }

    Ok(transfer_info)
}

#[tauri::command]
pub async fn respond_file_transfer(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    transfer_id: String,
    accepted: bool,
) -> Result<(), String> {
    let transfer = transfer_mgr
        .get_transfer(&transfer_id)
        .ok_or_else(|| "Transfer not found".to_string())?;

    let peers = discovery_mgr.get_peers();
    let peer = peers
        .iter()
        .find(|p| p.device_id == transfer.peer_id)
        .ok_or_else(|| "Sender peer is offline".to_string())?;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp_body = FileTransferResponse {
        transfer_id: transfer_id.clone(),
        accepted,
    };

    let url = format!("https://{}:{}/api/files/response", peer.ip, peer.port);
    let _ = client.post(&url).json(&resp_body).send().await;

    transfer_mgr.handle_response(&transfer_id, accepted);
    Ok(())
}

#[tauri::command]
pub async fn start_file_upload(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    transfer_id: String,
) -> Result<(), String> {
    let transfer = transfer_mgr
        .get_transfer(&transfer_id)
        .ok_or_else(|| "Transfer not found".to_string())?;

    let file_path_str = transfer
        .file_path
        .clone()
        .ok_or_else(|| "Source file path missing".to_string())?;

    let peers = discovery_mgr.get_peers();
    let peer = peers
        .iter()
        .find(|p| p.device_id == transfer.peer_id)
        .ok_or_else(|| "Receiver peer is offline".to_string())?
        .clone();

    let cancel_flag = transfer_mgr.register_cancellation(&transfer_id);
    let mgr_clone = transfer_mgr.inner().clone();
    let t_id = transfer_id.clone();

    tokio::spawn(async move {
        let mut file = match File::open(&file_path_str) {
            Ok(f) => f,
            Err(e) => {
                mgr_clone.update_progress(&t_id, 0, 0.0, TransferStatus::Failed(e.to_string()));
                return;
            }
        };

        let chunk_size: usize = 512 * 1024; // 512 KB chunks
        let mut buffer = vec![0u8; chunk_size];
        let mut offset: u64 = 0;
        let start_time = Instant::now();

        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                mgr_clone.update_progress(&t_id, 0, 0.0, TransferStatus::Failed(e.to_string()));
                return;
            }
        };

        mgr_clone.update_progress(&t_id, 0, 0.0, TransferStatus::Transferring);

        loop {
            if cancel_flag.load(Ordering::SeqCst) {
                let _ = client
                    .post(format!("https://{}:{}/api/files/cancel/{}", peer.ip, peer.port, t_id))
                    .send()
                    .await;
                mgr_clone.update_progress(&t_id, offset, 0.0, TransferStatus::Cancelled);
                break;
            }

            let bytes_read = match file.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(e) => {
                    mgr_clone.update_progress(&t_id, offset, 0.0, TransferStatus::Failed(e.to_string()));
                    return;
                }
            };

            let chunk_data = buffer[..bytes_read].to_vec();
            let url = format!(
                "https://{}:{}/api/files/chunk/{}?offset={}",
                peer.ip, peer.port, t_id, offset
            );

            let res = client
                .post(&url)
                .body(chunk_data)
                .send()
                .await;

            match res {
                Ok(r) if r.status().is_success() => {
                    offset += bytes_read as u64;
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.1 {
                        (offset as f64 / 1_048_576.0) / elapsed
                    } else {
                        0.0
                    };

                    let is_done = offset >= transfer.file_size;
                    let status = if is_done {
                        TransferStatus::Completed
                    } else {
                        TransferStatus::Transferring
                    };

                    mgr_clone.update_progress(&t_id, offset, speed, status);
                }
                _ => {
                    mgr_clone.update_progress(
                        &t_id,
                        offset,
                        0.0,
                        TransferStatus::Failed("Failed to send chunk".to_string()),
                    );
                    return;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_file_transfer(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    transfer_id: String,
) -> Result<(), String> {
    if let Some(t) = transfer_mgr.get_transfer(&transfer_id) {
        let peers = discovery_mgr.get_peers();
        if let Some(peer) = peers.iter().find(|p| p.device_id == t.peer_id) {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .ok();
            if let Some(c) = client {
                let _ = c
                    .post(format!("https://{}:{}/api/files/cancel/{}", peer.ip, peer.port, transfer_id))
                    .send()
                    .await;
            }
        }
    }
    transfer_mgr.cancel(&transfer_id);
    Ok(())
}

#[tauri::command]
pub fn get_transfer_history(
    db: State<'_, Arc<Database>>,
    limit: Option<usize>,
) -> Result<Vec<crate::db::TransferRecord>, String> {
    let lim = limit.unwrap_or(100);
    db.get_transfers(lim).map_err(|e| e.to_string())
}
