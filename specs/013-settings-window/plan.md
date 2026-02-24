# Implementation Plan: Settings Window & Panels

**Branch**: `013-settings-window` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/013-settings-window/spec.md`

## Summary

Implement the full settings/management window with a workspace layout: sidebar navigation, five content panels (Settings, History, Dictionary, Models, Logs), a custom scrollbar element, and a status bar. The workspace follows Tusk's pattern of `Entity<T>` per panel with enum-based panel switching — simpler than Tusk's full dock system since we have a fixed 5-panel layout. Scrollable panels use a custom GPUI `Element` for the scrollbar (sibling of scroll container). The History and Log panels use GPUI's `uniform_list` for virtualized rendering. The Log panel captures tracing output via a custom `tracing_subscriber::Layer` bridged to a GPUI entity through an mpsc channel (matching Zed's LSP log store pattern). Six reusable UI components (Button, TextInput, Toggle, Slider, Select, HotkeyRecorder) are built to support the Settings panel and reused across other panels.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: gpui (git rev 89e9ab97, v0.2.2), vox_core (workspace path dep), serde, serde_json, parking_lot, tracing, tracing-subscriber, smallvec, anyhow, cpal (via vox_core for device enumeration)
**Storage**: JSON (settings.json via vox_core), SQLite (vox.db via vox_core for transcripts + dictionary)
**Testing**: `cargo test -p vox_ui` (UI component tests), `cargo test -p vox_core --features cuda` (backend tests)
**Target Platform**: Windows (RTX 4090, CUDA), macOS (M4 Pro, Metal)
**Project Type**: Three-crate Rust workspace (vox, vox_core, vox_ui)
**Performance Goals**: <16ms panel switch (SC-002), 60fps scrolling @ 10K entries (SC-003), 100 logs/sec display without frame drops (SC-004), <10ms settings save (SC-005)
**Constraints**: Pure Rust/GPUI (no web tech), local-only (no network calls), single static binary, <500MB RAM, <15MB binary, zero compiler warnings
**Scale/Scope**: 45 functional requirements across 5 panels, 6 reusable UI components, 1 custom GPUI Element (scrollbar), 1 tracing integration (log sink)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|-----------|--------|-------|
| I | Local-Only Processing | PASS | No network calls introduced. Model download uses existing `ModelDownloader`. Settings/history/dictionary are local JSON/SQLite. |
| II | Real-Time Latency Budget | PASS | Settings window is a separate window — does not interfere with the audio callback or ML inference pipeline. Panel rendering targets <16ms. |
| III | Full Pipeline — No Fallbacks | PASS | The settings window is independent of the dictation pipeline. All pipeline components remain required. Model swap explicitly stops the pipeline, swaps, reloads, and restarts — no degraded mode. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | All UI built with GPUI `div()` builder, custom `Element` for scrollbar, `uniform_list` for virtualized lists. No JS/HTML/CSS/WebView. No IPC serialization — UI calls Rust functions directly via `VoxState` GPUI Global. |
| V | Zero-Click First Launch | PASS | No setup wizards or configuration required. Settings window is optional — users only open it if they choose to customize. Default settings work out of the box. |
| VI | Scope Only Increases | PASS | All 45 functional requirements from the spec are implemented. No features removed, deferred, or made optional. FR-037 (benchmark) and FR-038 (model swap) are fully designed. |
| VII | Public API Documentation | PASS | All new `pub` items (structs, enums, functions, methods, constants, modules) will have `///` doc comments describing what and why. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. No `#[ignore]`, no `#[cfg(skip)]`, no conditional compilation guards on tests. |
| IX | Explicit Commit Only | PASS | No commits without explicit user instruction. |
| X | No Deferral | PASS | Every requirement is addressed in this plan. No items labeled "deferred," "outstanding," or "better suited for later." All research unknowns resolved in research.md. |
| XI | No Optional Compilation | PASS | No new optional dependencies. No `#[cfg(feature = "...")]` guards on required functionality. Platform-specific code (CUDA/Metal) remains in existing feature flags only. |
| XII | No Blame Attribution | PASS | No blame attribution in any artifact. |
| XIII | No Placeholders | PASS | Every function body, struct, module, and component will contain real, working implementation. No `todo!()`, no dummy data, no stub implementations. Existing empty module files (workspace.rs, settings_panel.rs, etc.) will be filled with complete implementations. |

**Post-Phase 1 Re-check**: All gates still pass. No new dependencies, no web tech, no optional compilation, no scope reduction. The design adds 4 new Settings fields (window bounds), 3 new vox_core types (LogEntry, LogLevel, LogSink + LogReceiver), 2 new vox_core additions (BenchmarkResult, ModelRuntimeInfo), 6 new vox_ui modules (toggle, slider, select, hotkey_recorder, scrollbar, log_store — if log_store is in vox_ui), and fills 9 existing empty modules with real implementations.

## Project Structure

### Documentation (this feature)

```text
specs/013-settings-window/
├── plan.md                            # This file
├── spec.md                            # Feature specification (45 FRs)
├── research.md                        # Phase 0: 14 research decisions
├── data-model.md                      # Phase 1: Entity definitions and relationships
├── quickstart.md                      # Phase 1: 30 manual test scenarios
├── contracts/
│   └── internal-interfaces.md         # Phase 1: Rust API contracts between crates
├── checklists/
│   └── requirements.md                # Spec quality checklist (16/16 pass)
└── tasks.md                           # Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code (repository root)

```text
crates/vox_core/src/
├── config.rs              # MODIFY — Add window_x/y/width/height fields to Settings
├── state.rs               # MODIFY — Add model_runtime HashMap, BenchmarkResult storage
├── models.rs              # MODIFY — Add BenchmarkResult struct
├── log_sink.rs            # NEW — LogEntry, LogLevel, LogSink (tracing Layer), LogReceiver
└── vox_core.rs            # MODIFY — Add `pub mod log_sink;` declaration

crates/vox_ui/src/
├── vox_ui.rs              # MODIFY — Add new module declarations
├── theme.rs               # MODIFY — Add log-level colors, scrollbar colors to ThemeColors
├── layout.rs              # MODIFY — Add status bar height, sidebar width constants
├── scrollbar.rs           # NEW — Custom GPUI Element for vertical scrollbar
├── button.rs              # FILL — Reusable button component (RenderOnce)
├── text_input.rs          # FILL — Text input component (Entity + Render)
├── icon.rs                # FILL — Icon rendering utilities
├── toggle.rs              # NEW — Toggle switch component (RenderOnce)
├── slider.rs              # NEW — Range slider component (Entity + Render)
├── select.rs              # NEW — Dropdown select component (Entity + Render)
├── hotkey_recorder.rs     # NEW — Hotkey capture input (Entity + Render)
├── workspace.rs           # FILL — VoxWorkspace, open_settings_window, sidebar, status bar
├── settings_panel.rs      # FILL — Settings panel with 6 sections (Audio, VAD, Hotkey, LLM, Appearance, Advanced)
├── history_panel.rs       # FILL — Transcript history with search, copy, delete, clear, uniform_list
├── dictionary_panel.rs    # FILL — Dictionary CRUD, search, sort, import/export
├── model_panel.rs         # FILL — Model status display, download progress, benchmark, swap
├── log_panel.rs           # FILL — Live log viewer with level filter, auto-scroll, uniform_list
├── overlay_hud.rs         # MODIFY — Wire "Open Settings" in quick settings dropdown to OpenSettings action
└── key_bindings.rs        # MODIFY — Wire OpenSettings action handler (currently logs dispatch)
```

**Structure Decision**: Existing three-crate workspace (`vox`, `vox_core`, `vox_ui`). All UI code in `vox_ui`, all backend logic in `vox_core`, wiring in `vox` binary crate. This feature adds files to `vox_ui` (7 new + 9 filled) and `vox_core` (1 new + 3 modified). No new crate dependencies required — all needed functionality is available from the existing dependency set (gpui, tracing-subscriber, serde, cpal, etc.).

## Architecture Decisions

### A1: Workspace Layout Pattern

The VoxWorkspace renders a horizontal flexbox: sidebar (fixed 160px) + content area (flex-grow). The content area renders the active panel entity. All five panels are created upfront as `Entity<T>` and stored in VoxWorkspace. Only the active panel renders; inactive panels retain their state (scroll position, search queries, etc.) but don't execute render cycles.

```
VoxWorkspace (Entity, root of settings window)
├── active_panel: Panel (enum: Settings, History, Dictionary, Model, Log)
├── settings_panel: Entity<SettingsPanel>
├── history_panel: Entity<HistoryPanel>
├── dictionary_panel: Entity<DictionaryPanel>
├── model_panel: Entity<ModelPanel>
├── log_panel: Entity<LogPanel>
├── focus_handle: FocusHandle
└── _subscriptions: Vec<Subscription> (observe VoxState for status bar)

Render layout:
┌─────────────────────────────────────────────┐
│ div().size_full().flex().flex_col()          │
│ ┌─────────────────────────────────────────┐ │
│ │ div().flex_1().flex().flex_row()         │ │
│ │ ┌────────┐ ┌──────────────────────────┐ │ │
│ │ │Sidebar │ │ Active panel content     │ │ │
│ │ │160px   │ │ (flex-grow)              │ │ │
│ │ │fixed   │ │                          │ │ │
│ │ └────────┘ └──────────────────────────┘ │ │
│ └─────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────┐ │
│ │ StatusBar (28px fixed height)           │ │
│ └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

### A2: Scrollbar as Sibling Pattern

For panels that need a custom scrollbar (Settings panel, potentially others), the scrollbar `Element` MUST be a sibling of the scroll container, not a child:

```
div()                          // Non-scrolling wrapper (parent of both)
    .size_full()
    .child(
        div()                  // Scroll container
            .id("scroll-id")  // Required for overflow_y_scroll
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(/* content */)
    )
    .child(Scrollbar::new(    // Scrollbar as SIBLING
        scroll_handle.clone(),
        cx.entity_id(),
        scrollbar_drag.clone(),
        thumb_color,
        track_color,
    ))
```

This prevents GPUI's `with_element_offset(scroll_offset)` from shifting the scrollbar with the content.

### A3: Log Capture Data Flow

```
Any thread (audio, ML, async)     GPUI foreground thread
─────────────────────────────     ──────────────────────
tracing::info!("message")
        ↓
LogSink (Layer impl)
        ↓
mpsc::unbounded_send(LogEntry)
        ↓                        cx.spawn(async { rx.recv() })
                                          ↓
                                  LogStore.update(|store, cx| {
                                      store.entries.push_back(entry);
                                      // auto-evict if > 10,000
                                      cx.emit(Event::NewLogEntry);
                                  })
                                          ↓
                                  LogPanel (cx.subscribe → cx.notify)
                                          ↓
                                  uniform_list renders visible range
```

### A4: Settings Change Propagation

Settings changes propagate immediately through the `VoxState` GPUI Global:

1. User adjusts a slider in SettingsPanel
2. `cx.listener(|this, _, _, cx| { ... })` fires
3. Calls `cx.global::<VoxState>().update_settings(|s| s.field = new_value)?`
4. `update_settings` clones → modifies → persists to JSON → swaps RwLock
5. `cx.notify()` triggers re-render of SettingsPanel (reflects new value)
6. Other windows (overlay HUD) observe VoxState changes and re-render as needed

No explicit "apply" or "save" button — every change is atomic and immediate.

### A5: Model Swap Pipeline Integration

```
User clicks "Swap Model"
        ↓
File dialog opens (cx.prompt_for_paths)
        ↓
User selects .gguf/.ggml file
        ↓
Validate file extension matches model type
        ↓
Check if pipeline is active
├── Active: Stop pipeline → show loading in overlay HUD
└── Inactive: Proceed directly
        ↓
Copy file to model_dir() with original filename
        ↓
Update Settings (whisper_model or llm_model filename)
        ↓
Reload model (same path as initial load in main.rs pipeline init)
├── Success: Run benchmark → Update ModelRuntimeInfo → Restart pipeline (if was active)
└── Failure: Show error → Restore previous filename → Reload original → Restart pipeline
```

## Complexity Tracking

> No Constitution violations. All gates pass. No complexity exceptions needed.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| *(none)* | | |
