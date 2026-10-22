pub mod clipboard;
pub mod gdi;
pub mod hdr;
pub mod wgc;

use anyhow::{anyhow, Result};

use crate::config::Backend;
use crate::geometry::{Point, Rect};
use crate::logging;
use crate::monitor::{self, MonitorInfo};

use self::hdr::ColorMode;

#[derive(Clone, Debug)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    /// BGRA, top-down, stride = width * 4.
    pub bgra: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub origin: Point,
    pub width: u32,
    pub height: u32,
    /// BGRA, top-down, stride = width * 4.
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn new(bounds: Rect) -> Self {
        let width = bounds.width().max(0) as u32;
        let height = bounds.height().max(0) as u32;
        Self {
            origin: Point::new(bounds.left, bounds.top),
            width,
            height,
            bgra: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(
            self.origin.x,
            self.origin.y,
            self.origin.x + self.width as i32,
            self.origin.y + self.height as i32,
        )
    }

    pub fn put_monitor_image(&mut self, monitor: &MonitorInfo, image: &CapturedImage) {
        let dest_x = monitor.rect.left - self.origin.x;
        let dest_y = monitor.rect.top - self.origin.y;
        let copy_w = image.width.min(monitor.rect.width().max(0) as u32) as i32;
        let copy_h = image.height.min(monitor.rect.height().max(0) as u32) as i32;
        if copy_w <= 0 || copy_h <= 0 {
            return;
        }
        let src_stride = image.width as usize * 4;
        let dst_stride = self.stride();
        for row in 0..copy_h as usize {
            let sy = row;
            let dy = dest_y + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let sx0 = 0usize;
            let dx0 = dest_x;
            if dx0 >= self.width as i32 {
                continue;
            }
            let line_pixels = copy_w as usize;
            let src_start = sy * src_stride + sx0 * 4;
            let dst_start = dy as usize * dst_stride + dx0.max(0) as usize * 4;
            let available = if dx0 < 0 {
                (line_pixels as i32 + dx0).max(0) as usize
            } else {
                line_pixels
            };
            if available == 0 {
                continue;
            }
            let src_offset = if dx0 < 0 { (-dx0) as usize * 4 } else { 0 };
            let src_slice =
                &image.bgra[src_start + src_offset..src_start + src_offset + available * 4];
            self.bgra[dst_start..dst_start + available * 4].copy_from_slice(src_slice);
        }
    }

    /// Reads the RGB color of one virtual-desktop pixel from the frozen frame.
    ///
    /// Coordinates are global physical pixels, matching `origin`; pixels in the
    /// empty gaps between monitors are part of the frame and read as black.
    pub fn pixel_at(&self, point: Point) -> Option<[u8; 3]> {
        if point.x < self.origin.x || point.y < self.origin.y {
            return None;
        }
        let x = (point.x - self.origin.x) as usize;
        let y = (point.y - self.origin.y) as usize;
        if x >= self.width as usize || y >= self.height as usize {
            return None;
        }
        let index = (y * self.width as usize + x) * 4;
        let pixel = self.bgra.get(index..index + 4)?;
        Some([pixel[2], pixel[1], pixel[0]])
    }

    pub fn crop_bgra(&self, rect: Rect) -> Option<CapturedImage> {
        let rect = rect.intersection(self.bounds());
        if rect.is_empty() {
            return None;
        }
        let x = (rect.left - self.origin.x) as usize;
        let y = (rect.top - self.origin.y) as usize;
        let width = rect.width() as usize;
        let height = rect.height() as usize;
        let src_stride = self.stride();
        let dst_stride = width * 4;
        let mut out = vec![0u8; dst_stride * height];
        for row in 0..height {
            let src = &self.bgra[(y + row) * src_stride + x * 4..][..dst_stride];
            out[row * dst_stride..(row + 1) * dst_stride].copy_from_slice(src);
        }
        Some(CapturedImage {
            width: width as u32,
            height: height as u32,
            bgra: out,
        })
    }

    pub fn crop_rgb(&self, rect: Rect) -> Option<(Vec<u8>, u32, u32)> {
        let image = self.crop_bgra(rect)?;
        let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
        for pixel in image.bgra.chunks_exact(4) {
            rgb.push(pixel[2]);
            rgb.push(pixel[1]);
            rgb.push(pixel[0]);
        }
        Some((rgb, image.width, image.height))
    }
}

pub fn preferred_backend(config_backend: Backend, color_mode: ColorMode) -> Backend {
    match config_backend {
        // Only true HDR needs the FP16 tone-mapping path. WCG/ACM still carries
        // SDR (display-referred) content and stays on the silent GDI path.
        Backend::Auto if color_mode.needs_tone_map() => Backend::Wgc,
        Backend::Auto => Backend::Gdi,
        other => other,
    }
}

pub fn capture_monitor(
    monitor: &MonitorInfo,
    backend: Backend,
    hdr_exposure: f32,
) -> Result<CapturedImage> {
    match backend {
        Backend::Gdi => gdi::capture(monitor),
        Backend::Wgc => wgc::capture(monitor, hdr_exposure),
        Backend::Auto => {
            let actual = preferred_backend(Backend::Auto, monitor.color_mode);
            capture_monitor(monitor, actual, hdr_exposure)
        }
    }
}

/// Captures one monitor using the configured policy, with the documented fallback.
pub fn capture_monitor_with_fallback(
    monitor: &MonitorInfo,
    config_backend: Backend,
    hdr_exposure: f32,
) -> Result<(CapturedImage, Backend)> {
    let preferred = preferred_backend(config_backend, monitor.color_mode);
    match capture_monitor(monitor, preferred, hdr_exposure) {
        Ok(image) => Ok((image, preferred)),
        Err(first_error) => {
            logging::warn(&format!(
                "backend {} failed for {}: {first_error:#}",
                preferred.as_str(),
                monitor.device_name
            ));
            let fallback = match (config_backend, preferred) {
                (Backend::Gdi, _) => None,
                (_, Backend::Gdi) => Some(Backend::Wgc),
                (_, Backend::Wgc) => Some(Backend::Gdi),
                _ => None,
            };
            if let Some(fallback) = fallback {
                match capture_monitor(monitor, fallback, hdr_exposure) {
                    Ok(image) => {
                        logging::warn(&format!(
                            "backend {} fallback succeeded for {}",
                            fallback.as_str(),
                            monitor.device_name
                        ));
                        return Ok((image, fallback));
                    }
                    Err(second_error) => {
                        logging::error(&format!(
                            "backend {} fallback failed for {}: {second_error:#}",
                            fallback.as_str(),
                            monitor.device_name
                        ));
                    }
                }
            }
            Err(anyhow!("截图失败：{first_error:#}"))
        }
    }
}

/// Captures all monitors into one virtual-desktop frame. A monitor that fails
/// is left black and logged; at least one monitor must succeed.
pub fn capture_virtual(
    monitors: &[MonitorInfo],
    config_backend: Backend,
    hdr_exposure: f32,
) -> Result<Frame> {
    let bounds = monitor::virtual_rect(monitors);
    let mut frame = Frame::new(bounds);
    let mut successes = 0usize;
    for monitor in monitors {
        match capture_monitor_with_fallback(monitor, config_backend, hdr_exposure) {
            Ok((image, backend)) => {
                logging::info(&format!(
                    "captured {} {}x{} mode={} with {}",
                    monitor.device_name,
                    monitor.rect.width(),
                    monitor.rect.height(),
                    monitor.color_mode.as_str(),
                    backend.as_str()
                ));
                frame.put_monitor_image(monitor, &image);
                successes += 1;
            }
            Err(err) => {
                logging::error(&format!(
                    "capture failed for {}: {err:#}",
                    monitor.device_name
                ));
            }
        }
    }
    if successes == 0 {
        return Err(anyhow!("所有显示器截图均失败"));
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        let mut frame = Frame::new(Rect::new(-100, -50, -96, -47));
        assert_eq!(frame.width, 4);
        assert_eq!(frame.height, 3);
        for y in 0..frame.height as usize {
            for x in 0..frame.width as usize {
                let index = (y * frame.width as usize + x) * 4;
                frame.bgra[index] = 10 + x as u8; // B
                frame.bgra[index + 1] = 20 + x as u8; // G
                frame.bgra[index + 2] = 30 + x as u8; // R
                frame.bgra[index + 3] = 255;
            }
        }
        frame
    }

    #[test]
    fn pixel_at_uses_global_coordinates() {
        let frame = frame();
        assert_eq!(frame.pixel_at(Point::new(-100, -50)), Some([30, 20, 10]));
        assert_eq!(frame.pixel_at(Point::new(-97, -48)), Some([33, 23, 13]));
        assert_eq!(frame.pixel_at(Point::new(-97, -47)), None);
        assert_eq!(frame.pixel_at(Point::new(-96, -50)), None);
        assert_eq!(frame.pixel_at(Point::new(-101, -50)), None);
        assert_eq!(frame.pixel_at(Point::new(-100, -51)), None);
    }

    #[test]
    fn pixel_at_handles_empty_frame() {
        let frame = Frame::new(Rect::default());
        assert_eq!(frame.pixel_at(Point::new(0, 0)), None);
    }

    #[test]
    fn auto_backend_only_uses_wgc_for_hdr() {
        assert_eq!(
            preferred_backend(Backend::Auto, ColorMode::Sdr),
            Backend::Gdi
        );
        assert_eq!(
            preferred_backend(Backend::Auto, ColorMode::Wcg),
            Backend::Gdi
        );
        assert_eq!(
            preferred_backend(Backend::Auto, ColorMode::Hdr),
            Backend::Wgc
        );
    }

    #[test]
    fn explicit_backend_overrides_color_mode() {
        assert_eq!(
            preferred_backend(Backend::Gdi, ColorMode::Hdr),
            Backend::Gdi
        );
        assert_eq!(
            preferred_backend(Backend::Wgc, ColorMode::Sdr),
            Backend::Wgc
        );
    }
}
