//! User settings (config.json) and local state paths. Privacy options default to the most private value.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Master switch. When false, Discord activity is cleared but sessions are still tracked.
    pub enabled: bool,
    /// Show the project folder name in Discord. Off by default.
    pub show_project: bool,
    /// Discord art-asset key (or HTTPS URL) for the large image. Empty uses none.
    pub large_image: String,
    /// Minutes without any event before a hook-only session is cleared.
    pub stale_minutes: i64,
}

impl Default for Settings {
    fn default() -> Self {
        // "ctrl_alt_delegate_icon" is the Rich Presence art asset uploaded to the Ctrl Alt Delegate Discord application.
        Settings { enabled: true, show_project: false, large_image: "ctrl_alt_delegate_icon".into(), stale_minutes: 30 }
    }
}

const KEYS: [&str; 4] = ["enabled", "showProject", "largeImage", "staleMinutes"];
/// Settings the tray may change. Others are edited by hand in the settings file.
pub const TOGGLES: [&str; 2] = ["enabled", "showProject"];

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default()
}

/// Roaming config: %APPDATA%\CtrlAltDelegate (Windows), ~/Library/Application Support/CtrlAltDelegate (macOS).
pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Roaming")).join("CtrlAltDelegate")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/CtrlAltDelegate")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("ctrl-alt-delegate")
    }
}

/// Machine-local state and logs: %LOCALAPPDATA%\CtrlAltDelegate on Windows.
pub fn state_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Local")).join("CtrlAltDelegate")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/CtrlAltDelegate")
    } else {
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("ctrl-alt-delegate")
    }
}

pub fn settings_path() -> PathBuf {
    std::env::var_os("CAD_CONFIG").map(PathBuf::from).unwrap_or_else(|| config_dir().join("config.json"))
}

pub fn state_path() -> PathBuf {
    std::env::var_os("CAD_STATE").map(PathBuf::from).unwrap_or_else(|| state_dir().join("sessions.json"))
}

pub fn validate(value: &Value) -> Result<Settings, String> {
    let obj = value.as_object().ok_or("Expected settings object")?;
    let unknown: Vec<&str> = obj.keys().map(String::as_str).filter(|k| !KEYS.contains(k)).collect();
    if !unknown.is_empty() {
        return Err(format!("Unknown setting: {}", unknown.join(", ")));
    }
    let mut s = Settings::default();
    let boolean = |key: &str, default: bool| match obj.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err("enabled and showProject must be true or false".to_string()),
    };
    s.enabled = boolean("enabled", s.enabled)?;
    s.show_project = boolean("showProject", s.show_project)?;
    if let Some(v) = obj.get("largeImage") {
        let image = v.as_str().ok_or("largeImage must be a Discord art-asset key or an HTTPS URL")?;
        let key_ok = !image.is_empty() && image.len() <= 256 && image.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let url_ok = image.len() <= 256 && image.starts_with("https://") && !image.contains(char::is_whitespace) && !image.contains('@');
        if !(image.is_empty() || key_ok || url_ok) {
            return Err("largeImage must be a Discord art-asset key or an HTTPS URL".into());
        }
        s.large_image = image.to_string();
    }
    if let Some(v) = obj.get("staleMinutes") {
        s.stale_minutes = v.as_i64().filter(|m| (1..=1440).contains(m)).ok_or("staleMinutes must be 1–1440")?;
    }
    Ok(s)
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}')).map(Some).map_err(|e| format!("invalid JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// A missing file yields defaults; an invalid file is an error so callers can keep the last valid settings.
pub fn load(path: &Path) -> Result<Settings, String> {
    match read_json(path)? {
        None => Ok(Settings::default()),
        Some(v) => validate(&v),
    }
}

/// Atomic write: temp file then rename.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

/// Apply a toggle patch, preserving the user's other settings.
pub fn save_toggles(path: &Path, patch: &Value) -> Result<Settings, String> {
    let patch = patch.as_object().filter(|p| !p.is_empty()).ok_or("Expected settings patch")?;
    if patch.keys().any(|k| !TOGGLES.contains(&k.as_str())) {
        return Err(format!("Only {} can be changed here", TOGGLES.join(", ")));
    }
    let mut current: Map<String, Value> = match read_json(path) {
        Ok(Some(Value::Object(m))) => m,
        Ok(None) => Map::new(),
        _ => return Err("Settings file is invalid; fix it before changing settings".into()),
    };
    for (k, v) in patch {
        current.insert(k.clone(), v.clone());
    }
    let merged = Value::Object(current);
    let next = validate(&merged)?;
    write_atomic(path, &(serde_json::to_string_pretty(&merged).unwrap() + "\n")).map_err(|e| e.to_string())?;
    Ok(next)
}

pub fn to_json(s: &Settings) -> Value {
    json!({"enabled": s.enabled, "showProject": s.show_project, "largeImage": s.large_image, "staleMinutes": s.stale_minutes})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cad-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("config.json")
    }

    #[test]
    fn defaults_are_private_and_bad_values_are_rejected() {
        assert_eq!(validate(&json!({})).unwrap(), Settings::default());
        assert!(!Settings::default().show_project);
        for bad in [json!({"summaries": true}), json!({"largeImage": "http://x"}), json!({"largeImage": "a b"}), json!({"staleMinutes": 0}), json!({"enabled": "yes"})] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
        assert_eq!(validate(&json!({"largeImage": "ctrl_alt_delegate"})).unwrap().large_image, "ctrl_alt_delegate");
    }

    #[test]
    fn toggles_preserve_other_settings_and_missing_file_means_defaults() {
        let path = temp("toggle");
        assert_eq!(load(&path).unwrap(), Settings::default());
        write_atomic(&path, r#"{"largeImage": "keep_me", "staleMinutes": 15}"#).unwrap();
        save_toggles(&path, &json!({"showProject": true, "enabled": false})).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved, json!({"largeImage": "keep_me", "staleMinutes": 15, "showProject": true, "enabled": false}));
        assert!(save_toggles(&path, &json!({"largeImage": "x"})).unwrap_err().contains("Only enabled, showProject"));
        assert!(save_toggles(&path, &json!({"enabled": "yes"})).is_err());
        std::fs::write(&path, "\u{feff}{broken").unwrap();
        assert!(load(&path).is_err());
        assert!(save_toggles(&path, &json!({"enabled": true})).unwrap_err().contains("invalid"));
        std::fs::write(&path, "\u{feff}{\"enabled\": false}").unwrap();
        assert!(!load(&path).unwrap().enabled, "a UTF-8 BOM from Notepad is accepted");
    }
}
