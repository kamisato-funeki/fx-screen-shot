use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use crate::capture::hdr::{self, ColorMode};
use crate::geometry::{Point, Rect};
use crate::win_util;

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub hmon: HMONITOR,
    pub device_name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub color_mode: ColorMode,
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

    let color_mode = hdr::color_mode_for_device(&device_name).unwrap_or(ColorMode::Sdr);

    Some(MonitorInfo {
        hmon,
        device_name,
        rect,
        dpi,
        color_mode,
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

/// Resolves the `--monitor` CLI selector. Numeric selectors are 1-based
/// enumeration indexes; anything else matches the GDI device name, with or
/// without the `\\.\` prefix, case-insensitively.
pub fn find_monitor(monitors: &[MonitorInfo], selector: &str) -> Option<usize> {
    let selector = selector.trim();
    if selector.is_empty() {
        return None;
    }
    if let Ok(index) = selector.parse::<usize>() {
        return (1..=monitors.len()).contains(&index).then(|| index - 1);
    }
    let wanted = selector.strip_prefix(r"\\.\").unwrap_or(selector);
    monitors.iter().position(|monitor| {
        let name = monitor.device_name.as_str();
        let name = name.strip_prefix(r"\\.\").unwrap_or(name);
        name.eq_ignore_ascii_case(wanted)
    })
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
            color_mode: ColorMode::Sdr,
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

    #[test]
    fn finds_monitor_by_index_or_device_name() {
        let monitors = vec![
            fake(r"\\.\DISPLAY1", -3840, -327, 0, 1833),
            fake(r"\\.\DISPLAY2", 0, 0, 2560, 1600),
        ];
        assert_eq!(find_monitor(&monitors, "1"), Some(0));
        assert_eq!(find_monitor(&monitors, "2"), Some(1));
        assert_eq!(find_monitor(&monitors, r"\\.\DISPLAY2"), Some(1));
        assert_eq!(find_monitor(&monitors, "display1"), Some(0));
        assert_eq!(find_monitor(&monitors, " 2 "), Some(1));
        assert_eq!(find_monitor(&monitors, "0"), None);
        assert_eq!(find_monitor(&monitors, "3"), None);
        assert_eq!(find_monitor(&monitors, "K27"), None);
        assert_eq!(find_monitor(&monitors, ""), None);
    }
}
