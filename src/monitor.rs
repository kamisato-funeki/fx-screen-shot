use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use crate::capture::hdr;
use crate::geometry::{Point, Rect};
use crate::win_util;

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub hmon: HMONITOR,
    pub device_name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub hdr: bool,
}

struct EnumCtx {
    monitors: Vec<MonitorInfo>,
}

unsafe extern "system" fn enum_monitor_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let ctx = &mut *(data.0 as *mut EnumCtx);
    if let Some(info) = monitor_info_from_handle(hmon) {
        ctx.monitors.push(info);
    }
    TRUE
}

pub fn enumerate() -> Vec<MonitorInfo> {
    let mut ctx = EnumCtx {
        monitors: Vec::new(),
    };
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor_proc),
            LPARAM(&mut ctx as *mut EnumCtx as isize),
        );
    }
    ctx.monitors
}

pub fn monitor_info_from_handle(hmon: HMONITOR) -> Option<MonitorInfo> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe { GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) };
    if !ok.as_bool() {
        return None;
    }

    let device_name = win_util::from_wide(&info.szDevice);
    let rect = win_util::rect_from_rect(info.monitorInfo.rcMonitor);
    if rect.is_empty() {
        return None;
    }

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    let dpi = if unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
        .is_ok()
        && dpi_x >= 48
    {
        dpi_x
    } else {
        96
    };

    let hdr = hdr::is_advanced_color_enabled(&device_name);

    Some(MonitorInfo {
        hmon,
        device_name,
        rect,
        dpi,
        hdr,
    })
}

pub fn virtual_rect(monitors: &[MonitorInfo]) -> Rect {
    let mut result = Rect::default();
    for monitor in monitors {
        result = result.union(monitor.rect);
    }
    result
}

/// Picks the monitor containing `point`, otherwise the nearest monitor.
pub fn select_monitor(point: Point, monitors: &[MonitorInfo]) -> Option<usize> {
    if monitors.is_empty() {
        return None;
    }
    if let Some(index) = monitors.iter().position(|m| m.rect.contains(point)) {
        return Some(index);
    }
    let mut best = 0usize;
    let mut best_distance = i64::MAX;
    for (index, monitor) in monitors.iter().enumerate() {
        let distance = monitor.rect.nearest_distance_sq(point);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(device: &str, left: i32, top: i32, right: i32, bottom: i32) -> MonitorInfo {
        MonitorInfo {
            hmon: HMONITOR::default(),
            device_name: device.to_string(),
            rect: Rect::new(left, top, right, bottom),
            dpi: 96,
            hdr: false,
        }
    }

    #[test]
    fn selects_containing_monitor() {
        let monitors = vec![fake("A", 0, 0, 1920, 1080), fake("B", 1920, 0, 3840, 1080)];
        assert_eq!(select_monitor(Point::new(10, 10), &monitors), Some(0));
        assert_eq!(select_monitor(Point::new(2000, 500), &monitors), Some(1));
    }

    #[test]
    fn selects_nearest_for_gap_or_outside() {
        let monitors = vec![fake("A", 0, 0, 1920, 1080), fake("B", 2000, 0, 3920, 1080)];
        // Between the two screens: closer to B.
        assert_eq!(select_monitor(Point::new(1985, 100), &monitors), Some(1));
        // Above A: nearest is A.
        assert_eq!(select_monitor(Point::new(100, -500), &monitors), Some(0));
    }

    #[test]
    fn negative_coordinates() {
        let monitors = vec![fake("A", -2560, 0, 0, 1600), fake("B", 0, 0, 2560, 1600)];
        assert_eq!(select_monitor(Point::new(-1, 100), &monitors), Some(0));
        assert_eq!(select_monitor(Point::new(0, 100), &monitors), Some(1));
    }

    #[test]
    fn virtual_rect_covers_all() {
        let monitors = vec![fake("A", -1920, 0, 0, 1080), fake("B", 0, 0, 2560, 1440)];
        assert_eq!(virtual_rect(&monitors), Rect::new(-1920, 0, 2560, 1440));
    }
}
