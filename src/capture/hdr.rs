use std::mem::size_of;

use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};

/// Windows composes a display in one of three modes. Only true HDR is
/// scene-referred and needs tone mapping; wide-color (ACM/WCG) is still SDR
/// content and must keep display-referred colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Sdr,
    Wcg,
    Hdr,
}

impl ColorMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sdr => "sdr",
            Self::Wcg => "wcg",
            Self::Hdr => "hdr",
        }
    }

    pub fn needs_tone_map(self) -> bool {
        matches!(self, Self::Hdr)
    }

    fn from_active_color_mode(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Sdr),
            1 => Some(Self::Wcg),
            2 => Some(Self::Hdr),
            _ => None,
        }
    }
}

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` from the Windows 11 24H2 SDK. The
/// `windows` crate does not expose it yet, so mirror the SDK layout; only
/// `active_color_mode` is read.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    _advanced_color_bits: u32,
    _color_encoding: u32,
    _bits_per_color_channel: u32,
    active_color_mode: u32,
}

const TYPE_GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE =
    DISPLAYCONFIG_DEVICE_INFO_TYPE(15);

/// Returns the active composition mode of the monitor whose GDI device name is
/// `device_name` (for example `\\.\DISPLAY1`).
pub fn color_mode_for_device(device_name: &str) -> Option<ColorMode> {
    let paths = active_paths()?;
    for path in &paths {
        let Some(source_name) = source_device_name(path) else {
            continue;
        };
        if source_name.eq_ignore_ascii_case(device_name) {
            return Some(read_color_mode(path));
        }
    }
    None
}

fn active_paths() -> Option<Vec<DISPLAYCONFIG_PATH_INFO>> {
    unsafe {
        let mut path_count: u32 = 0;
        let mut mode_count: u32 = 0;
        let err =
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count);
        if err.0 != 0 || path_count == 0 {
            return None;
        }

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let err = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        );
        if err.0 != 0 {
            return None;
        }
        paths.truncate(path_count as usize);
        Some(paths)
    }
}

fn source_device_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
        return None;
    }
    Some(tensor_u16_to_string(&source.viewGdiDeviceName))
}

fn read_color_mode(path: &DISPLAYCONFIG_PATH_INFO) -> ColorMode {
    match active_color_mode(path) {
        Some(mode) => mode,
        None => legacy_color_mode(path),
    }
}

/// Queries the 24H2+ API. Returns None when the OS or the driver does not
/// support the newer query, or when the result is an unknown mode.
fn active_color_mode(path: &DISPLAYCONFIG_PATH_INFO) -> Option<ColorMode> {
    let mut info = AdvancedColorInfo2 {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: TYPE_GET_ADVANCED_COLOR_INFO_2,
            size: size_of::<AdvancedColorInfo2>() as u32,
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut info.header) } != 0 {
        return None;
    }
    ColorMode::from_active_color_mode(info.active_color_mode)
}

/// Pre-24H2 fallback: advanced color enabled is treated as HDR because WCG
/// cannot be distinguished from HDR there.
fn legacy_color_mode(path: &DISPLAYCONFIG_PATH_INFO) -> ColorMode {
    let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            size: size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut color.header) } != 0 {
        return ColorMode::Sdr;
    }
    // Bit 0: advanced color supported; bit 1: advanced color enabled.
    let value = unsafe { color.Anonymous.value };
    if value & 0b10 != 0 {
        ColorMode::Hdr
    } else {
        ColorMode::Sdr
    }
}

fn tensor_u16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Converts an IEEE 754 binary16 value to f32.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let mant = (h & 0x03ff) as f32;
    let value = match exp {
        0 => mant * 2f32.powi(-24),
        0x1f => {
            if mant == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + mant / 1024.0) * 2f32.powi(exp - 15),
    };
    sign * value
}

/// Maps a linear scRGB component to an sRGB-encoded 8-bit value.
/// The curve keeps SDR white near white while rolling off HDR highlights.
pub fn tone_map_component(value: f32, exposure: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let x = (value * exposure).max(0.0);
    let mapped = 1.0 - (-3.0 * x).exp();
    let mapped = mapped.clamp(0.0, 1.0);
    let srgb = if mapped <= 0.003_130_8 {
        12.92 * mapped
    } else {
        1.055 * mapped.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn tone_map_rgb(r: f32, g: f32, b: f32, exposure: f32) -> [u8; 3] {
    [
        tone_map_component(r, exposure),
        tone_map_component(g, exposure),
        tone_map_component(b, exposure),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_conversion() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0x4000), 2.0);
        assert_eq!(half_to_f32(0xbc00), -1.0);
    }

    #[test]
    fn tone_map_is_monotonic() {
        let dark = tone_map_component(0.0, 1.0);
        let mid = tone_map_component(0.5, 1.0);
        let white = tone_map_component(1.0, 1.0);
        let bright = tone_map_component(4.0, 1.0);
        assert_eq!(dark, 0);
        assert!(dark < mid);
        assert!(mid < white);
        assert!(white < bright);
        assert_eq!(bright, 255);
    }

    #[test]
    fn exposure_brightens() {
        let a = tone_map_component(0.2, 1.0);
        let b = tone_map_component(0.2, 2.0);
        assert!(a < b);
    }

    #[test]
    fn maps_active_color_mode() {
        assert_eq!(ColorMode::from_active_color_mode(0), Some(ColorMode::Sdr));
        assert_eq!(ColorMode::from_active_color_mode(1), Some(ColorMode::Wcg));
        assert_eq!(ColorMode::from_active_color_mode(2), Some(ColorMode::Hdr));
        assert_eq!(ColorMode::from_active_color_mode(3), None);
    }

    #[test]
    fn only_hdr_needs_tone_map() {
        assert!(!ColorMode::Sdr.needs_tone_map());
        assert!(!ColorMode::Wcg.needs_tone_map());
        assert!(ColorMode::Hdr.needs_tone_map());
        assert_eq!(ColorMode::Sdr.as_str(), "sdr");
        assert_eq!(ColorMode::Wcg.as_str(), "wcg");
        assert_eq!(ColorMode::Hdr.as_str(), "hdr");
    }

    #[test]
    fn advanced_color_info_2_matches_sdk_layout() {
        assert_eq!(size_of::<AdvancedColorInfo2>(), 36);
    }
}
