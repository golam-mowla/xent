use crate::db::{ChatMessage, Database};
use crate::discovery::DiscoveryManager;
use crate::server::SendMessagePayload;
use crate::settings::SettingsState;
use chrono::Local;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

#[tauri::command]
pub async fn send_chat_message(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    settings_state: State<'_, Arc<SettingsState>>,
    app_handle: AppHandle,
    peer_id: String,
    content: String,
) -> Result<ChatMessage, String> {
    let text = content.trim().to_string();
    if text.is_empty() {
        return Err("Message content cannot be empty".to_string());
    }

    let my_settings = settings_state.get_settings();
    let peers = discovery_mgr.get_peers();
    let peer = peers
        .iter()
        .find(|p| p.device_id == peer_id)
        .ok_or_else(|| "Peer is offline or not found".to_string())?
        .clone();

    let msg_id = Uuid::new_v4().to_string();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let time_str = Local::now().format("%I:%M %p").to_string();
    let initial_status = format!("Sending, {}", time_str);

    let mut msg = ChatMessage {
        id: msg_id.clone(),
        thread_peer_id: peer.device_id.clone(),
        sender_id: my_settings.device_id.clone(),
        sender_name: my_settings.alias.clone(),
        receiver_id: peer.device_id.clone(),
        receiver_name: peer.alias.clone(),
        content: text.clone(),
        status: initial_status,
        created_at: now_ms,
        is_incoming: false,
        is_read: true,
    };

    let _ = db.save_message(&msg);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let payload = SendMessagePayload {
        id: msg_id.clone(),
        sender_id: my_settings.device_id,
        sender_name: my_settings.alias,
        receiver_id: peer.device_id,
        receiver_name: peer.alias,
        content: text,
        created_at: now_ms,
    };

    let url = format!("https://{}:{}/api/message", peer.ip, peer.port);

    match client.post(&url).json(&payload).send().await {
        Ok(res) => {
            if res.status().is_success() {
                let delivered_status = format!("Delivered, {}", time_str);
                msg.status = delivered_status.clone();
                let _ = db.update_status(&msg_id, &delivered_status);
                let _ = app_handle.emit("message_status_updated", &msg);
                Ok(msg)
            } else {
                let failed_status = format!("Failed ({})", res.status());
                msg.status = failed_status.clone();
                let _ = db.update_status(&msg_id, &failed_status);
                let _ = app_handle.emit("message_status_updated", &msg);
                Err(format!("Peer returned error: {}", res.status()))
            }
        }
        Err(e) => {
            let failed_status = "Failed to deliver".to_string();
            msg.status = failed_status.clone();
            let _ = db.update_status(&msg_id, &failed_status);
            let _ = app_handle.emit("message_status_updated", &msg);
            Err(format!("Failed to reach peer: {}", e))
        }
    }
}

#[tauri::command]
pub fn get_chat_history(
    db: State<'_, Arc<Database>>,
    peer_id: String,
) -> Result<Vec<ChatMessage>, String> {
    db.get_messages(&peer_id, 200)
        .map_err(|e| format!("Database error: {}", e))
}

#[tauri::command]
pub fn mark_thread_read(
    db: State<'_, Arc<Database>>,
    peer_id: String,
) -> Result<(), String> {
    db.mark_as_read(&peer_id)
        .map_err(|e| format!("Database error: {}", e))
}

#[tauri::command]
pub fn get_unread_badges(
    db: State<'_, Arc<Database>>,
) -> Result<std::collections::HashMap<String, u32>, String> {
    db.get_unread_counts()
        .map_err(|e| format!("Database error: {}", e))
}
