use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, GetStockObject, RoundRect,
    SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HGDIOBJ, NULL_BRUSH,
    NULL_PEN, OUT_DEFAULT_PRECIS, PS_SOLID, TRANSPARENT,
};

use crate::geometry::{Point, Rect};
use crate::monitor::MonitorInfo;

pub const BUTTON_CANCEL: usize = 0;
pub const BUTTON_SAVE: usize = 1;
pub const BUTTON_COPY: usize = 2;
pub const LABELS: [&str; 3] = ["取消", "保存", "复制"];

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub button_w: i32,
    pub button_h: i32,
    pub pad: i32,
    pub gap: i32,
}

pub fn scale(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi as i64 + 48) / 96) as i32
}

pub fn metrics(dpi: u32) -> Metrics {
    Metrics {
        button_w: scale(58, dpi),
        button_h: scale(32, dpi),
        pad: scale(6, dpi),
        gap: scale(8, dpi),
    }
}

pub fn layout(selection: Rect, monitor: &MonitorInfo, dpi: u32) -> Rect {
    let m = metrics(dpi);
    let width = m.button_w * 3 + m.pad * 4;
    let height = m.button_h + m.pad * 2;

    let mut x = selection.right - width;
    let mut y = selection.bottom + m.gap;
    if y + height > monitor.rect.bottom - m.gap {
        y = selection.top - height - m.gap;
    }
    if y < monitor.rect.top + m.gap {
        y = selection.bottom - height - m.gap;
    }

    x = x.clamp(
        monitor.rect.left + m.gap,
        (monitor.rect.right - m.gap - width).max(monitor.rect.left + m.gap),
    );
    y = y.clamp(
        monitor.rect.top + m.gap,
        (monitor.rect.bottom - m.gap - height).max(monitor.rect.top + m.gap),
    );
    Rect::new(x, y, x + width, y + height)
}

pub fn button_rects(toolbar: Rect, dpi: u32) -> [Rect; 3] {
    let m = metrics(dpi);
    let y = toolbar.top + m.pad;
    let mut result = [Rect::default(); 3];
    for (i, slot) in result.iter_mut().enumerate() {
        let x = toolbar.left + m.pad + i as i32 * (m.button_w + m.pad);
        *slot = Rect::new(x, y, x + m.button_w, y + m.button_h);
    }
    result
}

pub fn button_at(toolbar: Rect, dpi: u32, point: Point) -> Option<usize> {
    button_rects(toolbar, dpi)
        .iter()
        .position(|rect| rect.contains(point))
}

pub fn draw(hdc: HDC, toolbar: Rect, origin: Point, dpi: u32, hover: Option<usize>) {
    let local = Rect::new(
        toolbar.left - origin.x,
        toolbar.top - origin.y,
        toolbar.right - origin.x,
        toolbar.bottom - origin.y,
    );
    if local.is_empty() {
        return;
    }

    let radius = scale(9, dpi);
    let panel = color(28, 30, 34);
    let border = color(80, 86, 96);
    let hover_color = color(58, 64, 74);
    let text = color(240, 240, 240);

    let brush = unsafe { CreateSolidBrush(panel) };
    let old_brush = unsafe { SelectObject(hdc, HGDIOBJ(brush.0)) };
    let old_pen = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
    unsafe {
        let _ = RoundRect(
            hdc,
            local.left,
            local.top,
            local.right,
            local.bottom,
            radius,
            radius,
        );
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }

    let pen = unsafe { CreatePen(PS_SOLID, 1, border) };
    let old_pen = unsafe { SelectObject(hdc, HGDIOBJ(pen.0)) };
    let old_brush = unsafe { SelectObject(hdc, GetStockObject(NULL_BRUSH)) };
    unsafe {
        let _ = RoundRect(
            hdc,
            local.left,
            local.top,
            local.right,
            local.bottom,
            radius,
            radius,
        );
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }

    let font = make_font(dpi);
    let old_font = if font.0.is_null() {
        HGDIOBJ(std::ptr::null_mut())
    } else {
        unsafe { SelectObject(hdc, HGDIOBJ(font.0)) }
    };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, text);
    }

    for (index, button) in button_rects(toolbar, dpi).iter().enumerate() {
        let button_local = Rect::new(
            button.left - origin.x,
            button.top - origin.y,
            button.right - origin.x,
            button.bottom - origin.y,
        );
        if hover == Some(index) {
            let hover_brush = unsafe { CreateSolidBrush(hover_color) };
            let old_b = unsafe { SelectObject(hdc, HGDIOBJ(hover_brush.0)) };
            let old_p = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
            unsafe {
                let _ = RoundRect(
                    hdc,
                    button_local.left,
                    button_local.top,
                    button_local.right,
                    button_local.bottom,
                    scale(6, dpi),
                    scale(6, dpi),
                );
                let _ = SelectObject(hdc, old_p);
                let _ = SelectObject(hdc, old_b);
                let _ = DeleteObject(HGDIOBJ(hover_brush.0));
            }
        }
        let mut wide: Vec<u16> = LABELS[index].encode_utf16().collect();
        let mut rect = RECT {
            left: button_local.left,
            top: button_local.top,
            right: button_local.right,
            bottom: button_local.bottom,
        };
        unsafe {
            DrawTextW(
                hdc,
                &mut wide,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }

    unsafe {
        if !font.0.is_null() {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

fn make_font(dpi: u32) -> windows::Win32::Graphics::Gdi::HFONT {
    let face = crate::win_util::wide("Microsoft YaHei UI");
    let height = -scale(12, dpi);
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

fn color(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> MonitorInfo {
        MonitorInfo {
            hmon: windows::Win32::Graphics::Gdi::HMONITOR::default(),
            device_name: "DISPLAY".to_string(),
            rect: Rect::new(0, 0, 1920, 1080),
            dpi: 96,
            color_mode: crate::capture::hdr::ColorMode::Sdr,
        }
    }

    #[test]
    fn places_toolbar_inside_monitor() {
        let selection = Rect::new(100, 100, 500, 400);
        let toolbar = layout(selection, &monitor(), 96);
        assert!(toolbar.left >= 0);
        assert!(toolbar.right <= 1920);
        assert!(toolbar.top >= 0);
        assert!(toolbar.bottom <= 1080);
        assert_eq!(toolbar.height(), metrics(96).button_h + metrics(96).pad * 2);
    }

    #[test]
    fn cancel_save_copy_are_right_aligned_in_that_order() {
        assert_eq!(LABELS, ["取消", "保存", "复制"]);
        assert_eq!(BUTTON_CANCEL, 0);
        assert_eq!(BUTTON_SAVE, 1);
        assert_eq!(BUTTON_COPY, 2);

        let selection = Rect::new(100, 100, 500, 400);
        let dpi = 96;
        let toolbar = layout(selection, &monitor(), dpi);
        assert_eq!(toolbar.right, selection.right);

        let buttons = button_rects(toolbar, dpi);
        assert!(buttons[BUTTON_CANCEL].right <= buttons[BUTTON_SAVE].left);
        assert!(buttons[BUTTON_SAVE].right <= buttons[BUTTON_COPY].left);
        assert_eq!(
            button_at(toolbar, dpi, buttons[BUTTON_CANCEL].center()),
            Some(BUTTON_CANCEL)
        );
        assert_eq!(
            button_at(toolbar, dpi, buttons[BUTTON_SAVE].center()),
            Some(BUTTON_SAVE)
        );
        assert_eq!(
            button_at(toolbar, dpi, buttons[BUTTON_COPY].center()),
            Some(BUTTON_COPY)
        );
    }

    #[test]
    fn places_toolbar_above_when_no_room_below() {
        let selection = Rect::new(100, 900, 500, 1050);
        let toolbar = layout(selection, &monitor(), 96);
        assert!(toolbar.bottom <= selection.top);
    }

    #[test]
    fn hit_tests_buttons() {
        let selection = Rect::new(100, 100, 500, 400);
        let dpi = 96;
        let toolbar = layout(selection, &monitor(), dpi);
        let buttons = button_rects(toolbar, dpi);
        assert_eq!(
            button_at(toolbar, dpi, buttons[BUTTON_COPY].center()),
            Some(BUTTON_COPY)
        );
        assert_eq!(button_at(toolbar, dpi, Point::new(0, 0)), None);
    }
}
