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
    quote: Option<String>,
) -> Result<ChatMessage, String> {
    let text = content.trim().to_string();
    if text.is_empty() {
        return Err("Message content cannot be empty".to_string());
    }

    let my_settings = settings_state.get_settings();
    let peers = discovery_mgr.get_peers();
    let peer_opt = peers.iter().find(|p| p.device_id == peer_id).cloned();

    let msg_id = Uuid::new_v4().to_string();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let time_str = Local::now().format("%I:%M %p").to_string();
    let is_peer_online = peer_opt.is_some();
    let initial_status = if is_peer_online {
        format!("Sending, {}", time_str)
    } else {
        "Queued".to_string()
    };

    let mut msg = ChatMessage {
        id: msg_id.clone(),
        thread_peer_id: peer_id.clone(),
        sender_id: my_settings.device_id.clone(),
        sender_name: my_settings.alias.clone(),
        receiver_id: peer_id.clone(),
        receiver_name: peer_opt.as_ref().map(|p| p.alias.clone()).unwrap_or_else(|| "Peer".to_string()),
        content: text.clone(),
        status: initial_status,
        created_at: now_ms,
        is_incoming: false,
        is_read: true,
        quote: quote.clone(),
        is_edited: false,
        is_deleted: false,
        reactions: None,
    };

    let _ = db.save_message(&msg);

    if let Some(peer) = peer_opt {
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
            quote,
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
            Err(_) => {
                // Keep as queued if unreachable
                let queued_status = "Queued".to_string();
                msg.status = queued_status.clone();
                let _ = db.update_status(&msg_id, &queued_status);
                let _ = app_handle.emit("message_status_updated", &msg);
                Ok(msg)
            }
        }
    } else {
        let _ = app_handle.emit("message_status_updated", &msg);
        Ok(msg)
    }
}

#[tauri::command]
pub async fn send_typing_indicator(
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    settings_state: State<'_, Arc<SettingsState>>,
    peer_id: String,
) -> Result<(), String> {
    let my_settings = settings_state.get_settings();
    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(1200))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("https://{}:{}/api/message/typing", peer.ip, peer.port);
        let payload = crate::server::TypingPayload {
            sender_id: my_settings.device_id,
            sender_name: my_settings.alias,
        };
        let _ = client.post(&url).json(&payload).send().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn edit_chat_message(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    app_handle: AppHandle,
    message_id: String,
    peer_id: String,
    new_content: String,
) -> Result<(), String> {
    db.edit_message(&message_id, &new_content).map_err(|e| e.to_string())?;
    let _ = app_handle.emit("message_edited", serde_json::json!({
        "message_id": message_id,
        "new_content": new_content
    }));

    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        if let Ok(client) = reqwest::Client::builder().danger_accept_invalid_certs(true).build() {
            let url = format!("https://{}:{}/api/message/edit", peer.ip, peer.port);
            let _ = client.post(&url).json(&serde_json::json!({
                "message_id": message_id,
                "new_content": new_content
            })).send().await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_chat_message(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    app_handle: AppHandle,
    message_id: String,
    peer_id: String,
) -> Result<(), String> {
    db.delete_message(&message_id).map_err(|e| e.to_string())?;
    let _ = app_handle.emit("message_deleted", serde_json::json!({
        "message_id": message_id
    }));

    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        if let Ok(client) = reqwest::Client::builder().danger_accept_invalid_certs(true).build() {
            let url = format!("https://{}:{}/api/message/delete", peer.ip, peer.port);
            let _ = client.post(&url).json(&serde_json::json!({
                "message_id": message_id
            })).send().await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn react_chat_message(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    settings_state: State<'_, Arc<SettingsState>>,
    app_handle: AppHandle,
    message_id: String,
    peer_id: String,
    emoji: String,
) -> Result<String, String> {
    let my_alias = settings_state.get_settings().alias;
    let res = db.add_reaction(&message_id, &my_alias, &emoji).map_err(|e| e.to_string())?;

    let _ = app_handle.emit("message_reaction_updated", serde_json::json!({
        "message_id": message_id,
        "reactions": res
    }));

    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        if let Ok(client) = reqwest::Client::builder().danger_accept_invalid_certs(true).build() {
            let url = format!("https://{}:{}/api/message/reaction", peer.ip, peer.port);
            let _ = client.post(&url).json(&serde_json::json!({
                "message_id": message_id,
                "user_name": my_alias,
                "emoji": emoji
            })).send().await;
        }
    }
    Ok(res)
}

#[tauri::command]
pub async fn mark_thread_read(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    peer_id: String,
) -> Result<(), String> {
    db.mark_as_read(&peer_id).map_err(|e| format!("Database error: {}", e))?;

    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        if let Ok(client) = reqwest::Client::builder().danger_accept_invalid_certs(true).build() {
            let url = format!("https://{}:{}/api/message/read", peer.ip, peer.port);
            let _ = client.post(&url).json(&serde_json::json!({
                "peer_id": peer_id
            })).send().await;
        }
    }
    Ok(())
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
pub fn get_unread_badges(
    db: State<'_, Arc<Database>>,
) -> Result<std::collections::HashMap<String, u32>, String> {
    db.get_unread_counts()
        .map_err(|e| format!("Database error: {}", e))
}
