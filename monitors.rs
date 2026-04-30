use anyhow::{Result, anyhow};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetVCPFeature,
};
use windows::Win32::Foundation::{BOOL, GetLastError, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const VCP_BRIGHTNESS: u8 = 0x10;

pub fn get_last_error() -> u32 {
    unsafe { GetLastError().0 }
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub hmonitor: isize,
    #[allow(dead_code)]
    pub name: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

pub fn enumerate_monitors() -> Vec<Monitor> {
    let mut monitors: Vec<Monitor> = Vec::new();

    unsafe extern "system" fn callback(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        unsafe {
            let monitors = &mut *(lparam.0 as *mut Vec<Monitor>);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
                let name = String::from_utf16_lossy(
                    &info.szDevice.iter().take_while(|&&c| c != 0).cloned().collect::<Vec<_>>()
                );
                let bounds = Rect::from(info.monitorInfo.rcMonitor);
                monitors.push(Monitor { hmonitor: hmonitor.0 as isize, name, bounds });
            }
        }
        TRUE
    }

    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(callback),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }

    monitors
}

pub fn get_cursor_monitor_index(monitors: &[Monitor]) -> Option<usize> {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return None;
        }
    }
    monitors.iter().position(|m| m.bounds.contains_point(point.x, point.y))
}

pub fn set_brightness(hmonitor: isize, brightness: u32) -> Result<()> {
    let brightness = brightness.clamp(0, 100);
    unsafe {
        let hmonitor = HMONITOR(hmonitor as _);
        let mut count: u32 = 0;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(hmonitor, &mut count).is_err() {
            return Err(anyhow!("GetNumberOfPhysicalMonitorsFromHMONITOR failed"));
        }
        if count == 0 {
            return Err(anyhow!("no physical monitors found for HMONITOR"));
        }
        let mut physical_monitors = vec![PHYSICAL_MONITOR::default(); count as usize];
        if GetPhysicalMonitorsFromHMONITOR(hmonitor, &mut physical_monitors).is_err() {
            return Err(anyhow!("GetPhysicalMonitorsFromHMONITOR failed"));
        }
        for pm in &physical_monitors {
            let _ = SetVCPFeature(pm.hPhysicalMonitor, VCP_BRIGHTNESS, brightness);
            let _ = DestroyPhysicalMonitor(pm.hPhysicalMonitor);
        }
    }
    Ok(())
}
