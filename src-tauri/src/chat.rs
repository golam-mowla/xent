use crate::db::{ChatMessage, Database, KnownPeer};
use crate::discovery::{DiscoveryManager, Peer};
use crate::server::SendMessagePayload;
use crate::settings::SettingsState;
use chrono::Local;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

fn shared_chat_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to build chat HTTP client")
        })
        .clone()
}

fn shared_short_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(3))
                .build()
                .expect("Failed to build short HTTP client")
        })
        .clone()
}

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

    let receiver_name = peer_opt
        .as_ref()
        .map(|p| p.alias.clone())
        .or_else(|| {
            db.get_known_peers()
                .ok()
                .and_then(|list| list.into_iter().find(|k| k.device_id == peer_id))
                .map(|k| k.alias)
        })
        .unwrap_or_else(|| "Peer".to_string());

    let mut msg = ChatMessage {
        id: msg_id.clone(),
        thread_peer_id: peer_id.clone(),
        sender_id: my_settings.device_id.clone(),
        sender_name: my_settings.alias.clone(),
        receiver_id: peer_id.clone(),
        receiver_name,
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
        let client = shared_chat_client();

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
                // Keep as queued if unreachable so queue flusher will deliver when reachable
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
pub async fn retry_chat_message(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    app_handle: AppHandle,
    message_id: String,
) -> Result<ChatMessage, String> {
    let mut msg = db
        .get_message_by_id(&message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Message not found".to_string())?;

    let peers = discovery_mgr.get_peers();
    let peer = peers
        .iter()
        .find(|p| p.device_id == msg.thread_peer_id)
        .cloned()
        .ok_or_else(|| "Device is currently offline".to_string())?;

    let client = shared_chat_client();
    deliver_stored(&client, &peer, &msg).await?;
    let time_str = Local::now().format("%I:%M %p").to_string();
    let delivered_status = format!("Delivered, {}", time_str);
    msg.status = delivered_status.clone();
    let _ = db.update_status(&msg.id, &delivered_status);
    let _ = app_handle.emit("message_status_updated", &msg);
    Ok(msg)
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
        let client = shared_short_client();
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
    let cleaned = new_content.trim().to_string();
    if cleaned.is_empty() {
        return Err("Message content cannot be empty".to_string());
    }
    db.edit_message(&message_id, &cleaned).map_err(|e| e.to_string())?;
    let _ = app_handle.emit("message_edited", serde_json::json!({
        "message_id": message_id,
        "new_content": cleaned
    }));

    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        let client = shared_short_client();
        let url = format!("https://{}:{}/api/message/edit", peer.ip, peer.port);
        let _ = client.post(&url).json(&serde_json::json!({
            "message_id": message_id,
            "new_content": cleaned
        })).send().await;
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
        let client = shared_short_client();
        let url = format!("https://{}:{}/api/message/delete", peer.ip, peer.port);
        let _ = client.post(&url).json(&serde_json::json!({
            "message_id": message_id
        })).send().await;
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
        let client = shared_short_client();
        let url = format!("https://{}:{}/api/message/reaction", peer.ip, peer.port);
        let _ = client.post(&url).json(&serde_json::json!({
            "message_id": message_id,
            "user_name": my_alias,
            "emoji": emoji
        })).send().await;
    }
    Ok(res)
}

#[tauri::command]
pub async fn mark_thread_read(
    db: State<'_, Arc<Database>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    settings_state: State<'_, Arc<SettingsState>>,
    peer_id: String,
) -> Result<(), String> {
    db.mark_as_read(&peer_id).map_err(|e| format!("Database error: {}", e))?;

    let my_id = settings_state.get_settings().device_id;
    let peers = discovery_mgr.get_peers();
    if let Some(peer) = peers.iter().find(|p| p.device_id == peer_id) {
        let client = shared_short_client();
        let url = format!("https://{}:{}/api/message/read", peer.ip, peer.port);
        // Send MY device ID so the remote marks its thread WITH ME as read!
        let _ = client.post(&url).json(&serde_json::json!({
            "peer_id": my_id
        })).send().await;
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

#[tauri::command]
pub fn get_known_peers(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<KnownPeer>, String> {
    db.get_known_peers().map_err(|e| e.to_string())
}

/// Sends one stored message to a peer. Used by the retry loop below.
pub async fn deliver_stored(
    client: &reqwest::Client,
    peer: &Peer,
    msg: &ChatMessage,
) -> Result<(), String> {
    let payload = SendMessagePayload {
        id: msg.id.clone(),
        sender_id: msg.sender_id.clone(),
        sender_name: msg.sender_name.clone(),
        receiver_id: peer.device_id.clone(),
        receiver_name: peer.alias.clone(),
        content: msg.content.clone(),
        created_at: msg.created_at,
        quote: msg.quote.clone(),
    };
    let url = format!("https://{}:{}/api/message", peer.ip, peer.port);
    let res = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("Peer returned error: {}", res.status()))
    }
}

/// Background loop: every few seconds, deliver messages that are still "Queued"
/// to peers that are reachable now.
pub fn start_queue_flusher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = shared_chat_client();

        loop {
            tokio::time::sleep(Duration::from_secs(4)).await;

            let db = match app.try_state::<Arc<Database>>() {
                Some(d) => d.inner().clone(),
                None => continue,
            };
            let discovery = match app.try_state::<Arc<DiscoveryManager>>() {
                Some(d) => d.inner().clone(),
                None => continue,
            };

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let pending = match db.get_undelivered_messages(now_ms - 30_000) {
                Ok(list) => list,
                Err(_) => continue,
            };
            if pending.is_empty() {
                continue;
            }

            let peers = discovery.get_peers();
            let mut blocked_threads: std::collections::HashSet<String> = std::collections::HashSet::new();

            for mut msg in pending {
                if blocked_threads.contains(&msg.thread_peer_id) {
                    continue;
                }
                let peer = match peers.iter().find(|p| p.device_id == msg.thread_peer_id) {
                    Some(p) => p.clone(),
                    None => continue,
                };
                match deliver_stored(&client, &peer, &msg).await {
                    Ok(()) => {
                        let time_str = Local::now().format("%I:%M %p").to_string();
                        let status = format!("Delivered, {}", time_str);
                        let _ = db.update_status(&msg.id, &status);
                        msg.status = status;
                        let _ = app.emit("message_status_updated", &msg);
                    }
                    Err(_) => {
                        blocked_threads.insert(msg.thread_peer_id.clone());
                    }
                }
            }
        }
    });
}
