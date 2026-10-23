use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, E_ACCESSDENIED, HANDLE, HINSTANCE, HWND,
    LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, EnumWindows, GetCursorPos, GetMessageW, KillTimer, LoadIconW, MessageBoxW,
    PostMessageW, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, TranslateMessage, MB_ICONINFORMATION, MB_OK, MF_CHECKED, MF_SEPARATOR,
    MF_STRING, MSG, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONDBLCLK,
    WM_LBUTTONUP, WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_POPUP,
};

use crate::autostart;
use crate::capture;
use crate::config::{self, AppConfig, Backend, Hotkey};
use crate::hotkey_settings;
use crate::logging;
use crate::monitor;
use crate::naming;
use crate::overlay;
use crate::win_util;

const MAIN_CLASS: &str = "FxShotMainWindow";
const MAIN_TITLE: &str = "ScreenShot FX";
const WM_APP_TRAY: u32 = WM_APP + 1;
const WM_APP_ALREADY_RUNNING: u32 = WM_APP + 2;
const TIMER_TRAY_RETRY: usize = 1;

const MENU_SELECT: u32 = 1001;
const MENU_QUICK: u32 = 1002;
const MENU_OPEN_SAVE: u32 = 1003;
const MENU_OPEN_CONFIG: u32 = 1004;
const MENU_AUTOSTART: u32 = 1005;
const MENU_COMPAT: u32 = 1006;
const MENU_RELOAD: u32 = 1007;
const MENU_ABOUT: u32 = 1008;
const MENU_EXIT: u32 = 1009;
const MENU_HOTKEYS: u32 = 1010;

struct Runtime {
    config: AppConfig,
    hotkeys: (Hotkey, Hotkey),
}

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);
static TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> Option<Self> {
        if BUSY.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

pub fn run(no_autostart: bool) -> i32 {
    // Portable/test hook: lets a second instance use its own singleton mutex
    // so it can run next to an installed copy with different hotkeys.
    let mutex_name_text = std::env::var("FXSHOT_MUTEX_NAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Local\\ScreenShotFX.Singleton".to_string());
    let mutex_name = win_util::wide(&mutex_name_text);
    let mutex = match unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) } {
        Ok(handle) => {
            if unsafe { GetLastError() }.0 == ERROR_ALREADY_EXISTS.0 {
                if let Some(existing) = find_main_window() {
                    let _ = unsafe {
                        PostMessageW(Some(existing), WM_APP_ALREADY_RUNNING, WPARAM(0), LPARAM(0))
                    };
                }
                let _ = unsafe { CloseHandle(handle) };
                logging::info("another instance is already running");
                return 0;
            }
            Some(handle)
        }
        Err(err) if err.code() == E_ACCESSDENIED => {
            // Restricted tokens sometimes do not have a usable default DACL
            // for named mutexes. Keep the app usable by falling back to the
            // hidden main window for the single-instance check.
            logging::warn(&format!(
                "CreateMutexW denied ({err}); using window-based single-instance check"
            ));
            if let Some(existing) = find_main_window() {
                let _ = unsafe {
                    PostMessageW(Some(existing), WM_APP_ALREADY_RUNNING, WPARAM(0), LPARAM(0))
                };
                logging::info("another instance is already running");
                return 0;
            }
            None
        }
        Err(err) => {
            logging::error(&format!("CreateMutexW failed: {err}"));
            return 1;
        }
    };

    let loaded = config::load_or_create();
    let mut warnings = loaded.warnings;
    let exe = win_util::current_exe();

    let mut config = loaded.config;
    if !config.autostart_initialized {
        if no_autostart {
            logging::info("first-run autostart skipped by --no-autostart");
        } else {
            config.autostart = true;
            config.autostart_initialized = true;
            if let Err(err) = config::update_runtime_flags(None, Some(true), Some(true)) {
                warnings.push(format!("写入首次自启配置失败：{err:#}"));
            }
        }
    }

    if !no_autostart {
        match autostart::sync(config.autostart, &exe) {
            Ok(()) => logging::info(&format!(
                "autostart {} for {}",
                if config.autostart {
                    "enabled"
                } else {
                    "disabled"
                },
                exe.display()
            )),
            Err(err) => {
                warnings.push(format!("同步开机自启失败：{err:#}"));
            }
        }
    }

    let hotkeys = match config::parse_hotkeys(&config) {
        Ok(hotkeys) => hotkeys,
        Err(err) => {
            warnings.push(format!("解析快捷键失败：{err}"));
            fallback_hotkeys()
        }
    };

    if RUNTIME
        .set(Mutex::new(Runtime { config, hotkeys }))
        .is_err()
    {
        logging::error("runtime state already initialized");
        close_mutex(mutex);
        return 1;
    }

    let hinstance = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(err) => {
            logging::error(&format!("GetModuleHandleW failed: {err}"));
            close_mutex(mutex);
            return 1;
        }
    };

    let class_name = win_util::wide(MAIN_CLASS);
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error.0 != 1410 {
            logging::error(&format!("RegisterClassExW failed: {}", error.0));
            close_mutex(mutex);
            return 1;
        }
    }

    let title = win_util::wide(MAIN_TITLE);
    let hwnd = match unsafe {
        CreateWindowExW(
            Default::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
    } {
        Ok(hwnd) => hwnd,
        Err(err) => {
            logging::error(&format!("CreateWindowExW failed: {err}"));
            close_mutex(mutex);
            return 1;
        }
    };

    unsafe {
        let taskbar = RegisterWindowMessageW(PCWSTR(win_util::wide("TaskbarCreated").as_ptr()));
        TASKBAR_CREATED.store(taskbar, Ordering::SeqCst);
    }

    TRAY_ICON.store(load_icon(hinstance).0 as isize, Ordering::SeqCst);
    if !add_tray_icon(hwnd, true) {
        warnings.push("托盘图标添加失败，正在重试".to_string());
        unsafe {
            let _ = SetTimer(Some(hwnd), TIMER_TRAY_RETRY, 2000, None);
        }
    }

    let (select, quick) = RUNTIME
        .get()
        .and_then(|runtime| runtime.lock().ok().map(|runtime| runtime.hotkeys))
        .unwrap_or_else(fallback_hotkeys);

    if let Err(err) = register_hotkeys(hwnd, select, quick) {
        warnings.push(err);
    }

    for warning in &warnings {
        logging::warn(warning);
    }
    if let Some(first) = warnings.first() {
        notify_error(hwnd, first);
    }

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    cleanup(hwnd);
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    close_mutex(mutex);
    logging::info("fxshot stopped");
    0
}

fn close_mutex(mutex: Option<HANDLE>) {
    if let Some(mutex) = mutex {
        let _ = unsafe { CloseHandle(mutex) };
    }
}

fn fallback_hotkeys() -> (Hotkey, Hotkey) {
    (
        Hotkey::parse(config::DEFAULT_SELECT_HOTKEY).expect("default select hotkey"),
        Hotkey::parse(config::DEFAULT_QUICK_HOTKEY).expect("default quick hotkey"),
    )
}

fn find_main_window() -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(
            Some(enum_main_window),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
    }
    found
}

unsafe extern "system" fn enum_main_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let slot = &mut *(lparam.0 as *mut Option<HWND>);
    if win_util::window_class(hwnd) == MAIN_CLASS {
        *slot = Some(hwnd);
        return BOOL(0);
    }
    BOOL(1)
}

fn load_icon(hinstance: HINSTANCE) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    let resource = PCWSTR(1 as *const u16);
    match unsafe { LoadIconW(Some(hinstance), resource) } {
        Ok(icon) => icon,
        Err(err) => {
            logging::warn(&format!("LoadIconW failed: {err}"));
            windows::Win32::UI::WindowsAndMessaging::HICON::default()
        }
    }
}

fn tray_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    windows::Win32::UI::WindowsAndMessaging::HICON(
        TRAY_ICON.load(Ordering::SeqCst) as *mut core::ffi::c_void
    )
}

fn add_tray_icon(hwnd: HWND, log_failure: bool) -> bool {
    let icon = tray_icon();
    if icon.0.is_null() {
        return false;
    }
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let tip = tray_tip_text();
    copy_wide(&mut data.szTip, &tip);
    let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
    if !ok && log_failure {
        let error = unsafe { GetLastError() };
        logging::warn(&format!(
            "Shell_NotifyIconW(NIM_ADD) failed: Windows error {}",
            error.0
        ));
    }
    ok
}

fn tray_tip_text() -> String {
    let (select, quick) = with_runtime(|runtime| runtime.hotkeys).unwrap_or_else(fallback_hotkeys);
    format!(
        "ScreenShot FX\n{} 选区 / {} 快速",
        select.display(),
        quick.display()
    )
}

fn update_tray_tip(hwnd: HWND) {
    if TRAY_ICON.load(Ordering::SeqCst) == 0 {
        return;
    }
    let tip = tray_tip_text();
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    copy_wide(&mut data.szTip, &tip);
    if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
        logging::warn("Shell_NotifyIconW(NIM_MODIFY, NIF_TIP) failed");
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
}

fn copy_wide<const N: usize>(target: &mut [u16; N], text: &str) {
    if N == 0 {
        return;
    }
    let encoded: Vec<u16> = text.encode_utf16().take(N - 1).collect();
    target[..encoded.len()].copy_from_slice(&encoded);
    target[encoded.len()] = 0;
}

pub fn notify_error(hwnd: HWND, message: &str) {
    logging::error(message);
    show_balloon(hwnd, "ScreenShot FX", message, true);
}

fn show_balloon(hwnd: HWND, title: &str, message: &str, warning: bool) {
    if TRAY_ICON.load(Ordering::SeqCst) == 0 {
        logging::warn("tray icon not ready; balloon skipped");
        return;
    }
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_INFO,
        dwInfoFlags: if warning { NIIF_WARNING } else { NIIF_INFO },
        ..Default::default()
    };
    copy_wide(&mut data.szInfoTitle, title);
    copy_wide(&mut data.szInfo, message);
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
}

fn register_hotkeys(hwnd: HWND, select: Hotkey, quick: Hotkey) -> Result<(), String> {
    let mut errors = Vec::new();
    if unsafe {
        RegisterHotKey(
            Some(hwnd),
            1,
            HOT_KEY_MODIFIERS(select.register_modifiers()),
            select.vk,
        )
    }
    .is_err()
    {
        let error = unsafe { GetLastError() };
        errors.push(format!(
            "{} 注册失败（可能被其他程序占用，Windows error {}）",
            select.display(),
            error.0
        ));
    }
    if unsafe {
        RegisterHotKey(
            Some(hwnd),
            2,
            HOT_KEY_MODIFIERS(quick.register_modifiers()),
            quick.vk,
        )
    }
    .is_err()
    {
        let error = unsafe { GetLastError() };
        errors.push(format!(
            "{} 注册失败（可能被其他程序占用，Windows error {}）",
            quick.display(),
            error.0
        ));
    }
    if errors.is_empty() {
        logging::info(&format!(
            "hotkeys registered: {} and {}",
            select.display(),
            quick.display()
        ));
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

pub(crate) fn apply_hotkeys(hwnd: HWND, select: Hotkey, quick: Hotkey) -> Result<(), String> {
    let Some((old_select, old_quick)) = with_runtime(|runtime| runtime.hotkeys) else {
        return Err("运行时状态不可用".to_string());
    };
    if select == old_select && quick == old_quick {
        return Ok(());
    }

    unregister_hotkeys(hwnd);
    if let Err(err) = register_hotkeys(hwnd, select, quick) {
        unregister_hotkeys(hwnd);
        if let Err(restore) = register_hotkeys(hwnd, old_select, old_quick) {
            logging::error(&format!("restore old hotkeys failed: {restore}"));
            return Err(format!(
                "{err}；恢复旧快捷键也失败：{restore}，请重启程序或重新加载配置"
            ));
        }
        return Err(err);
    }

    let select_text = select.display();
    let quick_text = quick.display();
    if let Err(err) = config::update_hotkeys(&select_text, &quick_text) {
        unregister_hotkeys(hwnd);
        if let Err(restore) = register_hotkeys(hwnd, old_select, old_quick) {
            logging::error(&format!("restore old hotkeys failed: {restore}"));
            return Err(format!(
                "保存配置失败：{err:#}；恢复旧快捷键也失败：{restore}，请重启程序"
            ));
        }
        return Err(format!("保存配置失败：{err:#}"));
    }

    if let Some(runtime) = RUNTIME.get() {
        if let Ok(mut runtime) = runtime.lock() {
            runtime.config.select_hotkey = select_text.clone();
            runtime.config.quick_hotkey = quick_text.clone();
            runtime.hotkeys = (select, quick);
        } else {
            logging::warn("runtime lock poisoned while updating hotkeys");
        }
    }

    update_tray_tip(hwnd);
    logging::info(&format!(
        "hotkeys updated: {} and {}",
        select_text, quick_text
    ));
    show_balloon(
        hwnd,
        "ScreenShot FX",
        &format!("快捷键已更新：{select_text} / {quick_text}"),
        false,
    );
    Ok(())
}

fn unregister_hotkeys(hwnd: HWND) {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), 1);
        let _ = UnregisterHotKey(Some(hwnd), 2);
    }
}

fn with_runtime<R>(f: impl FnOnce(&Runtime) -> R) -> Option<R> {
    RUNTIME
        .get()
        .and_then(|runtime| runtime.lock().ok().map(|runtime| f(&runtime)))
}

fn effective_backend(config: &AppConfig) -> Backend {
    config.backend
}

fn start_selection(hwnd: HWND) {
    if overlay::is_active() {
        overlay::finish();
        return;
    }
    let Some(_guard) = BusyGuard::acquire() else {
        return;
    };

    let Some((backend, exposure, save_dir, clipboard, snap_distance)) = with_runtime(|runtime| {
        (
            effective_backend(&runtime.config),
            runtime.config.hdr_exposure,
            std::path::PathBuf::from(&runtime.config.save_dir),
            runtime.config.clipboard,
            runtime.config.snap_distance,
        )
    }) else {
        return;
    };

    let monitors = monitor::enumerate();
    if monitors.is_empty() {
        notify_error(hwnd, "没有检测到显示器");
        return;
    }
    let virtual_rect = monitor::virtual_rect(&monitors);
    let frame = match capture::capture_virtual(&monitors, backend, exposure) {
        Ok(frame) => frame,
        Err(err) => {
            notify_error(hwnd, &format!("抓取屏幕失败：{err:#}"));
            return;
        }
    };
    let windows = win_util::top_level_windows(virtual_rect);
    let options = overlay::OverlayOptions {
        save_dir,
        clipboard,
        snap_distance,
    };
    if let Err(err) = overlay::start(hwnd, frame, monitors, windows, options) {
        notify_error(hwnd, &format!("打开选区失败：{err:#}"));
    }
}

fn quick_capture(hwnd: HWND) {
    if overlay::is_active() {
        overlay::finish();
        overlay::flush_after_close();
    }
    let Some(_guard) = BusyGuard::acquire() else {
        return;
    };
    let Some((backend, exposure, save_dir, clipboard)) = with_runtime(|runtime| {
        (
            effective_backend(&runtime.config),
            runtime.config.hdr_exposure,
            std::path::PathBuf::from(&runtime.config.save_dir),
            runtime.config.clipboard,
        )
    }) else {
        return;
    };

    let Some(cursor) = win_util::get_cursor_pos() else {
        notify_error(hwnd, "无法获取鼠标位置");
        return;
    };
    let monitors = monitor::enumerate();
    let Some(index) = monitor::select_monitor(cursor, &monitors) else {
        notify_error(hwnd, "没有检测到显示器");
        return;
    };
    let monitor = &monitors[index];
    let (image, used) = match capture::capture_monitor_with_fallback(monitor, backend, exposure) {
        Ok(result) => result,
        Err(err) => {
            notify_error(hwnd, &format!("快速截图失败：{err:#}"));
            return;
        }
    };
    logging::info(&format!(
        "quick capture {} with {}",
        monitor.device_name,
        used.as_str()
    ));

    let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    for pixel in image.bgra.chunks_exact(4) {
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }

    match naming::save_png(&save_dir, &rgb, image.width, image.height) {
        Ok(path) => logging::info(&format!("quick screenshot saved {}", path.display())),
        Err(err) => {
            notify_error(hwnd, &format!("保存快速截图失败：{err:#}"));
            return;
        }
    }

    if clipboard {
        if let Err(err) = capture::clipboard::copy_rgb(hwnd, &rgb, image.width, image.height) {
            logging::warn(&format!("clipboard copy failed: {err:#}"));
            notify_error(hwnd, &format!("复制到剪贴板失败：{err:#}"));
        }
    }
}

fn show_menu(hwnd: HWND) {
    let Some((select_label, quick_label, autostart_enabled, compat_enabled)) =
        with_runtime(|runtime| {
            (
                runtime.hotkeys.0.display(),
                runtime.hotkeys.1.display(),
                runtime.config.autostart,
                runtime.config.backend == Backend::Wgc,
            )
        })
    else {
        return;
    };

    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };

    let select_text = format!("选区截图\t{select_label}");
    let quick_text = format!("快速截图\t{quick_label}");
    append_menu(menu, MF_STRING, MENU_SELECT, &select_text);
    append_menu(menu, MF_STRING, MENU_QUICK, &quick_text);
    append_menu(menu, MF_SEPARATOR, 0, "");
    append_menu(menu, MF_STRING, MENU_HOTKEYS, "快捷键设置…");
    append_menu(menu, MF_STRING, MENU_OPEN_SAVE, "打开保存目录");
    append_menu(menu, MF_STRING, MENU_OPEN_CONFIG, "打开配置目录");
    append_menu(
        menu,
        if autostart_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        },
        MENU_AUTOSTART,
        "开机自启动",
    );
    append_menu(
        menu,
        if compat_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        },
        MENU_COMPAT,
        "兼容模式（WGC，有黄框）",
    );
    append_menu(menu, MF_STRING, MENU_RELOAD, "重新加载配置");
    append_menu(menu, MF_STRING, MENU_ABOUT, "关于");
    append_menu(menu, MF_SEPARATOR, 0, "");
    append_menu(menu, MF_STRING, MENU_EXIT, "退出");

    let mut point = windows::Win32::Foundation::POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    let command = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
    };
    unsafe {
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    }
    if command.0 != 0 {
        handle_menu_command(hwnd, command.0 as u32);
    }
}

fn append_menu(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    id: u32,
    text: &str,
) {
    let wide = win_util::wide(text);
    let _ = unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(wide.as_ptr())) };
}

fn show_hotkey_settings(hwnd: HWND) {
    if overlay::is_active() {
        overlay::finish();
        overlay::flush_after_close();
    }
    let current = with_runtime(|runtime| runtime.hotkeys).unwrap_or_else(fallback_hotkeys);
    if let Err(err) = hotkey_settings::show(hwnd, current) {
        notify_error(hwnd, &format!("打开快捷键设置失败：{err}"));
    }
}

fn handle_menu_command(hwnd: HWND, command: u32) {
    match command {
        MENU_SELECT => start_selection(hwnd),
        MENU_QUICK => quick_capture(hwnd),
        MENU_HOTKEYS => show_hotkey_settings(hwnd),
        MENU_OPEN_SAVE => {
            if let Some(path) =
                with_runtime(|runtime| std::path::PathBuf::from(&runtime.config.save_dir))
            {
                let _ = std::fs::create_dir_all(&path);
                let _ = win_util::open_path(&path);
            }
        }
        MENU_OPEN_CONFIG => {
            let dir = config::config_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = win_util::open_path(&dir);
        }
        MENU_AUTOSTART => toggle_autostart(hwnd),
        MENU_COMPAT => toggle_compat(hwnd),
        MENU_RELOAD => reload_config(hwnd),
        MENU_ABOUT => show_about(hwnd),
        MENU_EXIT => {
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        _ => {}
    }
}

fn toggle_autostart(hwnd: HWND) {
    let Some(desired) = with_runtime(|runtime| !runtime.config.autostart) else {
        return;
    };
    let exe = win_util::current_exe();
    if let Err(err) = autostart::sync(desired, &exe) {
        notify_error(hwnd, &format!("切换开机自启失败：{err:#}"));
        return;
    }
    if let Err(err) = config::update_runtime_flags(None, Some(desired), Some(true)) {
        notify_error(hwnd, &format!("保存配置失败：{err:#}"));
        return;
    }
    if let Some(runtime) = RUNTIME.get() {
        if let Ok(mut runtime) = runtime.lock() {
            runtime.config.autostart = desired;
            runtime.config.autostart_initialized = true;
        }
    }
    logging::info(&format!(
        "autostart toggled {}",
        if desired { "on" } else { "off" }
    ));
}

fn toggle_compat(hwnd: HWND) {
    let Some(current) = with_runtime(|runtime| runtime.config.backend) else {
        return;
    };
    let desired = if current == Backend::Wgc {
        Backend::Auto
    } else {
        Backend::Wgc
    };
    if let Err(err) = config::update_runtime_flags(Some(desired), None, None) {
        notify_error(hwnd, &format!("保存配置失败：{err:#}"));
        return;
    }
    if let Some(runtime) = RUNTIME.get() {
        if let Ok(mut runtime) = runtime.lock() {
            runtime.config.backend = desired;
        }
    }
    logging::info(&format!("backend switched to {}", desired.as_str()));
}

fn reload_config(hwnd: HWND) {
    let loaded = config::load_or_create();
    let mut warnings = loaded.warnings;
    let new_config = loaded.config;
    let new_hotkeys = match config::parse_hotkeys(&new_config) {
        Ok(hotkeys) => hotkeys,
        Err(err) => {
            notify_error(hwnd, &format!("重新加载失败：{err}"));
            return;
        }
    };

    let old_hotkeys = with_runtime(|runtime| runtime.hotkeys).unwrap_or_else(fallback_hotkeys);
    unregister_hotkeys(hwnd);
    if let Err(err) = register_hotkeys(hwnd, new_hotkeys.0, new_hotkeys.1) {
        unregister_hotkeys(hwnd);
        let _ = register_hotkeys(hwnd, old_hotkeys.0, old_hotkeys.1);
        notify_error(hwnd, &format!("重新加载失败：{err}"));
        return;
    }

    if let Some(runtime) = RUNTIME.get() {
        if let Ok(mut runtime) = runtime.lock() {
            runtime.config = new_config;
            runtime.hotkeys = new_hotkeys;
        }
    }

    update_tray_tip(hwnd);

    if let Err(err) = autostart::sync(
        with_runtime(|runtime| runtime.config.autostart).unwrap_or(true),
        &win_util::current_exe(),
    ) {
        warnings.push(format!("同步开机自启失败：{err:#}"));
    }
    for warning in &warnings {
        logging::warn(warning);
    }
    show_balloon(hwnd, "ScreenShot FX", "配置已重新加载", false);
}

fn show_about(hwnd: HWND) {
    let (select, quick, backend, save_dir) = with_runtime(|runtime| {
        (
            runtime.hotkeys.0.display(),
            runtime.hotkeys.1.display(),
            runtime.config.backend.display_name().to_string(),
            runtime.config.save_dir.clone(),
        )
    })
    .unwrap_or_else(|| {
        (
            config::DEFAULT_SELECT_HOTKEY.to_string(),
            config::DEFAULT_QUICK_HOTKEY.to_string(),
            Backend::Auto.display_name().to_string(),
            config::default_save_dir().to_string_lossy().to_string(),
        )
    });

    let text = format!(
        "ScreenShot FX {}\n\n选区截图：{}\n快速截图：{}\n快捷键可在托盘菜单“快捷键设置…”中修改。\n\n保存目录：{}\n配置文件：{}\n日志：{}\n当前后端：{}",
        env!("CARGO_PKG_VERSION"),
        select,
        quick,
        save_dir,
        config::config_path().display(),
        logging::log_path().display(),
        backend
    );
    let title = win_util::wide("关于 ScreenShot FX");
    let body = win_util::wide(&text);
    unsafe {
        let _ = MessageBoxW(
            Some(hwnd),
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn cleanup(hwnd: HWND) {
    overlay::finish();
    unregister_hotkeys(hwnd);
    remove_tray_icon(hwnd);
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_TRAY_RETRY);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if hotkey_settings::is_open() {
                return LRESULT(0);
            }
            match wparam.0 as i32 {
                1 => start_selection(hwnd),
                2 => quick_capture(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            if hotkey_settings::is_open() {
                return LRESULT(0);
            }
            let event = lparam.0 as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                start_selection(hwnd);
            } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_APP_ALREADY_RUNNING => {
            show_balloon(hwnd, "ScreenShot FX", "程序已在运行", false);
            LRESULT(0)
        }
        overlay::WM_APP_OVERLAY_CLOSE => {
            overlay::finish();
            LRESULT(0)
        }
        overlay::WM_APP_OVERLAY_KEY => {
            overlay::handle_key(wparam.0 as u32, lparam.0 as u32);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_TRAY_RETRY && add_tray_icon(hwnd, false) {
                logging::info("tray icon restored");
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_TRAY_RETRY);
                }
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            if overlay::is_active() {
                overlay::finish();
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            cleanup(hwnd);
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_menu_command(hwnd, (wparam.0 & 0xFFFF) as u32);
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => {
            let taskbar = TASKBAR_CREATED.load(Ordering::SeqCst);
            if taskbar != 0 && msg == taskbar {
                if !add_tray_icon(hwnd, true) {
                    unsafe {
                        let _ = SetTimer(Some(hwnd), TIMER_TRAY_RETRY, 2000, None);
                    }
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}
