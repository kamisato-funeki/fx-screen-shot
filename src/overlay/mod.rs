pub mod edit;
pub mod keyboard;
pub mod snap;
pub mod toolbar;
pub mod window;

use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, VK_C, VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SetForegroundWindow, WM_APP};

use crate::capture::clipboard;
use crate::geometry::{Point, Rect};
use crate::logging;
use crate::monitor::{self, MonitorInfo};
use crate::naming;
use crate::win_util::{self, WindowInfo};

use self::window::{DimSurface, FrameSurface, OverlayWindow};

pub const WM_APP_OVERLAY_CLOSE: u32 = WM_APP + 0x20;
pub use keyboard::WM_APP_OVERLAY_KEY;

#[derive(Clone, Debug)]
pub struct OverlayOptions {
    pub save_dir: PathBuf,
    pub clipboard: bool,
    pub snap_distance: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Hover,
    Dragging,
    Locked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Free,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Crosshair,
    Arrow,
    Move,
    SizeHorizontal,
    SizeVertical,
    SizeNwse,
    SizeNesw,
    Hand,
}

impl CursorKind {
    fn from_handle(handle: edit::Handle) -> Self {
        match handle {
            edit::Handle::West | edit::Handle::East => Self::SizeHorizontal,
            edit::Handle::North | edit::Handle::South => Self::SizeVertical,
            edit::Handle::NorthWest | edit::Handle::SouthEast => Self::SizeNwse,
            edit::Handle::NorthEast | edit::Handle::SouthWest => Self::SizeNesw,
        }
    }

    fn from_hit(hit: edit::Hit) -> Self {
        match hit {
            edit::Hit::Move => Self::Move,
            edit::Hit::Resize(handle) => Self::from_handle(handle),
        }
    }
}

pub struct OverlaySession {
    pub main_hwnd: HWND,
    pub options: OverlayOptions,
    pub frame: crate::capture::Frame,
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
    pub virtual_rect: Rect,
    pub overlays: Vec<OverlayWindow>,
    pub frame_surface: FrameSurface,
    pub dim_surface: DimSurface,
    pub phase: Phase,
    pub hover: Option<usize>,
    pub selection: Option<Rect>,
    pub selection_mode: SelectionMode,
    pub selection_drag: Option<edit::SelectionDrag>,
    pub locked_title: Option<String>,
    pub pressed: bool,
    pub dragging: bool,
    pub down_point: Point,
    pub down_window: Option<usize>,
    pub toolbar_rect: Option<Rect>,
    pub toolbar_dpi: u32,
    pub toolbar_hover: Option<usize>,
    pub close_requested: bool,
    pub hook: Option<keyboard::KeyboardHook>,
}

thread_local! {
    static SESSION: RefCell<Option<OverlaySession>> = const { RefCell::new(None) };
}

pub(super) fn with_session<R>(f: impl FnOnce(&OverlaySession) -> R) -> Option<R> {
    SESSION.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|session| session.as_ref().map(f))
    })
}

pub(super) fn with_session_mut<R>(f: impl FnOnce(&mut OverlaySession) -> R) -> Option<R> {
    SESSION.with(|cell| {
        cell.try_borrow_mut()
            .ok()
            .and_then(|mut session| session.as_mut().map(f))
    })
}

pub fn is_active() -> bool {
    SESSION.with(|cell| cell.borrow().is_some())
}

pub fn start(
    main_hwnd: HWND,
    frame: crate::capture::Frame,
    monitors: Vec<MonitorInfo>,
    windows: Vec<WindowInfo>,
    options: OverlayOptions,
) -> Result<()> {
    if monitors.is_empty() {
        return Err(anyhow!("没有可用的显示器"));
    }
    if is_active() {
        return Err(anyhow!("选区遮罩已经在运行"));
    }

    let virtual_rect = monitor::virtual_rect(&monitors);
    let frame_surface = FrameSurface::new(&frame)?;
    let dim_surface = DimSurface::new()?;
    let hinstance = HINSTANCE(unsafe { GetModuleHandleW(None)?.0 });

    let session = OverlaySession {
        main_hwnd,
        options,
        frame,
        monitors,
        windows,
        virtual_rect,
        overlays: Vec::new(),
        frame_surface,
        dim_surface,
        phase: Phase::Idle,
        hover: None,
        selection: None,
        selection_mode: SelectionMode::Free,
        selection_drag: None,
        locked_title: None,
        pressed: false,
        dragging: false,
        down_point: Point::default(),
        down_window: None,
        toolbar_rect: None,
        toolbar_dpi: 96,
        toolbar_hover: None,
        close_requested: false,
        hook: None,
    };

    SESSION.with(|cell| *cell.borrow_mut() = Some(session));

    let result = (|| -> Result<()> {
        let monitor_count = SESSION.with(|cell| {
            cell.borrow()
                .as_ref()
                .map(|session| session.monitors.len())
                .unwrap_or(0)
        });
        for index in 0..monitor_count {
            let monitor = SESSION
                .with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .and_then(|session| session.monitors.get(index).cloned())
                })
                .ok_or_else(|| anyhow!("显示器信息丢失"))?;
            let overlay = OverlayWindow::create(&monitor, index, hinstance)?;
            SESSION.with(|cell| {
                if let Some(session) = cell.borrow_mut().as_mut() {
                    session.overlays.push(overlay);
                }
            });
        }

        let fallback_hwnd = SESSION
            .with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|session| session.overlays.first().map(|overlay| overlay.hwnd))
            })
            .ok_or_else(|| anyhow!("创建遮罩窗口失败"))?;

        let cursor = win_util::get_cursor_pos().unwrap_or(Point::new(0, 0));
        let target = SESSION
            .with(|cell| {
                cell.borrow().as_ref().and_then(|session| {
                    session
                        .monitors
                        .iter()
                        .position(|monitor| monitor.rect.contains(cursor))
                        .and_then(|index| session.overlays.get(index).map(|overlay| overlay.hwnd))
                })
            })
            .unwrap_or(fallback_hwnd);

        unsafe {
            let _ = SetForegroundWindow(target);
            let _ = SetFocus(Some(target));
        }

        let hook = match keyboard::install(main_hwnd) {
            Ok(hook) => Some(hook),
            Err(err) => {
                logging::warn(&format!("keyboard hook install failed: {err:#}"));
                None
            }
        };
        SESSION.with(|cell| {
            if let Some(session) = cell.borrow_mut().as_mut() {
                session.hook = hook;
            }
        });

        with_session_mut(|session| {
            session.refresh_hover();
            session.invalidate_all();
        });
        window::set_cursor(CursorKind::Crosshair);
        logging::info("selection overlay opened");
        Ok(())
    })();

    if result.is_err() {
        finish();
    }
    result
}

pub fn finish() {
    let session = SESSION.with(|cell| cell.borrow_mut().take());
    if let Some(mut session) = session {
        session.hook = None;
        for overlay in session.overlays.drain(..) {
            overlay.destroy();
        }
        logging::info("selection overlay closed");
    }
    window::set_cursor(CursorKind::Arrow);
}

pub fn handle_key(vk: u32, ctrl: bool) {
    with_session_mut(|session| {
        if session.close_requested {
            return;
        }
        if vk == VK_ESCAPE.0 as u32 {
            session.request_close();
        } else if vk == VK_RETURN.0 as u32 || vk == VK_SPACE.0 as u32 {
            if session.phase == Phase::Locked {
                session.save_or_copy(false);
            }
        } else if vk == VK_C.0 as u32 && ctrl && session.phase == Phase::Locked {
            session.save_or_copy(true);
        }
    });
}

impl OverlaySession {
    fn dpi_at(&self, point: Point) -> u32 {
        monitor::select_monitor(point, &self.monitors)
            .and_then(|index| self.monitors.get(index))
            .map(|monitor| monitor.dpi)
            .unwrap_or(96)
    }

    fn monitor_for_rect(&self, rect: Rect) -> Option<&MonitorInfo> {
        let index = monitor::select_monitor(rect.center(), &self.monitors)?;
        self.monitors.get(index)
    }

    fn refresh_toolbar(&mut self, rect: Rect) {
        let dpi = self.dpi_at(rect.center());
        self.toolbar_dpi = dpi;
        self.toolbar_rect = self
            .monitor_for_rect(rect)
            .map(|monitor| toolbar::layout(rect, monitor, dpi));
        self.toolbar_hover = None;
    }

    fn hit_selection(&self, point: Point) -> Option<edit::Hit> {
        let selection = self.selection?;
        let tolerance = toolbar::scale(edit::HANDLE_TOLERANCE, self.dpi_at(point)).max(1);
        edit::hit_test(selection, point, tolerance)
    }

    pub fn cursor_kind(&self) -> CursorKind {
        if self.close_requested {
            return CursorKind::Arrow;
        }
        match self.phase {
            Phase::Idle | Phase::Hover | Phase::Dragging => CursorKind::Crosshair,
            Phase::Locked => {
                if let Some(drag) = self.selection_drag {
                    return match drag.mode {
                        edit::DragMode::Move => CursorKind::Move,
                        edit::DragMode::Resize(handle) => CursorKind::from_handle(handle),
                    };
                }
                let Some(point) = win_util::get_cursor_pos() else {
                    return CursorKind::Arrow;
                };
                if let Some(toolbar_rect) = self.toolbar_rect {
                    if toolbar::button_at(toolbar_rect, self.toolbar_dpi, point).is_some() {
                        return CursorKind::Hand;
                    }
                    if toolbar_rect.contains(point) {
                        return CursorKind::Arrow;
                    }
                }
                self.hit_selection(point)
                    .map(CursorKind::from_hit)
                    .unwrap_or(CursorKind::Arrow)
            }
        }
    }

    pub fn refresh_hover(&mut self) {
        let Some(point) = win_util::get_cursor_pos() else {
            return;
        };
        self.hover = win_util::window_at_point(&self.windows, point);
        if let Some(index) = self.hover {
            if let Some(window) = self.windows.get(index) {
                self.selection = Some(window.rect.clamp_to(self.virtual_rect));
                self.phase = Phase::Hover;
                return;
            }
        }
        self.selection = None;
        self.phase = Phase::Idle;
    }

    pub fn on_mouse_move(&mut self) {
        if self.close_requested {
            return;
        }
        let Some(point) = win_util::get_cursor_pos() else {
            return;
        };

        if self.phase == Phase::Locked {
            if let Some(drag) = self.selection_drag {
                let delta = Point::new(point.x - drag.start.x, point.y - drag.start.y);
                if delta.x != 0 || delta.y != 0 {
                    let updated = match drag.mode {
                        edit::DragMode::Move => {
                            edit::move_rect(drag.original, delta, self.virtual_rect)
                        }
                        edit::DragMode::Resize(handle) => {
                            edit::resize_rect(drag.original, delta, handle, self.virtual_rect)
                        }
                    };
                    if self.selection != Some(updated) {
                        self.selection = Some(updated);
                        self.selection_mode = SelectionMode::Free;
                        self.locked_title = None;
                        self.refresh_toolbar(updated);
                        self.invalidate_all();
                    }
                }
                return;
            }

            let hover = self
                .toolbar_rect
                .and_then(|toolbar| toolbar::button_at(toolbar, self.toolbar_dpi, point));
            if hover != self.toolbar_hover {
                self.toolbar_hover = hover;
                self.invalidate_all();
            }
            return;
        }

        if self.pressed {
            let distance = snap::distance(point, self.down_point);
            if !self.dragging && distance <= 3 {
                return;
            }
            self.dragging = true;
            self.phase = Phase::Dragging;
            let raw = Rect::from_points(self.down_point, point);
            let dpi = self.dpi_at(point);
            let threshold = toolbar::scale(self.options.snap_distance, dpi);
            let snapped = snap::snap_rect(
                raw,
                &self.windows,
                &self.monitors,
                threshold,
                self.virtual_rect,
            );
            self.selection = Some(snapped);
            self.invalidate_all();
            return;
        }

        self.hover = win_util::window_at_point(&self.windows, point);
        if let Some(index) = self.hover {
            if let Some(window) = self.windows.get(index) {
                self.selection = Some(window.rect.clamp_to(self.virtual_rect));
                self.phase = Phase::Hover;
                self.invalidate_all();
                return;
            }
        }
        if self.selection.is_some() || self.phase != Phase::Idle {
            self.selection = None;
            self.phase = Phase::Idle;
            self.invalidate_all();
        }
    }

    pub fn on_lbutton_down(&mut self, hwnd: HWND) {
        if self.close_requested {
            return;
        }
        let Some(point) = win_util::get_cursor_pos() else {
            return;
        };

        if self.phase == Phase::Locked {
            if let Some(toolbar) = self.toolbar_rect {
                if let Some(button) = toolbar::button_at(toolbar, self.toolbar_dpi, point) {
                    self.handle_toolbar_button(button);
                    return;
                }
                if toolbar.contains(point) {
                    return;
                }
            }
            if let (Some(original), Some(hit)) = (self.selection, self.hit_selection(point)) {
                let mode = match hit {
                    edit::Hit::Move => edit::DragMode::Move,
                    edit::Hit::Resize(handle) => edit::DragMode::Resize(handle),
                };
                self.selection_drag = Some(edit::SelectionDrag {
                    mode,
                    start: point,
                    original,
                });
                unsafe {
                    SetCapture(hwnd);
                    let _ = SetFocus(Some(hwnd));
                }
                return;
            }
            self.clear_selection();
            return;
        }

        self.pressed = true;
        self.dragging = false;
        self.down_point = point;
        self.down_window = self.hover;
        unsafe {
            SetCapture(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
    }

    pub fn on_lbutton_up(&mut self) {
        if self.selection_drag.take().is_some() {
            let _ = unsafe { ReleaseCapture() };
            match self.selection {
                Some(selection)
                    if selection.width() >= edit::MIN_SIZE
                        && selection.height() >= edit::MIN_SIZE =>
                {
                    self.refresh_toolbar(selection);
                }
                _ => self.clear_selection(),
            }
            self.invalidate_all();
            return;
        }
        if !self.pressed {
            return;
        }
        self.pressed = false;
        let _ = unsafe { ReleaseCapture() };

        if self.phase == Phase::Locked {
            return;
        }

        if self.dragging {
            self.dragging = false;
            if let Some(selection) = self.selection {
                if selection.width() >= 2 && selection.height() >= 2 {
                    self.lock_selection(selection, SelectionMode::Free, None);
                } else {
                    self.clear_selection();
                }
            } else {
                self.clear_selection();
            }
            return;
        }

        if let Some(index) = self.down_window {
            if let Some(window) = self.windows.get(index).cloned() {
                self.lock_selection(
                    window.rect.clamp_to(self.virtual_rect),
                    SelectionMode::Window,
                    Some(window.title),
                );
                return;
            }
        }
        self.clear_selection();
    }

    fn lock_selection(&mut self, rect: Rect, mode: SelectionMode, title: Option<String>) {
        let rect = rect.clamp_to(self.virtual_rect);
        if rect.width() < 2 || rect.height() < 2 {
            self.clear_selection();
            return;
        }
        self.selection = Some(rect);
        self.selection_mode = mode;
        self.selection_drag = None;
        self.locked_title = title;
        self.phase = Phase::Locked;
        self.refresh_toolbar(rect);
        self.invalidate_all();
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_drag = None;
        self.locked_title = None;
        self.toolbar_rect = None;
        self.toolbar_hover = None;
        self.phase = Phase::Idle;
        self.hover = None;
        self.invalidate_all();
    }

    fn handle_toolbar_button(&mut self, button: usize) {
        match button {
            toolbar::BUTTON_CANCEL => self.request_close(),
            toolbar::BUTTON_SAVE => self.save_or_copy(false),
            toolbar::BUTTON_COPY => self.save_or_copy(true),
            _ => {}
        }
    }

    fn save_or_copy(&mut self, copy_only: bool) {
        let Some(selection) = self.selection else {
            self.request_close();
            return;
        };
        let Some((rgb, width, height)) = self.frame.crop_rgb(selection) else {
            self.notify_error("截图范围为空");
            self.request_close();
            return;
        };

        if !copy_only {
            match naming::save_png(&self.options.save_dir, &rgb, width, height) {
                Ok(path) => logging::info(&format!("saved screenshot {}", path.display())),
                Err(err) => {
                    self.notify_error(&format!("保存截图失败：{err:#}"));
                    self.request_close();
                    return;
                }
            }
        }

        if copy_only || self.options.clipboard {
            if let Err(err) = clipboard::copy_rgb(self.main_hwnd, &rgb, width, height) {
                logging::warn(&format!("copy to clipboard failed: {err:#}"));
                self.notify_error(&format!("复制到剪贴板失败：{err:#}"));
            }
        }

        self.request_close();
    }

    fn notify_error(&self, message: &str) {
        crate::app::notify_error(self.main_hwnd, message);
    }

    pub fn request_close(&mut self) {
        if self.close_requested {
            return;
        }
        self.close_requested = true;
        let _ = unsafe {
            PostMessageW(
                Some(self.main_hwnd),
                WM_APP_OVERLAY_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }

    pub fn invalidate_all(&self) {
        for overlay in &self.overlays {
            let _ = unsafe { InvalidateRect(Some(overlay.hwnd), None, false) };
        }
    }
}

/// Waits for the compositor to remove the overlay before a follow-up capture.
pub fn flush_after_close() {
    let _ = unsafe { DwmFlush() };
    std::thread::sleep(std::time::Duration::from_millis(30));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_mapping_covers_all_handles() {
        assert_eq!(CursorKind::from_hit(edit::Hit::Move), CursorKind::Move);
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::West)),
            CursorKind::SizeHorizontal
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::East)),
            CursorKind::SizeHorizontal
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::North)),
            CursorKind::SizeVertical
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::South)),
            CursorKind::SizeVertical
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::NorthWest)),
            CursorKind::SizeNwse
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::SouthEast)),
            CursorKind::SizeNwse
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::NorthEast)),
            CursorKind::SizeNesw
        );
        assert_eq!(
            CursorKind::from_hit(edit::Hit::Resize(edit::Handle::SouthWest)),
            CursorKind::SizeNesw
        );
    }
}
