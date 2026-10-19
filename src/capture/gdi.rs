use std::mem::size_of;

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDCW, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush,
    GetDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
    DIB_RGB_COLORS, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::capture::CapturedImage;
use crate::monitor::MonitorInfo;
use crate::win_util;

pub fn capture(monitor: &MonitorInfo) -> Result<CapturedImage> {
    let width = monitor.rect.width();
    let height = monitor.rect.height();
    if width <= 0 || height <= 0 {
        return Err(anyhow!("显示器尺寸无效"));
    }

    let device = win_util::wide(&monitor.device_name);
    let dc = unsafe {
        CreateDCW(
            PCWSTR::null(),
            PCWSTR(device.as_ptr()),
            PCWSTR::null(),
            None,
        )
    };
    if !dc.0.is_null() {
        let result = capture_from_dc(dc, width, height, 0, 0);
        unsafe {
            let _ = DeleteDC(dc);
        }
        if result.is_ok() {
            return result;
        }
    }

    // Fallback: capture from the virtual-screen DC using virtual coordinates.
    let screen = unsafe { GetDC(None) };
    if screen.0.is_null() {
        return Err(anyhow!("无法获取屏幕 DC"));
    }
    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let result = capture_from_dc(
        screen,
        width,
        height,
        monitor.rect.left - vx,
        monitor.rect.top - vy,
    );
    unsafe {
        let _ = ReleaseDC(None, screen);
    }
    result
}

fn capture_from_dc(
    source: HDC,
    width: i32,
    height: i32,
    src_x: i32,
    src_y: i32,
) -> Result<CapturedImage> {
    let memory = unsafe { CreateCompatibleDC(Some(source)) };
    if memory.0.is_null() {
        return Err(anyhow!("CreateCompatibleDC 失败"));
    }

    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(
            Some(source),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    } {
        Ok(bitmap) => bitmap,
        Err(err) => {
            unsafe {
                let _ = DeleteDC(memory);
            }
            return Err(anyhow!("CreateDIBSection 失败：{err}"));
        }
    };

    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
        }
        return Err(anyhow!("DIB 像素指针为空"));
    }

    let old = unsafe { SelectObject(memory, HGDIOBJ(bitmap.0)) };
    let blt = unsafe {
        BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(source),
            src_x,
            src_y,
            windows::Win32::Graphics::Gdi::ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
        )
    };
    unsafe {
        let _ = GdiFlush();
    }

    let result = if let Err(err) = blt {
        Err(anyhow!("BitBlt 失败：{err}"))
    } else {
        let len = width as usize * height as usize * 4;
        let mut bgra = vec![0u8; len];
        unsafe {
            std::ptr::copy_nonoverlapping(bits as *const u8, bgra.as_mut_ptr(), len);
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(CapturedImage {
            width: width as u32,
            height: height as u32,
            bgra,
        })
    };

    unsafe {
        if !old.0.is_null() {
            let _ = SelectObject(memory, old);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
    }

    result
}
