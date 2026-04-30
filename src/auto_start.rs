use anyhow::{Context, Result};
use windows::Win32::System::Registry::{
    RegOpenKeyExW, RegSetValueExW, RegDeleteValueW, RegQueryValueExW, RegCloseKey,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, HKEY,
};
use windows::core::PCWSTR;
use windows::core::w;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

const APP_NAME: &str = "screen-light";
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn is_autostart_enabled() -> Result<bool> {
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_READ, &mut hkey)
            .ok()
            .context("could not open Run registry key")?;
        let name = to_wide(APP_NAME);
        let result = RegQueryValueExW(hkey, PCWSTR(name.as_ptr()), None, None, None, None);
        let _ = RegCloseKey(hkey);
        Ok(result.is_ok())
    }
}

pub fn set_autostart(enable: bool) -> Result<()> {
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_WRITE, &mut hkey)
            .ok()
            .context("could not open Run registry key")?;
        let name = to_wide(APP_NAME);
        if enable {
            let exe = std::env::current_exe().context("could not get exe path")?;
            let value = to_wide(exe.to_str().unwrap_or_default());
            RegSetValueExW(
                hkey,
                PCWSTR(name.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    value.as_ptr() as *const u8,
                    value.len() * 2,
                )),
            )
            .ok()
            .context("could not set autostart registry value")?;
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(name.as_ptr()));
        }
        let _ = RegCloseKey(hkey);
        Ok(())
    }
}
