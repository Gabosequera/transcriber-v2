//! Rutas de usuario: configuración, logs y cachés fuera del proyecto y del ejecutable.

use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("TRANSCRIPTOR_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("Transcriptor")
}

pub fn logs_dir() -> PathBuf {
    if let Ok(p) = std::env::var("TRANSCRIPTOR_LOGS_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| config_dir());
    base.join("Transcriptor").join("logs")
}

pub fn ui_state_file() -> PathBuf {
    config_dir().join("ui-state.json")
}

pub fn keymap_file() -> PathBuf {
    config_dir().join("keymap.json")
}

pub fn cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("TRANSCRIPTOR_CACHE_DIR") {
        return PathBuf::from(p);
    }
    std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| config_dir()).join("Transcriptor").join("cache").join("media-v1")
}
