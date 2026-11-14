use crate::cert::TlsCertificate;
use crate::db::{ChatMessage, Database};
use crate::settings::{AppSettings, SettingsState};
use crate::transfer::{FileTransferInfo, FileTransferRequest, FileTransferResponse, TransferManager, TransferStatus};
use axum::{
    body::Bytes,
    extract::{Path, Query, State, WebSocketUpgrade},
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
use tauri::{AppHandle, Emitter};
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

pub struct ServerState {
    pub settings: AppSettings,
    pub cert: TlsCertificate,
    pub db: Arc<Database>,
    pub transfer_mgr: Arc<TransferManager>,
    pub app_handle: Option<AppHandle>,
    pub settings_state: Option<Arc<SettingsState>>,
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

    let app = Router::new()
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(info_handler))
        .route("/api/message", post(message_handler))
        .route("/api/ws", get(ws_handler))
        .route("/api/files/request", post(file_request_handler))
        .route("/api/files/response", post(file_response_handler))
        .route("/api/files/chunk/:transfer_id", post(file_chunk_handler))
        .route("/api/files/cancel/:transfer_id", post(file_cancel_handler))
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
        .with_state(state);

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
                        .serve(app.into_make_service())
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
    let settings = if let Some(ref st) = state.settings_state {
        st.get_settings()
    } else {
        state.settings.clone()
    };
    Json(DeviceInfoResponse {
        device_id: settings.device_id,
        alias: settings.alias,
        device_type: settings.device_type,
        port: settings.port,
        fingerprint: state.cert.fingerprint.clone(),
        protocol_version: 1,
        status: settings.status,
        status_message: settings.status_message,
    })
}

async fn message_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SendMessagePayload>,
) -> Json<MessageAck> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

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

    if let Err(e) = state.db.save_message(&chat_msg) {
        eprintln!("[Server] Failed to save incoming message: {}", e);
    }

    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("new_message", &chat_msg);
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
    if let Some(ref handle) = state.app_handle {
        let _ = handle.emit("peer_typing", &payload);
    }
    Json(serde_json::json!({ "success": true }))
}

async fn read_handler(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<ReadPayload>,
) -> Json<serde_json::Value> {
    let _ = state.db.mark_as_read(&payload.peer_id);
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
    Json(req): Json<FileTransferRequest>,
) -> Json<serde_json::Value> {
    let download_dir = PathBuf::from(&state.settings.download_dir);
    let mut dest_path = download_dir.join(&req.file_name);

    // File name deduplication if file already exists
    if dest_path.exists() {
        let stem = dest_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = dest_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let mut counter = 1;
        loop {
            let new_name = if ext.is_empty() {
                format!("{} ({})", stem, counter)
            } else {
                format!("{} ({}).{}", stem, counter, ext)
            };
            let candidate = download_dir.join(new_name);
            if !candidate.exists() {
                dest_path = candidate;
                break;
            }
            counter += 1;
        }
    }

    let transfer_info = FileTransferInfo {
        transfer_id: req.transfer_id.clone(),
        peer_id: req.sender_id.clone(),
        sender_id: req.sender_id.clone(),
        sender_name: req.sender_name.clone(),
        receiver_id: req.receiver_id.clone(),
        receiver_name: req.receiver_name.clone(),
        file_name: req.file_name.clone(),
        file_size: req.file_size,
        transferred_bytes: 0,
        speed_mbps: 0.0,
        status: TransferStatus::Pending,
        is_incoming: true,
        file_path: Some(dest_path.to_string_lossy().to_string()),
    };

    state.transfer_mgr.add_transfer(transfer_info);

    Json(serde_json::json!({
        "success": true,
        "transfer_id": req.transfer_id
    }))
}

async fn file_response_handler(
    State(state): State<Arc<ServerState>>,
    Json(res): Json<FileTransferResponse>,
) -> Json<serde_json::Value> {
    state.transfer_mgr.handle_response(&res.transfer_id, res.accepted);
    Json(serde_json::json!({ "success": true }))
}

async fn file_chunk_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
    Query(query): Query<ChunkQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, String> {
    let updated = state.transfer_mgr.write_incoming_chunk(&transfer_id, query.offset, &body)?;
    Ok(Json(serde_json::json!({
        "success": true,
        "transferred": updated.transferred_bytes
    })))
}

async fn file_cancel_handler(
    State(state): State<Arc<ServerState>>,
    Path(transfer_id): Path<String>,
) -> Json<serde_json::Value> {
    state.transfer_mgr.cancel(&transfer_id);
    Json(serde_json::json!({ "success": true }))
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
