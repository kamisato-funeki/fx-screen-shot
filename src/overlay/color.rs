use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, FillRect, FrameRect, GetStockObject,
    RoundRect, SelectObject, SetBkMode, SetDCBrushColor, SetTextColor, DC_BRUSH, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HGDIOBJ, NULL_BRUSH, NULL_PEN,
    PS_SOLID, TRANSPARENT,
};

use crate::capture::Frame;
use crate::geometry::{Point, Rect};
use crate::monitor::MonitorInfo;

use super::toolbar;

pub const MAGNIFIER_CELLS: i32 = 9;
const MAGNIFIER_RADIUS: i32 = MAGNIFIER_CELLS / 2;
// At 96 DPI the widest line is "Shift 切换    C 复制" (105px) and
// "RGB  255, 0, 255" (96px). 112px keeps both on one line with a margin.
const INFO_WIDTH: i32 = 112;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFormat {
    #[default]
    Rgb,
    Hex,
}

impl ColorFormat {
    pub fn toggle(self) -> Self {
        match self {
            Self::Rgb => Self::Hex,
            Self::Hex => Self::Rgb,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Hex => "HEX",
        }
    }

    /// Text shown in the panel.
    pub fn value(self, rgb: [u8; 3]) -> String {
        match self {
            Self::Rgb => format!("{}, {}, {}", rgb[0], rgb[1], rgb[2]),
            Self::Hex => hex_value(rgb),
        }
    }

    /// Text written to the clipboard by `C`.
    pub fn copy_value(self, rgb: [u8; 3]) -> String {
        match self {
            Self::Rgb => format!("rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]),
            Self::Hex => hex_value(rgb),
        }
    }
}

fn hex_value(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn coordinates_text(point: Point) -> String {
    format!("X {}  Y {}", point.x, point.y)
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub panel: Rect,
    pub magnifier: Rect,
    pub info: Rect,
    pub cell: i32,
    pub line_height: i32,
    pub dpi: u32,
}

/// Places the color panel next to the pointer, flipping and clamping so that
/// it stays inside the monitor that currently contains the pointer.
pub fn layout(pointer: Point, monitor: &MonitorInfo) -> Layout {
    let dpi = monitor.dpi;
    let pad = toolbar::scale(8, dpi);
    let gap = toolbar::scale(8, dpi);
    let cell = toolbar::scale(10, dpi).max(6);
    let grid = cell * MAGNIFIER_CELLS;
    let info_width = toolbar::scale(INFO_WIDTH, dpi);
    let panel_width = pad * 2 + grid + gap + info_width;
    let panel_height = pad * 2 + grid;
    let offset = toolbar::scale(18, dpi);
    let margin = toolbar::scale(4, dpi);

    let min_x = monitor.rect.left + margin;
    let max_x = (monitor.rect.right - margin - panel_width).max(min_x);
    let min_y = monitor.rect.top + margin;
    let max_y = (monitor.rect.bottom - margin - panel_height).max(min_y);

    let mut x = pointer.x + offset;
    let mut y = pointer.y + offset;
    if x > max_x {
        x = pointer.x - offset - panel_width;
    }
    if y > max_y {
        y = pointer.y - offset - panel_height;
    }
    x = x.clamp(min_x, max_x);
    y = y.clamp(min_y, max_y);

    let panel = Rect::new(x, y, x + panel_width, y + panel_height);
    let magnifier = Rect::new(
        panel.left + pad,
        panel.top + pad,
        panel.left + pad + grid,
        panel.top + pad + grid,
    );
    let info = Rect::new(
        magnifier.right + gap,
        panel.top + pad,
        panel.right - pad,
        panel.top + pad + grid,
    );

    Layout {
        panel,
        magnifier,
        info,
        cell,
        line_height: grid / 3,
        dpi,
    }
}

pub struct DrawContext<'a> {
    pub monitor: &'a MonitorInfo,
    pub origin: Point,
    pub pointer: Point,
    pub format: ColorFormat,
    pub notice: Option<&'a str>,
    pub frame: &'a Frame,
    pub monitors: &'a [MonitorInfo],
}

pub fn draw(dc: HDC, ctx: &DrawContext<'_>) {
    let Some(rgb) = sample_pixel(ctx.frame, ctx.monitors, ctx.pointer) else {
        return;
    };
    let layout = layout(ctx.pointer, ctx.monitor);
    if layout.panel.is_empty() {
        return;
    }

    rounded_panel(dc, layout.panel, ctx.origin, layout.dpi);
    draw_magnifier(dc, &layout, ctx);
    draw_info(dc, &layout, ctx, rgb);
}

fn sample_pixel(frame: &Frame, monitors: &[MonitorInfo], point: Point) -> Option<[u8; 3]> {
    if !monitors.iter().any(|monitor| monitor.rect.contains(point)) {
        return None;
    }
    frame.pixel_at(point)
}

fn rounded_panel(dc: HDC, panel: Rect, origin: Point, dpi: u32) {
    let local = local_rect(panel, origin);
    if local.is_empty() {
        return;
    }
    let radius = toolbar::scale(9, dpi);

    let brush = unsafe { CreateSolidBrush(color(24, 26, 31)) };
    if !brush.0.is_null() {
        let old_brush = unsafe { SelectObject(dc, HGDIOBJ(brush.0)) };
        let old_pen = unsafe { SelectObject(dc, GetStockObject(NULL_PEN)) };
        unsafe {
            let _ = RoundRect(
                dc,
                local.left,
                local.top,
                local.right,
                local.bottom,
                radius,
                radius,
            );
            let _ = SelectObject(dc, old_pen);
            let _ = SelectObject(dc, old_brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    let pen = unsafe { CreatePen(PS_SOLID, 1, color(84, 90, 102)) };
    if !pen.0.is_null() {
        let old_pen = unsafe { SelectObject(dc, HGDIOBJ(pen.0)) };
        let old_brush = unsafe { SelectObject(dc, GetStockObject(NULL_BRUSH)) };
        unsafe {
            let _ = RoundRect(
                dc,
                local.left,
                local.top,
                local.right,
                local.bottom,
                radius,
                radius,
            );
            let _ = SelectObject(dc, old_brush);
            let _ = SelectObject(dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }
}

fn draw_magnifier(dc: HDC, layout: &Layout, ctx: &DrawContext<'_>) {
    let grid = layout.magnifier;
    let cell = layout.cell;
    let grid_color = [18, 19, 22];
    fill_rect(dc, grid, ctx.origin, grid_color);

    for row in 0..MAGNIFIER_CELLS {
        for col in 0..MAGNIFIER_CELLS {
            let sample = Point::new(
                ctx.pointer.x + col - MAGNIFIER_RADIUS,
                ctx.pointer.y + row - MAGNIFIER_RADIUS,
            );
            let rgb = sample_pixel(ctx.frame, ctx.monitors, sample)
                .unwrap_or_else(|| checker_color(row, col));
            let cell_rect = Rect::new(
                grid.left + col * cell,
                grid.top + row * cell,
                grid.left + (col + 1) * cell,
                grid.top + (row + 1) * cell,
            );
            fill_rect(dc, cell_rect, ctx.origin, rgb);
        }
    }

    let line = toolbar::scale(1, layout.dpi).max(1);
    for index in 1..MAGNIFIER_CELLS {
        let x = grid.left + index * cell;
        fill_rect(
            dc,
            Rect::new(x, grid.top, x + line, grid.bottom),
            ctx.origin,
            grid_color,
        );
        let y = grid.top + index * cell;
        fill_rect(
            dc,
            Rect::new(grid.left, y, grid.right, y + line),
            ctx.origin,
            grid_color,
        );
    }
    frame_rect(dc, grid, ctx.origin, [84, 90, 102]);

    let center = Rect::new(
        grid.left + MAGNIFIER_RADIUS * cell,
        grid.top + MAGNIFIER_RADIUS * cell,
        grid.left + (MAGNIFIER_RADIUS + 1) * cell,
        grid.top + (MAGNIFIER_RADIUS + 1) * cell,
    );
    let border = toolbar::scale(2, layout.dpi).max(1);
    for index in 0..border {
        frame_rect(dc, expand(center, index), ctx.origin, [0, 0, 0]);
    }
    frame_rect(dc, center, ctx.origin, [255, 255, 255]);
}

fn draw_info(dc: HDC, layout: &Layout, ctx: &DrawContext<'_>, rgb: [u8; 3]) {
    let dpi = layout.dpi;
    let info = layout.info;
    let font = super::window::ui_font(dpi, 12);
    let old_font = if font.0.is_null() {
        HGDIOBJ(std::ptr::null_mut())
    } else {
        unsafe { SelectObject(dc, HGDIOBJ(font.0)) }
    };

    let line_height = layout.line_height.max(1);
    let coordinates = coordinates_text(ctx.pointer);
    draw_text(
        dc,
        Rect::new(info.left, info.top, info.right, info.top + line_height),
        ctx.origin,
        &coordinates,
        color(225, 228, 235),
    );

    let value = format!("{}  {}", ctx.format.label(), ctx.format.value(rgb));
    draw_text(
        dc,
        Rect::new(
            info.left,
            info.top + line_height,
            info.right,
            info.top + line_height * 2,
        ),
        ctx.origin,
        &value,
        color(245, 246, 248),
    );

    let (hint, hint_color) = match ctx.notice {
        Some(notice) => (notice, color(96, 224, 138)),
        None => ("Shift 切换    C 复制", color(166, 172, 184)),
    };
    draw_text(
        dc,
        Rect::new(
            info.left,
            info.top + line_height * 2,
            info.right,
            info.top + line_height * 3,
        ),
        ctx.origin,
        hint,
        hint_color,
    );

    unsafe {
        if !font.0.is_null() {
            let _ = SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

fn draw_text(dc: HDC, rect: Rect, origin: Point, text: &str, text_color: COLORREF) {
    let local = local_rect(rect, origin);
    if local.is_empty() {
        return;
    }
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut native = to_rect(local);
    let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX;
    unsafe {
        let _ = SetBkMode(dc, TRANSPARENT);
        let _ = SetTextColor(dc, text_color);
        let _ = DrawTextW(dc, &mut wide, &mut native, flags);
    }
}

fn fill_rect(dc: HDC, rect: Rect, origin: Point, rgb: [u8; 3]) {
    let local = local_rect(rect, origin);
    if local.is_empty() {
        return;
    }
    let brush = unsafe { GetStockObject(DC_BRUSH) };
    let native = to_rect(local);
    unsafe {
        let _ = SetDCBrushColor(dc, color(rgb[0], rgb[1], rgb[2]));
        let _ = FillRect(dc, &native, HBRUSH(brush.0));
    }
}

fn frame_rect(dc: HDC, rect: Rect, origin: Point, rgb: [u8; 3]) {
    let local = local_rect(rect, origin);
    if local.is_empty() {
        return;
    }
    let brush = unsafe { GetStockObject(DC_BRUSH) };
    let native = to_rect(local);
    unsafe {
        let _ = SetDCBrushColor(dc, color(rgb[0], rgb[1], rgb[2]));
        let _ = FrameRect(dc, &native, HBRUSH(brush.0));
    }
}

fn local_rect(rect: Rect, origin: Point) -> Rect {
    Rect::new(
        rect.left - origin.x,
        rect.top - origin.y,
        rect.right - origin.x,
        rect.bottom - origin.y,
    )
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn expand(rect: Rect, by: i32) -> Rect {
    Rect::new(
        rect.left - by,
        rect.top - by,
        rect.right + by,
        rect.bottom + by,
    )
}

fn checker_color(row: i32, col: i32) -> [u8; 3] {
    if (row + col) % 2 == 0 {
        [42, 44, 49]
    } else {
        [58, 61, 68]
    }
}

fn color(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::HMONITOR;

    fn monitor(left: i32, top: i32, right: i32, bottom: i32, dpi: u32) -> MonitorInfo {
        MonitorInfo {
            hmon: HMONITOR::default(),
            device_name: "DISPLAY".to_string(),
            rect: Rect::new(left, top, right, bottom),
            dpi,
            hdr: false,
        }
    }

    #[test]
    fn formats_rgb_and_hex_values() {
        let rgb = [30, 144, 255];
        assert_eq!(ColorFormat::Rgb.value(rgb), "30, 144, 255");
        assert_eq!(ColorFormat::Rgb.copy_value(rgb), "rgb(30, 144, 255)");
        assert_eq!(ColorFormat::Hex.value(rgb), "#1E90FF");
        assert_eq!(ColorFormat::Hex.copy_value(rgb), "#1E90FF");
        assert_eq!(ColorFormat::Hex.value([0, 0, 0]), "#000000");
        assert_eq!(ColorFormat::Hex.value([255, 255, 255]), "#FFFFFF");
    }

    #[test]
    fn shift_toggles_between_rgb_and_hex() {
        assert_eq!(ColorFormat::default(), ColorFormat::Rgb);
        assert_eq!(ColorFormat::Rgb.toggle(), ColorFormat::Hex);
        assert_eq!(ColorFormat::Hex.toggle(), ColorFormat::Rgb);
        assert_eq!(ColorFormat::Rgb.label(), "RGB");
        assert_eq!(ColorFormat::Hex.label(), "HEX");
    }

    #[test]
    fn coordinates_use_global_physical_pixels() {
        assert_eq!(coordinates_text(Point::new(1234, -567)), "X 1234  Y -567");
    }

    #[test]
    fn layout_is_placed_next_to_pointer_and_inside_monitor() {
        let monitor = monitor(0, 0, 2560, 1600, 96);
        let layout = layout(Point::new(200, 200), &monitor);
        assert_eq!(MAGNIFIER_CELLS, 9);
        assert_eq!(layout.cell, 10);
        assert_eq!(layout.panel.left, 218);
        assert_eq!(layout.panel.top, 218);
        assert_eq!(layout.panel.width(), 226);
        assert!(layout.panel.right <= monitor.rect.right);
        assert!(layout.panel.bottom <= monitor.rect.bottom);
        assert_eq!(layout.magnifier.width(), layout.cell * MAGNIFIER_CELLS);
        assert_eq!(layout.magnifier.height(), layout.cell * MAGNIFIER_CELLS);
        assert_eq!(
            layout.info.right,
            layout.panel.right - toolbar::scale(8, 96)
        );
        assert_eq!(layout.info.width(), toolbar::scale(INFO_WIDTH, 96));
    }

    #[test]
    fn layout_flips_near_right_and_bottom_edges() {
        let monitor = monitor(0, 0, 1920, 1080, 96);
        let pointer = Point::new(1900, 1060);
        let layout = layout(pointer, &monitor);
        assert!(layout.panel.right < pointer.x);
        assert!(layout.panel.bottom < pointer.y);
        assert!(layout.panel.right <= monitor.rect.right);
        assert!(layout.panel.bottom <= monitor.rect.bottom);
        assert!(layout.panel.left >= monitor.rect.left);
        assert!(layout.panel.top >= monitor.rect.top);
    }

    #[test]
    fn layout_handles_negative_monitor_origin_and_high_dpi() {
        let monitor = monitor(-2560, -400, 0, 1200, 144);
        let layout = layout(Point::new(-2500, -350), &monitor);
        assert!(layout.panel.left >= monitor.rect.left);
        assert!(layout.panel.top >= monitor.rect.top);
        assert!(layout.panel.right <= monitor.rect.right);
        assert!(layout.panel.bottom <= monitor.rect.bottom);
        assert_eq!(layout.magnifier.width(), layout.cell * MAGNIFIER_CELLS);
        assert_eq!(layout.dpi, 144);
    }

    #[test]
    fn magnifier_cells_cover_the_grid() {
        let monitor = monitor(0, 0, 2560, 1600, 96);
        let layout = layout(Point::new(300, 300), &monitor);
        let last = Rect::new(
            layout.magnifier.left + (MAGNIFIER_CELLS - 1) * layout.cell,
            layout.magnifier.top + (MAGNIFIER_CELLS - 1) * layout.cell,
            layout.magnifier.left + MAGNIFIER_CELLS * layout.cell,
            layout.magnifier.top + MAGNIFIER_CELLS * layout.cell,
        );
        assert_eq!(last.right, layout.magnifier.right);
        assert_eq!(last.bottom, layout.magnifier.bottom);
        assert_eq!(last.width(), layout.cell);
        assert_eq!(last.height(), layout.cell);
    }
}
