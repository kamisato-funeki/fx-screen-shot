use std::path::{Path, PathBuf};

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{
    FOLDERID_Pictures, FOLDERID_RoamingAppData, SHGetKnownFolderPath, ShellExecuteW,
    KF_FLAG_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE, SW_SHOWNORMAL,
    WS_EX_TOOLWINDOW,
};

use crate::geometry::{Point, Rect};

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        text.map(PathBuf::from)
    }
}

pub fn pictures_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_Pictures)
}

pub fn roaming_appdata_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_RoamingAppData)
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("fxshot.exe"))
}

pub fn open_path(path: &Path) -> bool {
    let path_w = wide(&path.to_string_lossy());
    let op = wide("open");
    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR(op.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        result.0 as isize > 32
    }
}

pub fn get_cursor_pos() -> Option<Point> {
    let mut p = windows::Win32::Foundation::POINT::default();
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.is_ok() {
        Some(Point::new(p.x, p.y))
    } else {
        None
    }
}

pub fn current_process_id() -> u32 {
    unsafe { GetCurrentProcessId() }
}

pub fn rect_from_rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

pub fn dwm_extended_frame(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if ok.is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        Some(rect_from_rect(rect))
    } else {
        None
    }
}

pub fn window_rect(hwnd: HWND) -> Option<Rect> {
    if let Some(r) = dwm_extended_frame(hwnd) {
        return Some(r);
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok()
        && rect.right > rect.left
        && rect.bottom > rect.top
    {
        Some(rect_from_rect(rect))
    } else {
        None
    }
}

pub fn is_window_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    ok.is_ok() && cloaked != 0
}

pub fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len > 0 {
        from_wide(&buf[..len as usize])
    } else {
        String::new()
    }
}

pub fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len > 0 {
        from_wide(&buf[..len as usize])
    } else {
        String::new()
    }
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub rect: Rect,
    pub title: String,
}

struct EnumCtx {
    windows: Vec<WindowInfo>,
    process_id: u32,
    virtual_rect: Rect,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);

    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return TRUE;
    }

    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return TRUE;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.process_id {
        return TRUE;
    }

    if is_window_cloaked(hwnd) {
        return TRUE;
    }

    let class_name = window_class(hwnd);
    if is_shell_window_class(&class_name) {
        return TRUE;
    }

    let Some(rect) = window_rect(hwnd) else {
        return TRUE;
    };
    if rect.is_empty() || !rect.intersects(ctx.virtual_rect) {
        return TRUE;
    }

    let mut title = window_title(hwnd);
    if title.trim().is_empty() {
        title = class_name.clone();
    }
    if title.trim().is_empty() {
        title = "窗口".to_string();
    }

    ctx.windows.push(WindowInfo { rect, title });
    TRUE
}

fn is_shell_window_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "Windows.Internal.Shell.TabProxyWindow"
            | "ForegroundStaging"
            | "MultitaskingViewFrame"
            | "XamlExplorerHostIslandWindow"
    )
}

pub fn top_level_windows(virtual_rect: Rect) -> Vec<WindowInfo> {
    let mut ctx = EnumCtx {
        windows: Vec::new(),
        process_id: current_process_id(),
        virtual_rect,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut ctx as *mut EnumCtx as isize),
        );
    }
    ctx.windows
}

pub fn window_at_point(windows: &[WindowInfo], point: Point) -> Option<usize> {
    windows.iter().position(|w| w.rect.contains(point))
}
