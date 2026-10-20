use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, GetStockObject, RoundRect,
    SelectObject, SetBkMode, SetTextColor, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
    DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HBITMAP, HDC,
    HFONT, HGDIOBJ, NULL_BRUSH, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_C;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, RegisterClassExW, SetCursor,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HCURSOR, HWND_TOPMOST, IDC_ARROW, IDC_CROSS,
    IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, SWP_SHOWWINDOW,
    SW_SHOW, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WM_TIMER,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::capture::Frame;
use crate::geometry::{Point, Rect};
use crate::monitor::MonitorInfo;

use super::{color, toolbar, CursorKind, OverlaySession, Phase};

pub const TIMER_COLOR_PICK: usize = 0x31;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

pub struct FrameSurface {
    pub dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}

impl FrameSurface {
    pub fn new(frame: &Frame) -> Result<Self> {
        let width = frame.width.max(1);
        let height = frame.height.max(1);
        let (dc, bitmap, old, bits) = create_dib_surface_with_bits(width, height)?;
        if bits.is_null() {
            cleanup_surface(dc, bitmap, old);
            return Err(anyhow!("创建冻结帧缓冲区失败"));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(frame.bgra.as_ptr(), bits as *mut u8, frame.bgra.len());
        }
        Ok(Self { dc, bitmap, old })
    }
}

impl Drop for FrameSurface {
    fn drop(&mut self) {
        cleanup_surface(self.dc, self.bitmap, self.old);
    }
}

pub struct DimSurface {
    pub dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}

impl DimSurface {
    pub fn new() -> Result<Self> {
        let (dc, bitmap, old, bits) = create_dib_surface_with_bits(1, 1)?;
        if bits.is_null() {
            cleanup_surface(dc, bitmap, old);
            return Err(anyhow!("创建遮罩颜色缓冲区失败"));
        }
        unsafe {
            let pixels = bits as *mut u8;
            *pixels = 0;
            *pixels.add(1) = 0;
            *pixels.add(2) = 0;
            *pixels.add(3) = 255;
        }
        Ok(Self { dc, bitmap, old })
    }
}

impl Drop for DimSurface {
    fn drop(&mut self) {
        cleanup_surface(self.dc, self.bitmap, self.old);
    }
}

pub struct OverlayWindow {
    pub hwnd: HWND,
    pub monitor_index: usize,
    pub back_dc: HDC,
    back_bitmap: HBITMAP,
    back_old: HGDIOBJ,
    back_bits: *mut core::ffi::c_void,
}

impl OverlayWindow {
    pub fn create(
        monitor: &MonitorInfo,
        monitor_index: usize,
        hinstance: HINSTANCE,
    ) -> Result<Self> {
        register_class(hinstance)?;
        let width = monitor.rect.width().max(1);
        let height = monitor.rect.height().max(1);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                windows::core::w!("FxShotOverlay"),
                windows::core::w!(""),
                WS_POPUP,
                monitor.rect.left,
                monitor.rect.top,
                width,
                height,
                None,
                None,
                Some(hinstance),
                None,
            )?
        };
        let (back_dc, back_bitmap, back_old, back_bits) =
            create_dib_surface_with_bits(width as u32, height as u32)?;

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                monitor.rect.left,
                monitor.rect.top,
                width,
                height,
                SWP_SHOWWINDOW,
            );
        }

        Ok(Self {
            hwnd,
            monitor_index,
            back_dc,
            back_bitmap,
            back_old,
            back_bits,
        })
    }

    pub fn destroy(self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        // Drop runs here and releases GDI objects.
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        cleanup_surface(self.back_dc, self.back_bitmap, self.back_old);
    }
}

fn register_class(hinstance: HINSTANCE) -> Result<()> {
    if CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_proc),
        hInstance: hinstance,
        hCursor: cursor,
        lpszClassName: windows::core::w!("FxShotOverlay"),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        // A second load of the same class is harmless; surface a real failure.
        let error = unsafe { windows::Win32::Foundation::GetLastError() };
        if error.0 != 1410 {
            CLASS_REGISTERED.store(false, Ordering::SeqCst);
            return Err(anyhow!("注册遮罩窗口类失败：Windows 错误 {}", error.0));
        }
    }
    Ok(())
}

pub(super) fn apply_session_cursor() {
    let kind = super::with_session(|session| session.cursor_kind()).unwrap_or(CursorKind::Arrow);
    set_cursor(kind);
}

pub(super) fn set_cursor(kind: CursorKind) {
    let cursor = match kind {
        CursorKind::Crosshair => system_cursor(IDC_CROSS),
        CursorKind::Arrow => system_cursor(IDC_ARROW),
        CursorKind::Move => system_cursor(IDC_SIZEALL),
        CursorKind::SizeHorizontal => system_cursor(IDC_SIZEWE),
        CursorKind::SizeVertical => system_cursor(IDC_SIZENS),
        CursorKind::SizeNwse => system_cursor(IDC_SIZENWSE),
        CursorKind::SizeNesw => system_cursor(IDC_SIZENESW),
        CursorKind::Hand => system_cursor(IDC_HAND),
    };
    if let Some(cursor) = cursor {
        unsafe {
            let _ = SetCursor(Some(cursor));
        }
    }
}

fn system_cursor(id: PCWSTR) -> Option<HCURSOR> {
    unsafe { LoadCursorW(None, id).ok() }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => LRESULT(1),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            super::with_session(|session| {
                if let Some(index) = session
                    .overlays
                    .iter()
                    .position(|overlay| overlay.hwnd == hwnd)
                {
                    paint_session(session, index);
                }
            });
            LRESULT(0)
        }
        WM_SETCURSOR => {
            apply_session_cursor();
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            super::with_session_mut(|session| session.on_mouse_move());
            apply_session_cursor();
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            super::with_session_mut(|session| session.on_lbutton_down(hwnd));
            apply_session_cursor();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            super::with_session_mut(|session| session.on_lbutton_up());
            apply_session_cursor();
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            super::with_session_mut(|session| session.request_close());
            LRESULT(0)
        }
        WM_CLOSE => {
            super::with_session_mut(|session| session.request_close());
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let repeat = (lparam.0 & (1 << 30)) != 0;
            if wparam.0 as u32 == VK_C.0 as u32 && repeat {
                return LRESULT(0);
            }
            let modifiers = super::keyboard::current_modifiers();
            super::handle_key(wparam.0 as u32, modifiers);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_COLOR_PICK {
                super::with_session_mut(|session| session.on_tick());
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub(super) fn paint_session(session: &OverlaySession, overlay_index: usize) {
    let Some(overlay) = session.overlays.get(overlay_index) else {
        return;
    };
    let Some(monitor) = session.monitors.get(overlay.monitor_index) else {
        return;
    };
    let width = monitor.rect.width().max(1);
    let height = monitor.rect.height().max(1);

    let mut paint = PAINTSTRUCT::default();
    let window_dc = unsafe { BeginPaint(overlay.hwnd, &mut paint) };
    if window_dc.0.is_null() {
        return;
    }

    let src_x = monitor.rect.left - session.frame.origin.x;
    let src_y = monitor.rect.top - session.frame.origin.y;
    unsafe {
        let _ = BitBlt(
            overlay.back_dc,
            0,
            0,
            width,
            height,
            Some(session.frame_surface.dc),
            src_x,
            src_y,
            SRCCOPY,
        );
    }

    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 108,
        AlphaFormat: 0,
    };
    unsafe {
        let _ = AlphaBlend(
            overlay.back_dc,
            0,
            0,
            width,
            height,
            session.dim_surface.dc,
            0,
            0,
            1,
            1,
            blend,
        );
    }

    if let Some(selection) = session.selection {
        let clipped = selection.intersection(monitor.rect);
        if !clipped.is_empty() {
            let dx = clipped.left - monitor.rect.left;
            let dy = clipped.top - monitor.rect.top;
            unsafe {
                let _ = BitBlt(
                    overlay.back_dc,
                    dx,
                    dy,
                    clipped.width(),
                    clipped.height(),
                    Some(session.frame_surface.dc),
                    clipped.left - session.frame.origin.x,
                    clipped.top - session.frame.origin.y,
                    SRCCOPY,
                );
            }
            draw_border(overlay.back_dc, clipped, monitor, session.phase);
        }
        draw_label(session, overlay.back_dc, selection, monitor);
    }

    if session.phase == Phase::Locked {
        if let Some(toolbar_rect) = session.toolbar_rect {
            if toolbar_rect.intersects(monitor.rect) {
                toolbar::draw(
                    overlay.back_dc,
                    toolbar_rect,
                    Point::new(monitor.rect.left, monitor.rect.top),
                    session.toolbar_dpi,
                    session.toolbar_hover,
                );
            }
        }
    }

    if session.pointer_on_monitor {
        if let Some(pointer) = session.pointer {
            if monitor.rect.contains(pointer) {
                let notice = session.copy_notice.as_ref().map(|(text, _)| text.as_str());
                let context = color::DrawContext {
                    monitor,
                    origin: Point::new(monitor.rect.left, monitor.rect.top),
                    pointer,
                    format: session.color_format,
                    notice,
                    frame: &session.frame,
                    monitors: &session.monitors,
                };
                color::draw(overlay.back_dc, &context);
            }
        }
    }

    force_opaque(overlay.back_bits, width as usize * height as usize);

    unsafe {
        let _ = BitBlt(
            window_dc,
            0,
            0,
            width,
            height,
            Some(overlay.back_dc),
            0,
            0,
            SRCCOPY,
        );
        let _ = EndPaint(overlay.hwnd, &paint);
    }
}

fn force_opaque(bits: *mut core::ffi::c_void, pixel_count: usize) {
    if bits.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, pixel_count * 4) };
    for pixel in bytes.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
}

fn draw_border(dc: HDC, selection: Rect, monitor: &MonitorInfo, phase: Phase) {
    let local = Rect::new(
        selection.left - monitor.rect.left,
        selection.top - monitor.rect.top,
        selection.right - monitor.rect.left,
        selection.bottom - monitor.rect.top,
    );
    let pen_color = if phase == Phase::Locked {
        color(0, 120, 215)
    } else {
        color(0, 150, 255)
    };
    let pen = unsafe { CreatePen(PS_SOLID, 2, pen_color) };
    if pen.0.is_null() {
        return;
    }
    let old_pen = unsafe { SelectObject(dc, HGDIOBJ(pen.0)) };
    let old_brush = unsafe { SelectObject(dc, GetStockObject(NULL_BRUSH)) };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::Rectangle(
            dc,
            local.left,
            local.top,
            local.right,
            local.bottom,
        );
        let _ = SelectObject(dc, old_brush);
        let _ = SelectObject(dc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

fn draw_label(session: &OverlaySession, dc: HDC, selection: Rect, monitor: &MonitorInfo) {
    let mut parts = Vec::new();
    let title = match session.phase {
        Phase::Locked => session.locked_title.as_deref(),
        Phase::Hover => session
            .hover
            .and_then(|index| session.windows.get(index))
            .map(|window| window.title.as_str()),
        _ => None,
    };
    if let Some(title) = title {
        if !title.trim().is_empty() {
            let mut title = title.trim().to_string();
            if title.chars().count() > 60 {
                title = title.chars().take(60).collect::<String>() + "…";
            }
            parts.push(title);
        }
    }
    parts.push(format!("{} × {}", selection.width(), selection.height()));
    let text = parts.join("   ");

    let dpi = monitor.dpi;
    let font = ui_font(dpi, 12);
    let old_font = if font.0.is_null() {
        HGDIOBJ(std::ptr::null_mut())
    } else {
        unsafe { SelectObject(dc, HGDIOBJ(font.0)) }
    };

    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut measure = RECT::default();
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut measure,
            DT_CALCRECT | DT_LEFT | DT_SINGLELINE,
        );
    }
    let pad_x = toolbar::scale(7, dpi);
    let pad_y = toolbar::scale(4, dpi);
    let label_w = measure.right - measure.left + pad_x * 2;
    let label_h = measure.bottom - measure.top + pad_y * 2;

    let mut x = selection.left - monitor.rect.left;
    let mut y = selection.top - monitor.rect.top - label_h - toolbar::scale(4, dpi);
    if y < toolbar::scale(4, dpi) {
        y = selection.top - monitor.rect.top + toolbar::scale(4, dpi);
    }
    let max_x =
        (monitor.rect.width() - label_w - toolbar::scale(4, dpi)).max(toolbar::scale(4, dpi));
    x = x.clamp(toolbar::scale(4, dpi), max_x);

    let brush = unsafe { CreateSolidBrush(color(20, 22, 26)) };
    let old_brush = unsafe { SelectObject(dc, HGDIOBJ(brush.0)) };
    let old_pen = unsafe { SelectObject(dc, GetStockObject(NULL_PEN)) };
    let radius = toolbar::scale(5, dpi);
    unsafe {
        let _ = RoundRect(dc, x, y, x + label_w, y + label_h, radius, radius);
        let _ = SelectObject(dc, old_pen);
        let _ = SelectObject(dc, old_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = SetBkMode(dc, TRANSPARENT);
        let _ = SetTextColor(dc, color(245, 245, 245));
    }

    let mut text_rect = RECT {
        left: x + pad_x,
        top: y + pad_y,
        right: x + label_w - pad_x,
        bottom: y + label_h - pad_y,
    };
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut text_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        if !font.0.is_null() {
            let _ = SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

pub(super) fn ui_font(dpi: u32, size: i32) -> HFONT {
    let face = crate::win_util::wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            -toolbar::scale(size, dpi),
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

fn create_dib_surface_with_bits(
    width: u32,
    height: u32,
) -> Result<(HDC, HBITMAP, HGDIOBJ, *mut core::ffi::c_void)> {
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return Err(anyhow!("CreateCompatibleDC 失败"));
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) };
    let bitmap = match bitmap {
        Ok(bitmap) => bitmap,
        Err(err) => {
            unsafe {
                let _ = DeleteDC(dc);
            }
            return Err(anyhow!("CreateDIBSection 失败：{err}"));
        }
    };
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
        }
        return Err(anyhow!("DIB 像素指针为空"));
    }
    let old = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
    Ok((dc, bitmap, old, bits))
}

fn cleanup_surface(dc: HDC, bitmap: HBITMAP, old: HGDIOBJ) {
    unsafe {
        if !old.0.is_null() {
            let _ = SelectObject(dc, old);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
    }
}
