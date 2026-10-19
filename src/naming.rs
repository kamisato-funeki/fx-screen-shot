use std::fs::{self, File, OpenOptions};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn now_stamp() -> String {
    let st = unsafe { GetLocalTime() };
    format_stamp(&st)
}

pub fn format_stamp(st: &SYSTEMTIME) -> String {
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

pub fn filename_for(stamp: &str, attempt: u32) -> String {
    if attempt <= 1 {
        format!("Screenshot_{stamp}.png")
    } else {
        format!("Screenshot_{stamp}_{attempt}.png")
    }
}

pub fn save_png(dir: &Path, rgb: &[u8], width: u32, height: u32) -> Result<PathBuf> {
    if width == 0 || height == 0 {
        return Err(anyhow!("截图尺寸为空"));
    }
    let expected = width as usize * height as usize * 3;
    if rgb.len() != expected {
        return Err(anyhow!(
            "像素缓冲区长度不符：期望 {expected}，实际 {}",
            rgb.len()
        ));
    }
    fs::create_dir_all(dir).with_context(|| format!("创建保存目录失败：{}", dir.display()))?;

    let stamp = now_stamp();
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=999u32 {
        let path = dir.join(filename_for(&stamp, attempt));
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                last_error = Some(anyhow!("无法创建截图文件 {}：{err}", path.display()));
                break;
            }
        };

        match write_png(file, rgb, width, height) {
            Ok(()) => return Ok(path),
            Err(err) => {
                let _ = fs::remove_file(&path);
                return Err(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("为截图生成唯一文件名失败")))
}

pub fn write_png_file(path: &Path, rgb: &[u8], width: u32, height: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败：{}", parent.display()))?;
        }
    }
    let file = File::create(path).with_context(|| format!("创建文件失败：{}", path.display()))?;
    write_png(file, rgb, width, height)
}

fn write_png(file: File, rgb: &[u8], width: u32, height: u32) -> Result<()> {
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().context("写入 PNG 头失败")?;
    writer.write_image_data(rgb).context("写入 PNG 像素失败")?;
    writer.finish().context("完成 PNG 文件失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> SYSTEMTIME {
        SYSTEMTIME {
            wYear: 2026,
            wMonth: 10,
            wDay: 19,
            wHour: 12,
            wMinute: 50,
            wSecond: 14,
            ..Default::default()
        }
    }

    #[test]
    fn formats_names() {
        assert_eq!(format_stamp(&stamp()), "2026-10-19_12-50-14");
        assert_eq!(
            filename_for("2026-10-19_12-50-14", 1),
            "Screenshot_2026-10-19_12-50-14.png"
        );
        assert_eq!(
            filename_for("2026-10-19_12-50-14", 2),
            "Screenshot_2026-10-19_12-50-14_2.png"
        );
    }

    #[test]
    fn avoids_overwriting() {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-tmp");
        fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!(
            "fxshot-naming-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let rgb = vec![255u8; 2 * 2 * 3];
        let first = save_png(&dir, &rgb, 2, 2).unwrap();
        let second = save_png(&dir, &rgb, 2, 2).unwrap();
        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
