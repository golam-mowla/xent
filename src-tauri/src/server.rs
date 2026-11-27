use crate::cert::TlsCertificate;
use crate::db::{ChatMessage, Database, KnownPeer};
use crate::discovery::{DiscoveryManager, Peer};
use crate::settings::{AppSettings, SettingsState};
use crate::transfer::{
    sanitize_file_name, spawn_upload, FileTransferInfo, FileTransferRequest, FileTransferResponse,
    TransferManager, TransferStatus,
};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tower_http::cors::{Any, CorsLayer};

fn default_status() -> String {
    "available".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    pub device_id: String,
    pub alias: String,
    pub device_type: String,
    pub port: u16,
    pub fingerprint: String,
    pub protocol_version: u32,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub status_message: String,
    #[serde(default)]
    pub os: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessagePayload {
    pub id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub receiver_id: String,
    pub receiver_name: String,
    pub content: String,
    pub created_at: i64,
    #[serde(default)]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingPayload {
    pub sender_id: String,
    pub sender_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPayload {
    pub peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPayload {
    pub message_id: String,
    pub new_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePayload {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionPayload {
    pub message_id: String,
    pub user_name: String,
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAck {
    pub success: bool,
    pub id: String,
    pub status: String,
    pub timestamp: i64,
}

#[derive(Debug, Deserialize)]
pub struct ChunkQuery {
    pub offset: u64,
}

#[derive(Debug, Deserialize)]
pub struct LocalSendFilesPayload {
    #[serde(default)]
    pub paths: Vec<String>,
}

pub struct PendingSendFilesState(pub std::sync::Mutex<Vec<String>>);

pub struct LocalIpcTokenState(pub String);

fn format_file_size_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub struct ServerState {
    pub settings: AppSettings,
    pub cert: TlsCertificate,
    pub db: Arc<Database>,
    pub transfer_mgr: Arc<TransferManager>,
    pub app_handle: Option<AppHandle>,
    pub settings_state: Option<Arc<SettingsState>>,
}

impl ServerState {
    /// Always read the current settings. `self.settings` is only a snapshot taken
    /// when the server started (it goes stale when the user changes the download
    /// folder, favourites, etc.).
    fn live_settings(&self) -> AppSettings {
        match self.settings_state {
            Some(ref st) => st.get_settings(),
            None => self.settings.clone(),
        }
    }

    fn is_blocked(&self, device_id: &str) -> bool {
        match self.settings_state {
            Some(ref st) => st.is_blocked(device_id),
            None => self.settings.blocked_devices.iter().any(|d| d == device_id),
        }
    }

    fn is_favorite(&self, device_id: &str) -> bool {
        match self.settings_state {
            Some(ref st) => st.is_favorite(device_id),
            None => {
                self.settings.favorite_devices.iter().any(|d| d == device_id)
                    || self.settings.trusted_devices.iter().any(|d| d == device_id)
            }
        }
    }

    fn build_info_response(&self) -> DeviceInfoResponse {
        let settings = self.live_settings();
        DeviceInfoResponse {
            device_id: settings.device_id,
            alias: settings.alias,
            device_type: settings.device_type,
            port: settings.port,
            fingerprint: self.cert.fingerprint.clone(),
            protocol_version: 1,
            status: settings.status,
            status_message: settings.status_message,
            os: std::env::consts::OS.to_string(),
        }
    }
}

pub fn build_router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(info_handler))
        .route("/api/discovery/hello", post(discovery_hello_handler))
        .route("/api/message", post(message_handler))
        .route("/api/ws", get(ws_handler))
        .route("/api/files/request", post(file_request_handler))
        .route("/api/files/response", post(file_response_handler))
        .route("/api/files/chunk/:transfer_id", post(file_chunk_handler))
        .route("/api/files/pause/:transfer_id", post(file_pause_handler))
        .route("/api/files/resume/:transfer_id", post(file_resume_handler))
        .route("/api/files/cancel/:transfer_id", post(file_cancel_handler))
        .route("/api/local/open-send-files", post(local_open_send_files_handler))
        .route("/api/message/typing", post(typing_handler))
        .route("/api/message/read", post(read_handler))
        .route("/api/message/edit", post(edit_handler))
        .route("/api/message/delete", post(delete_handler))
        .route("/api/message/reaction", post(reaction_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

pub async fn start_server(
    settings: AppSettings,
    cert: TlsCertificate,
    db: Arc<Database>,
    transfer_mgr: Arc<TransferManager>,
    app_handle: Option<AppHandle>,
    settings_state: Option<Arc<SettingsState>>,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let state = Arc::new(ServerState {
        settings: settings.clone(),
        cert: cert.clone(),
        db,
        transfer_mgr,
        app_handle,
        settings_state,
    });

    let app = build_router(state);

    let rustls_config = RustlsConfig::from_pem(
        cert.cert_pem.into_bytes(),
        cert.key_pem.into_bytes(),
    )
    .await?;

    let mut current_port = settings.port;
    for _ in 0..10 {
        let addr = SocketAddr::from(([0, 0, 0, 0], current_port));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                drop(listener);
                let app = app.clone();
                let config = rustls_config.clone();
                tokio::spawn(async move {
                    if let Err(e) = axum_server::bind_rustls(addr, config)
                        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                        .await
                    {
                        eprintln!("[Server] HTTPS server error: {}", e);
                    }
                });
                return Ok(current_port);
            }
            Err(_) => {
                current_port += 1;
            }
        }
    }

    Err(format!("Could not bind HTTPS server starting from port {}", settings.port).into())
}

async fn ping_handler() -> &'static str {
    "pong"
}

async fn info_handler(
    State(state): State<Arc<ServerState>>,
) -> Json<DeviceInfoResponse> {
    Json(state.build_info_response())
}

async fn discovery_hello_handler(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Json(remote_info): Json<DeviceInfoResponse>,
) -> Json<DeviceInfoResponse> {
    let my_info = state.build_info_response();
    if remote_info.device_id != my_info.device_id && !state.is_blocked(&remote_info.device_id) {
        let ip_raw = remote.ip().to_canonical();
        let ip = if ip_raw.is_loopback() || ip_raw.is_unspecified() {
            "127.0.0.1".to_string()
        } else {
            ip_raw.to_string()
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let peer = Peer {
            device_id: remote_info.device_id.clone(),
            alias: remote_info.alias.clone(),
            device_type: remote_info.device_type.clone(),
            ip: ip.clone(),
            port: remote_info.port,
            fingerprint: remote_info.fingerprint.clone(),
            last_seen: now,
            status: if remote_info.status.is_empty() {
                "available".to_string()
            } else {
                remote_info.status.clone()
            },
            status_message: remote_info.status_message.clone(),
            os: remote_info.os.clone(),
        };

        if let Some(ref handle) = state.app_handle {
            if let Some(mgr) = handle.try_state::<Arc<DiscoveryManager>>() {
                mgr.register_or_update_peer(peer);
            } else {
                let _ = state.db.upsert_peer(&KnownPeer {
                    device_id: peer.device_id,
                    alias: peer.alias,
                    device_type: peer.device_type,
                    os: peer.os,
                    ip: peer.ip,
                    port: peer.port,
                    fingerprint: peer.fingerprint,
                    last_seen: peer.last_seen,
                });
            }
        }
    }
    Json(my_info)
}

async fn message_handler(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Json(payload): Json<SendMessagePayload>,
) -> Json<MessageAck> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // Blocked devices get a normal-looking ack, but nothing is stored or shown.
    if state.is_blocked(&payload.sender_id) {
        return Json(MessageAck {
            success: true,
            id: payload.id,
            status: "delivered".to_string(),
            timestamp: now,
        });
    }

    let chat_msg = ChatMessage {
        id: payload.id.clone(),
        thread_peer_id: payload.sender_id.clone(),
        sender_id: payload.sender_id.clone(),
        sender_name: payload.sender_name.clone(),
        receiver_id: payload.receiver_id.clone(),
        receiver_name: payload.receiver_name.clone(),
        content: payload.content.clone(),
        status: "delivered".to_string(),
        created_at: payload.created_at,
        is_incoming: true,
        is_read: false,
        quote: payload.quote.clone(),
        is_edited: false,
        is_deleted: false,
        reactions: None,
    };

    // Ensure sender exists in persistent peer table even if discovery didn't fire yet
    let ip_raw = remote.ip().to_canonical();
    let ip_str = if ip_raw.is_loopback() || ip_raw.is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        ip_raw.to_string()
    };
    let _ = state.db.upsert_peer(&KnownPeer {
        device_id: payload.sender_id.clone(),
        alias: payload.sender_name.clone(),
        device_type: "desktop".to_string(),
        os: String::new(),
        ip: ip_str,
        port: 53317,
        fingerprint: String::new(),
        last_seen: (now / 1000).max(0) as u64,
    });

    let is_new = match state.db.save_incoming_message(&chat_msg) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[Server] Failed to save incoming message: {}", e);
            false
        }
    };

    if is_new {
        if let Some(ref handle) = state.app_handle {
            let _ = handle.emit("new_message", &chat_msg);
            if state.live_settings().desktop_notifications {
                let title = format!("New message from {}", chat_msg.sender_name);
                let body = chat_msg.content.clone();
                crate::os_integration::send_os_notification(title, body);
            }
            if let Some(window) = handle.get_webview_window("main") {
                if !window.is_focused().unwrap_or(false) {
                    let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
                }
            }
        }
    }

    Json(MessageAck {
        success: true,
        id: payload.id,
        status: "delivered".to_string(),
        timestamp: now,
    })
}

async fn typing_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<TypingPayload>,
) -> Json<serde_json::Value> {
    if state.is_blocked(&payload.sender_id) {
        return Json(serde_json::json!({ "success": true }));
    }
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("peer_typing", &payload);
    }
    Json(serde_json::json!({ "success": true }))
}

async fn read_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<ReadPayload>,
) -> Json<serde_json::Value> {
    let _ = state.db.mark_outgoing_read(&payload.peer_id);
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("thread_marked_read", &payload.peer_id);
    }
    Json(serde_json::json!({ "success": true }))
}

async fn edit_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<EditPayload>,
) -> Json<serde_json::Value> {
    let _ = state.db.edit_message(&payload.message_id, &payload.new_content);
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("message_edited", serde_json::json!({
            "message_id": payload.message_id,
            "new_content": payload.new_content
        }));
    }
    Json(serde_json::json!({ "success": true }))
}

async fn delete_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<DeletePayload>,
) -> Json<serde_json::Value> {
    let _ = state.db.delete_message(&payload.message_id);
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("message_deleted", serde_json::json!({
            "message_id": payload.message_id
        }));
    }
    Json(serde_json::json!({ "success": true }))
}

async fn reaction_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<ReactionPayload>,
) -> Json<serde_json::Value> {
    let res = state.db.add_reaction(&payload.message_id, &payload.user_name, &payload.emoji);
    let reactions_json = res.unwrap_or_default();
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("message_reaction_updated", serde_json::json!({
            "message_id": payload.message_id,
            "reactions": reactions_json
        }));
    }
    Json(serde_json::json!({ "success": true }))
}

async fn file_request_handler(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Json(req): Json<FileTransferRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Blocked: pretend it was declined, do not reveal the block.
    if state.is_blocked(&req.sender_id) {
        return Ok(Json(serde_json::json!({
            "success": true,
            "transfer_id": req.transfer_id,
            "auto_declined": true
        })));
    }

    // Idempotency: if this transfer_id already exists, return its current status
    if let Some(existing) = state.transfer_mgr.get_transfer(&req.transfer_id) {
        return Ok(Json(serde_json::json!({
            "success": true,
            "transfer_id": req.transfer_id,
            "auto_accepted": matches!(existing.status, TransferStatus::Accepted | TransferStatus::Transferring | TransferStatus::Completed)
        })));
    }

    let settings = state.live_settings();
    let download_dir = PathBuf::from(&settings.download_dir);
    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Cannot create download directory: {}", e),
        ));
    }

    // Never trust the remote file name: strip folders and illegal characters.
    let safe_name = sanitize_file_name(&req.file_name);
    let mut dest_path = download_dir.join(&safe_name);

    // Reserve the destination file right now (create_new is atomic). Before, two
    // files with the same name sent at the same time both got the same path and
    // wrote into each other, corrupting both.
    let mut counter = 0;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest_path)
        {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                counter += 1;
                let stem = std::path::Path::new(&safe_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file")
                    .to_string();
                let ext = std::path::Path::new(&safe_name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let new_name = if ext.is_empty() {
                    format!("{} ({})", stem, counter)
                } else {
                    format!("{} ({}).{}", stem, counter, ext)
                };
                dest_path = download_dir.join(new_name);
                if counter > 10_000 {
                    break;
                }
            }
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Cannot create destination file: {}", e),
                ));
            }
        }
    }

    // Where to send the answer: the address this request really came from.
    let raw_ip = remote.ip().to_canonical();
    let peer_ip = if raw_ip.is_loopback() || raw_ip.is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        raw_ip.to_string()
    };
    let peer_port = if req.sender_port != 0 {
        req.sender_port
    } else {
        state
            .app_handle
            .as_ref()
            .and_then(|h| h.try_state::<Arc<DiscoveryManager>>())
            .and_then(|d| {
                d.get_peers()
                    .into_iter()
                    .find(|p| p.device_id == req.sender_id)
                    .map(|p| p.port)
            })
            .unwrap_or(53317)
    };

    // Ensure sender exists in persistent peer table
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = state.db.upsert_peer(&KnownPeer {
        device_id: req.sender_id.clone(),
        alias: req.sender_name.clone(),
        device_type: "desktop".to_string(),
        os: String::new(),
        ip: peer_ip.clone(),
        port: peer_port,
        fingerprint: String::new(),
        last_seen: now_secs,
    });

    // Favourites (or "accept everything") skip the permission prompt.
    let auto_accept = settings.auto_accept || state.is_favorite(&req.sender_id);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let transfer_info = FileTransferInfo {
        transfer_id: req.transfer_id.clone(),
        peer_id: req.sender_id.clone(),
        sender_id: req.sender_id.clone(),
        sender_name: req.sender_name.clone(),
        receiver_id: req.receiver_id.clone(),
        receiver_name: req.receiver_name.clone(),
        file_name: safe_name.clone(),
        file_size: req.file_size,
        transferred_bytes: 0,
        speed_mbps: 0.0,
        status: if auto_accept {
            TransferStatus::Accepted
        } else {
            TransferStatus::Pending
        },
        is_incoming: true,
        file_path: Some(dest_path.to_string_lossy().to_string()),
        peer_ip,
        peer_port,
        created_at: now_ms,
    };

    state.transfer_mgr.add_transfer(transfer_info);

    if let Some(ref handle) = state.app_handle {
        if settings.desktop_notifications {
            let verb = if auto_accept { "Receiving file" } else { "Incoming file offer" };
            let title = format!("{} from {}", verb, req.sender_name);
            let body = format!("{} ({})", safe_name, format_file_size_label(req.file_size));
            crate::os_integration::send_os_notification(title, body);
        }
        if let Some(window) = handle.get_webview_window("main") {
            if !window.is_focused().unwrap_or(false) {
                let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "transfer_id": req.transfer_id,
        "auto_accepted": auto_accept
    })))
}

async fn file_response_handler(
    State(state): State<Arc<ServerState>>,
    Json(res): Json<FileTransferResponse>,
) -> Json<serde_json::Value> {
    let is_outgoing = state
        .transfer_mgr
        .get_transfer(&res.transfer_id)
        .map(|t| !t.is_incoming)
        .unwrap_or(false);
    if !is_outgoing {
        return Json(serde_json::json!({ "success": false, "error": "unknown transfer" }));
    }

    state.transfer_mgr.handle_response(&res.transfer_id, res.accepted);

    if res.accepted {
        if let Err(e) = spawn_upload(state.transfer_mgr.clone(), res.transfer_id.clone()) {
            eprintln!("[Server] Failed to start upload: {}", e);
        }
    }
    Json(serde_json::json!({ "success": true }))
}

async fn file_chunk_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
    Query(query): Query<ChunkQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mgr = state.transfer_mgr.clone();
    let offset = query.offset;
    let updated = tokio::task::spawn_blocking(move || mgr.write_incoming_chunk(&transfer_id, offset, &body))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({
        "success": true,
        "transferred": updated.transferred_bytes
    })))
}

async fn file_pause_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
) -> Json<serde_json::Value> {
    state.transfer_mgr.pause(&transfer_id);
    Json(serde_json::json!({ "success": true }))
}

async fn file_resume_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
) -> Json<serde_json::Value> {
    state.transfer_mgr.resume(&transfer_id);
    Json(serde_json::json!({ "success": true }))
}

async fn file_cancel_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
) -> Json<serde_json::Value> {
    state.transfer_mgr.cancel(&transfer_id);
    Json(serde_json::json!({ "success": true }))
}

async fn local_open_send_files_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(payload): Json<LocalSendFilesPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let handle = state
        .app_handle
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Headless instance".to_string()))?;

    let token_state = handle
        .try_state::<Arc<LocalIpcTokenState>>()
        .ok_or((StatusCode::UNAUTHORIZED, "Missing IPC token".to_string()))?;

    let supplied = headers
        .get("x-xent-local-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !crate::os_integration::constant_time_eq(supplied, &token_state.0) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid IPC token".to_string()));
    }

    let expanded = crate::os_integration::expand_file_paths(payload.paths);
    if !expanded.is_empty() {
        if let Some(pending) = handle.try_state::<Arc<PendingSendFilesState>>() {
            if let Ok(mut lock) = pending.0.lock() {
                lock.extend(expanded.clone());
            }
        }
        let _ = handle.emit("open_send_files", expanded);
    }

    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(_state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|mut socket| async move {
        while let Some(Ok(msg)) = socket.recv().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                if text == "ping" {
                    let _ = socket.send(axum::extract::ws::Message::Text("pong".to_string())).await;
                }
            }
        }
    })
}
