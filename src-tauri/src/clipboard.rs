use crate::{
    dib, log_line, store,
    types::{classify, now, preview, Entry, EntryKind},
    AppState,
};
use std::{
    fs,
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

fn ignored(text: &str, words: &str) -> bool {
    let lower = text.to_lowercase();
    words
        .lines()
        .flat_map(|l| l.split(','))
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .any(|w| lower.contains(&w))
}

fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn start_monitor(app: tauri::AppHandle) {
    let mut last_seq = unsafe { GetClipboardSequenceNumber() };
    log_line(&app, &format!("monitor-start seq={} pid={}", last_seq, std::process::id()));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(180));
        let seq = unsafe { GetClipboardSequenceNumber() };
        if seq == last_seq {
            continue;
        }
        last_seq = seq;

        let state = app.state::<AppState>();
        if state.suppress.swap(false, Ordering::SeqCst) {
            log_line(&app, "skip: own write-back");
            continue;
        }
        let settings = { state.settings.lock().unwrap().clone() };
        if settings.paused {
            log_line(&app, "skip: paused");
            continue;
        }

        let text = clipboard_win::get_clipboard_string().unwrap_or_default();
        let dib_data = if text.trim().is_empty() {
            dib::read_clipboard_dib()
        } else {
            None
        };

        let id = format!("e{}", millis());
        let mut entry = Entry::blank();
        entry.id = id.clone();
        entry.created_at = now();
        entry.source = crate::foreground_title();

        if !text.trim().is_empty() {
            if ignored(&text, &settings.ignore_regex) {
                log_line(&app, "skip: ignored keyword");
                continue;
            }
            entry.kind = classify(&text);
            entry.preview = preview(&text);
            entry.text = text;
        } else if let Some(d) = dib_data {
            let info = match dib::parse(&d) {
                Some(v) => v,
                None => {
                    log_line(&app, &format!("skip: unsupported DIB ({})", dib::summary(&d)));
                    continue;
                }
            };
            let (thumb_dib, _tw, _th) = match dib::thumbnail(&d, 420, 260) {
                Some(v) => v,
                None => {
                    log_line(&app, "skip: thumbnail failed");
                    continue;
                }
            };
            let bmp = match dib::to_bmp(&thumb_dib) {
                Some(v) => v,
                None => continue,
            };
            let dir = store::data_dir(&app).join("images");
            if fs::create_dir_all(&dir).is_err() {
                continue;
            }
            let _ = fs::write(dir.join(format!("{}.dib", id)), &d);
            let _ = fs::write(dir.join(format!("{}.bmp", id)), &bmp);
            entry.kind = EntryKind::Image;
            entry.image = Some(format!("{}.dib", id));
            entry.image_width = info.width;
            entry.image_height = info.height;
            entry.preview = format!("图片 · {}×{}", info.width, info.height);
            entry.text = format!("image:{}", dib::hash(&d));
        } else {
            log_line(&app, "skip: no text and no DIB on clipboard");
            continue;
        }

        let mut history = state.history.lock().unwrap();
        if history.iter().rev().take(1).any(|e| e.text == entry.text) {
            log_line(&app, "skip: duplicate");
            continue;
        }
        history.push(entry);
        store::prune(&mut history, &settings);
        let snapshot = history.clone();
        drop(history);
        let _ = store::save(&app, &snapshot, &settings);
        log_line(&app, "captured");
        let _ = app.emit("history-changed", ());
    });
}
