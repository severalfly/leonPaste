mod clip;
mod clipboard;
mod dib;
mod commands;
mod stats;
mod store;
pub mod types;

use crate::types::{Entry, Settings};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};
use windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub struct AppState {
    pub history: Mutex<Vec<Entry>>,
    pub settings: Mutex<Settings>,
    pub suppress: AtomicBool,
    pub last_tick: AtomicU64,
    pub last_sig: Mutex<String>,
}

/// 只有托盘菜单「退出」会置位；其余任何退出请求一律拦下并记日志。
pub static USER_QUIT: AtomicBool = AtomicBool::new(false);
/// 供 SEH 崩溃回调使用的日志路径（崩溃时只能用最简单的文件 IO）。
static CRASH_LOG: OnceLock<String> = OnceLock::new();
/// 所有剪贴板读写共用一把锁，杜绝后台线程与主线程并发 OpenClipboard / GlobalLock。
pub static CLIP_LOCK: Mutex<()> = Mutex::new(());
/// 崩溃前的最后操作码：0=空闲 1=打开剪贴板 2=文件列表 3=文本 4=位图。
pub static LAST_OP: AtomicU8 = AtomicU8::new(0);
/// 显示器变化后的托盘重建防重入。
static REBUILD_PENDING: AtomicBool = AtomicBool::new(false);

fn cursor_point() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut p) } != 0 {
        Some((p.x, p.y))
    } else {
        None
    }
}

// 每次唤起都按鼠标所在显示器重新计算窗口：先定逻辑(CSS)尺寸，再乘该屏
// scale_factor 换算成物理像素。100% / 125% / 150% 缩放、外接屏与笔记本屏
// 都能得到一致的布局比例，且窗口横向铺满、不会超出屏幕。
fn fit_window(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let monitors = app.available_monitors().unwrap_or_default();
    let mut picked: Option<(f64, u32, u32, i32, i32)> = None;
    if let Some((cx, cy)) = cursor_point() {
        for m in &monitors {
            let p = m.position();
            let s = m.size();
            if cx >= p.x && cx < p.x + s.width as i32 && cy >= p.y && cy < p.y + s.height as i32 {
                picked = Some((m.scale_factor(), s.width, s.height, p.x, p.y));
                break;
            }
        }
    }
    if picked.is_none() {
        if let Some(m) = w.current_monitor().ok().flatten().or_else(|| app.primary_monitor().ok().flatten()) {
            picked = Some((m.scale_factor(), m.size().width, m.size().height, m.position().x, m.position().y));
        }
    }
    if picked.is_none() {
        if let Some(m) = monitors.first() {
            picked = Some((m.scale_factor(), m.size().width, m.size().height, m.position().x, m.position().y));
        }
    }
    let Some((raw_scale, sw, sh, px, py)) = picked else { return };
    let scale = if raw_scale > 0.5 { raw_scale } else { 1.0 };

    let logical_w = sw as f64 / scale;
    let logical_h = sh as f64 / scale;
    let width = (logical_w - 28.0).max(680.0);
    let height = (logical_h * 0.46).clamp(320.0, 440.0);

    let phys_w = (width * scale).round() as u32;
    let phys_h = (height * scale).round() as u32;
    let _ = w.set_size(tauri::Size::Physical(tauri::PhysicalSize { width: phys_w, height: phys_h }));

    let x = px + (((sw as i64 - phys_w as i64) / 2).max(0)) as i32;
    let gap = (72.0 * scale).round() as i64;
    let y = py + ((sh as i64 - phys_h as i64 - gap).max(0)) as i32;
    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

pub fn show_main(app: &tauri::AppHandle) {
    log_line(app, "show: begin");
    ensure_tray(app, true);
    if let Some(w) = app.get_webview_window("main") {
        fit_window(app);
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("window-shown", ());
    }
    log_line(app, "show: done");
}

pub fn hide_main(app: &tauri::AppHandle) {
    log_line(app, "hide");
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

pub fn toggle_main(app: &tauri::AppHandle) {
    log_line(app, "toggle");
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

pub fn log_line(app: &tauri::AppHandle, msg: &str) {
    let p = store::data_dir(app).join("leonpaste.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = writeln!(f, "[{}] {}", types::now(), msg);
    }
}

fn append_log(path: &std::path::Path, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{}", line);
    }
}

/// 任何线程里的 panic 都先写进日志，便于事后判断退出原因。
fn install_panic_hook(path: PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(v) = info.payload().downcast_ref::<&str>() {
            (*v).to_string()
        } else if let Some(v) = info.payload().downcast_ref::<String>() {
            v.clone()
        } else {
            "unknown payload".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        append_log(&path, &format!("[{}] PANIC: {} @ {}", types::now(), msg, loc));
        default_hook(info);
    }));
}

/// 捕获 native 层的未处理异常（access violation 等），写一行异常码再交给系统。
/// Rust 的 catch_unwind 拦不住这类异常，日志以前会「戛然而止」而无线索。
unsafe extern "system" fn crash_filter(info: *const EXCEPTION_POINTERS) -> i32 {
    let code = if info.is_null() {
        0u32
    } else {
        let rec = (*info).ExceptionRecord;
        if rec.is_null() { 0u32 } else { (*rec).ExceptionCode as u32 }
    };
    if let Some(p) = CRASH_LOG.get() {
        append_log(
            std::path::Path::new(p),
            &format!(
                "[{}] SEH_EXCEPTION code=0x{:08X} last_op={}",
                types::now(),
                code,
                LAST_OP.load(Ordering::Relaxed)
            ),
        );
    }
    1 // EXCEPTION_EXECUTE_HANDLER
}

fn install_crash_guard(path: PathBuf) {
    let _ = CRASH_LOG.set(path.to_string_lossy().to_string());
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
        use windows_sys::Win32::System::Recovery::RegisterApplicationRestart;
        SetUnhandledExceptionFilter(Some(crash_filter));
        // 崩溃后由 Windows 尝试自动拉起，托盘不至于一直消失
        let _ = RegisterApplicationRestart(std::ptr::null(), 0);
    }
}

/// 托盘是唯一的常驻入口，抽成函数便于丢失后整体重建。
fn build_tray(app: &tauri::AppHandle, hotkey: &str) -> Result<(), String> {
    let show_i = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let pause_i = MenuItem::with_id(app, "pause", "暂停 / 恢复记录", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let clear_i = MenuItem::with_id(app, "clear", "清空历史（保留收藏）", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit_i = MenuItem::with_id(app, "quit", "退出 leonPaste", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let status_i = MenuItem::with_id(
        app,
        "status",
        &format!("leonPaste {} · 后台运行中 · {} 唤起", env!("CARGO_PKG_VERSION"), hotkey),
        false,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&status_i, &sep, &show_i, &pause_i, &clear_i, &sep, &quit_i])
        .map_err(|e| e.to_string())?;
    let icon = app
        .default_window_icon()
        .ok_or_else(|| "window icon missing".to_string())?
        .clone();

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => toggle_main(app),
            "pause" => {
                let s = app.state::<AppState>();
                let snap = {
                    let mut cfg = s.settings.lock().unwrap_or_else(|e| e.into_inner());
                    cfg.paused = !cfg.paused;
                    cfg.clone()
                };
                let _ = store::save_settings(app, &snap);
                let _ = app.emit("history-changed", ());
            }
            "clear" => {
                let s = app.state::<AppState>();
                {
                    let mut hist = s.history.lock().unwrap_or_else(|e| e.into_inner());
                    hist.retain(|e| e.favorite);
                }
                let _ = store::clear_non_favorites(app);
                let _ = app.emit("history-changed", ());
            }
            "quit" => {
                USER_QUIT.store(true, Ordering::SeqCst);
                log_line(app, "quit: tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 托盘对象丢失就整体重建；refresh=true 时顺手刷新图标，
/// 应对资源管理器重启后「进程还活着，但通知区域图标被清掉」的情况。
fn rebuild_tray(app: &tauri::AppHandle) {
    let _ = app.remove_tray_by_id("main");
    let hotkey = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .hotkey
        .clone();
    match build_tray(app, &hotkey) {
        Ok(_) => log_line(app, "tray-rebuilt"),
        Err(e) => log_line(app, &format!("tray-rebuild-failed: {}", e)),
    }
}

fn ensure_tray(app: &tauri::AppHandle, refresh: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        if refresh {
            if let Some(icon) = app.default_window_icon() {
                let _ = tray.set_icon(Some(icon.clone()));
            }
        }
        return;
    }
    log_line(app, "tray-missing -> rebuild");
    rebuild_tray(app);
}

/// 显示器数量 / 分辨率 / 缩放 / 位置的指纹。外接屏 ↔ 笔记本屏切换时一定会变。
fn monitor_signature(app: &tauri::AppHandle) -> String {
    app.available_monitors()
        .map(|list| {
            let mut v: Vec<String> = list
                .iter()
                .map(|m| {
                    let s = m.size();
                    let p = m.position();
                    format!(
                        "{}x{}@{}+{}+{}",
                        s.width,
                        s.height,
                        (m.scale_factor() * 100.0) as u32,
                        p.x,
                        p.y
                    )
                })
                .collect();
            v.sort();
            format!("{}={}", v.len(), v.join(","))
        })
        .unwrap_or_else(|_| "err".to_string())
}

/// 显示器拓扑变化后 Windows 会重建通知区域，托盘图标常被清掉。
/// 在累计 1s / 3s / 8s / 15s 分四次重建，覆盖 explorer 重建的整个时间窗。
fn schedule_tray_rebuild(app: &tauri::AppHandle) {
    if REBUILD_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let h = app.clone();
    std::thread::spawn(move || {
        for delay in [1000u64, 2000, 5000, 7000] {
            std::thread::sleep(Duration::from_millis(delay));
            if USER_QUIT.load(Ordering::SeqCst) {
                break;
            }
            let hh = h.clone();
            if h.run_on_main_thread(move || {
                log_line(&hh, "tray-rebuild-after-display-change");
                rebuild_tray(&hh);
            })
            .is_err()
            {
                break;
            }
        }
        REBUILD_PENDING.store(false, Ordering::SeqCst);
    });
}

/// 每 5 秒检测显示器拓扑；alive 每 20 秒一条。日志尾部可直接区分：
/// 进程崩溃 / 被要求退出 / 进程仍在运行，以及退出是否发生在切屏瞬间。
fn start_heartbeat(app: tauri::AppHandle) {
    let initial = monitor_signature(&app);
    log_line(&app, &format!("display-topology: {}", initial));
    std::thread::spawn(move || {
        let mut n: u64 = 0;
        let mut last_sig = initial;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            if USER_QUIT.load(Ordering::SeqCst) {
                break;
            }
            n += 1;
            // 切屏瞬间不碰窗口（fit_window 只在用户唤起面板时执行），
            // 避免 DPI 切换过程中 resize WebView2 窗口。
            let sig = monitor_signature(&app);
            if sig != last_sig {
                log_line(&app, &format!("display-topology-changed: {} -> {}", last_sig, sig));
                last_sig = sig.clone();
                schedule_tray_rebuild(&app);
            }
            if n % 4 != 0 {
                continue;
            }
            let (tick, entries) = {
                let s = app.state::<AppState>();
                let t = s.last_tick.load(Ordering::SeqCst);
                let n = s.history.lock().unwrap_or_else(|e| e.into_inner()).len();
                (t, n)
            };
            let tray_alive = app.tray_by_id("main").is_some();
            log_line(&app, &format!("alive #{} tick={} entries={} tray={} mon={}", n / 4, tick, entries, tray_alive, sig));
            // 每 60 秒在主线程无条件重建托盘，作为兜底
            if (n / 4) % 3 == 0 {
                let h = app.clone();
                let _ = app.run_on_main_thread(move || {
                    log_line(&h, "tray-periodic-rebuild: begin");
                    rebuild_tray(&h);
                });
            }
        }
    });
}

fn parse_code(raw: &str) -> Option<Code> {
    let u = raw.trim().to_uppercase();
    const LETTERS: [Code; 26] = [
        Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF,
        Code::KeyG, Code::KeyH, Code::KeyI, Code::KeyJ, Code::KeyK, Code::KeyL,
        Code::KeyM, Code::KeyN, Code::KeyO, Code::KeyP, Code::KeyQ, Code::KeyR,
        Code::KeyS, Code::KeyT, Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX,
        Code::KeyY, Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4,
        Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
    ];
    if u.len() == 1 {
        let b = u.as_bytes()[0];
        if b.is_ascii_uppercase() {
            return Some(LETTERS[(b - b'A') as usize]);
        }
        if b.is_ascii_digit() {
            return Some(DIGITS[(b - b'0') as usize]);
        }
    }
    Some(match u.as_str() {
        "F1" => Code::F1, "F2" => Code::F2, "F3" => Code::F3, "F4" => Code::F4,
        "F5" => Code::F5, "F6" => Code::F6, "F7" => Code::F7, "F8" => Code::F8,
        "F9" => Code::F9, "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
        "SPACE" => Code::Space, "ENTER" => Code::Enter, "TAB" => Code::Tab,
        "BACKQUOTE" | "`" => Code::Backquote, "MINUS" | "-" => Code::Minus,
        "EQUAL" | "=" => Code::Equal, "ESC" | "ESCAPE" => Code::Escape,
        _ => return None,
    })
}

pub fn parse_shortcut(s: &str) -> Option<Shortcut> {
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    for part in s.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            other => key = parse_code(other),
        }
    }
    Some(Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, key?))
}

pub fn register_hotkey(app: &tauri::AppHandle, key: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let sc = parse_shortcut(key).ok_or_else(|| format!("无法识别快捷键: {}", key))?;
    gs.register(sc).map_err(|e| e.to_string())
}

pub fn foreground_title() -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n > 0 {
            let s = String::from_utf16_lossy(&buf[..n as usize]);
            let s = s.trim().to_string();
            if !s.is_empty() {
                return s.chars().take(80).collect();
            }
        }
    }
    "桌面".to_string()
}

pub fn auto_paste() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::keybd_event;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_CONTROL: u8 = 0x11;
    const VK_V: u8 = 0x56;
    unsafe {
        keybd_event(VK_CONTROL, 0, 0, 0);
        keybd_event(VK_V, 0, 0, 0);
        keybd_event(VK_V, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        log_line(app, "hotkey: pressed");
                        toggle_main(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let log_path = store::data_dir(&handle).join("leonpaste.log");
            install_panic_hook(log_path.clone());
            install_crash_guard(log_path);
            let (history, settings) = store::load(&handle);
            log_line(&handle, &format!("setup-start v{} pid={}", env!("CARGO_PKG_VERSION"), std::process::id()));
            let hotkey = settings.hotkey.clone();
            let autostart_on = settings.autostart;
            app.manage(AppState {
                history: Mutex::new(history),
                settings: Mutex::new(settings),
                suppress: AtomicBool::new(false),
                last_tick: AtomicU64::new(0),
                last_sig: Mutex::new(String::new()),
            });

            if autostart_on {
                let _ = handle.autolaunch().enable();
            }

            match build_tray(&handle, &hotkey) {
                Ok(_) => log_line(&handle, "tray-ok"),
                Err(e) => log_line(&handle, &format!("tray-failed: {}", e)),
            }

            match register_hotkey(&handle, &hotkey) {
                Ok(_) => log_line(&handle, "hotkey-ok"),
                Err(e) => log_line(&handle, &format!("hotkey-failed: {}", e)),
            }
            let first_run = !store::data_dir(&handle).join("settings.json").exists();
            if first_run {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1100));
                    show_main(&h);
                    crate::log_line(&h, "first-run panel shown");
                });
            }

            clipboard::start_monitor(handle.clone());
            log_line(&handle, "monitor-started");
            if let Some(w) = app.get_webview_window("main") {
                let h = handle.clone();
                w.on_window_event(move |ev| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                        api.prevent_close();
                        if let Some(win) = h.get_webview_window("main") {
                            let _ = win.hide();
                        }
                        log_line(&h, "window-close prevented -> hidden");
                    }
                });
            }
            start_heartbeat(handle.clone());
            log_line(&handle, "heartbeat-started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_history,
            commands::paste_entry,
            commands::toggle_favorite,
            commands::delete_entry,
            commands::clear_history,
            commands::get_settings,
            commands::save_settings,
            commands::hide_window,
            commands::get_diagnostics,
            commands::open_data_dir,
            commands::probe_clipboard,
            commands::sync_clipboard,
            commands::list_all,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    let exit_handle = built.handle().clone();
    built.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            if USER_QUIT.load(Ordering::SeqCst) {
                log_line(app_handle, "exit: tray menu requested");
            } else {
                log_line(app_handle, "exit: blocked unexpected ExitRequested");
                api.prevent_exit();
            }
        }
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::Destroyed,
            ..
        } => log_line(app_handle, "window-destroyed (kept alive)"),
        _ => {}
    });
    log_line(&exit_handle, "exit: event loop ended");
}
