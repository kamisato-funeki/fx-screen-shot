use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;

/// Copies an RGB8 image as a 24-bit CF_DIB bitmap.
pub fn copy_rgb(hwnd: HWND, rgb: &[u8], width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(anyhow!("剪贴板图像尺寸为空"));
    }
    let expected = width as usize * height as usize * 3;
    if rgb.len() != expected {
        return Err(anyhow!("剪贴板像素长度不符"));
    }

    let stride = (width as usize * 3).div_ceil(4) * 4;
    let image_size = stride * height as usize;
    let total = 40 + image_size;
    let mut data = vec![0u8; total];

    data[0..4].copy_from_slice(&40u32.to_le_bytes());
    data[4..8].copy_from_slice(&(width as i32).to_le_bytes());
    data[8..12].copy_from_slice(&(height as i32).to_le_bytes());
    data[12..14].copy_from_slice(&1u16.to_le_bytes());
    data[14..16].copy_from_slice(&24u16.to_le_bytes());
    data[16..20].copy_from_slice(&0u32.to_le_bytes());
    data[20..24].copy_from_slice(&(image_size as u32).to_le_bytes());

    for y in 0..height as usize {
        let dst_y = height as usize - 1 - y;
        let src_row = y * width as usize * 3;
        let dst_row = 40 + dst_y * stride;
        for x in 0..width as usize {
            let src = src_row + x * 3;
            let dst = dst_row + x * 3;
            data[dst] = rgb[src + 2];
            data[dst + 1] = rgb[src + 1];
            data[dst + 2] = rgb[src];
        }
    }

    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, total) }
        .map_err(|err| anyhow!("GlobalAlloc 失败：{err}"))?;
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(anyhow!("GlobalLock 失败"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), pointer as *mut u8, total);
        let _ = GlobalUnlock(memory);
    }

    let mut opened = false;
    for _ in 0..5 {
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            opened = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(anyhow!("无法打开剪贴板"));
    }

    let result = (|| -> Result<()> {
        unsafe { EmptyClipboard() }.map_err(|err| anyhow!("清空剪贴板失败：{err}"))?;
        match unsafe { SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(memory.0))) } {
            Ok(_) => Ok(()),
            Err(err) => {
                let _ = unsafe { GlobalFree(Some(memory)) };
                Err(anyhow!("写入剪贴板失败：{err}"))
            }
        }
    })();

    let _ = unsafe { CloseClipboard() };
    result
}
