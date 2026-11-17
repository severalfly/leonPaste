use crate::types::{now, Entry, Settings};
use std::{fs, path::PathBuf};
use tauri::Manager;

pub fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.dat").exists() {
                return dir.join("data");
            }
        }
    }
    app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn history_path(app: &tauri::AppHandle) -> PathBuf {
    data_dir(app).join("history.json")
}
fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    data_dir(app).join("settings.json")
}

pub fn load(app: &tauri::AppHandle) -> (Vec<Entry>, Settings) {
    let dir = data_dir(app);
    let _ = fs::create_dir_all(&dir);
    let history: Vec<Entry> = fs::read_to_string(history_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let settings: Settings = fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    (history, settings)
}

pub fn save(app: &tauri::AppHandle, history: &[Entry], settings: &Settings) -> Result<(), String> {
    let dir = data_dir(app);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (path, value) in [
        (history_path(app), serde_json::to_string_pretty(history)),
        (settings_path(app), serde_json::to_string_pretty(settings)),
    ] {
        let body = value.map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, body).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn remove_images(app: &tauri::AppHandle, entries: &[Entry]) {
    let dir = data_dir(app).join("images");
    for e in entries {
        if let Some(name) = &e.image {
            let stem = name.trim_end_matches(".dib");
            let _ = fs::remove_file(dir.join(format!("{}.dib", stem)));
            let _ = fs::remove_file(dir.join(format!("{}.bmp", stem)));
        }
    }
}

pub fn prune(history: &mut Vec<Entry>, settings: &Settings) {
    if settings.retention_days > 0 {
        let cutoff = now().saturating_sub(settings.retention_days as u64 * 86_400);
        history.retain(|e| e.favorite || e.created_at >= cutoff);
    }
    if history.len() > settings.max_items {
        let mut kept: Vec<Entry> = history.iter().filter(|e| e.favorite).cloned().collect();
        for e in history.iter().rev() {
            if kept.len() >= settings.max_items {
                break;
            }
            if !e.favorite {
                kept.push(e.clone());
            }
        }
        kept.sort_by_key(|e| e.created_at);
        *history = kept;
    }
}
