# Feature 013: Settings Window & Panels

**Status:** Not Started
**Dependencies:** 011-gpui-application-shell, 010-custom-dictionary
**Design Reference:** Sections 4.6.4 (Component Architecture), 4.6.5 (Settings Window Panels)
**Estimated Scope:** Workspace layout, five panels (Settings, History, Dictionary, Model, Log)

---

## Overview

Implement the full settings/management window accessible from the system tray or overlay menu. It uses a workspace layout with a sidebar navigation and five panels. This follows the Tusk workspace pattern — a dock-based layout where the left sidebar selects the active panel and the center area renders the selected panel's content.

---

## Requirements

### FR-001: Settings Window

```rust
// crates/vox_ui/src/workspace.rs

pub struct SettingsWindow {
    workspace: Entity<VoxWorkspace>,
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.workspace.clone()
    }
}
```

Window configuration:

```rust
fn open_settings_window(cx: &mut App) {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(
            Bounds::centered(None, size(px(800.0), px(600.0)), cx),
        )),
        window_min_size: Some(Size { width: px(600.0), height: px(400.0) }),
        focus: true,
        show: true,
        ..Default::default()
    };

    cx.open_window(window_options, |window, cx| {
        window.on_window_should_close(cx, |_window, cx| {
            // Hide window instead of closing app
            false
        });
        cx.new(|cx| SettingsWindow::new(window, cx))
    }).ok();
}
```

### FR-002: VoxWorkspace

```rust
pub struct VoxWorkspace {
    active_panel: Panel,
    settings_panel: Entity<SettingsPanel>,
    history_panel: Entity<HistoryPanel>,
    dictionary_panel: Entity<DictionaryPanel>,
    model_panel: Entity<ModelPanel>,
    log_panel: Entity<LogPanel>,
    focus_handle: FocusHandle,
}

#[derive(Clone, PartialEq)]
pub enum Panel {
    Settings,
    History,
    Dictionary,
    Model,
    Log,
}
```

Layout:

```
┌──────────────────────────────────────────────┐
│  Vox Settings                          [─ □ ×]│
├────────────┬─────────────────────────────────┤
│            │                                  │
│  Settings  │                                  │
│  History   │     Active Panel Content         │
│  Dictionary│                                  │
│  Models    │                                  │
│  Logs      │                                  │
│            │                                  │
│            │                                  │
├────────────┴─────────────────────────────────┤
│  Status: Ready | Latency: 165ms | VRAM: 5.2GB│
└──────────────────────────────────────────────┘
```

### FR-003: Settings Panel

| Section | Controls |
|---|---|
| **Audio** | Input device dropdown, noise gate slider |
| **VAD** | Threshold slider, min silence slider, min speech slider |
| **Hotkey** | Activation hotkey recorder, hold-to-talk toggle, hands-free toggle |
| **LLM** | Temperature slider, filler removal toggle, course correction toggle, punctuation toggle |
| **Appearance** | Theme dropdown (System/Light/Dark), overlay opacity slider, overlay position dropdown, show raw transcript toggle |
| **Advanced** | Max segment duration, overlap duration, command prefix input |

Each setting change saves to JSON immediately and takes effect without restart.

```rust
pub struct SettingsPanel {
    // Audio section
    input_devices: Vec<AudioDeviceInfo>,
    selected_device: Option<String>,

    // Sliders, toggles, etc. — all backed by Settings struct
    settings: Settings,
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        div()
            .flex()
            .flex_col()
            .size_full()
            .p(spacing::LG)
            .gap(spacing::XL)
            .overflow_y_scroll()
            .child(self.render_audio_section(cx))
            .child(self.render_vad_section(cx))
            .child(self.render_hotkey_section(cx))
            .child(self.render_llm_section(cx))
            .child(self.render_appearance_section(cx))
            .child(self.render_advanced_section(cx))
    }
}
```

### FR-004: History Panel

Displays past transcriptions with search and pagination:

```rust
pub struct HistoryPanel {
    transcripts: Vec<TranscriptEntry>,
    search_query: String,
    scroll_handle: UniformListScrollHandle,
}
```

Features:
- Search by raw or polished text
- Display: timestamp, raw text (optional), polished text, target app, latency
- Copy individual transcript to clipboard
- Delete individual transcript
- "Clear All" with confirmation dialog
- Infinite scroll using GPUI's `uniform_list`

### FR-005: Dictionary Panel

CRUD interface for the custom dictionary:

```rust
pub struct DictionaryPanel {
    entries: Vec<DictionaryEntry>,
    search_query: String,
    editing_entry: Option<DictionaryEntry>,
    new_spoken: String,
    new_written: String,
    new_category: String,
}
```

Features:
- List all entries with search/filter by category
- Add new entry (spoken, written, category)
- Edit existing entry inline
- Delete entry with confirmation
- Import/export buttons (JSON file)
- Toggle command phrase flag per entry
- Sort by name, category, use count

### FR-006: Model Panel

Model management interface:

```rust
pub struct ModelPanel {
    models: Vec<ModelStatus>,
    download_progress: HashMap<String, DownloadProgress>,
}

pub struct ModelStatus {
    pub info: ModelInfo,
    pub state: ModelState,
}

pub enum ModelState {
    Missing,
    Downloading { progress: DownloadProgress },
    Downloaded { file_size: u64 },
    Loaded { vram_usage: u64 },
    Error { message: String },
}
```

Features:
- Show status of each model (missing, downloading, loaded)
- Download progress for each model
- "Retry Download" for failed models
- "Open Model Folder" button
- Quick benchmark result (inference speed)
- Swap model button (select new GGUF/GGML file)

### FR-007: Log Panel

Live log viewer showing tracing output:

```rust
pub struct LogPanel {
    log_entries: Vec<LogEntry>,
    auto_scroll: bool,
    filter_level: LogLevel,
    scroll_handle: UniformListScrollHandle,
}

pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}
```

Features:
- Real-time log streaming from tracing subscriber
- Filter by log level
- Auto-scroll (toggle on/off)
- Copy log entries to clipboard
- Clear log display
- Color-coded by level (red=error, yellow=warn, white=info, gray=debug)

### FR-008: Sidebar Navigation

```rust
fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();

    div()
        .flex()
        .flex_col()
        .w(px(160.0))
        .bg(theme.colors.surface)
        .border_r_1()
        .border_color(theme.colors.border)
        .p(spacing::SM)
        .gap(spacing::XS)
        .child(self.sidebar_item("Settings", Panel::Settings, cx))
        .child(self.sidebar_item("History", Panel::History, cx))
        .child(self.sidebar_item("Dictionary", Panel::Dictionary, cx))
        .child(self.sidebar_item("Models", Panel::Model, cx))
        .child(self.sidebar_item("Logs", Panel::Log, cx))
}

fn sidebar_item(
    &self,
    label: &str,
    panel: Panel,
    cx: &mut Context<Self>,
) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();
    let is_active = self.active_panel == panel;

    div()
        .px(spacing::MD)
        .py(spacing::SM)
        .rounded(radius::SM)
        .cursor_pointer()
        .when(is_active, |d| d.bg(theme.colors.accent).text_color(theme.colors.button_primary_text))
        .when(!is_active, |d| d.text_color(theme.colors.text_muted).hover(|d| d.bg(theme.colors.elevated_surface)))
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.active_panel = panel.clone();
            cx.notify();
        }))
}
```

### FR-009: Status Bar

Bottom status bar showing runtime info:

```
Ready | Latency: 165ms | VRAM: 5.2 GB | Audio: MacBook Pro Microphone
```

---

## Acceptance Criteria

- [ ] Settings window opens from tray/overlay
- [ ] Sidebar navigation switches panels
- [ ] Settings panel: all controls work and persist immediately
- [ ] History panel: search, scroll, copy, delete work
- [ ] Dictionary panel: CRUD operations work
- [ ] Model panel: shows status of all models
- [ ] Log panel: displays logs in real-time
- [ ] Window remembers size/position between sessions
- [ ] All panels use consistent theme colors
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_panel_switching` | Active panel changes on sidebar click |
| `test_settings_persistence` | Changed setting persists to JSON |
| `test_history_search` | Search filters transcripts |
| `test_dictionary_add` | New entry appears in list |
| `test_dictionary_delete` | Deleted entry removed from list |

---

## Performance Targets

| Metric | Target |
|---|---|
| Panel switch time | < 16 ms |
| History list (10,000 entries) | 60 fps scroll |
| Log panel (live streaming) | No frame drops at 100 logs/sec |
| Settings save | < 10 ms |
