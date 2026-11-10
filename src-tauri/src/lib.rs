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
    _discovery_state: State<'_, Arc<DiscoveryManager>>,
    alias: String,
) -> Result<AppSettings, String> {
    let updated = settings_state.update_alias(alias)?;
    Ok(updated)
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

            let transfer_mgr = Arc::new(TransferManager::new(Some(handle.clone())));
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
            chat::send_chat_message,
            chat::get_chat_history,
            chat::mark_thread_read,
            chat::get_unread_badges,
            transfer::pick_file_to_send,
            transfer::send_file_request,
            transfer::respond_file_transfer,
            transfer::start_file_upload,
            transfer::cancel_file_transfer
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
