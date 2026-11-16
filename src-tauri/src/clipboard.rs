use crate::{
    store,
    types::{classify, now, preview, Entry},
    AppState,
};
use std::{
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

fn ignored(text: &str, words: &str) -> bool {
    let lower = text.to_lowercase();
    words
        .lines()
        .flat_map(|l| l.split(','))
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .any(|w| lower.contains(&w))
}

pub fn start_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(280));
            let state = app.state::<AppState>();
            if state.suppress.swap(false, Ordering::SeqCst) {
                last = clipboard_win::get_clipboard_string().unwrap_or_default();
                continue;
            }
            let text = match clipboard_win::get_clipboard_string() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if text.trim().is_empty() || text == last {
                continue;
            }
            last = text.clone();

            let settings = { state.settings.lock().unwrap().clone() };
            if settings.paused || ignored(&text, &settings.ignore_regex) {
                continue;
            }

            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let entry = Entry {
                id: format!("e{}", millis),
                kind: classify(&text),
                preview: preview(&text),
                text,
                favorite: false,
                created_at: now(),
                source: crate::foreground_title(),
            };

            let mut history = state.history.lock().unwrap();
            if history.iter().rev().take(1).any(|e| e.text == entry.text) {
                continue;
            }
            history.push(entry);
            store::prune(&mut history, &settings);
            let snapshot = history.clone();
            drop(history);
            let _ = store::save(&app, &snapshot, &settings);
            let _ = app.emit("history-changed", ());
        }
    });
}
