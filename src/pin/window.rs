use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetStockObject,
    InvalidateRect, LineTo, MoveToEx, Rectangle, RoundRect, SelectObject, SetBrushOrgEx,
    SetStretchBltMode, StretchBlt, HALFTONE, HBRUSH, HGDIOBJ, NULL_BRUSH, NULL_PEN, PAINTSTRUCT,
    PS_SOLID, SRCCOPY,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, GetWindowRect, SetLayeredWindowAttributes, SetWindowPos,
    HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    WM_CAPTURECHANGED, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
    WM_SETCURSOR, WM_WINDOWPOSCHANGED,
};

use crate::geometry::{Point, Rect};
use crate::overlay::edit;
use crate::overlay::{window as overlay_window, CursorKind};
use crate::win_util;

use super::geometry::{self, PinHit};
use super::{request_close, with_pin, with_pin_mut, PinDrag};

pub(super) unsafe extern "system" fn pin_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => LRESULT(1),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            apply_cursor(hwnd);
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_lbutton_down(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = unsafe { ReleaseCapture() };
            with_pin_mut(hwnd, |pin| pin.drag = None);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with_pin_mut(hwnd, |pin| pin.drag = None);
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            if with_pin(hwnd, |pin| pin.drag.is_some()).unwrap_or(false) {
                let _ = unsafe { ReleaseCapture() };
            }
            with_pin_mut(hwnd, |pin| pin.drag = None);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            on_mouse_wheel(hwnd, wparam);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == VK_ESCAPE.0 as u32 {
                request_close(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            request_close(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            let dpi = if dpi == 0 { 96 } else { dpi };
            with_pin_mut(hwnd, |pin| pin.dpi = dpi);
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED => {
            sync_rect(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            super::forget(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn sync_rect(hwnd: HWND) {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return;
    }
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let rect = crate::win_util::rect_from_rect(rect);
    with_pin_mut(hwnd, |pin| pin.rect = rect);
}

fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.0.is_null() {
        return;
    }

    let mut client = RECT::default();
    let has_client = unsafe { GetClientRect(hwnd, &mut client) }.is_ok();
    let width = if has_client {
        (client.right - client.left).max(1)
    } else {
        1
    };
    let height = if has_client {
        (client.bottom - client.top).max(1)
    } else {
        1
    };

    with_pin(hwnd, |pin| {
        unsafe {
            let _ = SetStretchBltMode(hdc, HALFTONE);
            let _ = SetBrushOrgEx(hdc, 0, 0, None);
            let drawn = StretchBlt(
                hdc,
                0,
                0,
                width,
                height,
                Some(pin.surface.dc),
                0,
                0,
                pin.surface.width as i32,
                pin.surface.height as i32,
                SRCCOPY,
            )
            .as_bool();
            if !drawn {
                fill_rect(hdc, 0, 0, width, height, color(20, 22, 26));
            }
        }
        draw_border(hdc, width, height, pin.dpi);
        draw_close_button(hdc, width, height, pin.dpi);
    });

    unsafe {
        let _ = EndPaint(hwnd, &paint);
    }
}

fn fill_rect(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    rgb: COLORREF,
) {
    let brush = unsafe { CreateSolidBrush(rgb) };
    if brush.0.is_null() {
        return;
    }
    let rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    unsafe {
        let _ = FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

fn draw_border(hdc: windows::Win32::Graphics::Gdi::HDC, width: i32, height: i32, dpi: u32) {
    let pen = unsafe {
        CreatePen(
            PS_SOLID,
            crate::overlay::toolbar::scale(2, dpi).max(1),
            color(0, 120, 215),
        )
    };
    if pen.0.is_null() {
        return;
    }
    let old_pen = unsafe { SelectObject(hdc, HGDIOBJ(pen.0)) };
    let old_brush = unsafe { SelectObject(hdc, GetStockObject(NULL_BRUSH)) };
    unsafe {
        let _ = Rectangle(hdc, 0, 0, width, height);
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

fn draw_close_button(hdc: windows::Win32::Graphics::Gdi::HDC, width: i32, height: i32, dpi: u32) {
    let close = geometry::close_rect(width, height, dpi);
    let radius = crate::overlay::toolbar::scale(5, dpi).max(1);
    let brush: HBRUSH = unsafe { CreateSolidBrush(color(28, 30, 34)) };
    if brush.0.is_null() {
        return;
    }
    let old_brush = unsafe { SelectObject(hdc, HGDIOBJ(brush.0)) };
    let old_pen = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
    unsafe {
        let _ = RoundRect(
            hdc,
            close.left,
            close.top,
            close.right,
            close.bottom,
            radius,
            radius,
        );
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }

    if close.width() <= 2 || close.height() <= 2 {
        return;
    }
    let pen = unsafe {
        CreatePen(
            PS_SOLID,
            crate::overlay::toolbar::scale(2, dpi).max(1),
            color(245, 245, 245),
        )
    };
    if pen.0.is_null() {
        return;
    }
    let inset = (close.width().min(close.height()) / 3).max(1);
    let old_pen = unsafe { SelectObject(hdc, HGDIOBJ(pen.0)) };
    unsafe {
        let _ = MoveToEx(hdc, close.left + inset, close.top + inset, None);
        let _ = LineTo(hdc, close.right - inset, close.bottom - inset);
        let _ = MoveToEx(hdc, close.right - inset, close.top + inset, None);
        let _ = LineTo(hdc, close.left + inset, close.bottom - inset);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

fn apply_cursor(hwnd: HWND) {
    let Some(cursor) = win_util::get_cursor_pos() else {
        return;
    };
    let kind = with_pin(hwnd, |pin| {
        if let Some(drag) = pin.drag {
            return match drag.mode {
                edit::DragMode::Move => CursorKind::Move,
                edit::DragMode::Resize(handle) => CursorKind::from_hit(edit::Hit::Resize(handle)),
            };
        }
        let local = Point::new(cursor.x - pin.rect.left, cursor.y - pin.rect.top);
        match geometry::hit_test(pin.rect.width(), pin.rect.height(), pin.dpi, local) {
            Some(PinHit::Close) => CursorKind::Hand,
            Some(PinHit::Move) => CursorKind::Move,
            Some(PinHit::Resize(handle)) => CursorKind::from_hit(edit::Hit::Resize(handle)),
            None => CursorKind::Arrow,
        }
    })
    .unwrap_or(CursorKind::Arrow);
    overlay_window::set_cursor(kind);
}

fn on_lbutton_down(hwnd: HWND) {
    let Some(cursor) = win_util::get_cursor_pos() else {
        return;
    };
    let hit = with_pin(hwnd, |pin| {
        let local = Point::new(cursor.x - pin.rect.left, cursor.y - pin.rect.top);
        (
            geometry::hit_test(pin.rect.width(), pin.rect.height(), pin.dpi, local),
            pin.rect,
        )
    });
    let Some((hit, original)) = hit else {
        return;
    };
    match hit {
        Some(PinHit::Close) => request_close(hwnd),
        Some(PinHit::Move) => begin_drag(hwnd, edit::DragMode::Move, cursor, original),
        Some(PinHit::Resize(handle)) => {
            begin_drag(hwnd, edit::DragMode::Resize(handle), cursor, original)
        }
        None => {}
    }
}

fn begin_drag(hwnd: HWND, mode: edit::DragMode, cursor: Point, original: Rect) {
    unsafe {
        // Raise the clicked pin within the topmost band so overlapping pins
        // and the hover wheel hook target the one the user just interacted with.
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        let _ = SetFocus(Some(hwnd));
        SetCapture(hwnd);
    }
    with_pin_mut(hwnd, |pin| {
        pin.drag = Some(PinDrag {
            mode,
            start_cursor: cursor,
            original,
        });
    });
}

fn on_mouse_move(hwnd: HWND) {
    let Some(cursor) = win_util::get_cursor_pos() else {
        return;
    };
    let data = with_pin(hwnd, |pin| {
        let drag = pin.drag?;
        Some((
            drag,
            pin.rect,
            pin.bounds,
            pin.monitors.clone(),
            pin.surface.width,
            pin.surface.height,
        ))
    });
    let Some(Some((drag, current, bounds, monitors, source_w, source_h))) = data else {
        return;
    };
    let delta = Point::new(
        cursor.x - drag.start_cursor.x,
        cursor.y - drag.start_cursor.y,
    );
    if delta == Point::default() {
        return;
    }
    let next = match drag.mode {
        edit::DragMode::Move => edit::move_rect(drag.original, delta, bounds),
        edit::DragMode::Resize(handle) => geometry::resize_rect_aspect(
            drag.original,
            delta,
            handle,
            source_w,
            source_h,
            bounds,
            &monitors,
        ),
    };
    let next = geometry::ensure_visible(next, &monitors, bounds);
    if next == current {
        return;
    }
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            next.left,
            next.top,
            next.width(),
            next.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if let Err(err) = result {
        crate::logging::warn(&format!("pin move/resize failed: {err}"));
        return;
    }
    with_pin_mut(hwnd, |pin| {
        pin.rect = next;
        if let edit::DragMode::Resize(_) = drag.mode {
            pin.scale = next.width() as f64 / pin.surface.width.max(1) as f64;
        }
    });
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn on_mouse_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
    let Some(cursor) = win_util::get_cursor_pos() else {
        return;
    };
    let over = with_pin(hwnd, |pin| pin.rect.contains(cursor)).unwrap_or(false);
    if !over {
        return;
    }
    let ctrl = win_util::key_down(VK_CONTROL.0 as u32);
    apply_wheel(hwnd, delta, ctrl);
}

pub(super) fn apply_wheel(hwnd: HWND, delta: i16, ctrl: bool) {
    let Some(steps) = nonzero_steps(delta) else {
        return;
    };
    if ctrl {
        let Some((current, layered_ok)) =
            with_pin(hwnd, |pin| (pin.opacity_percent, pin.layered_ok))
        else {
            return;
        };
        if !layered_ok {
            return;
        }
        let next = geometry::step_opacity(current, steps);
        if next == current {
            return;
        }
        let alpha = geometry::opacity_alpha(next);
        let result = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) };
        if let Err(err) = result {
            crate::logging::warn(&format!("pin opacity failed: {err}"));
            return;
        }
        with_pin_mut(hwnd, |pin| pin.opacity_percent = next);
        return;
    }

    let Some((current, source_w, source_h, rect, bounds, monitors)) = with_pin(hwnd, |pin| {
        Some((
            pin.scale,
            pin.surface.width,
            pin.surface.height,
            pin.rect,
            pin.bounds,
            pin.monitors.clone(),
        ))
    })
    .flatten() else {
        return;
    };
    let (min, max) = geometry::scale_limits(source_w, source_h, bounds);
    let next_scale = geometry::step_scale(current, steps, min, max);
    if (next_scale - current).abs() < f64::EPSILON {
        return;
    }
    let next = geometry::zoom_rect(rect, source_w, source_h, next_scale, bounds, &monitors);
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            next.left,
            next.top,
            next.width(),
            next.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if let Err(err) = result {
        crate::logging::warn(&format!("pin zoom failed: {err}"));
        return;
    }
    with_pin_mut(hwnd, |pin| {
        pin.scale = next_scale;
        pin.rect = next;
    });
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn nonzero_steps(delta: i16) -> Option<i32> {
    let steps = geometry::wheel_steps(delta);
    if steps == 0 {
        None
    } else {
        Some(steps)
    }
}

fn color(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}
