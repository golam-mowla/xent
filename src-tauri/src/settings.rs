use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

fn default_status() -> String {
    "available".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub device_id: String,
    pub alias: String,
    pub device_type: String, // "desktop", "laptop", "mobile"
    pub port: u16,
    pub download_dir: String,
    pub theme: String, // "system", "light", "dark"
    pub instance_id: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub status_message: String,
    #[serde(default)]
    pub status_expires_at: Option<u64>,
    #[serde(default)]
    pub manual_peers: Vec<String>,
    #[serde(default)]
    pub auto_accept: bool,
    /// Legacy list kept so old settings files keep working.
    /// Devices here are treated exactly like favourites.
    #[serde(default)]
    pub trusted_devices: Vec<String>,
    /// Favourite devices: pinned on top, files from them are accepted
    /// automatically (no permission prompt).
    #[serde(default)]
    pub favorite_devices: Vec<String>,
    /// Local-only names for other devices (device_id -> nickname).
    #[serde(default)]
    pub device_nicknames: HashMap<String, String>,
    /// Blocked devices: their messages and file offers are silently dropped.
    #[serde(default)]
    pub blocked_devices: Vec<String>,
}

pub struct SettingsState {
    pub settings: Mutex<AppSettings>,
    config_path: PathBuf,
}

impl SettingsState {
    pub fn new(instance_override: Option<String>, port_override: Option<u16>) -> Self {
        let instance = instance_override.unwrap_or_else(|| "default".to_string());
        let config_dir = crate::paths::data_dir();

        let _ = fs::create_dir_all(&config_dir);
        let config_file_name = if instance == "default" {
            "settings.json".to_string()
        } else {
            format!("settings_{}.json", instance)
        };
        let config_path = config_dir.join(config_file_name);

        let default_download_dir = crate::paths::default_download_dir()
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
            device_type: crate::paths::device_type().to_string(),
            port,
            download_dir: download_dir.to_string(),
            theme: "system".to_string(),
            instance_id: instance.to_string(),
            status: "available".to_string(),
            status_message: "".to_string(),
            status_expires_at: None,
            manual_peers: Vec::new(),
            auto_accept: false,
            trusted_devices: Vec::new(),
            favorite_devices: Vec::new(),
            device_nicknames: HashMap::new(),
            blocked_devices: Vec::new(),
        }
    }

    pub fn is_favorite(&self, device_id: &str) -> bool {
        let lock = self.settings.lock().unwrap();
        lock.favorite_devices.iter().any(|d| d == device_id)
            || lock.trusted_devices.iter().any(|d| d == device_id)
    }

    pub fn is_blocked(&self, device_id: &str) -> bool {
        self.settings
            .lock()
            .unwrap()
            .blocked_devices
            .iter()
            .any(|d| d == device_id)
    }

    pub fn toggle_favorite(&self, device_id: String) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        let was_fav = lock.favorite_devices.contains(&device_id)
            || lock.trusted_devices.contains(&device_id);
        if was_fav {
            lock.favorite_devices.retain(|id| id != &device_id);
            lock.trusted_devices.retain(|id| id != &device_id);
        } else {
            lock.blocked_devices.retain(|id| id != &device_id);
            lock.favorite_devices.push(device_id);
        }
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn toggle_blocked(&self, device_id: String) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        if lock.blocked_devices.contains(&device_id) {
            lock.blocked_devices.retain(|id| id != &device_id);
        } else {
            // A blocked device can never stay a favourite.
            lock.favorite_devices.retain(|id| id != &device_id);
            lock.trusted_devices.retain(|id| id != &device_id);
            lock.blocked_devices.push(device_id);
        }
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    /// Empty nickname removes the custom name.
    pub fn set_device_nickname(
        &self,
        device_id: String,
        nickname: String,
    ) -> Result<AppSettings, String> {
        let cleaned: String = nickname.trim().chars().take(32).collect();
        let mut lock = self.settings.lock().unwrap();
        if cleaned.is_empty() {
            lock.device_nicknames.remove(&device_id);
        } else {
            lock.device_nicknames.insert(device_id, cleaned);
        }
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn add_manual_peer(&self, peer_addr: String) -> Result<AppSettings, String> {
        let trimmed = peer_addr.trim().to_string();
        if trimmed.is_empty() {
            return Err("Peer address cannot be empty".to_string());
        }
        let mut lock = self.settings.lock().unwrap();
        if !lock.manual_peers.contains(&trimmed) {
            lock.manual_peers.push(trimmed);
            Self::save_to_disk(&self.config_path, &lock)?;
        }
        Ok(lock.clone())
    }

    pub fn remove_manual_peer(&self, peer_addr: String) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        lock.manual_peers.retain(|p| p != &peer_addr);
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn update_download_dir(&self, new_dir: String) -> Result<AppSettings, String> {
        let trimmed = new_dir.trim().to_string();
        if trimmed.is_empty() {
            return Err("Download directory cannot be empty".to_string());
        }
        let mut lock = self.settings.lock().unwrap();
        lock.download_dir = trimmed;
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn set_auto_accept(&self, enabled: bool) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        lock.auto_accept = enabled;
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn toggle_trusted_device(&self, device_id: String) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        if lock.trusted_devices.contains(&device_id) {
            lock.trusted_devices.retain(|id| id != &device_id);
        } else {
            lock.trusted_devices.push(device_id);
        }
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
    }

    pub fn set_presence(
        &self,
        status: String,
        message: String,
        expires_at: Option<u64>,
    ) -> Result<AppSettings, String> {
        let mut lock = self.settings.lock().unwrap();
        lock.status = status;
        lock.status_message = message;
        lock.status_expires_at = expires_at;
        Self::save_to_disk(&self.config_path, &lock)?;
        Ok(lock.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_settings_deserialization() {
        let legacy_json = r#"{
            "device_id": "device-12345",
            "alias": "Swift Falcon",
            "device_type": "desktop",
            "port": 53317,
            "download_dir": "C:/Downloads",
            "theme": "system",
            "instance_id": "default"
        }"#;

        let settings: AppSettings = serde_json::from_str(legacy_json).expect("Should deserialize legacy settings");
        assert_eq!(settings.device_id, "device-12345");
        assert_eq!(settings.alias, "Swift Falcon");
        assert_eq!(settings.status, "available");
        assert_eq!(settings.status_message, "");
        assert_eq!(settings.manual_peers.len(), 0);
        assert!(!settings.auto_accept);
        assert_eq!(settings.trusted_devices.len(), 0);
        assert_eq!(settings.favorite_devices.len(), 0);
        assert_eq!(settings.device_nicknames.len(), 0);
        assert_eq!(settings.blocked_devices.len(), 0);
    }

    fn temp_state(tag: &str) -> SettingsState {
        let mut path = std::env::temp_dir();
        path.push(format!("xent_test_settings_{}_{}.json", tag, Uuid::new_v4()));
        let settings = SettingsState::create_default_settings(53317, ".", "test");
        SettingsState {
            settings: Mutex::new(settings),
            config_path: path,
        }
    }

    #[test]
    fn favorite_toggle_and_legacy_trusted() {
        let st = temp_state("fav");
        assert!(!st.is_favorite("a"));
        st.toggle_favorite("a".into()).unwrap();
        assert!(st.is_favorite("a"));
        st.toggle_favorite("a".into()).unwrap();
        assert!(!st.is_favorite("a"));

        st.toggle_trusted_device("b".into()).unwrap();
        assert!(st.is_favorite("b"), "legacy trusted devices count as favourites");
        st.toggle_favorite("b".into()).unwrap();
        assert!(!st.is_favorite("b"));
    }

    #[test]
    fn blocking_removes_favorite() {
        let st = temp_state("block");
        st.toggle_favorite("a".into()).unwrap();
        st.toggle_blocked("a".into()).unwrap();
        assert!(st.is_blocked("a"));
        assert!(!st.is_favorite("a"));
        st.toggle_blocked("a".into()).unwrap();
        assert!(!st.is_blocked("a"));
    }

    #[test]
    fn nickname_set_trim_and_clear() {
        let st = temp_state("nick");
        let s = st.set_device_nickname("a".into(), "  My Laptop  ".into()).unwrap();
        assert_eq!(s.device_nicknames.get("a").unwrap(), "My Laptop");
        let s = st.set_device_nickname("a".into(), "   ".into()).unwrap();
        assert!(s.device_nicknames.get("a").is_none());
    }
}
