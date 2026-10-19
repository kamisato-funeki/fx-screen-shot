use std::mem::size_of;

use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};

/// Returns true when Windows advanced color (HDR/WCG) is enabled for the
/// monitor whose GDI device name is `device_name` (for example `\\.\DISPLAY1`).
pub fn is_advanced_color_enabled(device_name: &str) -> bool {
    unsafe { is_advanced_color_enabled_impl(device_name) }
}

unsafe fn is_advanced_color_enabled_impl(device_name: &str) -> bool {
    let mut path_count: u32 = 0;
    let mut mode_count: u32 = 0;
    let err = GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count);
    if err.0 != 0 || path_count == 0 {
        return false;
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
        return false;
    }
    paths.truncate(path_count as usize);

    for path in &paths {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
            continue;
        }
        let source_name = tensor_u16_to_string(&source.viewGdiDeviceName);
        if !source_name.eq_ignore_ascii_case(device_name) {
            continue;
        }

        let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        if DisplayConfigGetDeviceInfo(&mut color.header) != 0 {
            return false;
        }
        // Bit 0: advanced color supported; bit 1: advanced color enabled.
        let value = color.Anonymous.value;
        return value & 0b10 != 0;
    }
    false
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
}
