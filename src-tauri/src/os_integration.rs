use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod win_native {
    use std::path::{Path, PathBuf};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::Foundation::{PROPERTYKEY, S_FALSE, S_OK};
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::Interface;

    const XENT_AUMID: &str = "com.xent.app";
    const STARTUP_REG_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    // System.AppUserModel.ID = {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
    const PKEY_APPUSERMODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

    fn wide_z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set_startup_run_key(enabled: bool, exe_path: &Path) {
        unsafe {
            let subkey = wide_z(STARTUP_REG_SUBKEY);
            let value_name = wide_z("Xent");
            let mut hkey = HKEY::default();
            let res = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                Some(0),
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            );
            if res.is_err() {
                return;
            }
            if enabled {
                let cmd_val = format!("\"{}\"", exe_path.to_string_lossy());
                let wide_val = wide_z(&cmd_val);
                let byte_slice = std::slice::from_raw_parts(
                    wide_val.as_ptr() as *const u8,
                    wide_val.len() * 2,
                );
                let _ = RegSetValueExW(hkey, PCWSTR(value_name.as_ptr()), Some(0), REG_SZ, Some(byte_slice));
            } else {
                let _ = RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr()));
            }
            let _ = RegCloseKey(hkey);
        }
    }

    pub fn create_shortcut(lnk_path: &Path, exe_path: &Path, description: &str, aumid: Option<&str>) {
        if let Some(parent) = lnk_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        unsafe {
            let init_hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let need_uninit = init_hr == S_OK || init_hr == S_FALSE;
            let _ = || -> windows::core::Result<()> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                let target_w = wide_z(&exe_path.to_string_lossy());
                link.SetPath(PCWSTR(target_w.as_ptr()))?;

                if let Some(work) = exe_path.parent() {
                    let work_w = wide_z(&work.to_string_lossy());
                    link.SetWorkingDirectory(PCWSTR(work_w.as_ptr()))?;
                }
                let desc_w = wide_z(description);
                link.SetDescription(PCWSTR(desc_w.as_ptr()))?;
                link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0)?;

                if let Some(id) = aumid {
                    if let Ok(store) = link.cast::<IPropertyStore>() {
                        let pv = PROPVARIANT::from(id);
                        let _ = store.SetValue(&PKEY_APPUSERMODEL_ID, &pv);
                        let _ = store.Commit();
                    }
                }

                let persist: IPersistFile = link.cast()?;
                let out_w = wide_z(&lnk_path.to_string_lossy());
                persist.Save(PCWSTR(out_w.as_ptr()), true)?;
                Ok(())
            }();
            if need_uninit {
                CoUninitialize();
            }
        }
    }

    pub fn ensure_start_menu_shortcut() {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let start_lnk = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Xent.lnk");
        create_shortcut(&start_lnk, &exe, "Xent LAN Messenger", Some(XENT_AUMID));
    }

    fn escape_xml(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(ch),
            }
        }
        out
    }

    pub fn show_native_toast(title: &str, body: &str) -> bool {
        let xml_str = format!(
            "<toast><visual><binding template='ToastGeneric'><text>{}</text><text>{}</text></binding></visual></toast>",
            escape_xml(title),
            escape_xml(body)
        );
        let run = || -> windows::core::Result<()> {
            let doc = XmlDocument::new()?;
            doc.LoadXml(&HSTRING::from(xml_str))?;
            let toast = ToastNotification::CreateToastNotification(&doc)?;
            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(XENT_AUMID))?;
            notifier.Show(&toast)?;
            Ok(())
        };
        run().is_ok()
    }
}

pub fn token_file_path() -> PathBuf {
    crate::paths::data_dir().join("local_ipc.token")
}

pub fn load_or_create_ipc_token() -> String {
    let p = token_file_path();
    if let Ok(existing) = std::fs::read_to_string(&p) {
        let t = existing.trim().to_string();
        if t.len() >= 16 {
            return t;
        }
    }
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&p, &token);
    token
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Recursively expand any directory arguments into individual files (up to a safe cap).
pub fn expand_file_paths(raw: Vec<String>) -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>, depth: u8) {
        if depth > 6 || out.len() >= 500 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if out.len() >= 500 {
                    break;
                }
                let p = entry.path();
                if p.is_file() {
                    out.push(p.to_string_lossy().to_string());
                } else if p.is_dir() {
                    walk(&p, out, depth + 1);
                }
            }
        }
    }

    let mut out = Vec::new();
    for item in raw {
        let p = PathBuf::from(&item);
        if p.is_file() {
            out.push(item);
        } else if p.is_dir() {
            walk(&p, &mut out, 0);
        }
    }
    out
}

/// If a default primary Xent instance is already running, forward CLI file paths and focus it.
pub fn try_handoff_to_running_instance(port: u16, paths: &[String]) -> bool {
    let token = match std::fs::read_to_string(token_file_path()) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => return false,
    };

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    rt.block_on(async move {
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_millis(1200))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let url = format!("https://127.0.0.1:{}/api/local/open-send-files", port);
        let body = serde_json::json!({ "paths": paths });
        match client
            .post(&url)
            .header("X-Xent-Local-Token", token)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    })
}

/// Register or unregister Xent in OS startup via native Win32 Registry API (zero external processes).
pub fn sync_startup_registration(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        if let Ok(exe) = std::env::current_exe() {
            win_native::set_startup_run_key(enabled, &exe);
        }
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Some(cfg) = dirs::config_dir() {
            let autostart = cfg.join("autostart");
            let desktop_file = autostart.join("xent.desktop");
            if enabled {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::fs::create_dir_all(&autostart);
                    let content = format!(
                        "[Desktop Entry]\nType=Application\nName=Xent\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
                        exe.to_string_lossy()
                    );
                    let _ = std::fs::write(&desktop_file, content);
                }
            } else {
                let _ = std::fs::remove_file(&desktop_file);
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "android"))]
    {
        let _ = enabled;
    }
}

/// Create or remove Windows Explorer right-click "Send to -> Xent" shortcut + Start Menu AppID shortcut via native COM.
pub fn sync_send_to_shortcut(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        win_native::ensure_start_menu_shortcut();
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let sendto_lnk = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("SendTo")
            .join("Xent.lnk");

        if !enabled {
            let _ = std::fs::remove_file(&sendto_lnk);
            return;
        }

        if let Ok(exe) = std::env::current_exe() {
            win_native::create_shortcut(&sendto_lnk, &exe, "Send files with Xent", Some("com.xent.app"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
    }
}

/// Fire a real native OS desktop notification directly in-process (zero PowerShell).
pub fn send_os_notification(title: String, body: String) {
    let clean_title: String = title.chars().take(100).collect();
    let clean_body: String = body.chars().take(240).collect();

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            if !win_native::show_native_toast(&clean_title, &clean_body) {
                win_native::ensure_start_menu_shortcut();
                let _ = win_native::show_native_toast(&clean_title, &clean_body);
            }
        });
    }

    #[cfg(target_os = "macos")]
    {
        std::thread::spawn(move || {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                clean_body.replace('\"', "'"),
                clean_title.replace('\"', "'")
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output();
        });
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        std::thread::spawn(move || {
            let _ = std::process::Command::new("notify-send")
                .args([&clean_title, &clean_body])
                .output();
        });
    }

    #[cfg(target_os = "android")]
    {
        let _ = (clean_title, clean_body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_verifies_tokens() {
        assert!(constant_time_eq("abc123", "abc123"));
        assert!(!constant_time_eq("abc123", "abc124"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn expand_file_paths_walks_nested_directories() {
        let root = std::env::temp_dir().join(format!("xent_expand_{}", uuid::Uuid::new_v4()));
        let nested = root.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        let f1 = root.join("a.txt");
        let f2 = nested.join("b.txt");
        std::fs::write(&f1, b"1").unwrap();
        std::fs::write(&f2, b"2").unwrap();

        let expanded = expand_file_paths(vec![root.to_string_lossy().to_string()]);
        assert_eq!(expanded.len(), 2);
        assert!(expanded.iter().any(|p| p.ends_with("a.txt")));
        assert!(expanded.iter().any(|p| p.ends_with("b.txt")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_win_shortcut_and_toast_api_work() {
        let lnk = std::env::temp_dir().join(format!("xent_lnk_test_{}.lnk", uuid::Uuid::new_v4()));
        let exe = std::env::current_exe().unwrap();
        win_native::create_shortcut(&lnk, &exe, "Test shortcut", Some("com.xent.app"));
        assert!(lnk.exists(), "Native COM IShellLinkW must create .lnk file");
        let _ = std::fs::remove_file(&lnk);

        // Verify WinRT XmlDocument + ToastNotificationManager creation works on a spawned thread
        let ok = std::thread::spawn(|| {
            win_native::ensure_start_menu_shortcut();
            win_native::show_native_toast("Xent Test", "Native WinRT notification check")
        })
        .join()
        .unwrap();
        assert!(ok, "Native WinRT ToastNotificationManager.Show must succeed without PowerShell");
    }
}
