//! System sleep/wake event detection.
//!
//! Platform-specific listener threads detect when the system resumes from sleep
//! and send a `WakeEvent` via a tokio mpsc channel. The main application thread
//! receives these events and triggers the wake recovery sequence:
//! 1. Audio device health check → recovery loop
//! 2. GPU context verification → model reload if needed
//! 3. Hotkey re-registration → permission guidance if failed
//! 4. Pipeline reset to Idle

use std::time::Instant;

/// Marker event sent when the system resumes from sleep.
///
/// Contains a timestamp for logging how quickly the recovery sequence
/// completes after a wake event.
#[derive(Debug, Clone)]
pub struct WakeEvent {
    /// When the wake was detected by the platform listener.
    pub timestamp: Instant,
}

/// Start the platform-specific sleep/wake listener.
///
/// Spawns a dedicated thread that listens for OS power management events.
/// Returns a receiver that yields `WakeEvent` each time the system wakes.
///
/// - Windows: Creates a message-only window (`HWND_MESSAGE`) handling
///   `WM_POWERBROADCAST` / `PBT_APMRESUMEAUTOMATIC`.
/// - macOS: Uses `IORegisterForSystemPower` with a callback on
///   `kIOMessageSystemHasPoweredOn`.
///
/// The listener thread runs for the lifetime of the application.
pub fn start_wake_listener() -> tokio::sync::mpsc::UnboundedReceiver<WakeEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    #[cfg(target_os = "windows")]
    {
        start_wake_listener_windows(sender);
    }

    #[cfg(target_os = "macos")]
    {
        start_wake_listener_macos(sender);
    }

    receiver
}

#[cfg(target_os = "windows")]
fn start_wake_listener_windows(sender: tokio::sync::mpsc::UnboundedSender<WakeEvent>) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        HMENU, HWND_MESSAGE, MSG, PBT_APMRESUMEAUTOMATIC, WINDOW_EX_STYLE, WINDOW_STYLE,
        WNDCLASSW, WM_POWERBROADCAST,
    };

    static WAKE_SENDER: OnceLock<tokio::sync::mpsc::UnboundedSender<WakeEvent>> = OnceLock::new();
    WAKE_SENDER.get_or_init(|| sender);

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_POWERBROADCAST && wparam.0 as u32 == PBT_APMRESUMEAUTOMATIC {
            if let Some(tx) = WAKE_SENDER.get() {
                let _ = tx.send(WakeEvent {
                    timestamp: Instant::now(),
                });
                tracing::info!("System wake detected (PBT_APMRESUMEAUTOMATIC)");
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    std::thread::Builder::new()
        .name("vox-wake-listener".to_string())
        .spawn(move || {
            let class_name: Vec<u16> = "VoxWakeListener\0".encode_utf16().collect();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            unsafe {
                RegisterClassW(&wc);
                let _hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    windows::core::PCWSTR(class_name.as_ptr()),
                    windows::core::PCWSTR::null(),
                    WINDOW_STYLE::default(),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    Some(HMENU::default()),
                    None,
                    None,
                );

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
        })
        .expect("Failed to spawn wake listener thread");
}

#[cfg(target_os = "macos")]
fn start_wake_listener_macos(sender: tokio::sync::mpsc::UnboundedSender<WakeEvent>) {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    // IOKit power management types
    type IONotificationPortRef = *mut c_void;
    type IOObject = u32;
    type IOReturn = i32;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            notification_port: *mut IONotificationPortRef,
            callback: unsafe extern "C" fn(
                refcon: *mut c_void,
                service: IOObject,
                message_type: u32,
                message_argument: *mut c_void,
            ),
            notifier: *mut IOObject,
        ) -> IOObject;

        fn IONotificationPortGetRunLoopSource(
            notify_port: IONotificationPortRef,
        ) -> *mut c_void;

        fn IOAllowPowerChange(kernel_port: IOObject, notification_id: isize) -> IOReturn;
    }

    // CoreFoundation run loop types
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRun();
    }

    // kCFRunLoopDefaultMode
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopDefaultMode: CFStringRef;
    }

    const K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;
    const K_IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 0xe000_0280;
    const K_IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 0xe000_0280 + 0x10;

    static WAKE_SENDER_MAC: OnceLock<tokio::sync::mpsc::UnboundedSender<WakeEvent>> =
        OnceLock::new();

    WAKE_SENDER_MAC.get_or_init(|| sender);

    unsafe extern "C" fn power_callback(
        _refcon: *mut c_void,
        service: IOObject,
        message_type: u32,
        message_argument: *mut c_void,
    ) {
        match message_type {
            K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON => {
                if let Some(tx) = WAKE_SENDER_MAC.get() {
                    let _ = tx.send(WakeEvent {
                        timestamp: Instant::now(),
                    });
                    tracing::info!("System wake detected (kIOMessageSystemHasPoweredOn)");
                }
            }
            K_IO_MESSAGE_CAN_SYSTEM_SLEEP | K_IO_MESSAGE_SYSTEM_WILL_SLEEP => {
                // Allow sleep to proceed
                unsafe {
                    IOAllowPowerChange(service, message_argument as isize);
                }
            }
            _ => {}
        }
    }

    std::thread::Builder::new()
        .name("vox-wake-listener".to_string())
        .spawn(move || unsafe {
            let mut notify_port: IONotificationPortRef = std::ptr::null_mut();
            let mut notifier: IOObject = 0;

            let root_port = IORegisterForSystemPower(
                std::ptr::null_mut(),
                &mut notify_port,
                power_callback,
                &mut notifier,
            );

            if root_port == 0 {
                tracing::error!("IORegisterForSystemPower failed");
                return;
            }

            let source = IONotificationPortGetRunLoopSource(notify_port);
            let run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);

            CFRunLoopRun();
        })
        .expect("Failed to spawn wake listener thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wake_event_creation() {
        let event = WakeEvent {
            timestamp: Instant::now(),
        };
        // Verify the timestamp is recent (within 1 second)
        assert!(event.timestamp.elapsed().as_secs() < 1);
    }

    #[test]
    fn test_start_wake_listener_returns_receiver() {
        // Verify the listener creates a valid channel without panicking.
        // On Windows, this spawns a real message-only window thread.
        // On macOS, this spawns a real IOKit listener thread.
        let _receiver = start_wake_listener();
        // If we got here, the thread spawned successfully.
    }
}
