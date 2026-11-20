use crate::db::Database;
use crate::discovery::DiscoveryManager;
use crate::settings::SettingsState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Notify, Semaphore};
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
    /// Where the other device can be reached for this transfer.
    /// Stored per transfer so the flow never depends on the discovery list.
    #[serde(default)]
    pub peer_ip: String,
    #[serde(default)]
    pub peer_port: u16,
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
    /// Port of the sender's Xent server (0 = unknown / older client).
    #[serde(default)]
    pub sender_port: u16,
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
    uploads_started: Arc<RwLock<HashSet<String>>>,
    /// Last time a progress event was sent to the UI, per transfer (throttling).
    last_emit: Arc<Mutex<HashMap<String, Instant>>>,
    /// Limits how many files upload at the same time, so chat messages and
    /// discovery packets are never starved by a pile of big files.
    upload_slots: Arc<Semaphore>,
    app_handle: Option<AppHandle>,
    db: Option<Arc<Database>>,
}

/// Strip any directory part and characters that are illegal in file names,
/// so a remote device can never write outside the download folder.
pub fn sanitize_file_name(name: &str) -> String {
    let last = name.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let mut cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.chars().count() > 200 {
        cleaned = cleaned.chars().take(200).collect();
    }
    if cleaned.is_empty() {
        return "file".to_string();
    }
    let stem_upper = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let reserved = matches!(stem_upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem_upper.len() == 4
            && (stem_upper.starts_with("COM") || stem_upper.starts_with("LPT"))
            && stem_upper.chars().last().map_or(false, |c| c.is_ascii_digit()));
    if reserved {
        cleaned = format!("_{}", cleaned);
    }
    cleaned
}

impl TransferManager {
    pub fn new(app_handle: Option<AppHandle>, db: Option<Arc<Database>>) -> Self {
        Self {
            transfers: Arc::new(RwLock::new(HashMap::new())),
            cancellations: Arc::new(RwLock::new(HashMap::new())),
            response_notifiers: Arc::new(RwLock::new(HashMap::new())),
            response_results: Arc::new(RwLock::new(HashMap::new())),
            uploads_started: Arc::new(RwLock::new(HashSet::new())),
            last_emit: Arc::new(Mutex::new(HashMap::new())),
            upload_slots: Arc::new(Semaphore::new(MAX_PARALLEL_UPLOADS)),
            app_handle,
            db,
        }
    }

    pub fn upload_slots(&self) -> Arc<Semaphore> {
        self.upload_slots.clone()
    }

    /// True at most ~5 times per second per transfer. Progress events used to be
    /// sent for every 512 KB chunk, which made the UI rebuild itself so often
    /// that typing and sending messages felt frozen during a transfer.
    fn throttle_ok(&self, transfer_id: &str) -> bool {
        let mut map = self.last_emit.lock().unwrap();
        let now = Instant::now();
        match map.get(transfer_id) {
            Some(last) if now.duration_since(*last) < Duration::from_millis(200) => false,
            _ => {
                map.insert(transfer_id.to_string(), now);
                true
            }
        }
    }

    /// An incoming file that was declined or cancelled leaves an empty/partial
    /// file behind. Remove it.
    fn cleanup_partial(t: &FileTransferInfo) {
        if t.is_incoming {
            if let Some(ref p) = t.file_path {
                let _ = std::fs::remove_file(p);
            }
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
                local_path: t.file_path.clone(),
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
            // A late cancel must never turn a finished transfer into a cancelled one.
            if t.status == TransferStatus::Completed {
                return;
            }
            t.status = TransferStatus::Cancelled;
            Self::cleanup_partial(t);
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
                Self::cleanup_partial(t);
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
            if t.status == TransferStatus::Cancelled
                && matches!(status, TransferStatus::Transferring | TransferStatus::Accepted)
            {
                return;
            }
            // Always tell the UI about state changes; throttle plain progress ticks.
            let is_tick = status == TransferStatus::Transferring && t.status == TransferStatus::Transferring;
            t.transferred_bytes = transferred_bytes;
            t.speed_mbps = speed_mbps;
            t.status = status;
            if !is_tick || self.throttle_ok(transfer_id) {
                if let Some(ref handle) = self.app_handle {
                    let _ = handle.emit("file_transfer_progress", t.clone());
                }
            }
            self.maybe_record_history(t);
        }
    }

    /// Returns true only the first time it is called for a transfer,
    /// so an upload can never be started twice.
    pub fn try_begin_upload(&self, transfer_id: &str) -> bool {
        self.uploads_started
            .write()
            .unwrap()
            .insert(transfer_id.to_string())
    }

    pub fn mark_failed(&self, transfer_id: &str, reason: &str) {
        let sent = self
            .get_transfer(transfer_id)
            .map(|t| t.transferred_bytes)
            .unwrap_or(0);
        self.update_progress(transfer_id, sent, 0.0, TransferStatus::Failed(reason.to_string()));
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
        let (path_str, just_completed) = {
            let mut lock = self.transfers.write().unwrap();
            let t = lock
                .get_mut(transfer_id)
                .ok_or_else(|| "Transfer not found".to_string())?;

            match t.status {
                TransferStatus::Accepted | TransferStatus::Transferring | TransferStatus::Completed => {}
                TransferStatus::Cancelled => return Err("Transfer cancelled".to_string()),
                _ => return Err("Transfer was not accepted".to_string()),
            }
            if !t.is_incoming {
                return Err("Not an incoming transfer".to_string());
            }

            let p = t
                .file_path
                .as_ref()
                .ok_or_else(|| "Destination file path not set".to_string())?
                .clone();

            let was_completed = t.status == TransferStatus::Completed;
            t.transferred_bytes = t.transferred_bytes.max(offset + data.len() as u64);
            t.status = if t.transferred_bytes >= t.file_size {
                TransferStatus::Completed
            } else {
                TransferStatus::Transferring
            };

            (p, t.status == TransferStatus::Completed && !was_completed)
        };

        if let Some(parent) = std::path::Path::new(&path_str).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path_str)
            .map_err(|e| format!("Failed to open destination file: {}", e))?;

        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("Failed to seek in destination file: {}", e))?;

        file.write_all(data)
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        let updated = self
            .get_transfer(transfer_id)
            .ok_or_else(|| "Transfer not found".to_string())?;
        if just_completed || self.throttle_ok(transfer_id) {
            if let Some(ref handle) = self.app_handle {
                let _ = handle.emit("file_transfer_progress", updated.clone());
            }
        }
        if just_completed {
            // Incoming files used to be missing from the history.
            self.maybe_record_history(&updated);
        }

        Ok(updated)
    }
}

// Commands
#[tauri::command]
pub async fn pick_file_to_send() -> Option<String> {
    #[cfg(not(target_os = "android"))]
    {
        let file = rfd::AsyncFileDialog::new().pick_file().await;
        return file.map(|f| f.path().to_string_lossy().to_string());
    }
    #[cfg(target_os = "android")]
    {
        None
    }
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
        .ok_or_else(|| "Device not found or offline".to_string())?
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
        peer_ip: peer.ip.clone(),
        peer_port: peer.port,
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
        sender_port: my_settings.port,
    };

    let url = format!("https://{}:{}/api/files/request", peer.ip, peer.port);
    let res = match client.post(&url).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => {
            // Previously this failed silently and the card stayed on "waiting" forever.
            transfer_mgr.mark_failed(&transfer_id, "Could not reach the other device");
            return Err(format!("Could not reach {}: {}", peer.alias, e));
        }
    };

    if !res.status().is_success() {
        let code = res.status();
        transfer_mgr.mark_failed(&transfer_id, "The other device rejected the request");
        return Err(format!("{} rejected the request ({})", peer.alias, code));
    }

    // Favourite devices (or "accept everything") answer right away.
    let ack: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
    let flag = |name: &str| ack.get(name).and_then(|v| v.as_bool()).unwrap_or(false);

    if flag("auto_declined") {
        transfer_mgr.handle_response(&transfer_id, false);
    } else if flag("auto_accepted") {
        transfer_mgr.handle_response(&transfer_id, true);
        spawn_upload(transfer_mgr.inner().clone(), transfer_id.clone())?;
    }

    Ok(transfer_mgr.get_transfer(&transfer_id).unwrap_or(transfer_info))
}

#[tauri::command]
pub async fn respond_file_transfer(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    transfer_id: String,
    accepted: bool,
) -> Result<(), String> {
    let transfer = transfer_mgr
        .get_transfer(&transfer_id)
        .ok_or_else(|| "Transfer not found".to_string())?;

    if !transfer.is_incoming {
        return Err("This is not an incoming transfer".to_string());
    }
    if transfer.peer_ip.is_empty() || transfer.peer_port == 0 {
        return Err("Sender address unknown".to_string());
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp_body = FileTransferResponse {
        transfer_id: transfer_id.clone(),
        accepted,
    };

    let url = format!(
        "https://{}:{}/api/files/response",
        transfer.peer_ip, transfer.peer_port
    );

    // Only mark the transfer accepted locally if the sender really got the answer.
    // Before, a failed request was ignored: this side showed "Accepted" while
    // the sender stayed on "Waiting..." forever.
    match client.post(&url).json(&resp_body).send().await {
        Ok(r) if r.status().is_success() => {
            transfer_mgr.handle_response(&transfer_id, accepted);
            Ok(())
        }
        Ok(r) => Err(format!("Sender answered with status {}", r.status())),
        Err(e) => Err(format!(
            "Could not reach {}: {}",
            transfer.sender_name, e
        )),
    }
}

/// Kept for compatibility. The sender's backend now starts the upload by itself
/// as soon as the receiver accepts, so this is safe to call but not required.
#[tauri::command]
pub async fn start_file_upload(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    transfer_id: String,
) -> Result<(), String> {
    spawn_upload(transfer_mgr.inner().clone(), transfer_id)
}

const UPLOAD_CHUNK_SIZE: usize = 512 * 1024;
const MAX_PARALLEL_UPLOADS: usize = 2;
const UPLOAD_CHUNK_RETRIES: u32 = 3;

/// Fill `buf` as far as possible (a plain read can return fewer bytes).
async fn read_full(file: &mut tokio::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use tokio::io::AsyncReadExt;
    let mut n = 0;
    while n < buf.len() {
        let r = file.read(&mut buf[n..]).await?;
        if r == 0 {
            break;
        }
        n += r;
    }
    Ok(n)
}

async fn post_chunk(
    client: &reqwest::Client,
    base: &str,
    transfer_id: &str,
    offset: u64,
    data: Vec<u8>,
) -> Result<(), String> {
    let url = format!("{}/api/files/chunk/{}?offset={}", base, transfer_id, offset);
    let res = client
        .post(&url)
        .body(data)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    if res.status().is_success() {
        return Ok(());
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    Err(if body.trim().is_empty() {
        format!("Receiver rejected the data ({})", status)
    } else {
        body
    })
}

/// Starts uploading an accepted outgoing transfer in the background.
/// Idempotent: calling it again for the same transfer does nothing.
pub fn spawn_upload(mgr: Arc<TransferManager>, transfer_id: String) -> Result<(), String> {
    let transfer = mgr
        .get_transfer(&transfer_id)
        .ok_or_else(|| "Transfer not found".to_string())?;

    if transfer.is_incoming {
        return Err("Cannot upload an incoming transfer".to_string());
    }
    if !mgr.try_begin_upload(&transfer_id) {
        return Ok(());
    }

    let file_path = match transfer.file_path.clone() {
        Some(p) => p,
        None => {
            mgr.mark_failed(&transfer_id, "Source file path missing");
            return Err("Source file path missing".to_string());
        }
    };
    if transfer.peer_ip.is_empty() || transfer.peer_port == 0 {
        mgr.mark_failed(&transfer_id, "Receiver address unknown");
        return Err("Receiver address unknown".to_string());
    }

    let cancel_flag = mgr.register_cancellation(&transfer_id);
    let base = format!("https://{}:{}", transfer.peer_ip, transfer.peer_port);
    let file_size = transfer.file_size;

    tokio::spawn(async move {
        let t_id = transfer_id;

        // Wait for a free upload slot (max 2 files at once).
        let _slot = match mgr.upload_slots().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };
        if cancel_flag.load(Ordering::SeqCst) {
            return;
        }

        let mut file = match tokio::fs::File::open(&file_path).await {
            Ok(f) => f,
            Err(e) => {
                mgr.mark_failed(&t_id, &format!("Cannot open file: {}", e));
                return;
            }
        };

        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                mgr.mark_failed(&t_id, &e.to_string());
                return;
            }
        };

        mgr.update_progress(&t_id, 0, 0.0, TransferStatus::Transferring);

        // An empty file has no chunks: send one empty chunk so the receiver creates it.
        if file_size == 0 {
            match post_chunk(&client, &base, &t_id, 0, Vec::new()).await {
                Ok(()) => mgr.update_progress(&t_id, 0, 0.0, TransferStatus::Completed),
                Err(e) => mgr.mark_failed(&t_id, &e),
            }
            return;
        }

        let mut buffer = vec![0u8; UPLOAD_CHUNK_SIZE];
        let mut offset: u64 = 0;
        let start_time = Instant::now();

        loop {
            if cancel_flag.load(Ordering::SeqCst) {
                let _ = client
                    .post(format!("{}/api/files/cancel/{}", base, t_id))
                    .send()
                    .await;
                mgr.update_progress(&t_id, offset, 0.0, TransferStatus::Cancelled);
                return;
            }

            let bytes_read = match read_full(&mut file, &mut buffer).await {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(e) => {
                    mgr.mark_failed(&t_id, &format!("Read error: {}", e));
                    return;
                }
            };

            let chunk = buffer[..bytes_read].to_vec();
            let mut attempt: u32 = 0;
            loop {
                match post_chunk(&client, &base, &t_id, offset, chunk.clone()).await {
                    Ok(()) => break,
                    Err(e) => {
                        attempt += 1;
                        if attempt >= UPLOAD_CHUNK_RETRIES || cancel_flag.load(Ordering::SeqCst) {
                            mgr.mark_failed(&t_id, &e);
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(400 * attempt as u64)).await;
                    }
                }
            }

            offset += bytes_read as u64;
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.1 {
                (offset as f64 / 1_048_576.0) / elapsed
            } else {
                0.0
            };
            let status = if offset >= file_size {
                TransferStatus::Completed
            } else {
                TransferStatus::Transferring
            };
            mgr.update_progress(&t_id, offset, speed, status);
        }

        if offset < file_size {
            mgr.mark_failed(&t_id, "File changed while sending");
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_file_transfer(
    transfer_mgr: State<'_, Arc<TransferManager>>,
    transfer_id: String,
) -> Result<(), String> {
    if let Some(t) = transfer_mgr.get_transfer(&transfer_id) {
        if !t.peer_ip.is_empty() && t.peer_port != 0 {
            if let Ok(c) = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(3))
                .build()
            {
                let _ = c
                    .post(format!(
                        "https://{}:{}/api/files/cancel/{}",
                        t.peer_ip, t.peer_port, transfer_id
                    ))
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

#[tauri::command]
pub async fn pick_files_to_send() -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "android"))]
    {
        let files = rfd::AsyncFileDialog::new().pick_files().await;
        return match files {
            Some(handles) => Ok(handles
                .into_iter()
                .map(|h| h.path().to_string_lossy().to_string())
                .collect()),
            None => Ok(Vec::new()),
        };
    }
    #[cfg(target_os = "android")]
    {
        Err("Sending files from Android needs the Android file picker (not added yet)".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedFileMeta {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[tauri::command]
pub async fn pick_folder_to_send() -> Result<Vec<String>, String> {
    #[cfg(target_os = "android")]
    {
        Err("Folder sending is not available on Android".to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        let folder = rfd::AsyncFileDialog::new().pick_folder().await;
        let mut files = Vec::new();
        if let Some(h) = folder {
            let root = h.path();
            if root.is_dir() {
                fn collect_files(dir: &std::path::Path, acc: &mut Vec<String>) {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_file() {
                                acc.push(path.to_string_lossy().to_string());
                            } else if path.is_dir() {
                                collect_files(&path, acc);
                            }
                        }
                    }
                }
                collect_files(root, &mut files);
            }
        }
        Ok(files)
    }
}

#[tauri::command]
pub fn get_files_metadata(paths: Vec<String>) -> Vec<SelectedFileMeta> {
    paths
        .into_iter()
        .filter_map(|p| {
            let pb = std::path::PathBuf::from(&p);
            if pb.is_file() {
                let size = std::fs::metadata(&pb).map(|m| m.len()).unwrap_or(0);
                let name = pb.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
                Some(SelectedFileMeta { path: p, name, size })
            } else {
                None
            }
        })
        .collect()
}

#[tauri::command]
pub async fn pick_download_dir(
    settings_state: State<'_, Arc<SettingsState>>,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        let _ = settings_state;
        return Err("Changing the download folder is not available on Android".to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let folder = rfd::AsyncFileDialog::new().pick_folder().await;
        if let Some(h) = folder {
            let path = h.path().to_string_lossy().to_string();
            settings_state.update_download_dir(path.clone())?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("File not found (moved or deleted)".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        // `cmd /c start` breaks on names containing & ^ % and similar characters.
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "android")]
    {
        let _ = path;
    }
    Ok(())
}

#[tauri::command]
pub fn show_in_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("File not found (moved or deleted)".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        // The quotes must reach explorer.exe unchanged, so use raw_arg.
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        let dir = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "android")]
    {
        let _ = path;
    }
    Ok(())
}

#[tauri::command]
pub fn save_clipboard_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("xent_paste_{}.{}", Uuid::new_v4(), ext);
    let path = temp_dir.join(file_name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_chat_timeline(
    db: State<'_, Arc<Database>>,
    peer_id: String,
    limit: Option<usize>,
) -> Result<Vec<crate::db::TimelineItem>, String> {
    let lim = limit.unwrap_or(200);
    db.get_timeline(&peer_id, lim).map_err(|e| e.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_paths() {
        assert_eq!(sanitize_file_name("..\\..\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("C:\\x\\photo.png"), "photo.png");
    }

    #[test]
    fn sanitize_handles_bad_names() {
        assert_eq!(sanitize_file_name(""), "file");
        assert_eq!(sanitize_file_name("..."), "file");
        assert_eq!(sanitize_file_name("a<b>c?.txt"), "abc.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("com1"), "_com1");
        assert_eq!(sanitize_file_name("report final.pdf"), "report final.pdf");
    }

    fn mgr() -> TransferManager {
        TransferManager::new(None, None)
    }

    fn incoming(status: TransferStatus, path: &str, size: u64) -> FileTransferInfo {
        FileTransferInfo {
            transfer_id: "t1".into(),
            peer_id: "p".into(),
            sender_id: "p".into(),
            sender_name: "Sender".into(),
            receiver_id: "me".into(),
            receiver_name: "Me".into(),
            file_name: "f.bin".into(),
            file_size: size,
            transferred_bytes: 0,
            speed_mbps: 0.0,
            status,
            is_incoming: true,
            file_path: Some(path.into()),
            peer_ip: "127.0.0.1".into(),
            peer_port: 1,
        }
    }

    #[test]
    fn chunks_rejected_until_accepted() {
        let m = mgr();
        let mut path = std::env::temp_dir();
        path.push(format!("xent_test_{}.bin", Uuid::new_v4()));
        let p = path.to_string_lossy().to_string();
        m.add_transfer(incoming(TransferStatus::Pending, &p, 4));
        assert!(m.write_incoming_chunk("t1", 0, b"abcd").is_err());
        assert!(!path.exists());
        m.handle_response("t1", true);
        let t = m.write_incoming_chunk("t1", 0, b"abcd").unwrap();
        assert_eq!(t.status, TransferStatus::Completed);
        assert_eq!(std::fs::read(&path).unwrap(), b"abcd");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_file_completes() {
        let m = mgr();
        let mut path = std::env::temp_dir();
        path.push(format!("xent_test_{}.bin", Uuid::new_v4()));
        let p = path.to_string_lossy().to_string();
        m.add_transfer(incoming(TransferStatus::Accepted, &p, 0));
        let t = m.write_incoming_chunk("t1", 0, b"").unwrap();
        assert_eq!(t.status, TransferStatus::Completed);
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn upload_starts_only_once() {
        let m = mgr();
        assert!(m.try_begin_upload("x"));
        assert!(!m.try_begin_upload("x"));
    }
}
