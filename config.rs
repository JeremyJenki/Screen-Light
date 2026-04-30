use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::slice;
use std::thread::{self, JoinHandle};
use std::time;
use windows::Win32::Foundation::{HANDLE, CloseHandle};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_LAST_WRITE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    ReadDirectoryChangesW,
};
use windows::Win32::System::IO::CancelIoEx;
use windows::core::PCWSTR;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Seconds of cursor absence before a monitor dims. Set to 0 for instant.
    pub idle_delay_seconds: u64,
    /// Brightness to restore when a monitor becomes active (0-100).
    pub active_brightness: u32,
    /// Brightness to set when a monitor becomes inactive (0-100).
    pub inactive_brightness: u32,
    /// Enable the Win+Shift+B hotkey to toggle Screen Light on/off.
    pub hotkey_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idle_delay_seconds: 1,
            active_brightness: 75,
            inactive_brightness: 0,
            hotkey_enabled: true,
        }
    }
}

pub fn get_config_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("could not determine exe path")?;
    let dir = exe.parent().context("could not determine exe directory")?;
    Ok(dir.join("config.yaml"))
}

pub fn write_default_config() -> Result<()> {
    let path = get_config_path()?;
    let default = Config::default();
    let contents = format!(
        "idle_delay_seconds: {}\nactive_brightness: {}\ninactive_brightness: {}\n\n# Win+Shift+B to toggle Screen Light on/off.\nhotkey_enabled: {}\n",
        default.idle_delay_seconds,
        default.active_brightness,
        default.inactive_brightness,
        default.hotkey_enabled,
    );
    std::fs::write(&path, contents).context("could not write default config")
}

pub fn load_config() -> Result<Config> {
    let path = get_config_path()?;

    if !path.exists() {
        let _ = write_default_config();
        return Ok(Config::default());
    }

    let contents = std::fs::read_to_string(&path).context("could not read config file")?;
    serde_yaml::from_str(&contents).context("could not parse config file")
}

// ---- Config Watcher ----

pub struct ConfigWatcher {
    dir_handle: HANDLE,
    thread_handle: Option<JoinHandle<()>>,
}

impl ConfigWatcher {
    pub fn new(config_path: PathBuf, callback_fn: fn()) -> anyhow::Result<Self> {
        let config_dir = config_path
            .parent()
            .context("could not get parent dir for config watcher")?;

        let config_dir_wide: Vec<u16> = OsStr::new(config_dir)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let dir_handle = unsafe {
            CreateFileW(
                PCWSTR(config_dir_wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
            .context("could not create dir handle for config watcher")?
        };

        let dir_handle_isize = dir_handle.0 as isize;

        let config_name = config_path
            .file_name()
            .context("could not get config name for config watcher")?
            .to_owned()
            .into_string()
            .map_err(|_| anyhow!("could not convert config name for config watcher"))?;

        let thread_handle = thread::spawn(move || unsafe {
            let dir_handle = HANDLE(dir_handle_isize as _);
            let mut buffer = [0u8; 1024];
            let mut bytes_returned = 0u32;

            loop {
                if ReadDirectoryChangesW(
                    dir_handle,
                    buffer.as_mut_ptr() as _,
                    buffer.len() as u32,
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE,
                    Some(&mut bytes_returned),
                    None,
                    None,
                )
                .is_err()
                {
                    break;
                }

                let mut offset = 0usize;
                while offset < bytes_returned as usize {
                    let info = &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION);
                    let name_slice = slice::from_raw_parts(
                        info.FileName.as_ptr(),
                        info.FileNameLength as usize / 2,
                    );
                    let file_name = String::from_utf16_lossy(name_slice);

                    if file_name == config_name {
                        callback_fn();
                        break;
                    }

                    if info.NextEntryOffset == 0 {
                        break;
                    } else {
                        offset += info.NextEntryOffset as usize;
                    }
                }

                // Debounce — hold off before checking again
                thread::sleep(time::Duration::from_millis(300));
            }
        });

        Ok(Self {
            dir_handle,
            thread_handle: Some(thread_handle),
        })
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        unsafe { let _ = CancelIoEx(self.dir_handle, None); }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        unsafe { let _ = CloseHandle(self.dir_handle); }
    }
}
