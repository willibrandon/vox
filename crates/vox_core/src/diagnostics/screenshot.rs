//! Window screenshot capture for diagnostics.
//!
//! Captures the client area of a window as a PNG-encoded byte buffer.
//! Uses platform-specific APIs: `PrintWindow` + GDI on Windows,
//! `CGWindowListCreateImage` on macOS.

use anyhow::Result;

/// Capture the client area of a window and return PNG-encoded bytes.
///
/// On Windows, `handle` is the raw HWND value (as `isize`).
/// On macOS, `handle` is the `CGWindowID` (as `isize`).
#[cfg(target_os = "windows")]
pub fn capture_window(handle: isize) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let hwnd = HWND(handle as *mut _);

    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }
        .map_err(|e| anyhow::anyhow!("GetClientRect failed: {e}"))?;

    let width = (rect.right - rect.left) as u32;
    let height = (rect.bottom - rect.top) as u32;

    if width == 0 || height == 0 {
        anyhow::bail!("window has zero client area ({width}x{height})");
    }

    unsafe {
        let screen_dc = GetDC(Some(hwnd));
        if screen_dc.is_invalid() {
            anyhow::bail!("GetDC returned null");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.is_invalid() {
            ReleaseDC(Some(hwnd), screen_dc);
            anyhow::bail!("CreateCompatibleDC failed");
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(Some(hwnd), screen_dc);
            anyhow::bail!("CreateCompatibleBitmap failed");
        }

        let old_bitmap = SelectObject(mem_dc, bitmap.into());

        // PW_RENDERFULLCONTENT = 2 — captures DWM-composed content
        let printed = PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(2));
        if !printed.as_bool() {
            SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(mem_dc);
            ReleaseDC(Some(hwnd), screen_dc);
            anyhow::bail!("PrintWindow failed");
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // negative = top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let rows = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(Some(hwnd), screen_dc);

        if rows == 0 {
            anyhow::bail!("GetDIBits returned 0 rows");
        }

        // GDI returns BGRA — convert to RGBA for PNG
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2); // B <-> R
        }

        encode_png(&pixels, width, height)
    }
}

/// Capture a window screenshot on macOS using Core Graphics.
///
/// `handle` is the `CGWindowID` cast to `isize`.
#[cfg(target_os = "macos")]
pub fn capture_window(handle: isize) -> Result<Vec<u8>> {
    use objc2_core_foundation::CGRect;
    use objc2_core_graphics::{
        CGImage, CGWindowImageOption, CGWindowListOption,
    };
    use std::ptr::NonNull;

    // Direct extern declaration to avoid the objc2 crate's deprecation annotation.
    // Apple has NOT deprecated CGWindowListCreateImage — the Rust bindings suggest
    // ScreenCaptureKit, but that API is async and heavyweight for single-window capture.
    unsafe extern "C-unwind" {
        fn CGWindowListCreateImage(
            screen_bounds: CGRect,
            list_option: CGWindowListOption,
            window_id: u32,
            image_option: CGWindowImageOption,
        ) -> Option<NonNull<CGImage>>;
    }

    let window_id = handle as u32;

    let bounds = CGRect::ZERO;
    let list_option = CGWindowListOption::OptionIncludingWindow;
    let image_option = CGWindowImageOption::BoundsIgnoreFraming;

    let image_ptr = unsafe {
        CGWindowListCreateImage(bounds, list_option, window_id, image_option)
    }
    .ok_or_else(|| anyhow::anyhow!("CGWindowListCreateImage returned null"))?;

    let image = unsafe { objc2_core_foundation::CFRetained::from_raw(image_ptr) };

    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));
    let bytes_per_row = CGImage::bytes_per_row(Some(&image));

    if width == 0 || height == 0 {
        anyhow::bail!("captured image has zero dimensions ({width}x{height})");
    }

    let provider = CGImage::data_provider(Some(&image))
        .ok_or_else(|| anyhow::anyhow!("CGImage has no data provider"))?;

    use objc2_core_graphics::CGDataProvider;
    let cf_data = CGDataProvider::data(Some(&provider))
        .ok_or_else(|| anyhow::anyhow!("CGDataProvider returned null data"))?;

    let data_len = cf_data.length() as usize;
    let data_ptr = cf_data.byte_ptr();
    let raw_bytes = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };

    // Core Graphics typically returns BGRA (or RGBA depending on display config).
    // Check bitmap info to determine actual pixel order.
    let bitmap_info = CGImage::bitmap_info(Some(&image));
    let alpha_info = bitmap_info.0 & 0x1F; // kCGBitmapAlphaInfoMask

    let mut rgba_pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row_start = y * bytes_per_row;
        for x in 0..width {
            let offset = row_start + x * 4;
            if offset + 3 < raw_bytes.len() {
                match alpha_info {
                    // kCGImageAlphaPremultipliedFirst / kCGImageAlphaFirst (ARGB → RGBA)
                    1 | 3 => {
                        rgba_pixels.push(raw_bytes[offset + 1]); // R
                        rgba_pixels.push(raw_bytes[offset + 2]); // G
                        rgba_pixels.push(raw_bytes[offset + 3]); // B
                        rgba_pixels.push(raw_bytes[offset]);     // A
                    }
                    // kCGImageAlphaPremultipliedLast / kCGImageAlphaLast (RGBA)
                    2 | 4 => {
                        rgba_pixels.push(raw_bytes[offset]);     // R
                        rgba_pixels.push(raw_bytes[offset + 1]); // G
                        rgba_pixels.push(raw_bytes[offset + 2]); // B
                        rgba_pixels.push(raw_bytes[offset + 3]); // A
                    }
                    // kCGImageAlphaNoneSkipFirst (xRGB → RGBa)
                    5 => {
                        rgba_pixels.push(raw_bytes[offset + 1]); // R
                        rgba_pixels.push(raw_bytes[offset + 2]); // G
                        rgba_pixels.push(raw_bytes[offset + 3]); // B
                        rgba_pixels.push(255);                   // A
                    }
                    // kCGImageAlphaNoneSkipLast (RGBx) or unknown — treat as BGRA
                    _ => {
                        rgba_pixels.push(raw_bytes[offset + 2]); // R (from BGRA)
                        rgba_pixels.push(raw_bytes[offset + 1]); // G
                        rgba_pixels.push(raw_bytes[offset]);     // B
                        rgba_pixels.push(255);                   // A
                    }
                }
            }
        }
    }

    encode_png(&rgba_pixels, width as u32, height as u32)
}

/// Capture a screenshot from a GPUI window.
///
/// Extracts the platform-specific native handle from the GPUI [`gpui::Window`]
/// and delegates to [`capture_window`] for the actual pixel capture.
pub fn capture_gpui_window(window: &gpui::Window) -> Result<Vec<u8>> {
    use raw_window_handle::HasWindowHandle;

    let handle = HasWindowHandle::window_handle(window)
        .map_err(|e| anyhow::anyhow!("failed to get native window handle: {e}"))?;

    #[cfg(target_os = "windows")]
    {
        let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() else {
            anyhow::bail!("expected Win32 window handle");
        };
        capture_window(win32.hwnd.get())
    }

    #[cfg(target_os = "macos")]
    {
        let raw_window_handle::RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            anyhow::bail!("expected AppKit window handle");
        };

        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        unsafe {
            let ns_view: *const AnyObject = appkit.ns_view.as_ptr().cast();
            let ns_window: *const AnyObject = msg_send![&*ns_view, window];
            if ns_window.is_null() {
                anyhow::bail!("NSView has no parent NSWindow");
            }
            let window_number: isize = msg_send![&*ns_window, windowNumber];
            capture_window(window_number)
        }
    }
}

/// Encode raw RGBA pixels as a PNG byte buffer.
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| anyhow::anyhow!("PNG header write failed: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| anyhow::anyhow!("PNG data write failed: {e}"))?;
    }
    Ok(buf)
}
