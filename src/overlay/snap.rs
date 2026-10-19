use crate::geometry::{Point, Rect};
use crate::monitor::MonitorInfo;
use crate::win_util::WindowInfo;

pub fn snap_rect(
    raw: Rect,
    windows: &[WindowInfo],
    monitors: &[MonitorInfo],
    threshold: i32,
    virtual_rect: Rect,
) -> Rect {
    let mut rect = raw.normalized().clamp_to(virtual_rect);
    if rect.is_empty() || threshold <= 0 {
        return rect;
    }

    let mut xs = vec![virtual_rect.left, virtual_rect.right];
    let mut ys = vec![virtual_rect.top, virtual_rect.bottom];
    for monitor in monitors {
        xs.push(monitor.rect.left);
        xs.push(monitor.rect.right);
        ys.push(monitor.rect.top);
        ys.push(monitor.rect.bottom);
    }
    for window in windows {
        xs.push(window.rect.left);
        xs.push(window.rect.right);
        ys.push(window.rect.top);
        ys.push(window.rect.bottom);
    }

    let left = nearest(rect.left, &xs, threshold).unwrap_or(rect.left);
    let right = nearest(rect.right, &xs, threshold).unwrap_or(rect.right);
    if right - left >= 2 {
        rect.left = left;
        rect.right = right;
    }

    let top = nearest(rect.top, &ys, threshold).unwrap_or(rect.top);
    let bottom = nearest(rect.bottom, &ys, threshold).unwrap_or(rect.bottom);
    if bottom - top >= 2 {
        rect.top = top;
        rect.bottom = bottom;
    }

    rect.clamp_to(virtual_rect)
}

fn nearest(value: i32, anchors: &[i32], threshold: i32) -> Option<i32> {
    anchors
        .iter()
        .copied()
        .filter(|anchor| (anchor - value).abs() <= threshold)
        .min_by_key(|anchor| (anchor - value).abs())
}

pub fn distance(a: Point, b: Point) -> i32 {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    dx.max(dy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win_util::WindowInfo;

    fn window(left: i32, top: i32, right: i32, bottom: i32) -> WindowInfo {
        WindowInfo {
            rect: Rect::new(left, top, right, bottom),
            title: "w".to_string(),
        }
    }

    fn monitor(left: i32, top: i32, right: i32, bottom: i32) -> MonitorInfo {
        MonitorInfo {
            hmon: windows::Win32::Graphics::Gdi::HMONITOR::default(),
            device_name: "DISPLAY".to_string(),
            rect: Rect::new(left, top, right, bottom),
            dpi: 96,
            hdr: false,
        }
    }

    #[test]
    fn snaps_to_window_edges() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        let windows = vec![window(100, 100, 500, 400)];
        let raw = Rect::new(103, 98, 497, 402);
        let snapped = snap_rect(raw, &windows, &monitors, 10, monitors[0].rect);
        assert_eq!(snapped, Rect::new(100, 100, 500, 400));
    }

    #[test]
    fn does_not_snap_beyond_threshold() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        let windows = vec![window(100, 100, 500, 400)];
        let raw = Rect::new(111, 111, 489, 389);
        let snapped = snap_rect(raw, &windows, &monitors, 5, monitors[0].rect);
        assert_eq!(snapped, raw);
    }

    #[test]
    fn snaps_to_screen_edges() {
        let monitors = vec![monitor(-1920, 0, 0, 1080)];
        let raw = Rect::new(-1915, 10, -500, 500);
        let snapped = snap_rect(raw, &[], &monitors, 8, monitors[0].rect);
        assert_eq!(snapped.left, -1920);
    }

    #[test]
    fn never_collapses_below_two_pixels() {
        let monitors = vec![monitor(0, 0, 1000, 1000)];
        let windows = vec![window(500, 0, 501, 1000)];
        let raw = Rect::new(499, 100, 501, 200);
        let snapped = snap_rect(raw, &windows, &monitors, 10, monitors[0].rect);
        assert!(snapped.width() >= 2);
    }
}
