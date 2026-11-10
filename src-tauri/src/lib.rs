pub mod settings;

use settings::{AppSettings, SettingsState};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
fn get_device_info(state: State<'_, Arc<SettingsState>>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<SettingsState>>) -> AppSettings {
    state.get_settings()
}

#[tauri::command]
fn update_alias(state: State<'_, Arc<SettingsState>>, alias: String) -> Result<AppSettings, String> {
    state.update_alias(alias)
}

#[tauri::command]
fn update_settings(state: State<'_, Arc<SettingsState>>, settings: AppSettings) -> Result<AppSettings, String> {
    state.update_settings(settings)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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

    tauri::Builder::default()
        .manage(settings_state)
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            get_settings,
            update_alias,
            update_settings
        ])
        .setup(|_app| {
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
