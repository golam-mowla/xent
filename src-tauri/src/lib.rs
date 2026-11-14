pub mod cert;
pub mod chat;
pub mod db;
pub mod discovery;
pub mod server;
pub mod settings;
pub mod transfer;

use cert::TlsCertificate;
use db::Database;
use discovery::{DiscoveryManager, Peer};
use settings::{AppSettings, SettingsState};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, State, WindowEvent};
use transfer::TransferManager;

#[tauri::command]
fn get_device_info(state: State<'_, Arc<SettingsState>>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<SettingsState>>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
fn get_nearby_devices(state: State<'_, Arc<DiscoveryManager>>) -> Vec<Peer> {
    state.get_peers()
}

#[tauri::command]
fn update_alias(
    settings_state: State<'_, Arc<SettingsState>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    alias: String,
) -> Result<AppSettings, String> {
    let updated = settings_state.update_alias(alias)?;
    discovery_mgr.update_alias(updated.alias.clone());
    Ok(updated)
}

#[tauri::command]
async fn set_presence(
    settings_state: State<'_, Arc<SettingsState>>,
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    status: String,
    message: String,
    clear_after: String,
) -> Result<AppSettings, String> {
    let valid_statuses = ["available", "busy", "away", "offline"];
    if !valid_statuses.contains(&status.as_str()) {
        return Err(format!(
            "Invalid status: {}. Allowed values: available, busy, away, offline",
            status
        ));
    }

    let trimmed_msg = message.trim();
    let truncated_msg = if trimmed_msg.chars().count() > 80 {
        trimmed_msg.chars().take(80).collect::<String>()
    } else {
        trimmed_msg.to_string()
    };

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let expires_at = match clear_after.as_str() {
        "1h" => Some(now_secs + 3600),
        "4h" => Some(now_secs + 4 * 3600),
        "today" => {
            let now_local = chrono::Local::now();
            let midnight = now_local
                .date_naive()
                .and_hms_opt(23, 59, 59)
                .and_then(|t| t.and_local_timezone(chrono::Local).single());
            if let Some(m) = midnight {
                Some(m.timestamp() as u64)
            } else {
                Some(now_secs + 24 * 3600)
            }
        }
        _ => None,
    };

    let updated = settings_state.set_presence(status.clone(), truncated_msg.clone(), expires_at)?;
    discovery_mgr.set_presence(status, truncated_msg, expires_at).await;
    Ok(updated)
}

#[tauri::command]
fn get_network_diagnostics(
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
) -> discovery::NetworkDiagnostics {
    discovery_mgr.get_network_diagnostics()
}

#[tauri::command]
async fn add_peer_by_ip(
    discovery_mgr: State<'_, Arc<DiscoveryManager>>,
    target: String,
) -> Result<Peer, String> {
    discovery_mgr.add_peer_by_ip(target).await
}

#[tauri::command]
fn update_download_dir(
    settings_state: State<'_, Arc<SettingsState>>,
    download_dir: String,
) -> Result<AppSettings, String> {
    settings_state.update_download_dir(download_dir)
}

#[tauri::command]
fn set_auto_accept(
    settings_state: State<'_, Arc<SettingsState>>,
    enabled: bool,
) -> Result<AppSettings, String> {
    settings_state.set_auto_accept(enabled)
}

#[tauri::command]
fn toggle_trusted_device(
    settings_state: State<'_, Arc<SettingsState>>,
    device_id: String,
) -> Result<AppSettings, String> {
    settings_state.toggle_trusted_device(device_id)
}

#[tauri::command]
fn open_log_folder() -> Result<(), String> {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Xent");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn update_settings(
    state: State<'_, Arc<SettingsState>>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    state.update_settings(settings)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let mut instance_override = None;
    let mut port_override = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--instance" && i + 1 < args.len() {
            instance_override = Some(args[i + 1].clone());
            i += 1;
        } else if args[i] == "--port" && i + 1 < args.len() {
            if let Ok(p) = args[i + 1].parse::<u16>() {
                port_override = Some(p);
            }
            i += 1;
        }
        i += 1;
    }

    let settings_state = Arc::new(SettingsState::new(instance_override, port_override));
    let initial_settings = settings_state.get_settings();

    let cert = TlsCertificate::generate().expect("Failed to generate TLS certificate");
    let database = Arc::new(Database::init(&initial_settings.instance_id).expect("Failed to initialize database"));

    let settings_state_for_setup = settings_state.clone();
    let cert_clone = cert.clone();
    let db_for_setup = database.clone();

    tauri::Builder::default()
        .manage(settings_state)
        .manage(cert)
        .manage(database)
        .setup(move |app| {
            let handle = app.handle().clone();
            let mut current_settings = initial_settings.clone();
            let cert_for_server = cert_clone.clone();
            let db_for_server = db_for_setup.clone();
            let settings_state_for_server = settings_state_for_setup.clone();
            let handle_for_server = handle.clone();

            let transfer_mgr = Arc::new(TransferManager::new(
                Some(handle.clone()),
                Some(db_for_server.clone()),
            ));
            handle.manage(transfer_mgr.clone());

            let transfer_mgr_for_server = transfer_mgr.clone();

            // Tray setup
            let quit_i = MenuItem::with_id(app, "quit", "Quit Xent", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Open Xent", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            if let Some(icon) = app.default_window_icon() {
                let _tray = TrayIconBuilder::new()
                    .icon(icon.clone())
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "quit" => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Some(mgr) = app_handle.try_state::<Arc<DiscoveryManager>>() {
                                    mgr.send_bye().await;
                                }
                                app_handle.exit(0);
                            });
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            tauri::async_runtime::spawn(async move {
                match server::start_server(
                    current_settings.clone(),
                    cert_for_server.clone(),
                    db_for_server,
                    transfer_mgr_for_server,
                    Some(handle_for_server),
                    Some(settings_state_for_server.clone()),
                )
                .await
                {
                    Ok(bound_port) => {
                        if bound_port != current_settings.port {
                            current_settings.port = bound_port;
                            let _ = settings_state_for_server.update_settings(current_settings.clone());
                        }
                        println!("[Server] HTTPS server running on port {}", bound_port);
                    }
                    Err(e) => {
                        eprintln!("[Server] Failed to start HTTPS server: {}", e);
                    }
                }

                let discovery_mgr = Arc::new(DiscoveryManager::new(
                    current_settings,
                    cert_for_server.fingerprint,
                    Some(handle.clone()),
                    Some(settings_state_for_server.clone()),
                ));

                handle.manage(discovery_mgr.clone());
                discovery_mgr.start();
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Minimize/hide to tray when user closes window
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            get_settings,
            get_nearby_devices,
            update_alias,
            update_settings,
            set_presence,
            get_network_diagnostics,
            add_peer_by_ip,
            update_download_dir,
            set_auto_accept,
            toggle_trusted_device,
            open_log_folder,
            chat::send_chat_message,
            chat::get_chat_history,
            chat::mark_thread_read,
            chat::get_unread_badges,
            chat::send_typing_indicator,
            chat::edit_chat_message,
            chat::delete_chat_message,
            chat::react_chat_message,
            transfer::pick_file_to_send,
            transfer::pick_files_to_send,
            transfer::pick_download_dir,
            transfer::open_file,
            transfer::show_in_folder,
            transfer::save_clipboard_image,
            transfer::get_chat_timeline,
            transfer::send_file_request,
            transfer::respond_file_transfer,
            transfer::start_file_upload,
            transfer::cancel_file_transfer,
            transfer::get_transfer_history
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
