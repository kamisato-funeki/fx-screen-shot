use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use windows::core::{Interface, HSTRING};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::RoGetActivationFactory;

use crate::capture::hdr;
use crate::capture::CapturedImage;
use crate::logging;
use crate::monitor::MonitorInfo;

pub fn capture(monitor: &MonitorInfo, hdr_exposure: f32) -> Result<CapturedImage> {
    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(anyhow!("当前系统不支持 Windows Graphics Capture"));
    }

    let device = create_device()?;
    let dxgi_device: IDXGIDevice = device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    let direct_device: IDirect3DDevice = inspectable.cast()?;

    let interop: IGraphicsCaptureItemInterop = unsafe {
        RoGetActivationFactory(&HSTRING::from(
            "Windows.Graphics.Capture.GraphicsCaptureItem",
        ))?
    };
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor.hmon)? };

    let preferred = if monitor.hdr {
        DirectXPixelFormat::R16G16B16A16Float
    } else {
        DirectXPixelFormat::B8G8R8A8UIntNormalized
    };

    match capture_with_format(
        &device,
        &direct_device,
        &item,
        monitor,
        preferred,
        hdr_exposure,
    ) {
        Ok(image) => Ok(image),
        Err(first) if monitor.hdr => {
            logging::warn(&format!(
                "WGC FP16 capture failed on {}, retrying BGRA8: {first:#}",
                monitor.device_name
            ));
            capture_with_format(
                &device,
                &direct_device,
                &item,
                monitor,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                hdr_exposure,
            )
        }
        Err(err) => Err(err),
    }
}

fn create_device() -> Result<ID3D11Device> {
    let levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(std::ptr::null_mut()),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    device.ok_or_else(|| anyhow!("D3D11CreateDevice 未返回设备"))
}

fn capture_with_format(
    device: &ID3D11Device,
    direct_device: &IDirect3DDevice,
    item: &GraphicsCaptureItem,
    monitor: &MonitorInfo,
    format: DirectXPixelFormat,
    hdr_exposure: f32,
) -> Result<CapturedImage> {
    let width = monitor.rect.width().max(1) as u32;
    let height = monitor.rect.height().max(1) as u32;
    let size = SizeInt32 {
        Width: width as i32,
        Height: height as i32,
    };

    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(direct_device, format, 2, size)?;
    let session = pool.CreateCaptureSession(item)?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false);
    session.StartCapture()?;

    let result = (|| -> Result<CapturedImage> {
        let frame = wait_for_frame(&pool, Duration::from_millis(700))
            .ok_or_else(|| anyhow!("等待 WGC 首帧超时"))?;
        let content = frame.ContentSize()?;
        if content.Width <= 0 || content.Height <= 0 {
            return Err(anyhow!("WGC 返回空画面"));
        }
        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };

        let dxgi_format = match format {
            DirectXPixelFormat(10) => DXGI_FORMAT_R16G16B16A16_FLOAT,
            _ => DXGI_FORMAT_B8G8R8A8_UNORM,
        };

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: dxgi_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut staging))?;
        }
        let staging = staging.ok_or_else(|| anyhow!("创建 WGC staging 纹理失败"))?;
        let context = unsafe { device.GetImmediateContext()? };
        unsafe {
            context.CopyResource(&staging, &texture);
        }

        let mut mapped = Default::default();
        unsafe {
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        }

        let copy_w = (content.Width as u32).min(width) as usize;
        let copy_h = (content.Height as u32).min(height) as usize;
        let row_pitch = mapped.RowPitch as usize;
        let mut bgra = vec![0u8; width as usize * height as usize * 4];

        if format == DirectXPixelFormat::R16G16B16A16Float {
            for y in 0..copy_h {
                let src_row =
                    unsafe { (mapped.pData as *const u8).add(y * row_pitch) as *const u16 };
                for x in 0..copy_w {
                    let src = unsafe { src_row.add(x * 4) };
                    let r = hdr::half_to_f32(unsafe { *src });
                    let g = hdr::half_to_f32(unsafe { *src.add(1) });
                    let b = hdr::half_to_f32(unsafe { *src.add(2) });
                    let out = hdr::tone_map_rgb(r, g, b, hdr_exposure);
                    let dst = (y * width as usize + x) * 4;
                    bgra[dst] = out[2];
                    bgra[dst + 1] = out[1];
                    bgra[dst + 2] = out[0];
                    bgra[dst + 3] = 255;
                }
            }
        } else {
            for y in 0..copy_h {
                let src_row = unsafe { (mapped.pData as *const u8).add(y * row_pitch) };
                let dst_start = y * width as usize * 4;
                let src_len = copy_w * 4;
                bgra[dst_start..dst_start + src_len]
                    .copy_from_slice(unsafe { std::slice::from_raw_parts(src_row, src_len) });
            }
            for pixel in bgra.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }

        unsafe {
            context.Unmap(&staging, 0);
        }

        Ok(CapturedImage {
            width,
            height,
            bgra,
        })
    })();

    let _ = session.Close();
    let _ = pool.Close();
    result
}

fn wait_for_frame(
    pool: &Direct3D11CaptureFramePool,
    timeout: Duration,
) -> Option<windows::Graphics::Capture::Direct3D11CaptureFrame> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(frame) = pool.TryGetNextFrame() {
            return Some(frame);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    None
}
