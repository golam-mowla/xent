use crate::cert::TlsCertificate;
use crate::settings::AppSettings;
use axum::{routing::get, Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
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

pub struct ServerState {
    pub settings: AppSettings,
    pub cert: TlsCertificate,
}

pub async fn start_server(
    settings: AppSettings,
    cert: TlsCertificate,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let state = Arc::new(ServerState {
        settings: settings.clone(),
        cert: cert.clone(),
    });

    let app = Router::new()
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(info_handler))
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

    // Try binding starting from configured port, increment if occupied
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
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
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
