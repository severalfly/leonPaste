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
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Mutex,
    },
};
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
}

pub fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(Some(monitor)) = w.current_monitor() {
            let screen = monitor.size();
            if let Ok(win) = w.outer_size() {
                let x = (screen.width.saturating_sub(win.width)) / 2;
                let y = screen.height.saturating_sub(win.height.saturating_add(96));
                let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                    x: x as i32,
                    y: y as i32,
                }));
            }
        }
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("window-shown", ());
    }
}

pub fn hide_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

pub fn toggle_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

fn shortcut_from_str(s: &str) -> Shortcut {
    match s {
        "Ctrl+Alt+V" => Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV),
        "Alt+V" => Shortcut::new(Some(Modifiers::ALT), Code::KeyV),
        "Ctrl+`" => Shortcut::new(Some(Modifiers::CONTROL), Code::Backquote),
        _ => Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyV),
    }
}

pub fn register_hotkey(app: &tauri::AppHandle, key: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.register(shortcut_from_str(key))
        .map_err(|e| e.to_string())
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
    tauri::Builder::default()
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
                        toggle_main(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let (history, settings) = store::load(&handle);
            log_line(&handle, "setup-start");
            let hotkey = settings.hotkey.clone();
            let autostart_on = settings.autostart;
            app.manage(AppState {
                history: Mutex::new(history),
                settings: Mutex::new(settings),
                suppress: AtomicBool::new(false),
                last_tick: AtomicU64::new(0),
            });

            if autostart_on {
                let _ = handle.autolaunch().enable();
            }

            let show_i = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
            let pause_i = MenuItem::with_id(app, "pause", "暂停 / 恢复记录", true, None::<&str>)?;
            let clear_i = MenuItem::with_id(app, "clear", "清空历史（保留收藏）", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出 leonPaste", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show_i, &pause_i, &clear_i, &sep, &quit_i])?;

            let tray_result = TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("window icon").clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => toggle_main(app),
                    "pause" => {
                        let s = app.state::<AppState>();
                        let snap = {
                            let mut cfg = s.settings.lock().unwrap();
                            cfg.paused = !cfg.paused;
                            cfg.clone()
                        };
                        let h = s.history.lock().unwrap().clone();
                        let _ = store::save(app, &h, &snap);
                        let _ = app.emit("history-changed", ());
                    }
                    "clear" => {
                        let s = app.state::<AppState>();
                        let (h, cfg) = {
                            let mut hist = s.history.lock().unwrap();
                            hist.retain(|e| e.favorite);
                            (hist.clone(), s.settings.lock().unwrap().clone())
                        };
                        let _ = store::save(app, &h, &cfg);
                        let _ = app.emit("history-changed", ());
                    }
                    "quit" => app.exit(0),
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
                .build(app);
            match tray_result {
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
