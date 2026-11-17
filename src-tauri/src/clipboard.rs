use crate::{
    clip, dib, log_line, store,
    types::{classify, now, preview, Entry, EntryKind},
    AppState,
};
use std::{
    fs,
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

fn secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn start_monitor(app: tauri::AppHandle) {
    log_line(&app, "monitor-start (direct-read mode)");
    let mut last_sig = String::new();
    let mut busy_count = 0u32;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(180));
        let state = app.state::<AppState>();
        state.last_tick.store(secs(), Ordering::SeqCst);

        let cap = clip::read();
        if matches!(cap, clip::Capture::Busy) {
            busy_count += 1;
            if busy_count % 30 == 1 {
                log_line(&app, "clipboard busy, retrying");
            }
            continue;
        }

        let sig = cap.signature();
        if state.suppress.swap(false, Ordering::SeqCst) {
            last_sig = sig;
            log_line(&app, "skip: own write-back");
            continue;
        }
        if sig == last_sig {
            continue;
        }
        last_sig = sig;

        let settings = { state.settings.lock().unwrap().clone() };
        if settings.paused {
            log_line(&app, "skip: paused");
            continue;
        }

        let id = format!("e{}", millis());
        let mut entry = Entry::blank();
        entry.id = id.clone();
        entry.created_at = now();
        entry.source = crate::foreground_title();

        match cap {
            clip::Capture::Text(t) => {
                if ignored(&t, &settings.ignore_regex) {
                    log_line(&app, "skip: ignored keyword");
                    continue;
                }
                entry.kind = classify(&t);
                entry.preview = preview(&t);
                entry.text = t;
            }
            clip::Capture::Dib(d) => {
                let info = match dib::parse(&d) {
                    Some(v) => v,
                    None => {
                        log_line(&app, &format!("skip: unsupported DIB ({})", dib::summary(&d)));
                        continue;
                    }
                };
                let (thumb_dib, _w, _h) = match dib::thumbnail(&d, 420, 260) {
                    Some(v) => v,
                    None => continue,
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
            }
            clip::Capture::Files(files) => {
                let first = files
                    .first()
                    .map(|p| p.rsplit(['\\', '/']).next().unwrap_or(p))
                    .unwrap_or("");
                entry.kind = EntryKind::Files;
                entry.preview = format!("{} 个文件 · {}", files.len(), first);
                entry.text = files.join("\r\n");
            }
            clip::Capture::Empty => continue,
            clip::Capture::Busy => continue,
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
