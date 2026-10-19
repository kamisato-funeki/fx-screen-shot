use std::path::Path;

use anyhow::{anyhow, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_CREATE_SUB_KEY, KEY_QUERY_VALUE, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open_run_key(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<RegKey> {
    let subkey = crate::win_util::wide(RUN_SUBKEY);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            access,
            &mut key,
        )
    };
    if status.0 != 0 {
        return Err(win32_error("打开启动项注册表键", status));
    }
    Ok(RegKey(key))
}

fn create_run_key() -> Result<RegKey> {
    let subkey = crate::win_util::wide(RUN_SUBKEY);
    let mut key = HKEY::default();

    // Prefer opening the existing key. Some restricted tokens deny
    // RegCreateKeyExW even when the key is already writable.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status.0 == 0 {
        return Ok(RegKey(key));
    }

    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE | KEY_CREATE_SUB_KEY,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        return Err(win32_error("创建启动项注册表键", status));
    }
    Ok(RegKey(key))
}

pub fn command_line(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

pub fn current_value() -> Option<String> {
    let key = open_run_key(KEY_QUERY_VALUE).ok()?;
    let name = w!("ScreenShotFX");
    let mut value_type = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
    let mut size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name,
            None,
            Some(&mut value_type),
            None,
            Some(&mut size),
        )
    };
    if status.0 != 0 || size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name,
            None,
            Some(&mut value_type),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status.0 != 0 {
        return None;
    }

    let units: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string(),
    )
}

pub fn set_enabled(enabled: bool, exe: &Path) -> Result<()> {
    if enabled {
        let key = create_run_key()?;
        let data = command_line(exe);
        let mut wide: Vec<u8> = data.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        wide.extend_from_slice(&[0, 0]);
        let name = w!("ScreenShotFX");
        let status = unsafe { RegSetValueExW(key.0, name, Some(0), REG_SZ, Some(&wide)) };
        if status.0 != 0 {
            return Err(win32_error("写入开机自启", status));
        }
    } else {
        let key = match open_run_key(KEY_SET_VALUE) {
            Ok(key) => key,
            Err(_) => return Ok(()),
        };
        let name = w!("ScreenShotFX");
        let status = unsafe { RegDeleteValueW(key.0, name) };
        if status.0 != 0 && status != ERROR_FILE_NOT_FOUND {
            return Err(win32_error("删除开机自启", status));
        }
    }
    Ok(())
}

/// Reconciles the registry with the desired state and the current exe path.
pub fn sync(enabled: bool, exe: &Path) -> Result<()> {
    if enabled {
        let expected = command_line(exe);
        match current_value() {
            Some(value) if value.eq_ignore_ascii_case(&expected) => Ok(()),
            _ => set_enabled(true, exe),
        }
    } else {
        set_enabled(false, exe)
    }
}

fn win32_error(operation: &str, status: WIN32_ERROR) -> anyhow::Error {
    anyhow!("{operation}失败（Windows 错误 {}）", status.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_is_quoted() {
        let line = command_line(Path::new(r"C:\Program Files\fxshot\fxshot.exe"));
        assert_eq!(line, r#""C:\Program Files\fxshot\fxshot.exe" --autostart"#);
    }
}
