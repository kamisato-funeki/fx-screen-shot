#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod capture;
mod config;
mod diagnostics;
mod geometry;
mod hotkey_settings;
mod logging;
mod monitor;
mod naming;
mod overlay;
mod pin;
mod win_util;

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
};

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let com_initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
    logging::init();

    let mut capture_once: Option<Option<PathBuf>> = None;
    let mut backend_override = None;
    let mut monitor_selector: Option<String> = None;
    let mut set_autostart = None;
    let mut no_autostart = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--capture-once" => {
                let mut path = None;
                if index + 1 < args.len() && !args[index + 1].starts_with("--") {
                    path = Some(PathBuf::from(&args[index + 1]));
                    index += 1;
                }
                capture_once = Some(path);
            }
            "--backend" => {
                if index + 1 >= args.len() {
                    show_error("--backend 需要一个参数：auto、gdi 或 wgc");
                    finish(com_initialized, 2);
                }
                match config::backend_from_arg(&args[index + 1]) {
                    Ok(backend) => backend_override = Some(backend),
                    Err(err) => {
                        show_error(&err);
                        finish(com_initialized, 2);
                    }
                }
                index += 1;
            }
            "--monitor" => {
                if index + 1 >= args.len() || args[index + 1].starts_with("--") {
                    show_error(
                        "--monitor 需要一个参数：显示器序号（从 1 开始）或设备名，例如 \\\\.\\DISPLAY1",
                    );
                    finish(com_initialized, 2);
                }
                monitor_selector = Some(args[index + 1].clone());
                index += 1;
            }
            "--set-autostart" => {
                if index + 1 >= args.len() {
                    show_error("--set-autostart 需要一个参数：on 或 off");
                    finish(com_initialized, 2);
                }
                set_autostart = match args[index + 1].to_ascii_lowercase().as_str() {
                    "on" | "true" | "1" => Some(true),
                    "off" | "false" | "0" => Some(false),
                    _ => {
                        show_error("--set-autostart 只接受 on 或 off");
                        finish(com_initialized, 2);
                    }
                };
                index += 1;
            }
            "--no-autostart" => no_autostart = true,
            "--help" | "-h" | "/?" => {
                show_help();
                finish(com_initialized, 0);
            }
            other => {
                logging::warn(&format!("ignoring unknown argument: {other}"));
            }
        }
        index += 1;
    }

    let code = if let Some(path) = capture_once {
        match diagnostics::capture_once(path, backend_override, monitor_selector) {
            Ok(saved) => {
                logging::info(&format!("capture-once finished: {}", saved.display()));
                0
            }
            Err(err) => {
                logging::error(&format!("capture-once failed: {err:#}"));
                1
            }
        }
    } else if let Some(enabled) = set_autostart {
        match diagnostics::set_autostart(enabled) {
            Ok(()) => 0,
            Err(err) => {
                logging::error(&format!("set-autostart failed: {err:#}"));
                1
            }
        }
    } else {
        app::run(no_autostart)
    };

    finish(com_initialized, code);
}

fn finish(com_initialized: bool, code: i32) -> ! {
    if com_initialized {
        unsafe {
            RoUninitialize();
        }
    }
    std::process::exit(code);
}

fn show_error(message: &str) {
    logging::error(message);
    let body = win_util::wide(message);
    let title = win_util::wide("ScreenShot FX");
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn show_help() {
    let text = "ScreenShot FX\n\n\
        默认快捷键（可在托盘菜单“快捷键设置…”中修改）：\n\
        Shift+F1：选区截图（悬停吸附窗口，拖拽选区，锁定后可拖动调整）\n\
        Shift 切换 RGB/HEX，C 复制色值，方向键 1px 微调，Ctrl+C 复制选区，Enter 保存\n\
        工具条“钉图”把当前选区钉在桌面；滚轮缩放，Ctrl+滚轮调透明度，拖边框缩放，拖内部移动，X/Esc 关闭\n\
        Shift+F2：快速截图（鼠标所在显示器，静默保存）\n\n\
        命令行：\n\
        --capture-once [png] [--backend auto|gdi|wgc] [--monitor <序号|设备名>]\n\
        --monitor 仅配合 --capture-once：序号从 1 开始，或设备名如 \\\\.\\DISPLAY1\n\
        --set-autostart on|off\n\
        --no-autostart\n\n\
        配置：%APPDATA%\\ScreenShotFX\\config.toml\n\
        日志：%LOCALAPPDATA%\\ScreenShotFX\\fxshot.log";
    let body = win_util::wide(text);
    let title = win_util::wide("ScreenShot FX 帮助");
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
