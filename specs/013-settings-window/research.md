# Research: Settings Window & Panels

**Feature Branch**: `013-settings-window`
**Date**: 2026-02-23

## R1: Window Singleton Pattern

**Decision**: Store `Option<WindowHandle<SettingsWindow>>` at the application level. Before opening, check if the handle exists and the window is still valid; if so, call `window.activate_window()` to focus it instead of creating a duplicate.

**Rationale**: GPUI's `cx.open_window()` returns `WindowHandle<V>`. The handle can be stored and later used via `handle.update(cx, |view, window, cx| { window.activate_window(); })`. This is the standard Zed/Tusk pattern for singleton windows. The handle is set to `None` when the window closes via `on_window_should_close`.

**Alternatives considered**:
- Iterating `cx.windows()` to find an existing SettingsWindow by type — rejected because it requires downcasting `AnyWindowHandle` and is less direct than tracking the handle explicitly.

## R2: Panel Switching Architecture

**Decision**: Enum-based routing with `Entity<T>` per panel stored in `VoxWorkspace`. An `active_panel: Panel` enum field determines which panel renders in the main content area. Sidebar items call `cx.listener(move |this, _, _, cx| { this.active_panel = panel; cx.notify(); })`.

**Rationale**: Vox has a fixed set of 5 panels with no reordering, resizing, or drag-and-drop. Tusk's full dock system (`Entity<Dock>` with `Arc<dyn PanelHandle>` type erasure) is designed for flexible IDE layouts with resizable, collapsible, multi-tab docks. That complexity is unnecessary for a fixed sidebar → content layout. The enum approach is simpler, has zero overhead for panel switching (just changing an enum value), and compiles without dynamic dispatch.

**Alternatives considered**:
- Tusk's `Panel` trait + `Dock` system — rejected because the settings window has a fixed layout; dock flexibility adds complexity with no user benefit.
- Rendering all panels and hiding inactive ones — rejected because it wastes memory and CPU rendering invisible panels.

## R3: Custom Scrollbar Element

**Decision**: Implement `Scrollbar` as a custom GPUI `Element` (not a View/Entity) that renders track and thumb via `paint_quad`, reads scroll position from a shared `ScrollHandle`, and handles mouse events for wheel tracking, click-to-jump, and thumb dragging. The scrollbar MUST be a sibling of the scroll container, never a child.

**Rationale**: GPUI's `div().overflow_y_scroll()` handles scrolling but provides no visible scrollbar. GPUI's `with_element_offset(scroll_offset)` in the div prepaint phase shifts ALL children of a scroll container — including `Position::Absolute` elements. If the scrollbar were a child, it would scroll with the content. As a sibling sharing the same non-scrolling parent, it stays fixed while reading the shared `ScrollHandle` for position data. This matches Zed's `ScrollbarElement` pattern (`crates/ui/src/components/scrollbar.rs`).

**Alternatives considered**:
- Using GPUI's built-in scrollbar (none exists for this version).
- Placing scrollbar as a child with compensating offset — rejected because GPUI's `with_element_offset` is applied unconditionally to all children during prepaint; there's no way to exempt specific children.

## R4: Virtualized List Rendering

**Decision**: Use GPUI's `uniform_list(id, item_count, render_fn)` with `UniformListScrollHandle` for the History panel (10,000+ entries) and Log panel (bounded buffer, rapid updates). Dictionary panel uses `uniform_list` as well for consistency.

**Rationale**: `uniform_list` renders only the visible range of items, maintaining 60fps with arbitrarily large datasets. The render callback receives a `Range<usize>` and returns a `Vec<impl IntoElement>` for just those items. `UniformListScrollHandle::scroll_to_item(ix, ScrollStrategy)` provides programmatic scrolling for auto-scroll in the log panel.

**Alternatives considered**:
- Rendering all items with `overflow_y_scroll()` — rejected because it would render all 10,000+ items every frame, violating the 60fps requirement (SC-003).

## R5: Log Capture Architecture

**Decision**: Three-layer architecture:
1. `LogSink` — a custom `tracing_subscriber::Layer` in `vox_core` that formats log events into `LogEntry` structs and sends them over an `mpsc::unbounded()` channel.
2. `LogStore` — a GPUI `Entity` in `vox_ui` that owns the channel receiver, polls it via a spawned foreground task, stores entries in a bounded `VecDeque<LogEntry>` (capacity 10,000), and emits `Event::NewLogEntry` for subscribers.
3. `LogPanel` — subscribes to `LogStore` events via `cx.subscribe()`, renders entries via `uniform_list`, and manages auto-scroll state.

**Rationale**: Tracing events fire from any thread (audio, ML inference, async runtime). GPUI entities run on the foreground thread. The mpsc channel bridges the gap — the `Layer` sends from any thread, and a foreground `cx.spawn()` task receives and updates the entity. This matches Zed's LSP log store pattern (`crates/project/src/lsp_store/log_store.rs`): bounded VecDeque with auto-eviction, event emission via `cx.emit()`, and UI subscription via `cx.subscribe()`.

**Alternatives considered**:
- Direct `Arc<Mutex<VecDeque>>` shared between tracing layer and UI — rejected because accessing the mutex from the UI thread would contend with high-frequency logging, and there's no mechanism to notify GPUI of new entries without a channel.
- Writing logs to a file and tailing it — rejected because it adds filesystem I/O latency and complexity for a feature that only needs in-memory buffering.

## R6: File Dialog Integration

**Decision**: Use GPUI's `cx.prompt_for_paths(PathPromptOptions)` for file selection (dictionary import, model swap) and `cx.prompt_for_new_path(directory, suggested_name)` for file saving (dictionary export). Both return `oneshot::Receiver<Result<Option<...>>>` and must be awaited in a `cx.spawn()` task.

**Rationale**: These are the native GPUI file dialog APIs that delegate to the OS file picker. No additional dependencies needed.

**Alternatives considered**:
- `rfd` crate (Rust File Dialog) — rejected because GPUI already provides native file dialogs; adding another crate is unnecessary.

## R7: Clipboard Access

**Decision**: Use `cx.write_to_clipboard(ClipboardItem::new_string(text))` for all copy actions (history entries, log entries, injected text).

**Rationale**: Built-in GPUI API, no additional dependencies.

## R8: Window Position Persistence

**Decision**: Add `window_x: Option<f32>`, `window_y: Option<f32>`, `window_width: Option<f32>`, `window_height: Option<f32>` fields to the `Settings` struct in `config.rs`. On window open, if all four fields are `Some`, construct `WindowBounds::Windowed(Bounds { origin, size })` with display bounds clamping (off-screen fallback to centered). On window close (via `on_window_should_close`), read `window.window_bounds()` and persist the bounds to these fields.

**Rationale**: Settings already persists to JSON atomically (`save()` writes to .tmp then renames). Adding four optional fields is minimal. The existing overlay HUD already persists position via `OverlayPosition` in Settings, so this follows the established pattern. `Option<f32>` defaults to `None` (centered) on first launch without needing migration.

**Alternatives considered**:
- Separate window state file — rejected because it fragments persistence and the Settings struct already handles this pattern.
- Saving on every resize/move — rejected because saving only on close is sufficient and avoids frequent disk writes during interactive resizing.

## R9: Status Bar

**Decision**: Implement as a stateless `RenderOnce` component that reads from `VoxState` (GPUI Global) each frame. Displays: pipeline state label, last transcription latency, VRAM usage, and active audio device name. The status bar is rendered as the bottom child of the workspace layout.

**Rationale**: The status bar has no internal state — it purely reflects VoxState. `RenderOnce` avoids the overhead of a separate Entity. This matches Tusk's `StatusBar` pattern.

**Alternatives considered**:
- Entity with subscriptions — rejected because the workspace already re-renders when VoxState changes (via `observe_global`), so the status bar naturally updates.

## R10: UI Component Library

**Decision**: Build six reusable GPUI components in `vox_ui`:

| Component | File | Pattern | Key Features |
|-----------|------|---------|-------------|
| Button | `button.rs` | `RenderOnce` | Label, icon, variants (primary/secondary/ghost/danger), disabled state, on_click |
| TextInput | `text_input.rs` | `Entity<TextInput>` + `Render` | Focus, placeholder, change events, submit on Enter |
| Toggle | `toggle.rs` | `RenderOnce` | Boolean on/off, label, on_change callback |
| Slider | `slider.rs` | `Entity<Slider>` + `Render` | Min/max/step, current value display, drag-to-change, on_change callback |
| Select | `select.rs` | `Entity<Select<T>>` + `Render` | Dropdown options, selected state, keyboard nav, on_change |
| HotkeyRecorder | `hotkey_recorder.rs` | `Entity<HotkeyRecorder>` + `Render` | Captures keystroke on focus, displays current binding, on_change |

**Rationale**: These components are used across multiple panels (Settings uses all six; Dictionary uses Button, TextInput, Toggle; History uses Button, TextInput). Building them as reusable components avoids duplication and ensures visual consistency. Stateless components (Button, Toggle) use `RenderOnce` for zero-allocation rendering. Stateful components (TextInput, Slider, Select, HotkeyRecorder) use `Entity<T>` for focus management and internal state.

**Alternatives considered**:
- Inline rendering without components — rejected because it would duplicate styling/behavior logic across panels, violating DRY and making theme consistency harder to maintain.

## R11: Model Benchmarks

**Decision**: Compute benchmarks once per model load and store results in `VoxState` as `BenchmarkResult { metric_name: String, value: f64 }` per model. Metrics:
- VAD (Silero): inferences per second (inference time for one 512-sample chunk, extrapolated)
- ASR (Whisper): real-time factor (audio_duration / processing_time, where >1.0 means faster than real-time)
- LLM (Qwen): tokens per second (output_tokens / generation_time)

Benchmarks are computed at the end of model loading (in the existing async pipeline init in `main.rs`) using a short warm-up inference. Results are displayed as static text in the Model panel until the model is reloaded.

**Rationale**: A single warm-up inference after loading provides a representative metric without user-facing delay (loading already takes seconds). Storing in VoxState makes the data available to the UI without re-computation. This satisfies FR-037 and Assumption 5.

**Alternatives considered**:
- On-demand benchmark button — rejected because Assumption 5 states benchmarks are computed once on load, and an on-demand button adds UI complexity for minimal benefit.
- Continuous averaging — rejected because it would measure production workloads (variable audio lengths, prompts) rather than a consistent baseline.

## R12: Model Swap Flow

**Decision**: Multi-step swap operation:
1. User clicks "Swap Model" → GPUI file dialog (`cx.prompt_for_paths`) opens with file filter
2. Validate selected file has `.gguf`, `.ggml`, or `.onnx` extension (matching model type)
3. If pipeline is active → stop pipeline, show loading state in overlay HUD
4. Copy selected file to the model directory with its original filename
5. Update the model filename in Settings (`whisper_model` or `llm_model` field)
6. Reload the model (trigger the same loading path as initial startup)
7. On success → update model status to Loaded, run benchmark, restart pipeline
8. On failure → show error message, restore previous model filename in Settings, reload original model, restart pipeline

**Rationale**: Copying to the model directory keeps all models in one location (consistent with `open_model_directory()`). Updating the Settings filename means the swap persists across restarts. The stop→swap→reload→restart sequence ensures no concurrent access to model files. Error recovery restores the previous state.

**Alternatives considered**:
- Symlink to external file — rejected because Windows symlinks require elevated privileges and are unreliable.
- Referencing external path directly — rejected because it would break if the user moves/deletes the file, and `model_dir()` is the canonical model location.

## R13: Inline Confirmation Pattern

**Decision**: For delete actions (history entry, dictionary entry), replace the delete button with a "Confirm? [Yes] [No]" prompt within the same row for 5 seconds. After 5 seconds without confirmation, revert to the original delete button. Implementation: store `confirming_delete: Option<(EntityId, Task<()>)>` in the panel. The `Task` is a `cx.spawn(async { Timer::after(Duration::from_secs(5)).await })` that clears the state on timeout.

**Rationale**: Matches Assumption 7 from the spec. Avoids disruptive modal dialogs for frequent single-item operations. The 5-second timeout prevents accidental confirmations if the user walks away. The `Task`-based timer follows the existing pattern from the overlay HUD's injection fade timer.

**Alternatives considered**:
- Modal confirmation dialog for all deletes — rejected by Assumption 7 (only "Clear All" uses modal).
- Undo-based deletion (delete immediately, offer undo) — rejected because it requires temporary storage and complicates the data layer.

## R14: Theme Extensions

**Decision**: Add log-level colors to `ThemeColors` in `theme.rs`:
- `log_error: Hsla` — red (matches `status_error`)
- `log_warn: Hsla` — amber/yellow
- `log_info: Hsla` — white (matches `text`)
- `log_debug: Hsla` — gray (matches `text_muted`)
- `log_trace: Hsla` — dimmer gray

Also add `scrollbar_thumb: Hsla` and `scrollbar_track: Hsla` for scrollbar styling, keeping them consistent with the theme rather than hardcoded in the scrollbar element.

**Rationale**: Semantic color tokens ensure log levels are visually distinct (FR-045) and themes can override them. Using existing colors as anchors (red for error, text_muted for debug) maintains visual consistency.
