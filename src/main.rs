#![windows_subsystem = "windows"]

mod auto_start;
mod config;
mod monitors;
mod tray;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, MSG, PeekMessageW, PM_REMOVE,
    PostQuitMessage, PostMessageW, RegisterClassExW, TranslateMessage,
    WM_APP, WM_COMMAND, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey, VK_B,
};
use windows::core::w;

use auto_start::{is_autostart_enabled, set_autostart};
use config::load_config;
use monitors::{enumerate_monitors, get_cursor_monitor_index, set_brightness};
use tray::{
    FORCE_DISABLE_REQUESTED, FORCE_ENABLE_REQUESTED, RELOAD_REQUESTED,
    TOGGLE_AUTOSTART_REQUESTED, TOGGLE_ENABLED_REQUESTED, WM_TRAY,
    add_tray_icon, handle_menu_command, handle_tray_message, remove_tray_icon,
};

// IPC messages sent from CLI instances to the running instance
const WM_IPC_TOGGLE: u32 = WM_APP + 10;
const WM_IPC_ENABLE: u32 = WM_APP + 11;
const WM_IPC_DISABLE: u32 = WM_APP + 12;
const WM_IPC_RELOAD: u32 = WM_APP + 13;
const WM_IPC_EXIT: u32 = WM_APP + 14;
const WINDOW_CLASS: &str = "screen-light-window";

fn send_ipc(msg: u32) {
    let class_wide: Vec<u16> = WINDOW_CLASS.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if let Ok(hwnd) = FindWindowW(
            windows::core::PCWSTR(class_wide.as_ptr()),
            None,
        ) {
            if !hwnd.is_invalid() {
                let _ = PostMessageW(hwnd, msg, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn main() -> Result<()> {
    // Handle CLI flags
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--toggle"  => send_ipc(WM_IPC_TOGGLE),
            "--enable"  => send_ipc(WM_IPC_ENABLE),
            "--disable" => send_ipc(WM_IPC_DISABLE),
            "--reload"  => send_ipc(WM_IPC_RELOAD),
            "--exit"    => send_ipc(WM_IPC_EXIT),
            _ => {}
        }
        return Ok(());
    }
    // Single instance check
    unsafe {
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        let _ = CreateMutexW(None, false, w!("Local\\screen-light-mutex"));
        if monitors::get_last_error() == ERROR_ALREADY_EXISTS.0 {
            return Ok(());
        }
    }

    let hwnd = create_message_window()?;
    let mut config = load_config()?;
    add_tray_icon(hwnd)?;

    // Start config file watcher
    let _config_watcher = config::get_config_path()
        .ok()
        .and_then(|p| config::ConfigWatcher::new(p, config_watcher_callback).ok());

    let mut inactive_since: HashMap<isize, Option<Instant>> = HashMap::new();
    let mut dimmed: HashMap<isize, bool> = HashMap::new();
    let mut last_active_monitor: Option<isize> = None;
    let mut spotlight_enabled = true;

    // Register Win+Shift+B hotkey if enabled
    const HOTKEY_ID: i32 = 1;
    if config.hotkey_enabled {
        unsafe {
            let _ = RegisterHotKey(
                hwnd,
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(MOD_WIN.0 | MOD_SHIFT.0),
                VK_B.0 as u32,
            );
        }
    }

    // Apply active brightness to current monitor on startup
    let initial_monitors = enumerate_monitors();
    if let Some(idx) = get_cursor_monitor_index(&initial_monitors) {
        let hm = initial_monitors[idx].hmonitor;
        let _ = set_brightness(hm, config.active_brightness);
        last_active_monitor = Some(hm);
    }

    let poll_interval = Duration::from_millis(200);

    loop {
        // Drain Windows messages
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    restore_all_monitors(&mut dimmed, config.active_brightness);
                    remove_tray_icon(hwnd);
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Handle reload
        if RELOAD_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            match load_config() {
                Ok(new_config) => {
                    // Update hotkey registration if setting changed
                    unsafe { let _ = UnregisterHotKey(hwnd, HOTKEY_ID); }
                    if new_config.hotkey_enabled {
                        unsafe {
                            let _ = RegisterHotKey(
                                hwnd,
                                HOTKEY_ID,
                                HOT_KEY_MODIFIERS(MOD_WIN.0 | MOD_SHIFT.0),
                                VK_B.0 as u32,
                            );
                        }
                    }
                    config = new_config;
                    if let Some(hm) = last_active_monitor {
                        let _ = set_brightness(hm, config.active_brightness);
                    }
                    // Re-apply inactive brightness to already-dimmed monitors
                    for (hm, is_dimmed) in dimmed.iter() {
                        if *is_dimmed {
                            let _ = set_brightness(*hm, config.inactive_brightness);
                        }
                    }
                }
                Err(_) => {
                    config = config::Config::default();
                    let _ = config::write_default_config();
                }
            }
        }

        // Handle autostart toggle
        if TOGGLE_AUTOSTART_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let enabled = is_autostart_enabled().unwrap_or(false);
            let _ = set_autostart(!enabled);
        }

        // Handle enable/disable toggle
        if TOGGLE_ENABLED_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            spotlight_enabled = !spotlight_enabled;
            tray::SPOTLIGHT_ENABLED.store(spotlight_enabled, std::sync::atomic::Ordering::SeqCst);
            if !spotlight_enabled {
                restore_all_monitors(&mut dimmed, config.active_brightness);
                inactive_since.clear();
                last_active_monitor = None;
            }
        }

        // Handle force enable
        if FORCE_ENABLE_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if !spotlight_enabled {
                spotlight_enabled = true;
                tray::SPOTLIGHT_ENABLED.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        // Handle force disable
        if FORCE_DISABLE_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if spotlight_enabled {
                spotlight_enabled = false;
                tray::SPOTLIGHT_ENABLED.store(false, std::sync::atomic::Ordering::SeqCst);
                restore_all_monitors(&mut dimmed, config.active_brightness);
                inactive_since.clear();
                last_active_monitor = None;
            }
        }

        // Get current monitors
        let monitors = enumerate_monitors();
        if monitors.is_empty() || !spotlight_enabled {
            std::thread::sleep(poll_interval);
            continue;
        }

        let active_idx = get_cursor_monitor_index(&monitors);
        let active_hmonitor = active_idx.map(|i| monitors[i].hmonitor);

        // Restore brightness when cursor moves to a new monitor
        if active_hmonitor != last_active_monitor {
            if let Some(hm) = active_hmonitor {
                if *dimmed.get(&hm).unwrap_or(&false) {
                    let _ = set_brightness(hm, config.active_brightness);
                    dimmed.insert(hm, false);
                }
                inactive_since.remove(&hm);
            }
            last_active_monitor = active_hmonitor;
        }

        let delay = Duration::from_secs(config.idle_delay_seconds);
        let now = Instant::now();

        for monitor in &monitors {
            let hm = monitor.hmonitor;
            let is_active = active_hmonitor == Some(hm);

            if is_active {
                inactive_since.insert(hm, None);
            } else {
                let entry = inactive_since.entry(hm).or_insert(Some(now));
                if entry.is_none() {
                    *entry = Some(now);
                }

                if let Some(Some(since)) = inactive_since.get(&hm) {
                    if now.duration_since(*since) >= delay && !dimmed.get(&hm).unwrap_or(&false) {
                        let _ = set_brightness(hm, config.inactive_brightness);
                        dimmed.insert(hm, true);
                    }
                }
            }
        }

        // Clean up state for disconnected monitors
        let current_hmonitors: std::collections::HashSet<isize> =
            monitors.iter().map(|m| m.hmonitor).collect();
        inactive_since.retain(|hm, _| current_hmonitors.contains(hm));
        dimmed.retain(|hm, _| current_hmonitors.contains(hm));

        std::thread::sleep(poll_interval);
    }
}

fn config_watcher_callback() {
    tray::RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn restore_all_monitors(dimmed: &mut HashMap<isize, bool>, active_brightness: u32) {
    for (hm, is_dimmed) in dimmed.iter() {
        if *is_dimmed {
            let _ = set_brightness(*hm, active_brightness);
        }
    }
    dimmed.clear();
}

fn create_message_window() -> Result<HWND> {
    unsafe {
        let class_wide: Vec<u16> = WINDOW_CLASS.encode_utf16().chain(std::iter::once(0)).collect();
        let class_pcwstr = windows::core::PCWSTR(class_wide.as_ptr());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            lpszClassName: class_pcwstr,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_pcwstr,
            windows::core::PCWSTR(class_wide.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            HWND::default(),
            None,
            None,
            None,
        )?;
        Ok(hwnd)
    }
}

static LAST_HOTKEY_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            m if m == WM_TRAY => {
                let autostart = is_autostart_enabled().unwrap_or(false);
                let enabled = tray::SPOTLIGHT_ENABLED.load(std::sync::atomic::Ordering::SeqCst);
                handle_tray_message(hwnd, wparam, lparam, autostart, enabled);
                LRESULT(0)
            }
            m if m == WM_IPC_TOGGLE => {
                TOGGLE_ENABLED_REQUESTED.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            m if m == WM_IPC_ENABLE => {
                tray::FORCE_ENABLE_REQUESTED.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            m if m == WM_IPC_DISABLE => {
                tray::FORCE_DISABLE_REQUESTED.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            m if m == WM_IPC_RELOAD => {
                RELOAD_REQUESTED.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            m if m == WM_IPC_EXIT => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_HOTKEY => {
                let now = now_ms();
                let last = LAST_HOTKEY_MS.load(Ordering::SeqCst);
                if now.saturating_sub(last) >= 500 {
                    LAST_HOTKEY_MS.store(now, Ordering::SeqCst);
                    TOGGLE_ENABLED_REQUESTED.store(true, Ordering::SeqCst);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as u32;
                handle_menu_command(id);
                LRESULT(0)
            }
            WM_ENDSESSION => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
