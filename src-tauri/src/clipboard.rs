use crate::{
    clip, dib, log_line, store,
    types::{classify, now, preview, Entry, EntryKind},
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

/// 共享捕获逻辑：后台监听线程与前端主线程定时器都走这里，
/// 用 AppState.last_sig 做统一去重，避免重复入库。
pub fn process_capture(app: &tauri::AppHandle, cap: clip::Capture) -> bool {
    if matches!(cap, clip::Capture::Busy) {
        crate::stats::note_busy();
        return false;
    }
    if matches!(cap, clip::Capture::Empty) { return false; }
    let state = app.state::<AppState>();
    let sig = cap.signature();

    if state.suppress.swap(false, Ordering::SeqCst) {
        *state.last_sig.lock().unwrap_or_else(|e| e.into_inner()) = sig;
        log_line(app, "skip: own write-back");
        return false;
    }
    {
        let mut last = state.last_sig.lock().unwrap_or_else(|e| e.into_inner());
        if *last == sig {
            // 同签名是常态（180ms 轮询），不写日志避免刷屏
            return false;
        }
        *last = sig;
    }

    let settings = { state.settings.lock().unwrap_or_else(|e| e.into_inner()).clone() };
    if settings.paused {
        log_line(app, "skip: paused");
        return false;
    }

    let id = format!("e{}", millis());
    let mut entry = Entry::blank();
    entry.id = id.clone();
    let mut image_blob: Option<Vec<u8>> = None;
    let mut thumb_blob: Option<Vec<u8>> = None;
    entry.created_at = now();
    entry.source = crate::foreground_title();

    match cap {
        clip::Capture::Text(t) => {
            if ignored(&t, &settings.ignore_regex) {
                log_line(app, "skip: ignored keyword");
                return false;
            }
            entry.kind = classify(&t);
            entry.preview = preview(&t);
            entry.text = t;
        }
        clip::Capture::Dib(d) => {
            let info = match dib::parse(&d) {
                Some(v) => v,
                None => {
                    log_line(app, &format!("skip: unsupported DIB ({})", dib::summary(&d)));
                    return false;
                }
            };
            let (thumb_dib, _w, _h) = match dib::thumbnail(&d, 420, 260) {
                Some(v) => v,
                None => return false,
            };
            let bmp = match dib::to_bmp(&thumb_dib) {
                Some(v) => v,
                None => return false,
            };
            // 图片直接随条目进 SQLite BLOB，不再落盘成散落文件
            thumb_blob = Some(bmp);
            entry.kind = EntryKind::Image;
            entry.image = Some("db".into());
            entry.image_width = info.width;
            entry.image_height = info.height;
            entry.preview = format!("图片 · {}×{}", info.width, info.height);
            entry.text = format!("image:{}", dib::hash(&d));
            image_blob = Some(d);
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
        clip::Capture::Empty | clip::Capture::Busy => return false,
    }

    {
        let history = state.history.lock().unwrap_or_else(|e| e.into_inner());
        if history.iter().rev().take(1).any(|e| e.text == entry.text) {
            log_line(app, "skip: duplicate");
            return false;
        }
    }
    if let Err(e) = store::insert(app, &entry, image_blob.as_deref(), thumb_blob.as_deref()) {
        log_line(app, &format!("db-insert-failed: {}", e));
        return false;
    }
    // 清理降频：每 32 次捕获才查一次过期/超限，避免频繁 COUNT + 扫描
    // note_capture() 会递增 CAPTURES，这里用其返回值判断即可（不要重复 fetch_add）
    let tick = crate::stats::note_capture();
    if tick % 32 == 1 { let _ = store::prune(app, &settings); }
    // 直接把新条目 push 到内存，不再全表 load_entries（省一次完整 DB 读）
    {
        let mut history = state.history.lock().unwrap_or_else(|e| e.into_inner());
        history.retain(|e| e.id != entry.id);
        history.push(entry.clone());
    }
    log_line(app, &format!("captured id={} text_len={}", entry.id, entry.text.len()));
    let _ = app.emit("history-changed", ());
    true
}

/// 后台线程继续读剪贴板（窗口隐藏时 WebView2 会节流 JS 定时器，主线程
/// sync_clipboard 可能变慢），但所有读写已被 CLIP_LOCK 串行化。
pub fn start_monitor(app: tauri::AppHandle) {
    log_line(&app, "monitor-start (background thread, serialized clipboard access)");
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(180));
        let a = app.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            {
                let state = a.state::<AppState>();
                state.last_tick.store(secs(), Ordering::SeqCst);
            }
            crate::stats::note_tick();
            let cap = clip::read();
            process_capture(&a, cap);
        }));
    });
}
