//! GPU detection and hardware information.
//!
//! Detects the GPU at startup using platform-specific APIs:
//! - Windows: DXGI adapter enumeration for GPU name and dedicated VRAM
//! - macOS: sysctl for Apple Silicon chip name and unified memory
//!
//! The detected `GpuInfo` is stored in `VoxState` and used for VRAM
//! availability checks, OOM error messages, and the Model panel display.

use std::fmt;

/// Detected GPU hardware information, queried once at startup.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// GPU adapter name (e.g., "NVIDIA GeForce RTX 4090", "Apple M4 Pro").
    pub name: String,
    /// Dedicated video memory in bytes (Windows) or total unified memory (macOS).
    pub vram_bytes: u64,
    /// Driver version string (Windows only, from DXGI adapter description).
    pub driver_version: Option<String>,
    /// Which GPU compute platform is available.
    pub platform: GpuPlatform,
}

/// Which GPU compute platform the detected hardware supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPlatform {
    /// NVIDIA GPU with CUDA support (Windows).
    Cuda,
    /// Apple Silicon with Metal (macOS).
    Metal,
}

impl fmt::Display for GpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} MB VRAM, {:?})",
            self.name,
            self.vram_bytes / (1024 * 1024),
            self.platform
        )
    }
}

impl fmt::Display for GpuPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuPlatform::Cuda => write!(f, "CUDA"),
            GpuPlatform::Metal => write!(f, "Metal"),
        }
    }
}

/// Detect the GPU hardware on the current system.
///
/// Windows: Uses `CreateDXGIFactory1` → `EnumAdapters1` → `DXGI_ADAPTER_DESC1`
/// to get adapter name and dedicated video memory.
///
/// macOS: Uses `sysctl` to read chip name (`machdep.cpu.brand_string`) and
/// total unified memory (`hw.memsize`).
///
/// Returns `None` if no compatible GPU is found.
pub fn detect_gpu() -> Option<GpuInfo> {
    #[cfg(target_os = "windows")]
    {
        detect_gpu_windows()
    }

    #[cfg(target_os = "macos")]
    {
        detect_gpu_macos()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn detect_gpu_windows() -> Option<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;

    // Enumerate adapters — index 0 is typically the primary GPU.
    // Skip software adapters (DXGI_ADAPTER_FLAG_SOFTWARE).
    for index in 0..16 {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(_) => break,
        };

        let desc = match unsafe { adapter.GetDesc1() } {
            Ok(desc) => desc,
            Err(_) => continue,
        };

        // Skip software adapters
        if desc.Flags
            & windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32
            != 0
        {
            continue;
        }

        // NVIDIA PCI vendor ID — only NVIDIA GPUs support CUDA.
        // Intel (0x8086) and AMD (0x1002) adapters have nonzero
        // DedicatedVideoMemory but cannot run CUDA workloads.
        const NVIDIA_VENDOR_ID: u32 = 0x10DE;

        if desc.VendorId != NVIDIA_VENDOR_ID {
            continue;
        }

        let name = String::from_utf16_lossy(
            &desc
                .Description
                .iter()
                .take_while(|&&c| c != 0)
                .copied()
                .collect::<Vec<u16>>(),
        );

        let vram_bytes = desc.DedicatedVideoMemory as u64;

        if vram_bytes == 0 {
            continue;
        }

        return Some(GpuInfo {
            name,
            vram_bytes,
            driver_version: None,
            platform: GpuPlatform::Cuda,
        });
    }

    None
}

#[cfg(target_os = "macos")]
fn detect_gpu_macos() -> Option<GpuInfo> {
    // hw.optional.arm64 is 1 on Apple Silicon, absent or 0 on Intel Macs.
    // Intel Macs cannot run Metal-accelerated ggml inference (no unified
    // memory architecture), so we must reject them here rather than letting
    // them fail later during model load with a less actionable error.
    let is_arm64 = sysctl_u64("hw.optional.arm64").unwrap_or(0);
    if is_arm64 != 1 {
        tracing::info!("Intel Mac detected — Metal GPU acceleration not supported");
        return None;
    }

    let name = sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "Apple Silicon".to_string());
    let memsize = sysctl_u64("hw.memsize")?;

    Some(GpuInfo {
        name,
        vram_bytes: memsize,
        driver_version: None,
        platform: GpuPlatform::Metal,
    })
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> Option<String> {
    use std::ffi::CString;
    let c_name = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;

    // First call to get buffer size
    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || size == 0 {
        return None;
    }

    let mut buf = vec![0u8; size];
    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return None;
    }

    // Trim null terminator
    if let Some(pos) = buf.iter().position(|&b| b == 0) {
        buf.truncate(pos);
    }
    String::from_utf8(buf).ok()
}

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    use std::ffi::CString;
    let c_name = CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>() as libc::size_t;

    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            &mut value as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_info_display() {
        let info = GpuInfo {
            name: "NVIDIA GeForce RTX 4090".to_string(),
            vram_bytes: 24_576 * 1024 * 1024,
            driver_version: Some("560.94".to_string()),
            platform: GpuPlatform::Cuda,
        };
        let display = format!("{info}");
        assert!(display.contains("RTX 4090"));
        assert!(display.contains("MB VRAM"));
    }

    #[test]
    fn test_gpu_platform_display() {
        assert_eq!(format!("{}", GpuPlatform::Cuda), "CUDA");
        assert_eq!(format!("{}", GpuPlatform::Metal), "Metal");
    }

    #[test]
    fn test_detect_gpu_returns_result() {
        // On a developer machine, this should return Some with real GPU info.
        // In CI without a GPU, it may return None. Both are valid outcomes.
        let result = detect_gpu();
        if let Some(info) = &result {
            assert!(!info.name.is_empty());
            assert!(info.vram_bytes > 0);
        }
        // Test passes regardless — we're verifying it doesn't panic.
    }
}
