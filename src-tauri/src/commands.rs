use crate::{
    store,
    types::{Entry, Settings},
    AppState,
};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

#[tauri::command]
pub fn list_history(
    state: State<'_, AppState>,
    query: String,
    filter: String,
    favorites_only: bool,
) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let history = state.history.lock().unwrap();
    let mut out: Vec<Entry> = history
        .iter()
        .filter(|e| {
            if favorites_only && !e.favorite {
                return false;
            }
            if !q.is_empty() && !e.text.to_lowercase().contains(&q) {
                return false;
            }
            if filter == "all" {
                return true;
            }
            let kind = format!("{:?}", e.kind).to_lowercase();
            kind == filter
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then(b.created_at.cmp(&a.created_at))
    });
    if out.len() > 200 {
        out.truncate(200);
    }
    out
}

#[tauri::command]
pub fn paste_entry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let text = {
        let history = state.history.lock().unwrap();
        history
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.text.clone())
            .ok_or_else(|| "找不到该条目".to_string())?
    };
    state.suppress.store(true, Ordering::SeqCst);
    clipboard_win::set_clipboard_string(&text)
        .map_err(|_| "写入剪贴板失败".to_string())?;
    let auto_paste = { state.settings.lock().unwrap().auto_paste };
    crate::hide_main(&app);
    if auto_paste {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(140));
            crate::auto_paste();
        });
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_favorite(app: tauri::AppHandle, state: State<'_, AppState>, id: String) {
    let (snapshot, settings) = {
        let mut history = state.history.lock().unwrap();
        if let Some(e) = history.iter_mut().find(|e| e.id == id) {
            e.favorite = !e.favorite;
        }
        (history.clone(), state.settings.lock().unwrap().clone())
    };
    let _ = store::save(&app, &snapshot, &settings);
    let _ = app.emit("history-changed", ());
}

#[tauri::command]
pub fn delete_entry(app: tauri::AppHandle, state: State<'_, AppState>, id: String) {
    let (snapshot, settings) = {
        let mut history = state.history.lock().unwrap();
        history.retain(|e| e.id != id);
        (history.clone(), state.settings.lock().unwrap().clone())
    };
    let _ = store::save(&app, &snapshot, &settings);
    let _ = app.emit("history-changed", ());
}

#[tauri::command]
pub fn clear_history(app: tauri::AppHandle, state: State<'_, AppState>) {
    let (snapshot, settings) = {
        let mut history = state.history.lock().unwrap();
        history.retain(|e| e.favorite);
        (history.clone(), state.settings.lock().unwrap().clone())
    };
    let _ = store::save(&app, &snapshot, &settings);
    let _ = app.emit("history-changed", ());
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    let old = { state.settings.lock().unwrap().clone() };
    if old.hotkey != settings.hotkey {
        if let Err(e) = crate::register_hotkey(&app, &settings.hotkey) {
            crate::log_line(&app, &format!("hotkey-change-failed: {}", e));
        }
    }
    if old.autostart != settings.autostart {
        use tauri_plugin_autostart::ManagerExt;
        let auto = app.autolaunch();
        if settings.autostart {
            let _ = auto.enable();
        } else {
            let _ = auto.disable();
        }
    }
    {
        let mut guard = state.settings.lock().unwrap();
        *guard = settings.clone();
    }
    let history = { state.history.lock().unwrap().clone() };
    store::save(&app, &history, &settings)?;
    let _ = app.emit("settings-changed", ());
    Ok(())
}

#[tauri::command]
pub fn hide_window(app: tauri::AppHandle) {
    crate::hide_main(&app);
}
