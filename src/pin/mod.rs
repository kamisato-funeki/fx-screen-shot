use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{HBITMAP, HDC, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DestroyWindow, GetWindowLongPtrW, LoadCursorW, PostMessageW,
    RegisterClassExW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx, WindowFromPoint, CS_HREDRAW,
    CS_VREDRAW, GWL_EXSTYLE, HC_ACTION, HHOOK, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, MSLLHOOKSTRUCT,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_SHOW, WH_MOUSE_LL, WM_APP, WM_MOUSEWHEEL, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::capture::CapturedImage;
use crate::geometry::{Point, Rect};
use crate::logging;

use self::window::apply_wheel;

pub mod geometry;
mod window;

pub const WM_APP_PIN_CLOSE: u32 = WM_APP + 0x30;
pub const WM_APP_PIN_WHEEL: u32 = WM_APP + 0x31;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
static HOOK_TARGET: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static PINS: RefCell<PinManager> = RefCell::new(PinManager::default());
}

pub struct PinRequest {
    pub main_hwnd: HWND,
    pub image: CapturedImage,
    pub rect: Rect,
    pub virtual_rect: Rect,
}

pub(super) struct PinSurface {
    pub dc: HDC,
    pub width: u32,
    pub height: u32,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}

impl PinSurface {
    fn new(image: &CapturedImage) -> Result<Self> {
        if image.width == 0 || image.height == 0 {
            return Err(anyhow!("钉图图像尺寸为空"));
        }
        let expected = image.width as usize * image.height as usize * 4;
        if image.bgra.len() != expected {
            return Err(anyhow!("钉图像素长度不符"));
        }
        let (dc, bitmap, old, bits) =
            crate::overlay::window::create_dib_surface_with_bits(image.width, image.height)?;
        if bits.is_null() {
            crate::overlay::window::cleanup_surface(dc, bitmap, old);
            return Err(anyhow!("创建钉图缓冲区失败"));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(image.bgra.as_ptr(), bits as *mut u8, expected);
        }
        Ok(Self {
            dc,
            width: image.width,
            height: image.height,
            bitmap,
            old,
        })
    }
}

impl Drop for PinSurface {
    fn drop(&mut self) {
        crate::overlay::window::cleanup_surface(self.dc, self.bitmap, self.old);
    }
}

pub(super) struct PinState {
    pub hwnd: HWND,
    pub main_hwnd: HWND,
    pub surface: PinSurface,
    pub rect: Rect,
    pub bounds: Rect,
    pub monitors: Vec<Rect>,
    pub dpi: u32,
    pub scale: f64,
    pub opacity_percent: u8,
    pub layered_ok: bool,
    pub drag: Option<PinDrag>,
}

#[derive(Clone, Copy)]
pub(super) struct PinDrag {
    pub mode: crate::overlay::edit::DragMode,
    pub start_cursor: Point,
    pub original: Rect,
}

#[derive(Default)]
struct PinManager {
    pins: Vec<PinState>,
    wheel_hook: Option<WheelHook>,
}

pub(super) fn with_pin<R>(hwnd: HWND, f: impl FnOnce(&PinState) -> R) -> Option<R> {
    with_manager(|manager| manager.pins.iter().find(|pin| pin.hwnd == hwnd).map(f)).flatten()
}

pub(super) fn with_pin_mut<R>(hwnd: HWND, f: impl FnOnce(&mut PinState) -> R) -> Option<R> {
    with_manager_mut(|manager| manager.pins.iter_mut().find(|pin| pin.hwnd == hwnd).map(f))
        .flatten()
}

fn with_manager<R>(f: impl FnOnce(&PinManager) -> R) -> Option<R> {
    PINS.with(|cell| cell.try_borrow().ok().map(|manager| f(&manager)))
}

fn with_manager_mut<R>(f: impl FnOnce(&mut PinManager) -> R) -> Option<R> {
    PINS.with(|cell| {
        cell.try_borrow_mut()
            .ok()
            .map(|mut manager| f(&mut manager))
    })
}

pub fn create(request: PinRequest) -> Result<()> {
    let PinRequest {
        main_hwnd,
        image,
        rect,
        virtual_rect,
    } = request;
    let hinstance = HINSTANCE(unsafe { GetModuleHandleW(None)?.0 });
    register_class(hinstance)?;

    let surface = PinSurface::new(&image)?;
    let monitor_info = crate::monitor::enumerate();
    let monitors: Vec<Rect> = monitor_info.iter().map(|monitor| monitor.rect).collect();
    let bounds = if monitor_info.is_empty() {
        virtual_rect
    } else {
        crate::monitor::virtual_rect(&monitor_info)
    };
    if bounds.is_empty() {
        return Err(anyhow!("当前桌面范围为空"));
    }

    let (min_scale, max_scale) = geometry::scale_limits(surface.width, surface.height, bounds);
    let scale = 1.0f64.clamp(min_scale, max_scale);
    let (width, height) = geometry::scaled_size(surface.width, surface.height, scale);
    let mut window_rect = Rect::new(rect.left, rect.top, rect.left + width, rect.top + height);
    window_rect = geometry::ensure_visible(window_rect, &monitors, bounds);

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            windows::core::w!("FxShotPinWindow"),
            windows::core::w!("ScreenShot FX Pin"),
            WS_POPUP,
            window_rect.left,
            window_rect.top,
            window_rect.width(),
            window_rect.height(),
            None,
            None,
            Some(hinstance),
            None,
        )
    }?;

    let layered_ok =
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) }.is_ok();
    if !layered_ok {
        // A layered window without attributes may not render. Drop the style
        // so the pin stays visible, but keep opacity control disabled.
        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_LAYERED.0 as isize));
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        logging::warn("pin set layered attributes failed; opacity control is disabled");
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi == 0 { 96 } else { dpi };

    let source_width = surface.width;
    let source_height = surface.height;
    let state = PinState {
        hwnd,
        main_hwnd,
        surface,
        rect: window_rect,
        bounds,
        monitors: monitors.clone(),
        dpi,
        scale,
        opacity_percent: 100,
        layered_ok,
        drag: None,
    };
    let inserted = with_manager_mut(|manager| {
        manager.pins.push(state);
        manager.pins.len()
    });
    if inserted.is_none() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(anyhow!("初始化钉图状态失败"));
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        if let Err(err) = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            window_rect.left,
            window_rect.top,
            window_rect.width(),
            window_rect.height(),
            SWP_SHOWWINDOW,
        ) {
            logging::warn(&format!("pin show failed: {err}"));
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            logging::warn("pin foreground focus failed");
        }
        if let Err(err) = SetFocus(Some(hwnd)) {
            logging::warn(&format!("pin focus failed: {err}"));
        }
    }

    install_wheel_hook_if_needed(main_hwnd);
    logging::info(&format!(
        "pin created {}x{} at ({}, {}) source={}x{} scale={:.2} opacity=100",
        window_rect.width(),
        window_rect.height(),
        window_rect.left,
        window_rect.top,
        source_width,
        source_height,
        scale
    ));
    Ok(())
}

fn register_class(hinstance: HINSTANCE) -> Result<()> {
    if CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window::pin_proc),
        hInstance: hinstance,
        hCursor: cursor,
        lpszClassName: windows::core::w!("FxShotPinWindow"),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error.0 != 1410 {
            CLASS_REGISTERED.store(false, Ordering::SeqCst);
            return Err(anyhow!("注册钉图窗口类失败：Windows 错误 {}", error.0));
        }
    }
    Ok(())
}

pub fn handle_close_message(hwnd: HWND) {
    let removed = with_manager_mut(|manager| {
        let index = manager.pins.iter().position(|pin| pin.hwnd == hwnd)?;
        let state = manager.pins.remove(index);
        let hook = if manager.pins.is_empty() {
            manager.wheel_hook.take()
        } else {
            None
        };
        Some((state, hook))
    })
    .flatten();
    let Some((state, hook)) = removed else {
        return;
    };
    drop(hook);
    let size = (state.rect.width(), state.rect.height());
    unsafe {
        let _ = DestroyWindow(state.hwnd);
    }
    drop(state);
    logging::info(&format!("pin closed {}x{}", size.0, size.1));
}

pub(super) fn forget(hwnd: HWND) {
    let removed = with_manager_mut(|manager| {
        let index = manager.pins.iter().position(|pin| pin.hwnd == hwnd)?;
        let state = manager.pins.remove(index);
        let hook = if manager.pins.is_empty() {
            manager.wheel_hook.take()
        } else {
            None
        };
        Some((state, hook))
    })
    .flatten();
    if let Some((state, hook)) = removed {
        drop(hook);
        logging::info("pin window destroyed");
        drop(state);
    }
}

pub fn close_all() {
    let taken = with_manager_mut(|manager| {
        let pins = std::mem::take(&mut manager.pins);
        let hook = manager.wheel_hook.take();
        (pins, hook)
    });
    let Some((pins, hook)) = taken else {
        return;
    };
    drop(hook);
    let count = pins.len();
    for state in &pins {
        unsafe {
            let _ = DestroyWindow(state.hwnd);
        }
    }
    drop(pins);
    if count > 0 {
        logging::info(&format!("pin closed all ({count})"));
    }
}

pub fn on_display_change() {
    let monitor_info = crate::monitor::enumerate();
    if monitor_info.is_empty() {
        close_all();
        return;
    }
    let bounds = crate::monitor::virtual_rect(&monitor_info);
    let monitor_rects: Vec<Rect> = monitor_info.iter().map(|monitor| monitor.rect).collect();
    let updates = with_manager(|manager| {
        manager
            .pins
            .iter()
            .map(|pin| {
                let (min, max) =
                    geometry::scale_limits(pin.surface.width, pin.surface.height, bounds);
                let scale = pin.scale.clamp(min, max);
                let rect = geometry::zoom_rect(
                    pin.rect,
                    pin.surface.width,
                    pin.surface.height,
                    scale,
                    bounds,
                    &monitor_rects,
                );
                (pin.hwnd, rect, scale)
            })
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let count = updates.len();
    for (hwnd, rect, scale) in updates {
        if let Err(err) = unsafe {
            SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } {
            logging::warn(&format!("pin display change position failed: {err}"));
            continue;
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let dpi = if dpi == 0 { 96 } else { dpi };
        with_pin_mut(hwnd, |pin| {
            pin.rect = rect;
            pin.scale = scale;
            pin.bounds = bounds;
            pin.monitors = monitor_rects.clone();
            pin.dpi = dpi;
        });
        let _ = unsafe { windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false) };
    }
    if count > 0 {
        logging::info(&format!("pin display change: {count} pins"));
    }
}

pub(super) fn request_close(hwnd: HWND) {
    let Some(main_hwnd) = with_pin(hwnd, |pin| pin.main_hwnd) else {
        return;
    };
    if let Err(err) = unsafe {
        PostMessageW(
            Some(main_hwnd),
            WM_APP_PIN_CLOSE,
            WPARAM(hwnd.0 as usize),
            LPARAM(0),
        )
    } {
        logging::warn(&format!("pin close request failed: {err}"));
    }
}

pub fn handle_wheel_message(hwnd: HWND, lparam: LPARAM) {
    let delta = ((lparam.0 >> 16) & 0xffff) as u16 as i16;
    let ctrl = (lparam.0 & 1) != 0;
    apply_wheel(hwnd, delta, ctrl);
}

fn pack_wheel(delta: i16, ctrl: bool) -> isize {
    (((delta as i32) << 16) | ctrl as i32) as isize
}

struct WheelHook {
    handle: HHOOK,
}

impl WheelHook {
    fn install(main_hwnd: HWND) -> Result<Self> {
        match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_proc), None, 0) } {
            Ok(handle) => {
                HOOK_TARGET.store(main_hwnd.0 as isize, Ordering::SeqCst);
                logging::info("pin wheel hook installed");
                Ok(Self { handle })
            }
            Err(err) => {
                HOOK_TARGET.store(0, Ordering::SeqCst);
                Err(err.into())
            }
        }
    }
}

impl Drop for WheelHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.handle);
        }
        HOOK_TARGET.store(0, Ordering::SeqCst);
        logging::info("pin wheel hook uninstalled");
    }
}

fn install_wheel_hook_if_needed(main_hwnd: HWND) {
    let needed = with_manager(|manager| manager.wheel_hook.is_none()).unwrap_or(false);
    if !needed {
        return;
    }
    match WheelHook::install(main_hwnd) {
        Ok(hook) => {
            if with_manager_mut(|manager| manager.wheel_hook = Some(hook)).is_none() {
                logging::warn("pin wheel hook installed but state storage failed");
            }
        }
        Err(err) => logging::warn(&format!("pin wheel hook install failed: {err:#}")),
    }
}

fn pin_at_point(point: Point) -> Option<HWND> {
    let hwnd = unsafe {
        WindowFromPoint(POINT {
            x: point.x,
            y: point.y,
        })
    };
    if hwnd.0.is_null() {
        return None;
    }
    let is_pin =
        with_manager(|manager| manager.pins.iter().any(|pin| pin.hwnd == hwnd)).unwrap_or(false);
    if is_pin {
        Some(hwnd)
    } else {
        None
    }
}

unsafe extern "system" fn wheel_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEWHEEL {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let point = Point::new(info.pt.x, info.pt.y);
        if let Some(hwnd) = pin_at_point(point) {
            let delta = ((info.mouseData >> 16) & 0xffff) as u16 as i16;
            let ctrl = crate::win_util::key_down(VK_CONTROL.0 as u32);
            let target = HOOK_TARGET.load(Ordering::SeqCst);
            if target != 0 {
                let posted = PostMessageW(
                    Some(HWND(target as *mut core::ffi::c_void)),
                    WM_APP_PIN_WHEEL,
                    WPARAM(hwnd.0 as usize),
                    LPARAM(pack_wheel(delta, ctrl)),
                );
                if posted.is_ok() {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}
