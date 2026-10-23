use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::win_util;

pub const APP_DIR_NAME: &str = "ScreenShotFX";
pub const DEFAULT_SELECT_HOTKEY: &str = "Shift+F1";
pub const DEFAULT_QUICK_HOTKEY: &str = "Shift+F2";

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
pub const MOD_NOREPEAT: u32 = 0x4000;
pub const KNOWN_MODIFIER_MASK: u32 = MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Auto,
    Gdi,
    Wgc,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Gdi => "gdi",
            Backend::Wgc => "wgc",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Backend::Auto => "自动（仅 HDR 用 WGC）",
            Backend::Gdi => "GDI（静默）",
            Backend::Wgc => "WGC（兼容模式，有黄框）",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub save_dir: String,
    pub select_hotkey: String,
    pub quick_hotkey: String,
    pub backend: Backend,
    pub clipboard: bool,
    pub snap_distance: i32,
    pub hdr_exposure: f32,
    pub autostart: bool,
    pub autostart_initialized: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            save_dir: default_save_dir().to_string_lossy().to_string(),
            select_hotkey: DEFAULT_SELECT_HOTKEY.to_string(),
            quick_hotkey: DEFAULT_QUICK_HOTKEY.to_string(),
            backend: Backend::Auto,
            clipboard: true,
            snap_distance: 10,
            hdr_exposure: 1.0,
            autostart: true,
            autostart_initialized: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: AppConfig,
    pub warnings: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join(APP_DIR_NAME);
        }
    }
    if let Some(dir) = win_util::roaming_appdata_dir() {
        return dir.join(APP_DIR_NAME);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn default_save_dir() -> PathBuf {
    if let Some(pictures) = win_util::pictures_dir() {
        return pictures.join("fx");
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile).join("Pictures").join("fx");
    }
    PathBuf::from("Pictures").join("fx")
}

pub fn load_or_create() -> LoadedConfig {
    let path = config_path();
    if !path.exists() {
        let config = AppConfig::default();
        if let Err(err) = write_default_config(&path, &config) {
            return LoadedConfig {
                config,
                warnings: vec![format!("无法创建配置文件 {}：{err}", path.display())],
            };
        }
        return LoadedConfig {
            config,
            warnings: Vec::new(),
        };
    }

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            return LoadedConfig {
                config: AppConfig::default(),
                warnings: vec![format!("无法读取配置文件 {}：{err}", path.display())],
            };
        }
    };

    let mut warnings = Vec::new();
    let mut config = match toml::from_str::<AppConfig>(&text) {
        Ok(config) => config,
        Err(err) => {
            warnings.push(format!("配置文件解析失败，已使用默认值：{err}"));
            AppConfig::default()
        }
    };

    normalize(&mut config, &mut warnings);
    LoadedConfig { config, warnings }
}

fn normalize(config: &mut AppConfig, warnings: &mut Vec<String>) {
    if config.save_dir.trim().is_empty() {
        config.save_dir = default_save_dir().to_string_lossy().to_string();
    }
    config.save_dir = expand_env_vars(&config.save_dir);

    if Hotkey::parse(&config.select_hotkey).is_err() {
        warnings.push(format!(
            "选区快捷键 {:?} 无效，已回退为 {}",
            config.select_hotkey, DEFAULT_SELECT_HOTKEY
        ));
        config.select_hotkey = DEFAULT_SELECT_HOTKEY.to_string();
    }
    if Hotkey::parse(&config.quick_hotkey).is_err() {
        warnings.push(format!(
            "快速快捷键 {:?} 无效，已回退为 {}",
            config.quick_hotkey, DEFAULT_QUICK_HOTKEY
        ));
        config.quick_hotkey = DEFAULT_QUICK_HOTKEY.to_string();
    }
    if Hotkey::parse(&config.select_hotkey).ok() == Hotkey::parse(&config.quick_hotkey).ok() {
        warnings.push("选区与快速快捷键相同，快速截图已回退为默认值".to_string());
        config.quick_hotkey = DEFAULT_QUICK_HOTKEY.to_string();
    }

    config.snap_distance = config.snap_distance.clamp(0, 50);
    if !config.hdr_exposure.is_finite() {
        config.hdr_exposure = 1.0;
    }
    config.hdr_exposure = config.hdr_exposure.clamp(0.25, 4.0);
}

pub fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '%') {
                let end = i + 1 + end;
                let name: String = chars[i + 1..end].iter().collect();
                if !name.is_empty() && !name.contains('%') {
                    if let Ok(value) = std::env::var(&name) {
                        result.push_str(&value);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_default_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败：{}", parent.display()))?;
    }
    let template = format!(
        r#"# ScreenShot FX 配置文件
# 修改后可在托盘菜单选择“重新加载配置”，无需重启。
# 保存目录，支持 %USERPROFILE% 等环境变量。
save_dir = "{save_dir}"

# 全局快捷键。必须至少包含一个修饰键（Shift/Ctrl/Alt/Win）。
# 也可以在托盘菜单“快捷键设置…”中直接录制修改，保存后立即生效。
select_hotkey = "{select_hotkey}"
quick_hotkey = "{quick_hotkey}"

# 截图后端：
#   auto = SDR/广色域(WCG/ACM) 用 GDI（静默），仅 HDR 用 WGC
#   gdi  = 始终使用 GDI
#   wgc  = 兼容模式，始终使用 WGC（屏幕边缘可能出现黄色指示框）
backend = "{backend}"

# 截图后是否同时复制到剪贴板。
clipboard = {clipboard}

# 窗口/屏幕边缘吸附距离（96 DPI 下的像素，会按显示器缩放）。
snap_distance = {snap_distance}

# WGC HDR 色调映射曝光，范围 0.25 - 4.0。
hdr_exposure = {hdr_exposure}

# 开机自启。首次运行会自动设为 true。
autostart = {autostart}

# 内部标记：用户是否已经完成首次自启配置（一般不需要手动修改）。
autostart_initialized = {autostart_initialized}
"#,
        save_dir = toml_escape(&config.save_dir),
        select_hotkey = toml_escape(&config.select_hotkey),
        quick_hotkey = toml_escape(&config.quick_hotkey),
        backend = config.backend.as_str(),
        clipboard = config.clipboard,
        snap_distance = config.snap_distance,
        hdr_exposure = config.hdr_exposure,
        autostart = config.autostart,
        autostart_initialized = config.autostart_initialized,
    );

    // Validate before writing so a malformed template cannot brick startup.
    let _: AppConfig = toml::from_str(&template).context("默认配置模板无效")?;
    fs::write(path, template).with_context(|| format!("写入配置失败：{}", path.display()))?;
    Ok(())
}

/// Updates only the given keys, preserving comments and user formatting.
pub fn update_runtime_flags(
    backend: Option<Backend>,
    autostart: Option<bool>,
    autostart_initialized: Option<bool>,
) -> Result<()> {
    let path = config_path();
    let text =
        fs::read_to_string(&path).with_context(|| format!("读取配置失败：{}", path.display()))?;
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .context("配置文件不是有效的 TOML")?;

    if let Some(backend) = backend {
        document["backend"] = toml_edit::value(backend.as_str());
    }
    if let Some(value) = autostart {
        document["autostart"] = toml_edit::value(value);
    }
    if let Some(value) = autostart_initialized {
        document["autostart_initialized"] = toml_edit::value(value);
    }

    fs::write(&path, document.to_string())
        .with_context(|| format!("写入配置失败：{}", path.display()))?;
    Ok(())
}

/// Updates only the two hotkey values, preserving comments and other keys.
pub fn update_hotkeys(select: &str, quick: &str) -> Result<()> {
    let path = config_path();
    let text =
        fs::read_to_string(&path).with_context(|| format!("读取配置失败：{}", path.display()))?;
    let updated = rewrite_hotkeys(&text, select, quick)?;
    fs::write(&path, updated).with_context(|| format!("写入配置失败：{}", path.display()))?;
    Ok(())
}

fn rewrite_hotkeys(text: &str, select: &str, quick: &str) -> Result<String> {
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .context("配置文件不是有效的 TOML")?;
    set_hotkey_value(&mut document, "select_hotkey", select);
    set_hotkey_value(&mut document, "quick_hotkey", quick);
    Ok(document.to_string())
}

fn set_hotkey_value(document: &mut toml_edit::DocumentMut, key: &str, text: &str) {
    // Keep the original inline comment/whitespace decor when the key exists.
    let decor = document
        .get(key)
        .and_then(|item| item.as_value())
        .map(|value| value.decor().clone());
    let mut item = toml_edit::value(text);
    if let (Some(decor), Some(value)) = (decor, item.as_value_mut()) {
        *value.decor_mut() = decor;
    }
    document[key] = item;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

impl Hotkey {
    pub fn parse(text: &str) -> Result<Self, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("快捷键为空".to_string());
        }

        let mut modifiers = 0u32;
        let mut key: Option<&str> = None;
        for raw in trimmed.split('+') {
            let token = raw.trim();
            if token.is_empty() {
                return Err(format!("快捷键 {text:?} 含有空片段"));
            }
            let lower = token.to_ascii_lowercase();
            let modifier = match lower.as_str() {
                "shift" => Some(MOD_SHIFT),
                "ctrl" | "control" => Some(MOD_CONTROL),
                "alt" => Some(MOD_ALT),
                "win" | "windows" | "super" => Some(MOD_WIN),
                _ => None,
            };
            if let Some(modifier) = modifier {
                modifiers |= modifier;
            } else if key.is_some() {
                return Err(format!("快捷键 {text:?} 只能包含一个主键"));
            } else {
                key = Some(token);
            }
        }

        let key = key.ok_or_else(|| format!("快捷键 {text:?} 缺少主键"))?;
        if modifiers == 0 {
            return Err(format!("快捷键 {text:?} 必须包含修饰键"));
        }
        let vk = virtual_key_for_name(key).ok_or_else(|| format!("不支持的按键 {key:?}"))?;
        Ok(Self { modifiers, vk })
    }

    pub fn from_parts(modifiers: u32, vk: u32) -> Result<Self, String> {
        if modifiers & KNOWN_MODIFIER_MASK == 0 {
            return Err("快捷键必须包含至少一个修饰键".to_string());
        }
        if modifiers & !KNOWN_MODIFIER_MASK != 0 {
            return Err(format!("快捷键包含未知修饰位：0x{modifiers:04X}"));
        }
        if is_modifier_vk(vk) {
            return Err(format!("按键 {} 只能作为修饰键", virtual_key_name(vk)));
        }
        if !is_supported_vk(vk) {
            return Err(format!("不支持的按键 VK_{vk:02X}"));
        }
        Ok(Self { modifiers, vk })
    }

    pub fn register_modifiers(self) -> u32 {
        self.modifiers | MOD_NOREPEAT
    }

    pub fn display(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers & MOD_CONTROL != 0 {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers & MOD_ALT != 0 {
            parts.push("Alt".to_string());
        }
        if self.modifiers & MOD_SHIFT != 0 {
            parts.push("Shift".to_string());
        }
        if self.modifiers & MOD_WIN != 0 {
            parts.push("Win".to_string());
        }
        parts.push(virtual_key_name(self.vk));
        parts.join("+")
    }
}

fn virtual_key_for_name(name: &str) -> Option<u32> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.len() == 1 {
        let ch = upper.chars().next()?;
        if ch.is_ascii_uppercase() {
            return Some(0x41 + (ch as u32 - 'A' as u32));
        }
        if ch.is_ascii_digit() {
            return Some(0x30 + (ch as u32 - '0' as u32));
        }
        return None;
    }
    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(number) = rest.parse::<u32>() {
            if (1..=24).contains(&number) {
                return Some(0x70 + number - 1);
            }
        }
    }
    let named = match upper.as_str() {
        "SPACE" => 0x20,
        "TAB" => 0x09,
        "ENTER" | "RETURN" => 0x0D,
        "ESC" | "ESCAPE" => 0x1B,
        "BACKSPACE" => 0x08,
        "INSERT" | "INS" => 0x2D,
        "DELETE" | "DEL" => 0x2E,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "PRINTSCREEN" | "PRTSC" => 0x2C,
        "PAUSE" => 0x13,
        _ => return None,
    };
    Some(named)
}

fn virtual_key_name(vk: u32) -> String {
    match vk {
        0x41..=0x5A => char::from_u32(vk).unwrap_or('?').to_string(),
        0x30..=0x39 => char::from_u32(vk).unwrap_or('?').to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x20 => "Space".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x1B => "Esc".to_string(),
        0x08 => "Backspace".to_string(),
        0x2D => "Insert".to_string(),
        0x2E => "Delete".to_string(),
        0x24 => "Home".to_string(),
        0x23 => "End".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x2C => "PrintScreen".to_string(),
        0x13 => "Pause".to_string(),
        _ => format!("VK_{vk:02X}"),
    }
}

fn is_supported_vk(vk: u32) -> bool {
    matches!(
        vk,
        0x41..=0x5A
            | 0x30..=0x39
            | 0x70..=0x87
            | 0x20
            | 0x09
            | 0x0D
            | 0x1B
            | 0x08
            | 0x2D
            | 0x2E
            | 0x24
            | 0x23
            | 0x21
            | 0x22
            | 0x2C
            | 0x13
    )
}

fn is_modifier_vk(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

pub fn validate_hotkeys(select_text: &str, quick_text: &str) -> Result<(Hotkey, Hotkey), String> {
    let select = Hotkey::parse(select_text)?;
    let quick = Hotkey::parse(quick_text)?;
    if select == quick {
        return Err("选区与快速快捷键不能相同".to_string());
    }
    Ok((select, quick))
}

pub fn parse_hotkeys(config: &AppConfig) -> Result<(Hotkey, Hotkey), String> {
    validate_hotkeys(&config.select_hotkey, &config.quick_hotkey)
}

pub fn config_exists() -> bool {
    config_path().exists()
}

pub fn backend_from_arg(value: &str) -> Result<Backend, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(Backend::Auto),
        "gdi" => Ok(Backend::Gdi),
        "wgc" => Ok(Backend::Wgc),
        other => Err(format!("未知 backend：{other}，可选 auto/gdi/wgc")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkeys() {
        let select = Hotkey::parse("Shift+F1").unwrap();
        assert_eq!(select.modifiers, MOD_SHIFT);
        assert_eq!(select.vk, 0x70);
        assert_eq!(select.display(), "Shift+F1");

        let quick = Hotkey::parse("shift+f2").unwrap();
        assert_eq!(quick.display(), "Shift+F2");
    }

    #[test]
    fn parses_combined_modifiers() {
        let hotkey = Hotkey::parse("Ctrl+Alt+Shift+Win+X").unwrap();
        assert_eq!(
            hotkey.modifiers,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_WIN
        );
        assert_eq!(hotkey.vk, 0x58);
        assert_eq!(hotkey.display(), "Ctrl+Alt+Shift+Win+X");
    }

    #[test]
    fn rejects_invalid_hotkeys() {
        assert!(Hotkey::parse("F1").is_err());
        assert!(Hotkey::parse("Shift+").is_err());
        assert!(Hotkey::parse("Shift+Unknown").is_err());
        assert!(Hotkey::parse("Shift+A+B").is_err());
        assert!(Hotkey::parse("Shift+F25").is_err());
    }

    #[test]
    fn builds_hotkeys_from_recorded_parts() {
        let hotkey = Hotkey::from_parts(MOD_SHIFT | MOD_CONTROL, 0x70).unwrap();
        assert_eq!(hotkey.display(), "Ctrl+Shift+F1");

        assert!(Hotkey::from_parts(0, 0x70).is_err());
        assert!(Hotkey::from_parts(MOD_SHIFT, 0x10).is_err());
        assert!(Hotkey::from_parts(MOD_SHIFT, 0x25).is_err());
        assert!(Hotkey::from_parts(MOD_SHIFT | 0x8000, 0x70).is_err());
    }

    #[test]
    fn validates_and_rejects_equal_custom_hotkeys() {
        let pair = validate_hotkeys("Ctrl+Alt+F1", "Ctrl+Shift+F2").unwrap();
        assert_eq!(pair.0.display(), "Ctrl+Alt+F1");
        assert_eq!(pair.1.display(), "Ctrl+Shift+F2");
        assert!(validate_hotkeys("Shift+F1", "Shift+F1").is_err());
        assert!(validate_hotkeys("Shift+F1", "F2").is_err());
    }

    #[test]
    fn rewrites_hotkeys_preserving_comments_and_other_keys() {
        let text = r#"# top comment
select_hotkey = "Shift+F1" # select inline
quick_hotkey = "Shift+F2"
backend = "auto"

# keep me
snap_distance = 10
"#;
        let updated = rewrite_hotkeys(text, "Ctrl+Alt+F1", "Ctrl+Shift+F2").unwrap();
        assert!(updated.contains("# top comment"));
        assert!(updated.contains("# select inline"));
        assert!(updated.contains("# keep me"));
        assert!(updated.contains("select_hotkey = \"Ctrl+Alt+F1\""));
        assert!(updated.contains("quick_hotkey = \"Ctrl+Shift+F2\""));
        assert!(updated.contains("backend = \"auto\""));
        assert!(updated.contains("snap_distance = 10"));
    }

    #[test]
    fn expands_environment_variables() {
        std::env::set_var("FXSHOT_TEST_DIR", r"C:\Temp\fx");
        assert_eq!(
            expand_env_vars(r"%FXSHOT_TEST_DIR%\shots"),
            r"C:\Temp\fx\shots"
        );
        assert_eq!(
            expand_env_vars("%NOT_DEFINED_XYZ%\\a"),
            "%NOT_DEFINED_XYZ%\\a"
        );
    }

    #[test]
    fn clamps_values() {
        let mut config = AppConfig {
            snap_distance: 999,
            hdr_exposure: 99.0,
            ..AppConfig::default()
        };
        let mut warnings = Vec::new();
        normalize(&mut config, &mut warnings);
        assert_eq!(config.snap_distance, 50);
        assert_eq!(config.hdr_exposure, 4.0);
    }

    #[test]
    fn parses_toml() {
        let text = r#"
save_dir = 'C:\Users\Lenovo\Pictures\fx'
select_hotkey = "Shift+F1"
quick_hotkey = "Shift+F2"
backend = "auto"
clipboard = true
snap_distance = 10
hdr_exposure = 1.0
autostart = true
autostart_initialized = false
"#;
        let config: AppConfig = toml::from_str(text).unwrap();
        assert_eq!(config.save_dir, r"C:\Users\Lenovo\Pictures\fx");
        assert!(config.clipboard);
        assert_eq!(config.backend, Backend::Auto);
    }
}
