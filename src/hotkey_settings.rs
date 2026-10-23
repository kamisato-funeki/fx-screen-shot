use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, GetMonitorInfoW, GetSysColorBrush, MonitorFromPoint, UpdateWindow,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_BTNFACE, DEFAULT_CHARSET, DEFAULT_PITCH,
    FW_NORMAL, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetActiveWindow, SetFocus, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, IsDialogMessageW, LoadCursorW,
    MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, GWLP_USERDATA, HC_ACTION, HHOOK, HMENU, IDC_ARROW, KBDLLHOOKSTRUCT, MSG,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, WA_INACTIVE, WH_KEYBOARD_LL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_DPICHANGED, WM_KEYDOWN, WM_KEYUP, WM_NCDESTROY, WM_SETFONT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME,
    WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, UnhookWindowsHookEx};

use crate::config::{self, Hotkey, KNOWN_MODIFIER_MASK, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use crate::logging;
use crate::overlay::toolbar;
use crate::win_util;

const CLASS_NAME: &str = "FxShotHotkeySettings";
const WINDOW_TITLE: &str = "快捷键设置 - ScreenShot FX";
const ID_RECORD_SELECT: usize = 1001;
const ID_RECORD_QUICK: usize = 1002;
const ID_DEFAULTS: usize = 1003;
const ID_SAVE: usize = 1;
const ID_CANCEL: usize = 2;

const WM_APP_HOTKEY_CAPTURED: u32 = WM_APP + 0x41;
const WM_APP_HOTKEY_CANCEL: u32 = WM_APP + 0x42;
const WM_APP_HOTKEY_ERROR: u32 = WM_APP + 0x43;

const CLIENT_WIDTH: i32 = 520;
const CLIENT_HEIGHT: i32 = 260;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
static OPEN: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);
static HOOK_TARGET: AtomicIsize = AtomicIsize::new(0);
static HOOK_MODIFIERS: AtomicU32 = AtomicU32::new(0);

struct OpenGuard;

impl Drop for OpenGuard {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Select,
    Quick,
}

#[derive(Default)]
struct Controls {
    label_select: HWND,
    value_select: HWND,
    record_select: HWND,
    label_quick: HWND,
    value_quick: HWND,
    record_quick: HWND,
    hint: HWND,
    status: HWND,
    save: HWND,
    defaults: HWND,
    cancel: HWND,
}

struct SettingsState {
    owner: HWND,
    hwnd: HWND,
    select_text: String,
    quick_text: String,
    dpi: u32,
    font: HFONT,
    recording: Option<Slot>,
    hook: Option<RecordingHook>,
    controls: Controls,
}

impl Drop for SettingsState {
    fn drop(&mut self) {
        self.hook = None;
        if !self.font.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
        }
    }
}

impl SettingsState {
    fn create_controls(&mut self, hinstance: HINSTANCE) -> Result<(), String> {
        let base = WS_CHILD | WS_VISIBLE;
        let button = base | WS_TABSTOP | WINDOW_STYLE(0);
        let value_style = base | WS_BORDER | WINDOW_STYLE(0x0201);

        self.controls.label_select = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "选区截图快捷键",
            base,
            0,
            hinstance,
        )?;
        self.controls.value_select = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "",
            value_style,
            0,
            hinstance,
        )?;
        self.controls.record_select = create_control(
            self.hwnd,
            windows::core::w!("BUTTON"),
            "录制…",
            button,
            ID_RECORD_SELECT,
            hinstance,
        )?;

        self.controls.label_quick = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "快速截图快捷键",
            base,
            0,
            hinstance,
        )?;
        self.controls.value_quick = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "",
            value_style,
            0,
            hinstance,
        )?;
        self.controls.record_quick = create_control(
            self.hwnd,
            windows::core::w!("BUTTON"),
            "录制…",
            button,
            ID_RECORD_QUICK,
            hinstance,
        )?;

        self.controls.hint = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "点击“录制…”，然后按下新的组合键；Esc 取消录制。必须包含 Shift/Ctrl/Alt/Win 中至少一个修饰键。",
            base,
            0,
            hinstance,
        )?;
        self.controls.status = create_control(
            self.hwnd,
            windows::core::w!("STATIC"),
            "当前快捷键；修改后点击“保存”立即生效。",
            base,
            0,
            hinstance,
        )?;

        self.controls.defaults = create_control(
            self.hwnd,
            windows::core::w!("BUTTON"),
            "恢复默认",
            button,
            ID_DEFAULTS,
            hinstance,
        )?;
        self.controls.cancel = create_control(
            self.hwnd,
            windows::core::w!("BUTTON"),
            "取消",
            button,
            ID_CANCEL,
            hinstance,
        )?;
        self.controls.save = create_control(
            self.hwnd,
            windows::core::w!("BUTTON"),
            "保存",
            base | WS_TABSTOP | WINDOW_STYLE(1),
            ID_SAVE,
            hinstance,
        )?;

        self.set_font();
        self.refresh_values();
        self.refresh_buttons();
        Ok(())
    }

    fn set_font(&self) {
        if self.font.0.is_null() {
            return;
        }
        for control in [
            self.controls.label_select,
            self.controls.value_select,
            self.controls.record_select,
            self.controls.label_quick,
            self.controls.value_quick,
            self.controls.record_quick,
            self.controls.hint,
            self.controls.status,
            self.controls.save,
            self.controls.defaults,
            self.controls.cancel,
        ] {
            if !control.0.is_null() {
                unsafe {
                    let _ = SendMessageW(
                        control,
                        WM_SETFONT,
                        Some(WPARAM(self.font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }
        }
    }

    fn refresh_values(&self) {
        set_control_text(self.controls.value_select, &self.select_text);
        set_control_text(self.controls.value_quick, &self.quick_text);
    }

    fn refresh_buttons(&self) {
        let select_recording = self.recording == Some(Slot::Select);
        let quick_recording = self.recording == Some(Slot::Quick);
        set_control_text(
            self.controls.record_select,
            if select_recording {
                "按下组合键…"
            } else {
                "录制…"
            },
        );
        set_control_text(
            self.controls.record_quick,
            if quick_recording {
                "按下组合键…"
            } else {
                "录制…"
            },
        );
        let enabled = self.recording.is_none();
        unsafe {
            let _ = EnableWindow(self.controls.save, enabled);
            let _ = EnableWindow(self.controls.defaults, enabled);
        }
    }

    fn set_status(&self, text: &str) {
        set_control_text(self.controls.status, text);
    }

    fn start_recording(&mut self, slot: Slot) {
        self.stop_recording();
        match RecordingHook::install(self.hwnd) {
            Ok(hook) => {
                self.hook = Some(hook);
                self.recording = Some(slot);
                self.refresh_buttons();
                self.set_status("请按下新的组合键（Esc 取消录制）");
            }
            Err(err) => {
                self.set_status(&format!("无法开始录制：{err}"));
            }
        }
    }

    fn stop_recording(&mut self) {
        self.hook = None;
        self.recording = None;
        self.refresh_buttons();
    }

    fn cancel_recording(&mut self) {
        self.stop_recording();
        self.set_status("已取消录制。");
    }

    fn recording_error(&self, code: usize) {
        let text = match code {
            1 => "快捷键必须包含至少一个修饰键（Shift/Ctrl/Alt/Win）。",
            2 => "不支持的按键。支持 F1-F24、A-Z、0-9、Space/Tab/Enter/Esc、Insert/Delete/Home/End/PageUp/PageDown/PrintScreen/Pause。",
            _ => "录制失败，请重试。",
        };
        self.set_status(text);
    }

    fn captured(&mut self, vk: u32, modifiers: u32) {
        let Some(slot) = self.recording.take() else {
            return;
        };
        self.hook = None;
        let hotkey = match Hotkey::from_parts(modifiers, vk) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                self.refresh_buttons();
                self.set_status(&format!("录制失败：{err}"));
                return;
            }
        };
        match slot {
            Slot::Select => self.select_text = hotkey.display(),
            Slot::Quick => self.quick_text = hotkey.display(),
        }
        self.refresh_values();
        self.refresh_buttons();
        self.set_status(&format!(
            "已录制：{}；点击“保存”立即生效。",
            hotkey.display()
        ));
    }

    fn restore_defaults(&mut self) {
        self.select_text = config::DEFAULT_SELECT_HOTKEY.to_string();
        self.quick_text = config::DEFAULT_QUICK_HOTKEY.to_string();
        self.refresh_values();
        self.set_status("已恢复默认值；点击“保存”生效。");
    }

    fn save(&mut self) {
        if self.recording.is_some() {
            self.set_status("请先完成录制，或按 Esc 取消录制。");
            return;
        }
        let pair = match config::validate_hotkeys(&self.select_text, &self.quick_text) {
            Ok(pair) => pair,
            Err(err) => {
                self.set_status(&format!("无法保存：{err}"));
                return;
            }
        };
        match crate::app::apply_hotkeys(self.owner, pair.0, pair.1) {
            Ok(()) => {
                let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            Err(err) => {
                self.set_status(&format!("保存失败：{err}"));
            }
        }
    }

    fn layout_controls(&self) {
        let mut client = RECT::default();
        if unsafe { GetClientRect(self.hwnd, &mut client) }.is_err() {
            return;
        }
        let width = client.right - client.left;
        if width <= 0 {
            return;
        }
        let dpi = self.dpi.max(96);
        let margin = toolbar::scale(18, dpi);
        let top = toolbar::scale(20, dpi);
        let row_h = toolbar::scale(32, dpi);
        let gap = toolbar::scale(12, dpi);
        let label_w = toolbar::scale(150, dpi);
        let value_w = toolbar::scale(190, dpi);
        let record_w = toolbar::scale(120, dpi);
        let x_label = margin;
        let x_value = x_label + label_w + toolbar::scale(10, dpi);
        let x_record = x_value + value_w + toolbar::scale(10, dpi);

        let y_select = top;
        let y_quick = top + row_h + gap;
        let y_hint = y_quick + row_h + toolbar::scale(10, dpi);
        let y_status = y_hint + toolbar::scale(42, dpi);

        move_control(
            self.controls.label_select,
            x_label,
            y_select,
            label_w,
            row_h,
        );
        move_control(
            self.controls.value_select,
            x_value,
            y_select,
            value_w,
            row_h,
        );
        move_control(
            self.controls.record_select,
            x_record,
            y_select,
            record_w,
            row_h,
        );
        move_control(self.controls.label_quick, x_label, y_quick, label_w, row_h);
        move_control(self.controls.value_quick, x_value, y_quick, value_w, row_h);
        move_control(
            self.controls.record_quick,
            x_record,
            y_quick,
            record_w,
            row_h,
        );
        move_control(
            self.controls.hint,
            margin,
            y_hint,
            width - margin * 2,
            toolbar::scale(42, dpi),
        );
        move_control(
            self.controls.status,
            margin,
            y_status,
            width - margin * 2,
            toolbar::scale(22, dpi),
        );

        let button_h = toolbar::scale(32, dpi);
        let button_w = toolbar::scale(90, dpi);
        let y_buttons = toolbar::scale(CLIENT_HEIGHT, dpi) - margin - button_h;
        move_control(
            self.controls.defaults,
            margin,
            y_buttons,
            toolbar::scale(120, dpi),
            button_h,
        );
        move_control(
            self.controls.cancel,
            width - margin - button_w * 2 - toolbar::scale(10, dpi),
            y_buttons,
            button_w,
            button_h,
        );
        move_control(
            self.controls.save,
            width - margin - button_w,
            y_buttons,
            button_w,
            button_h,
        );
    }

    fn apply_window_layout(&mut self, center: bool) {
        let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
        let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: toolbar::scale(CLIENT_WIDTH, self.dpi),
            bottom: toolbar::scale(CLIENT_HEIGHT, self.dpi),
        };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut outer, style, false, ex_style, self.dpi);
        }
        let width = outer.right - outer.left;
        let height = outer.bottom - outer.top;
        if center {
            center_window(self.hwnd, width, height);
        } else {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    width,
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        self.layout_controls();
    }

    fn recreate_font(&mut self) {
        self.dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        if !self.font.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
        }
        self.font = create_font(self.dpi);
        self.set_font();
    }
}

pub fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

pub fn show(owner: HWND, current: (Hotkey, Hotkey)) -> Result<(), String> {
    if OPEN.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let _guard = OpenGuard;
    DONE.store(false, Ordering::SeqCst);
    show_inner(owner, current)
}

fn show_inner(owner: HWND, current: (Hotkey, Hotkey)) -> Result<(), String> {
    register_class()?;
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|err| format!("获取模块句柄失败：{err}"))?;
    let hinstance = HINSTANCE(module.0);
    let class_name = win_util::wide(CLASS_NAME);
    let title = win_util::wide(WINDOW_TITLE);

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|err| format!("创建快捷键设置窗口失败：{err}"))?;

    center_window(hwnd, 1, 1);

    let state = Box::new(SettingsState {
        owner,
        hwnd,
        select_text: current.0.display(),
        quick_text: current.1.display(),
        dpi: 96,
        font: HFONT::default(),
        recording: None,
        hook: None,
        controls: Controls::default(),
    });
    let state_ptr = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
    }
    logging::info("hotkey settings opened");

    let setup = {
        let state = unsafe { &mut *state_ptr };
        setup_state(state, hinstance)
    };
    if let Err(err) = setup {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(err);
    }

    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }

    let mut message = MSG::default();
    while !DONE.load(Ordering::SeqCst) {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            break;
        }
        if result.0 == 0 {
            unsafe {
                PostQuitMessage(message.wParam.0 as i32);
            }
            break;
        }
        if unsafe { IsDialogMessageW(hwnd, &message) }.as_bool() {
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    unsafe {
        let _ = EnableWindow(owner, true);
    }
    Ok(())
}

fn setup_state(state: &mut SettingsState, hinstance: HINSTANCE) -> Result<(), String> {
    state.dpi = unsafe { GetDpiForWindow(state.hwnd) }.max(96);
    state.font = create_font(state.dpi);
    state.create_controls(hinstance)?;
    state.apply_window_layout(true);
    Ok(())
}

fn register_class() -> Result<(), String> {
    if CLASS_REGISTERED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|err| format!("获取模块句柄失败：{err}"))?;
    let hinstance = HINSTANCE(module.0);
    let class_name = win_util::wide(CLASS_NAME);
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        hCursor: cursor,
        hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error.0 != 1410 {
            return Err(format!("注册快捷键设置窗口失败：Windows error {}", error.0));
        }
    }
    CLASS_REGISTERED.store(true, Ordering::SeqCst);
    Ok(())
}

fn create_control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: usize,
    hinstance: HINSTANCE,
) -> Result<HWND, String> {
    let text = win_util::wide(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(text.as_ptr()),
            style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(id as isize as *mut core::ffi::c_void)),
            Some(hinstance),
            None,
        )
    }
    .map_err(|err| format!("创建设置控件失败：{err}"))
}

fn create_font(dpi: u32) -> HFONT {
    let face = win_util::wide("Microsoft YaHei UI");
    unsafe {
        CreateFontW(
            -toolbar::scale(9, dpi),
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

fn move_control(control: HWND, x: i32, y: i32, width: i32, height: i32) {
    if control.0.is_null() {
        return;
    }
    unsafe {
        let _ = MoveWindow(control, x, y, width, height, true);
    }
}

fn set_control_text(control: HWND, text: &str) {
    if control.0.is_null() {
        return;
    }
    let wide = win_util::wide(text);
    unsafe {
        let _ = SetWindowTextW(control, PCWSTR(wide.as_ptr()));
    }
}

fn center_window(hwnd: HWND, width: i32, height: i32) {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (x, y) = if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let work_width = info.rcWork.right - info.rcWork.left;
        let work_height = info.rcWork.bottom - info.rcWork.top;
        (
            info.rcWork.left + (work_width - width) / 2,
            info.rcWork.top + (work_height - height) / 2,
        )
    } else {
        (0, 0)
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut SettingsState) -> R) -> Option<R> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsState;
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { f(&mut *pointer) })
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            with_state(hwnd, |state| match id {
                ID_RECORD_SELECT => state.start_recording(Slot::Select),
                ID_RECORD_QUICK => state.start_recording(Slot::Quick),
                ID_DEFAULTS => state.restore_defaults(),
                ID_SAVE => state.save(),
                ID_CANCEL => {
                    state.stop_recording();
                    let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                }
                _ => {}
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            with_state(hwnd, |state| {
                if state.recording.is_some() {
                    return;
                }
                if vk == VK_ESCAPE.0 as u32 {
                    let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                } else if vk == VK_RETURN.0 as u32 {
                    state.save();
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            let active = (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE;
            if !active {
                with_state(hwnd, |state| {
                    if state.recording.is_some() {
                        state.cancel_recording();
                    }
                });
            }
            LRESULT(0)
        }
        WM_APP_HOTKEY_CAPTURED => {
            with_state(hwnd, |state| {
                state.captured(wparam.0 as u32, lparam.0 as u32);
            });
            LRESULT(0)
        }
        WM_APP_HOTKEY_CANCEL => {
            with_state(hwnd, |state| state.cancel_recording());
            LRESULT(0)
        }
        WM_APP_HOTKEY_ERROR => {
            with_state(hwnd, |state| state.recording_error(wparam.0));
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = lparam.0 as *const RECT;
            if !suggested.is_null() {
                let rect = unsafe { *suggested };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            with_state(hwnd, |state| {
                state.recreate_font();
                state.layout_controls();
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            with_state(hwnd, |state| {
                state.hook = None;
                state.recording = None;
            });
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            DONE.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsState;
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            if !pointer.is_null() {
                unsafe {
                    drop(Box::from_raw(pointer));
                }
            }
            DONE.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

struct RecordingHook {
    handle: HHOOK,
}

impl RecordingHook {
    fn install(target: HWND) -> Result<Self, String> {
        HOOK_TARGET.store(target.0 as isize, Ordering::SeqCst);
        HOOK_MODIFIERS.store(current_modifiers(), Ordering::SeqCst);
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
            Ok(handle) => Ok(Self { handle }),
            Err(err) => {
                HOOK_TARGET.store(0, Ordering::SeqCst);
                HOOK_MODIFIERS.store(0, Ordering::SeqCst);
                Err(format!("安装键盘钩子失败：{err}"))
            }
        }
    }
}

impl Drop for RecordingHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.handle);
        }
        HOOK_TARGET.store(0, Ordering::SeqCst);
        HOOK_MODIFIERS.store(0, Ordering::SeqCst);
    }
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode != HC_ACTION as i32 {
        return CallNextHookEx(None, ncode, wparam, lparam);
    }
    let target = HOOK_TARGET.load(Ordering::SeqCst);
    if target == 0 {
        return CallNextHookEx(None, ncode, wparam, lparam);
    }
    let keyboard = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let vk = keyboard.vkCode;
    let message = wparam.0 as u32;

    if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
        if let Some(bit) = modifier_bit(vk) {
            HOOK_MODIFIERS.fetch_or(bit, Ordering::SeqCst);
            return LRESULT(1);
        }
        let modifiers = HOOK_MODIFIERS.load(Ordering::SeqCst);
        if vk == VK_ESCAPE.0 as u32 && modifiers == 0 {
            post_hook_message(target, WM_APP_HOTKEY_CANCEL, 0, 0);
            return LRESULT(1);
        }
        if modifiers & KNOWN_MODIFIER_MASK == 0 {
            post_hook_message(target, WM_APP_HOTKEY_ERROR, 1, 0);
            return LRESULT(1);
        }
        match Hotkey::from_parts(modifiers, vk) {
            Ok(_) => post_hook_message(
                target,
                WM_APP_HOTKEY_CAPTURED,
                vk as usize,
                modifiers as isize,
            ),
            Err(_) => post_hook_message(target, WM_APP_HOTKEY_ERROR, 2, 0),
        }
        return LRESULT(1);
    }

    if message == WM_KEYUP || message == WM_SYSKEYUP {
        if let Some(bit) = modifier_bit(vk) {
            HOOK_MODIFIERS.fetch_and(!bit, Ordering::SeqCst);
        }
        return LRESULT(1);
    }

    CallNextHookEx(None, ncode, wparam, lparam)
}

fn post_hook_message(target: isize, message: u32, wparam: usize, lparam: isize) {
    let _ = unsafe {
        PostMessageW(
            Some(HWND(target as *mut core::ffi::c_void)),
            message,
            WPARAM(wparam),
            LPARAM(lparam),
        )
    };
}

fn modifier_bit(vk: u32) -> Option<u32> {
    match vk {
        vk if vk == VK_SHIFT.0 as u32 || vk == VK_LSHIFT.0 as u32 || vk == VK_RSHIFT.0 as u32 => {
            Some(MOD_SHIFT)
        }
        vk if vk == VK_CONTROL.0 as u32
            || vk == VK_LCONTROL.0 as u32
            || vk == VK_RCONTROL.0 as u32 =>
        {
            Some(MOD_CONTROL)
        }
        vk if vk == VK_MENU.0 as u32 || vk == VK_LMENU.0 as u32 || vk == VK_RMENU.0 as u32 => {
            Some(MOD_ALT)
        }
        vk if vk == VK_LWIN.0 as u32 || vk == VK_RWIN.0 as u32 => Some(MOD_WIN),
        _ => None,
    }
}

fn current_modifiers() -> u32 {
    let mut modifiers = 0;
    if win_util::key_down(VK_SHIFT.0 as u32)
        || win_util::key_down(VK_LSHIFT.0 as u32)
        || win_util::key_down(VK_RSHIFT.0 as u32)
    {
        modifiers |= MOD_SHIFT;
    }
    if win_util::key_down(VK_CONTROL.0 as u32)
        || win_util::key_down(VK_LCONTROL.0 as u32)
        || win_util::key_down(VK_RCONTROL.0 as u32)
    {
        modifiers |= MOD_CONTROL;
    }
    if win_util::key_down(VK_MENU.0 as u32)
        || win_util::key_down(VK_LMENU.0 as u32)
        || win_util::key_down(VK_RMENU.0 as u32)
    {
        modifiers |= MOD_ALT;
    }
    if win_util::key_down(VK_LWIN.0 as u32) || win_util::key_down(VK_RWIN.0 as u32) {
        modifiers |= MOD_WIN;
    }
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_modifier_keys() {
        assert_eq!(modifier_bit(VK_SHIFT.0 as u32), Some(MOD_SHIFT));
        assert_eq!(modifier_bit(VK_LCONTROL.0 as u32), Some(MOD_CONTROL));
        assert_eq!(modifier_bit(VK_RMENU.0 as u32), Some(MOD_ALT));
        assert_eq!(modifier_bit(VK_LWIN.0 as u32), Some(MOD_WIN));
        assert_eq!(modifier_bit(VK_ESCAPE.0 as u32), None);
    }

    #[test]
    fn default_texts_are_valid() {
        let select = Hotkey::parse(config::DEFAULT_SELECT_HOTKEY).unwrap();
        let quick = Hotkey::parse(config::DEFAULT_QUICK_HOTKEY).unwrap();
        assert_eq!(select.display(), "Shift+F1");
        assert_eq!(quick.display(), "Shift+F2");
    }

    #[test]
    #[ignore = "requires an interactive desktop"]
    fn settings_window_opens_and_closes() {
        let closer = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(600));
            let class = win_util::wide(CLASS_NAME);
            let title = win_util::wide(WINDOW_TITLE);
            let hwnd = unsafe {
                windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                    PCWSTR(class.as_ptr()),
                    PCWSTR(title.as_ptr()),
                )
            }
            .unwrap_or_default();
            if !hwnd.0.is_null() {
                let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        });

        let select = Hotkey::parse(config::DEFAULT_SELECT_HOTKEY).unwrap();
        let quick = Hotkey::parse(config::DEFAULT_QUICK_HOTKEY).unwrap();
        let result = show(HWND::default(), (select, quick));
        closer.join().expect("closer thread");
        assert!(result.is_ok());
        assert!(!is_open());
    }
}
