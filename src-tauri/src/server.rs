use crate::cert::TlsCertificate;
use crate::db::{ChatMessage, Database};
use crate::settings::AppSettings;
use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tower_http::cors::{Any, CorsLayer};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    pub device_id: String,
    pub alias: String,
    pub device_type: String,
    pub port: u16,
    pub fingerprint: String,
    pub protocol_version: u32,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAck {
    pub success: bool,
    pub id: String,
    pub status: String,
    pub timestamp: i64,
}

pub struct ServerState {
    pub settings: AppSettings,
    pub cert: TlsCertificate,
    pub db: Arc<Database>,
    pub app_handle: Option<AppHandle>,
}

pub async fn start_server(
    settings: AppSettings,
    cert: TlsCertificate,
    db: Arc<Database>,
    app_handle: Option<AppHandle>,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let state = Arc::new(ServerState {
        settings: settings.clone(),
        cert: cert.clone(),
        db,
        app_handle,
    });

    let app = Router::new()
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(info_handler))
        .route("/api/message", post(message_handler))
        .route("/api/ws", get(ws_handler))
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
    Json(DeviceInfoResponse {
        device_id: state.settings.device_id.clone(),
        alias: state.settings.alias.clone(),
        device_type: state.settings.device_type.clone(),
        port: state.settings.port,
        fingerprint: state.cert.fingerprint.clone(),
        protocol_version: 1,
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
