//! Rutas de usuario: configuración, logs y cachés fuera del proyecto y del ejecutable.

use std::path::PathBuf;

/// Mirrors V1 app_paths without importing Python or writing its configuration.
pub fn v1_marks_stores() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("TRANSCRIPTOR_V1_MARKS_STORE") {
        return vec![PathBuf::from(path)];
    }
    if let Some(root) = std::env::var_os("TRANSCRIPTOR_ROOT").filter(|r| !r.is_empty()) {
        let shared = std::env::var_os("TRANSCRIPTOR_SHARED_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(root).join("shared"));
        return vec![shared.join("marcas_store")];
    }
    if let Some(root) = std::env::var_os("TRANSCRIPTOR_V1_DIR") {
        return vec![PathBuf::from(root).join("marcas_store")];
    }
    // Discover a sibling installation relative to the running executable, never
    // embed the build machine's checkout path in the installed product.
    std::env::current_exe()
        .ok()
        .map(|exe| exe.parent().into_iter().flat_map(|dir| dir.ancestors().take(4)).map(|dir| dir.join("transcriber/marcas_store")).collect())
        .unwrap_or_default()
}

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
