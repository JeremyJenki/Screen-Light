use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostQuitMessage,
    SetForegroundWindow, TrackPopupMenu, MF_STRING, TPM_BOTTOMALIGN,
    TPM_RIGHTALIGN, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP, IDI_APPLICATION,
};
use windows::core::{PCWSTR, w};
use std::sync::atomic::{AtomicBool, Ordering};

pub const WM_TRAY: u32 = WM_APP + 1;
pub const ID_AUTOSTART: u32 = 1;
pub const ID_TOGGLE: u32 = 2;
pub const ID_CONFIG: u32 = 3;
pub const ID_RELOAD: u32 = 4;
pub const ID_EXIT: u32 = 5;

pub static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static TOGGLE_AUTOSTART_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static TOGGLE_ENABLED_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static SPOTLIGHT_ENABLED: AtomicBool = AtomicBool::new(true);
pub static FORCE_ENABLE_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static FORCE_DISABLE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Returns true when Windows is currently in dark mode (light-on-dark UI).
/// Reads the AppsUseLightTheme registry value; absence means dark mode.
pub fn is_dark_mode() -> bool {
    use windows::Win32::System::Registry::{
        RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    unsafe {
        let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
            .encode_utf16()
            .collect();
        let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();

        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            // Can't read registry — assume light mode
            return false;
        }

        let mut data: u32 = 1; // default: light
        let mut data_size = std::mem::size_of::<u32>() as u32;
        let mut reg_type = 0u32;
        let _ = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut reg_type as *mut u32 as *mut _),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_size),
        );
        let _ = windows::Win32::System::Registry::RegCloseKey(hkey);

        // AppsUseLightTheme == 0  →  dark mode is active
        reg_type == REG_DWORD.0 && data == 0
    }
}

/// Load the tray icon appropriate for the current system theme.
/// Resource ID 1 = icon.ico (light mode / exe icon)
/// Resource ID 2 = icon-dark.ico (dark mode tray icon)
fn get_tray_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::LoadIconW;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let resource_id = if is_dark_mode() { 2u16 } else { 1u16 };
        LoadIconW(hinstance, PCWSTR(resource_id as *const u16))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}

/// Update the tray icon in-place to match the current system theme.
pub fn update_tray_icon(hwnd: HWND) {
    unsafe {
        let icon = get_tray_icon();
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON;
        nid.hIcon = icon;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn add_tray_icon(hwnd: HWND) -> Result<()> {
    unsafe {
        let icon = get_tray_icon();
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;

        let tip = "Screen Light";
        let tip_wide: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
        let len = tip_wide.len().min(128);
        nid.szTip[..len].copy_from_slice(&tip_wide[..len]);

        Shell_NotifyIconW(NIM_ADD, &nid).ok().context("could not add tray icon")?;
    }
    Ok(())
}

pub fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn show_tray_menu(hwnd: HWND, autostart_enabled: bool, spotlight_enabled: bool) {
    unsafe {
        let menu = CreatePopupMenu().unwrap();

        let autostart_label = if autostart_enabled {
            w!("✓ Auto-start")
        } else {
            w!("Auto-start")
        };

        let toggle_label = if spotlight_enabled {
            w!("Disable")
        } else {
            w!("Enable")
        };

        AppendMenuW(menu, MF_STRING, ID_AUTOSTART as usize, autostart_label).unwrap();
        AppendMenuW(menu, MF_STRING, ID_TOGGLE as usize, toggle_label).unwrap();
        AppendMenuW(menu, MF_STRING, ID_CONFIG as usize, w!("Config")).unwrap();
        AppendMenuW(menu, MF_STRING, ID_RELOAD as usize, w!("Reload")).unwrap();
        AppendMenuW(menu, MF_STRING, ID_EXIT as usize, w!("Exit")).unwrap();

        let mut pt = POINT::default();
        GetCursorPos(&mut pt).unwrap();
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        DestroyMenu(menu).unwrap();
    }
}

pub fn handle_tray_message(hwnd: HWND, _wparam: WPARAM, lparam: LPARAM, autostart_enabled: bool, spotlight_enabled: bool) {
    let msg = lparam.0 as u32;
    match msg {
        WM_RBUTTONUP | WM_LBUTTONUP => {
            show_tray_menu(hwnd, autostart_enabled, spotlight_enabled);
        }
        _ => {}
    }
}

pub fn handle_menu_command(id: u32) {
    match id {
        ID_AUTOSTART => {
            TOGGLE_AUTOSTART_REQUESTED.store(true, Ordering::SeqCst);
        }
        ID_TOGGLE => {
            TOGGLE_ENABLED_REQUESTED.store(true, Ordering::SeqCst);
        }
        ID_CONFIG => {
            if let Ok(path) = crate::config::get_config_path() {
                let _ = open_config(path);
            }
        }
        ID_RELOAD => {
            RELOAD_REQUESTED.store(true, Ordering::SeqCst);
        }
        ID_EXIT => unsafe {
            PostQuitMessage(0);
        },
        _ => {}
    }
}

fn open_config(path: std::path::PathBuf) -> Result<()> {
    std::process::Command::new("explorer")
        .arg(path.parent().unwrap_or(&path))
        .spawn()
        .context("could not open config folder")?;
    Ok(())
}
