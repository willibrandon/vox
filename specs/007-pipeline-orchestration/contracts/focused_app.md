# API Contract: Focused Application Name Detection

**Module**: `crates/vox_core/src/injector.rs` (public function added to existing module)

## get_focused_app_name

```rust
/// Get the name of the currently focused application.
///
/// Returns the executable/application name (not the window title):
/// - Windows: Process executable stem (e.g., "notepad", "Code")
///   via GetForegroundWindow → GetWindowThreadProcessId → OpenProcess →
///   QueryFullProcessImageNameW → extract filename stem.
/// - macOS: Application localized name (e.g., "Safari", "Visual Studio Code")
///   via NSWorkspace.shared().frontmostApplication()?.localizedName().
///
/// Returns "Unknown" if detection fails for any reason (no focused window,
/// API error, elevated process). This is non-fatal — the LLM uses app name
/// for tone hints, not critical logic.
pub fn get_focused_app_name() -> String;
```

### Platform implementations

**Windows** (`injector/windows.rs`):
```rust
pub(super) fn get_focused_app_name_impl() -> String {
    // GetForegroundWindow() → HWND
    // GetWindowThreadProcessId(hwnd) → PID
    // OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, pid) → handle
    // QueryFullProcessImageNameW(handle) → "C:\\...\\notepad.exe"
    // Extract filename stem → "notepad"
    // Fallback: "Unknown"
}
```

**macOS** (`injector/macos.rs`):
```rust
pub(super) fn get_focused_app_name_impl() -> String {
    // NSWorkspace.shared().frontmostApplication() → NSRunningApplication
    // app.localizedName() → "Safari"
    // Fallback: "Unknown"
}
```

All required Win32 features (`Win32_System_Threading`, `Win32_UI_WindowsAndMessaging`, `Win32_Foundation`) and macOS frameworks (`objc2`) are already in Cargo.toml.
