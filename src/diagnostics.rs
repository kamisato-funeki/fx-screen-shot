use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::capture;
use crate::config::{self, AppConfig, Backend};
use crate::geometry::Point;
use crate::logging;
use crate::monitor;
use crate::naming;
use crate::win_util;

pub fn capture_once(target: Option<PathBuf>, backend: Option<Backend>) -> Result<PathBuf> {
    let config = if config::config_exists() {
        config::load_or_create().config
    } else {
        AppConfig::default()
    };

    let monitors = monitor::enumerate();
    if monitors.is_empty() {
        return Err(anyhow!("没有检测到任何显示器"));
    }
    let index = monitor::select_monitor(Point::new(0, 0), &monitors)
        .ok_or_else(|| anyhow!("无法选择显示器"))?;
    let monitor = &monitors[index];
    let backend = backend.unwrap_or(config.backend);
    let (image, used) =
        capture::capture_monitor_with_fallback(monitor, backend, config.hdr_exposure)?;

    let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    for pixel in image.bgra.chunks_exact(4) {
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }

    let path = target.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("fxshot-capture-{}.png", std::process::id()))
    });
    naming::write_png_file(&path, &rgb, image.width, image.height)?;
    logging::info(&format!(
        "capture-once wrote {} ({}x{}, backend={})",
        path.display(),
        image.width,
        image.height,
        used.as_str()
    ));
    Ok(path)
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let _ = config::load_or_create();
    config::update_runtime_flags(None, Some(enabled), Some(true))?;
    let exe = win_util::current_exe();
    crate::autostart::sync(enabled, &exe)?;
    logging::info(&format!(
        "autostart set to {} for {}",
        enabled,
        exe.display()
    ));
    Ok(())
}
