use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use rand::seq::SliceRandom;
use uuid::Uuid;

const ADJECTIVES: &[&str] = &[
    "Swift", "Calm", "Blue", "Gold", "Silent", "Brave", "Amber", "Silver",
    "Lunar", "Solar", "Cosmic", "Mystic", "Velvet", "Rapid", "Gentle", "Keen",
    "Wise", "Radiant", "Crimson", "Emerald", "Frosty", "Starlight", "Noble", "Nimble"
];

const ANIMALS: &[&str] = &[
    "Falcon", "Panda", "Otter", "Tiger", "Fox", "Eagle", "Wolf", "Owl",
    "Dolphin", "Hawk", "Leopard", "Bear", "Lynx", "Robin", "Badger", "Koala",
    "Cheetah", "Raven", "Penguin", "Jaguar", "Beaver", "Gazelle", "Puma", "Stag"
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub device_id: String,
    pub alias: String,
    pub device_type: String, // "desktop", "laptop", "mobile"
    pub port: u16,
    pub download_dir: String,
    pub theme: String, // "system", "light", "dark"
    pub instance_id: String,
}

pub struct SettingsState {
    pub settings: Mutex<AppSettings>,
    config_path: PathBuf,
}

impl SettingsState {
    pub fn new(instance_override: Option<String>, port_override: Option<u16>) -> Self {
        let instance = instance_override.unwrap_or_else(|| "default".to_string());
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Xent");
        
        let _ = fs::create_dir_all(&config_dir);
        let config_file_name = if instance == "default" {
            "settings.json".to_string()
        } else {
            format!("settings_{}.json", instance)
        };
        let config_path = config_dir.join(config_file_name);

        let default_download_dir = dirs::download_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string();

        let default_port = port_override.unwrap_or(53317);

        let mut settings = if config_path.exists() {
            if let Ok(data) = fs::read_to_string(&config_path) {
                serde_json::from_str::<AppSettings>(&data).unwrap_or_else(|_| {
                    Self::create_default_settings(default_port, &default_download_dir, &instance)
                })
            } else {
                Self::create_default_settings(default_port, &default_download_dir, &instance)
            }
        } else {
            Self::create_default_settings(default_port, &default_download_dir, &instance)
        };

        if let Some(port) = port_override {
            settings.port = port;
        }
        settings.instance_id = instance;

        // Persist initial settings
        let _ = Self::save_to_disk(&config_path, &settings);

        Self {
            settings: Mutex::new(settings),
            config_path,
        }
    }

    fn create_default_settings(port: u16, download_dir: &str, instance: &str) -> AppSettings {
        let mut rng = rand::thread_rng();
        let adj = ADJECTIVES.choose(&mut rng).unwrap_or(&"Swift");
        let animal = ANIMALS.choose(&mut rng).unwrap_or(&"Falcon");
        let alias = format!("{} {}", adj, animal);
        let device_id = Uuid::new_v4().to_string();

        AppSettings {
            device_id,
            alias,
            device_type: "desktop".to_string(),
            port,
            download_dir: download_dir.to_string(),
            theme: "system".to_string(),
            instance_id: instance.to_string(),
        }
    }

    fn save_to_disk(path: &PathBuf, settings: &AppSettings) -> Result<(), String> {
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        fs::write(path, json)
            .map_err(|e| format!("Failed to write settings file: {}", e))?;
        Ok(())
    }

    pub fn get_settings(&self) -> AppSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn update_alias(&self, new_alias: String) -> Result<AppSettings, String> {
        let trimmed = new_alias.trim().to_string();
        if trimmed.is_empty() {
            return Err("Alias cannot be empty".to_string());
        }
        let mut lock = self.settings.lock().unwrap();
        lock.alias = trimmed;
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn update_settings(&self, new_settings: AppSettings) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        *lock = new_settings;
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }
}
