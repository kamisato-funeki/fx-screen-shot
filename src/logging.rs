use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use windows::Win32::System::SystemInformation::GetLocalTime;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

static LOGGER: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn data_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if !local.is_empty() {
            return PathBuf::from(local).join("ScreenShotFX");
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        if !profile.is_empty() {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Local")
                .join("ScreenShotFX");
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn log_path() -> PathBuf {
    data_dir().join("fxshot.log")
}

pub fn init() {
    let path = log_path();
    let _ = fs::create_dir_all(data_dir());

    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let backup = path.with_extension("log.1");
            let _ = fs::remove_file(&backup);
            let _ = fs::rename(&path, &backup);
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();

    let _ = LOGGER.set(Mutex::new(file));
    info("========================================");
    info(&format!("fxshot {} starting", env!("CARGO_PKG_VERSION")));
    info(&format!("log path: {}", path.display()));
}

pub fn info(message: &str) {
    write_line("INFO", message);
}

pub fn warn(message: &str) {
    write_line("WARN", message);
}

pub fn error(message: &str) {
    write_line("ERROR", message);
}

fn write_line(level: &str, message: &str) {
    let Some(lock) = LOGGER.get() else {
        return;
    };
    let Ok(mut guard) = lock.lock() else {
        return;
    };
    let Some(file) = guard.as_mut() else {
        return;
    };
    let line = format!("{} [{}] {}\r\n", timestamp(), level, message);
    let _ = file.write_all(line.as_bytes());
    let _ = file.flush();
}

fn timestamp() -> String {
    let st = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}
