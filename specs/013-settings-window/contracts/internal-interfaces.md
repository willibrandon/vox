# Internal Interfaces: Settings Window & Panels

**Feature Branch**: `013-settings-window`
**Date**: 2026-02-23

> Per Constitution Principle IV (Pure Rust / GPUI — No Web Tech), there are no HTTP endpoints, REST APIs, or IPC serialization boundaries. All interfaces are direct Rust function calls between crates. This document specifies the internal API contracts between `vox_core` (backend) and `vox_ui` (UI layer).

## 1. Settings Read/Write Interface

**Provider**: `vox_core::config::Settings` via `VoxState`
**Consumer**: `vox_ui::settings_panel::SettingsPanel`

```rust
// READ: Settings panel reads current settings on render
let settings = cx.global::<VoxState>().settings(); // → RwLockReadGuard<Settings>
let device = settings.input_device.clone();
let noise_gate = settings.noise_gate;
// ... all 27 fields (23 existing + 4 new window bounds)

// WRITE: Settings panel updates a single field immediately
cx.global::<VoxState>().update_settings(|s| {
    s.noise_gate = new_value;
})?; // → Result<()>, persists to JSON atomically
```

**Contract guarantees**:
- `update_settings` is atomic: clone → modify → persist → swap
- Concurrent readers see the old value until swap completes
- JSON write uses .tmp → rename for crash safety
- New `window_*` fields default to `None` via `#[serde(default)]`

## 2. Audio Device Enumeration Interface

**Provider**: `vox_core::audio::list_input_devices()`
**Consumer**: `vox_ui::settings_panel::SettingsPanel`

```rust
// Returns all available audio input devices
fn list_input_devices() -> Result<Vec<AudioDeviceInfo>>

pub struct AudioDeviceInfo {
    pub name: String,      // OS-reported device name
    pub is_default: bool,  // Exactly one device is marked default
}
```

**Contract guarantees**:
- Called when Settings panel opens (not auto-refreshed)
- Returns empty `Vec` if no devices available (FR-010: "No devices found")
- `is_default` is true for exactly one device (or none if no devices)

## 3. Transcript History Interface

**Provider**: `vox_core::pipeline::transcript::TranscriptStore` via `VoxState`
**Consumer**: `vox_ui::history_panel::HistoryPanel`

```rust
// LIST: Paginated, newest first
fn list(limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>>

// SEARCH: LIKE on raw_text | polished_text
fn search(query: &str) -> Result<Vec<TranscriptEntry>>

// DELETE: Single entry by UUID
fn delete(id: &str) -> Result<()>

// CLEAR: Overwrite + delete + VACUUM (secure)
fn clear_secure() -> Result<()>

// COUNT: Total entries
fn count() -> Result<usize>
```

**Contract guarantees**:
- `list` returns entries in descending `created_at` order
- `search` is case-insensitive substring match on both raw and polished text
- `clear_secure` overwrites data before deleting (per existing implementation)
- All methods are thread-safe (`Arc<Mutex<Connection>>`)

## 4. Dictionary CRUD Interface

**Provider**: `vox_core::dictionary::DictionaryCache` via `VoxState`
**Consumer**: `vox_ui::dictionary_panel::DictionaryPanel`

```rust
// CRUD
fn add(spoken: &str, written: &str, category: &str, is_command_phrase: bool) -> Result<i64>
fn update(id: i64, spoken: &str, written: &str, category: &str, is_command_phrase: bool) -> Result<()>
fn delete(id: i64) -> Result<()>

// QUERY
fn list(category: Option<&str>) -> Vec<DictionaryEntry>  // Sorted by spoken
fn search(query: &str) -> Vec<DictionaryEntry>            // Partial match, case-insensitive

// IMPORT/EXPORT
fn export_json() -> Result<String>                         // JSON array of DictionaryExportEntry
fn import_json(json: &str) -> Result<ImportResult>         // Skips duplicates

pub struct ImportResult {
    pub added: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}
```

**Contract guarantees**:
- `add` errors on duplicate spoken form (case-insensitive)
- `import_json` skips entries with existing spoken forms, reports counts
- `list` returns entries sorted alphabetically by spoken form
- `search` matches on spoken, written, or category fields

## 5. Model Status Interface

**Provider**: `vox_core::models` + `VoxState`
**Consumer**: `vox_ui::model_panel::ModelPanel`

```rust
// Static model registry
pub const MODELS: &[ModelInfo; 3] = &[...];

// Runtime state per model (NEW — added to VoxState)
fn model_runtime(&self, model_name: &str) -> Option<&ModelRuntimeInfo>
fn set_model_runtime(&self, model_name: &str, info: ModelRuntimeInfo)

// Download management
fn download_missing(missing: &[&ModelInfo]) -> Result<()>  // Async
fn subscribe() -> broadcast::Receiver<DownloadEvent>         // Progress events

// File system
fn open_model_directory() -> Result<()>  // Fire-and-forget OS file manager
fn model_path(filename: &str) -> Result<PathBuf>
```

**Contract guarantees**:
- `MODELS` is a compile-time constant with exactly 3 models
- `DownloadEvent` variants: Started, Progress, Complete, Failed, VerificationFailed, DetectedOnDisk
- `open_model_directory` creates the directory if it doesn't exist

## 6. Log Capture Interface

**Provider**: `vox_core::log_sink::LogSink` (tracing Layer)
**Consumer**: `vox_ui::log_panel::LogPanel` (via LogStore entity)

```rust
// SETUP (in main.rs)
let (log_sink, log_receiver) = LogSink::new();
// Add log_sink as a tracing_subscriber Layer
// Pass log_receiver to LogStore entity in vox_ui

// LOG ENTRY
pub struct LogEntry {
    pub timestamp: String,    // ISO 8601
    pub level: LogLevel,
    pub target: String,       // Tracing target (e.g., "vox_core::audio")
    pub message: String,
}

pub enum LogLevel {
    Error, Warn, Info, Debug, Trace,
}

// RECEIVER
pub struct LogReceiver {
    rx: mpsc::UnboundedReceiver<LogEntry>,
}
```

**Contract guarantees**:
- `LogSink` implements `tracing_subscriber::Layer<S>` for any `S: Subscriber`
- Log entries are sent over unbounded mpsc channel (never blocks the logging thread)
- `LogReceiver` is consumed by `LogStore` entity on the GPUI foreground thread
- `LogStore` maintains bounded `VecDeque<LogEntry>` with 10,000 entry capacity, auto-evicting oldest

## 7. Clipboard Interface

**Provider**: GPUI framework
**Consumer**: All panels with copy actions

```rust
// WRITE (from any App context)
cx.write_to_clipboard(ClipboardItem::new_string(text));
```

**Contract guarantees**:
- Platform-native clipboard integration
- No read-back needed (write-only for this feature)

## 8. File Dialog Interface

**Provider**: GPUI framework
**Consumer**: Dictionary panel (import/export), Model panel (swap)

```rust
// OPEN file picker
cx.prompt_for_paths(PathPromptOptions {
    files: true,
    directories: false,
    multiple: false,
    prompt: Some("Select file".into()),
}) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>>

// SAVE file picker
cx.prompt_for_new_path(
    directory: &Path,
    suggested_name: Option<&str>,
) -> oneshot::Receiver<Result<Option<PathBuf>>>
```

**Contract guarantees**:
- Both return async receivers — must be awaited in `cx.spawn()`
- Returns `None` if user cancels the dialog
- Platform-native file dialogs (no web tech)

## 9. Window Management Interface

**Provider**: GPUI framework
**Consumer**: `vox_ui::workspace::SettingsWindow`

```rust
// OPEN
cx.open_window(options, build_fn) -> Result<WindowHandle<V>>

// FOCUS existing
handle.update(cx, |_, window, _| { window.activate_window(); })

// READ bounds (for persistence)
window.window_bounds() -> WindowBounds

// CLOSE handler
window.on_window_should_close(cx, |window, cx| -> bool { ... })
```

**Contract guarantees**:
- `WindowHandle<V>` is invalidated when the window closes
- `activate_window()` brings window to foreground and focuses it
- `on_window_should_close` callback fires before the window closes — return `false` to prevent close, `true` to allow
