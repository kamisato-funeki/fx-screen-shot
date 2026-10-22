use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::capture;
use crate::config::{self, AppConfig, Backend};
use crate::geometry::Point;
use crate::logging;
use crate::monitor;
use crate::naming;
use crate::win_util;

pub fn capture_once(
    target: Option<PathBuf>,
    backend: Option<Backend>,
    monitor_selector: Option<String>,
) -> Result<PathBuf> {
    let config = if config::config_exists() {
        config::load_or_create().config
    } else {
        AppConfig::default()
    };

    let monitors = monitor::enumerate();
    if monitors.is_empty() {
        return Err(anyhow!("没有检测到任何显示器"));
    }
    let index = match monitor_selector.as_deref() {
        Some(selector) => monitor::find_monitor(&monitors, selector).ok_or_else(|| {
            anyhow!(
                "找不到显示器 {selector}；可用：{}",
                describe_monitors(&monitors)
            )
        })?,
        None => monitor::select_monitor(Point::new(0, 0), &monitors)
            .ok_or_else(|| anyhow!("无法选择显示器"))?,
    };
    let monitor = &monitors[index];
    let backend = backend.unwrap_or(config.backend);
    let (image, used) =
        capture::capture_monitor_with_fallback(monitor, backend, config.hdr_exposure)?;
    logging::info(&format!(
        "capture-once monitor {} {}x{} mode={} backend={}",
        monitor.device_name,
        monitor.rect.width(),
        monitor.rect.height(),
        monitor.color_mode.as_str(),
        used.as_str()
    ));

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

fn describe_monitors(monitors: &[monitor::MonitorInfo]) -> String {
    monitors
        .iter()
        .map(|monitor| {
            format!(
                "{} {}x{} mode={}",
                monitor.device_name,
                monitor.rect.width(),
                monitor.rect.height(),
                monitor.color_mode.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
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
