# Implementation Plan: Overlay HUD

**Branch**: `012-overlay-hud` | **Date**: 2026-02-22 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/012-overlay-hud/spec.md`

## Summary

Implement the overlay HUD — the primary visual interface for Vox during dictation. A compact, floating, always-on-top, borderless pill that displays pipeline state through color-coded indicators and text labels. The overlay covers all app states: downloading (per-model progress bars), loading (stage messages), idle (hotkey hint), listening (real-time waveform), processing (raw transcript), injected (polished text with fade), error (guidance message), injection failure (buffered text with Copy button), and download failure (action buttons).

Technical approach: Expand the existing `OverlayHud` view (currently 136 lines, zero-field struct showing only readiness text) into a full state-aware component with status bar, content area, animations, and interactive elements. Introduce `OverlayDisplayState` as a lightweight GPUI Global bridge for reactive state updates. Implement `WaveformVisualizer` as a custom GPUI `Element` using `paint_quad` for real-time audio bar rendering at 30fps. Add `InjectionFailed` variant to `PipelineState` for text recovery. Wire position persistence via `observe_window_bounds` and settings save.

## Technical Context

**Language/Version**: Rust 2024 (1.85+)
**Primary Dependencies**: gpui (git rev 89e9ab97, v0.2.2), vox_core (workspace path dep), serde, parking_lot, tracing, smallvec
**Storage**: Settings persisted to JSON (`settings.json`) — overlay position and opacity stored in existing `Settings` struct
**Testing**: `cargo test -p vox_ui` for overlay and waveform unit tests
**Target Platform**: Windows (CUDA) + macOS (Metal)
**Project Type**: Three-crate Rust workspace (vox binary, vox_core backend, vox_ui frontend)
**Performance Goals**: Overlay render < 2ms/frame, state update → render < 16ms, waveform 30fps with 50 bars
**Constraints**: No web tech, no network calls, no focus stealing, < 500MB RAM, < 2% idle CPU, zero warnings
**Scale/Scope**: Single overlay window, 10+ distinct visual states, 1 custom Element (waveform), 1 context menu (quick settings)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| I. Local-Only Processing | PASS | Overlay is pure UI rendering. Clipboard write uses local OS API via GPUI. No network calls. |
| II. Real-Time Latency Budget | PASS | Overlay render target < 2ms/frame (SC-002). State updates via GPUI global observation — no blocking, no audio thread involvement. Waveform reads VoxState.latest_rms from processing thread (RwLock, < 1μs). |
| III. Full Pipeline — No Fallbacks | PASS | Every possible app state has a distinct, informative overlay display (SC-008). 10+ states mapped. No degraded modes. Overlay shows download progress when pipeline not ready (FR-014). |
| IV. Pure Rust / GPUI — No Web Tech | PASS | All UI built with GPUI `div()` builder API and custom `Element` trait. WaveformVisualizer uses `paint_quad` for GPU-accelerated rendering. No HTML, CSS, JS, WebView. |
| V. Zero-Click First Launch | PASS | Overlay shows download progress automatically (FR-010). Hotkey responds in every state (FR-014). No setup wizards or confirmation dialogs. |
| VI. Scope Only Increases | PASS | All 22 functional requirements implemented. All 6 user stories addressed. InjectionFailed variant added to PipelineState (scope increase). No features removed. |
| VII. Public API Documentation | PASS | All pub structs (OverlayHud, OverlayDisplayState, WaveformVisualizer), pub functions (open_overlay_window), and pub methods documented with `///`. Module-level `//!` docs on overlay_hud.rs and waveform.rs. |
| VIII. No Test Skipping | PASS | All tests run unconditionally via `cargo test -p vox_ui`. No `#[ignore]`, no `#[cfg(skip)]`. Tests use GPUI's `VisualTestContext` for headless rendering. |
| IX. Explicit Commit Only | PASS | No commits created during implementation. User must explicitly instruct. |
| X. No Deferral | PASS | All features implemented in this plan. All 22 FRs addressed. All edge cases handled. No items labeled deferred, outstanding, or pending. |
| XI. No Optional Compilation | PASS | No feature flags on overlay code. gpui is a required dependency in vox_ui/Cargo.toml. WaveformVisualizer Element impl is unconditional. |

**Post-Phase-1 re-check**: All gates still pass. No new dependencies introduced. No web tech. No feature flags. All public items documented. All tests unconditional.

## Project Structure

### Documentation (this feature)

```text
specs/012-overlay-hud/
├── spec.md              # Feature specification (22 FRs, 6 user stories, 8 SCs)
├── plan.md              # This file
├── research.md          # Phase 0: GPUI patterns, state bridging, Element trait
├── data-model.md        # Phase 1: Entity definitions and state transitions
├── quickstart.md        # Phase 1: Build, test, and verification instructions
├── contracts/           # Phase 1: Module public API contracts
│   ├── overlay-hud-api.md
│   ├── waveform-api.md
│   └── display-state-api.md
├── checklists/
│   └── requirements.md  # Spec quality checklist (all pass)
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_ui/src/
├── vox_ui.rs            # Lib root — module declarations (existing, unchanged)
├── overlay_hud.rs       # OverlayHud view + OverlayDisplayState global
│                        #   EXPAND: zero-field → full state, status bar, content area,
│                        #   10+ state renderers, quick settings dropdown,
│                        #   position persistence, fade animation, waveform timer
├── waveform.rs          # WaveformVisualizer custom Element
│                        #   IMPLEMENT: Element trait with paint_quad bars,
│                        #   IntoElement impl, bar rendering algorithm
├── theme.rs             # VoxTheme colors
│                        #   ADD: status_loading, status_injection_failed colors
├── layout.rs            # Layout constants
│                        #   ADD: WAVEFORM_WIDTH, WAVEFORM_HEIGHT, PROGRESS_BAR_HEIGHT
└── key_bindings.rs      # Actions and keybindings
                         #   ADD: CopyInjectedText, RetryDownload, OpenModelFolder, DismissOverlay

crates/vox_core/src/
├── state.rs             # VoxState
│                        #   ADD: latest_rms field (RwLock<f32>), getter/setter methods
├── config.rs            # Settings
│                        #   VERIFY: overlay_position, overlay_opacity already present
└── pipeline/
    └── state.rs         # PipelineState enum
                         #   ADD: InjectionFailed { polished_text, error } variant

crates/vox/src/
└── main.rs              # App shell
                         #   ADD: OverlayDisplayState initialization + bridge updates,
                         #   update_overlay_state() helper called after every
                         #   set_readiness() and set_pipeline_state()
```

**Structure Decision**: Existing three-crate workspace. All overlay UI code in `vox_ui`. State bridge (`OverlayDisplayState`) defined in `vox_ui` and consumed by `vox` (main.rs). `VoxState` additions (`latest_rms`) in `vox_core`. `PipelineState` modification (`InjectionFailed`) in `vox_core`. No new crates. No new modules beyond what already exists (overlay_hud.rs and waveform.rs are existing files).

## Design Decisions

### D-001: State Reactivity via OverlayDisplayState Bridge

**Problem**: VoxState uses `RwLock` interior mutability. GPUI's `observe_global` fires only when `cx.set_global()` replaces the global, not on internal RwLock mutations.

**Solution**: A lightweight `OverlayDisplayState` GPUI Global that is replaced via `set_global()` on every state change. The overlay subscribes with `observe_global::<OverlayDisplayState>()`.

**Contract**: Every call to `VoxState::set_readiness()` or `VoxState::set_pipeline_state()` in main.rs MUST be followed by `cx.set_global(OverlayDisplayState { ... })` with current values.

See: [research.md#R-001](research.md), [contracts/display-state-api.md](contracts/display-state-api.md)

### D-002: Waveform Data Flow

**Problem**: Audio RMS values arrive at ~32ms intervals on the processing thread. The overlay needs 50 recent values for visualization at 30fps.

**Solution**: VoxState stores a single `latest_rms: RwLock<f32>` (written by processing thread). OverlayHud maintains its own `VecDeque<f32>` ring buffer (capacity 50), populated by a 33ms animation timer that reads `latest_rms` and calls `cx.notify()`. Timer runs only during Listening state.

See: [research.md#R-003](research.md)

### D-003: WaveformVisualizer as Custom Element

**Problem**: GPUI's `div()` builder API cannot efficiently render 50 dynamic-height bars per frame. Creating 50 div elements per render adds layout overhead.

**Solution**: `WaveformVisualizer` implements the low-level `Element` trait directly, using `paint_quad(fill(...))` in its `paint()` method. One quad per bar, no layout engine involvement beyond the initial size request.

See: [research.md#R-002](research.md), [contracts/waveform-api.md](contracts/waveform-api.md)

### D-004: InjectionFailed PipelineState Variant

**Problem**: When text injection fails, the overlay must display the polished text with a Copy button. The existing `Error { message }` variant only stores an error message — the polished text is lost.

**Solution**: Add `InjectionFailed { polished_text: String, error: String }` variant to `PipelineState`. The pipeline orchestrator emits this state when injection fails. The overlay renders the polished text and a Copy button. State persists until Copy is clicked or hotkey starts new dictation.

See: [research.md#R-009](research.md), [data-model.md#E-004](data-model.md)

### D-005: Position Persistence

**Problem**: Overlay position must survive app restarts (FR-016) and be clamped to screen bounds (FR-018).

**Solution**: Use `cx.observe_window_bounds()` to detect position changes. Save to `Settings.overlay_position` as `Custom { x, y }`. On launch, read saved position, clamp to current screen bounds, and pass to `WindowOptions::window_bounds`. If saved monitor is disconnected, fall back to primary monitor center.

See: [research.md#R-005](research.md)

### D-006: Pulsing and Fade Animations

**Problem**: Listening state needs a pulsing green dot. Injected state needs a 2-second text fade.

**Solution**: Use GPUI's `AnimationExt::with_animation()` for the pulsing indicator (repeating, 1-second cycle, opacity 0.4→1.0). Use `cx.spawn()` with a 2-second timer for the injection fade (sets `showing_injected_fade = false` and calls `cx.notify()`).

See: [research.md#R-007](research.md)

### D-007: Quick Settings Dropdown

**Problem**: The overlay needs a compact dropdown with dictation toggle and language selector (FR-003).

**Solution**: A boolean `quick_settings_open` flag on OverlayHud. When true, render an anchored dropdown div below the ▾ button. Contains a toggle for dictation (dispatches `ToggleRecording`) and a language selector (updates `Settings.language`). Click-outside or Escape dismisses. Reference: Tusk's `ContextMenu` pattern.

See: [research.md#R-008](research.md)

### D-008: Clipboard Copy for Injection Failure Recovery

**Problem**: When injection fails, the user must be able to copy the polished text (FR-013).

**Solution**: Use GPUI's `cx.write_to_clipboard(ClipboardItem::new_string(text))`. The Copy button handler reads `polished_text` from the `InjectionFailed` state, writes to clipboard, shows brief confirmation (checkmark indicator), then transitions to Idle.

See: [research.md#R-006](research.md)

## Complexity Tracking

No constitution violations. No justifications needed.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none)    | —          | —                                   |
