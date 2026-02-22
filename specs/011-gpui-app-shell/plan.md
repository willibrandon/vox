# Implementation Plan: GPUI Application Shell

**Branch**: `011-gpui-app-shell` | **Date**: 2026-02-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/011-gpui-app-shell/spec.md`

## Summary

Wire together the existing Vox backend (VoxState, model management, pipeline orchestration, dictionary) into a working GPUI desktop application. The app shell creates a GPUI `Application`, initializes global state, registers actions and keyboard shortcuts, opens a themed overlay HUD window, sets up the system tray, registers a global hotkey, initializes structured logging, and kicks off async model downloading/pipeline loading. After this feature, Vox launches, shows a themed window immediately, and transitions to "Ready" state without user interaction.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: gpui (git rev 89e9ab97, v0.2.2), tray-icon 0.19, global-hotkey 0.6, tracing 0.1, tracing-subscriber 0.3, tracing-appender 0.2 (NEW), dirs 5
**Storage**: rusqlite 0.38 (existing — vox.db), JSON settings (existing — settings.json)
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows (CUDA RTX 4090) + macOS (Metal M4 Pro)
**Project Type**: Three-crate workspace (vox binary, vox_core lib, vox_ui lib)
**Performance Goals**: Window appearance <100ms, state initialization <50ms, theme initialization <1ms
**Constraints**: No web tech, single static binary, zero compiler warnings, pure Rust/GPUI

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Evidence |
|---|-----------|--------|----------|
| I | Local-Only Processing | PASS | No new network calls. Model download uses existing `ModelDownloader`. No telemetry added. |
| II | Real-Time Latency Budget | PASS | Window opens in <100ms before any ML loading. Pipeline init is async background work. No blocking on audio callback thread. |
| III | Full Pipeline — No Fallbacks | PASS | All pipeline components (VAD, ASR, LLM, injector) remain required. App stays in Downloading/Loading state until all are ready. No degraded modes. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | All new code is Rust. UI uses GPUI `div()` builder. No JS/HTML/CSS/WebView. |
| V | Zero-Click First Launch | PASS | Models auto-download on launch. No setup wizards or confirmation dialogs. Hotkey responds in every app state. |
| VI | Scope Only Increases | PASS | Adding new capability (app shell). No features removed or deferred. |
| VII | Public API Documentation | PASS | All `pub` items will have `///` doc comments. Module-level `//!` docs on all modules. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. No `#[ignore]` or conditional compilation guards. |
| IX | Explicit Commit Only | PASS | No auto-commits. User controls all git operations. |
| X | No Deferral | PASS | All requirements addressed in this plan. No items deferred to later phases. |
| XI | No Optional Compilation | PASS | `tray-icon` and `global-hotkey` are required dependencies (not optional). `tracing-appender` is required. Only `cuda`/`metal` feature flags remain for platform backends. |

**Post-design re-check**: All gates still pass. No violations introduced during Phase 1 design.

## Project Structure

### Documentation (this feature)

```text
specs/011-gpui-app-shell/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0 research decisions
├── data-model.md        # Entity definitions
├── quickstart.md        # Verification scenarios
├── contracts/
│   └── public-api.md    # Public API contracts
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (repository root)

```text
Cargo.toml                              # Add tracing-appender to workspace deps
crates/
├── vox/
│   ├── Cargo.toml                      # Add tray-icon, global-hotkey deps
│   └── src/
│       └── main.rs                     # IMPLEMENT: Full app entry point
├── vox_core/
│   ├── Cargo.toml                      # Add tracing-appender dep
│   └── src/
│       ├── vox_core.rs                 # ADD: pub mod logging
│       └── logging.rs                  # IMPLEMENT: Logging init + cleanup
└── vox_ui/
    ├── Cargo.toml                      # No changes needed
    └── src/
        ├── vox_ui.rs                   # No changes (modules already declared)
        ├── theme.rs                    # IMPLEMENT: VoxTheme + ThemeColors
        ├── layout.rs                   # IMPLEMENT: spacing, radius, size
        ├── key_bindings.rs             # IMPLEMENT: actions + key bindings
        └── overlay_hud.rs             # IMPLEMENT: Overlay window view
assets/
└── icons/                              # IMPLEMENT: Tray icon PNGs (embedded)
```

**Structure Decision**: Existing three-crate workspace. New code goes into existing empty stubs in `vox_ui/src/` and the stub `vox/src/main.rs`. One new module (`logging.rs`) added to `vox_core/src/`. No new crates or structural changes.

### Files Changed Summary

| File | Action | FR Coverage |
|------|--------|-------------|
| `Cargo.toml` (root) | Modify — add `tracing-appender` workspace dep | FR-007 |
| `crates/vox/Cargo.toml` | Modify — add `tray-icon`, `global-hotkey` deps | FR-010, FR-011, FR-012 |
| `crates/vox/src/main.rs` | Rewrite — full app entry point | FR-001, FR-002, FR-003, FR-004, FR-008, FR-009, FR-010, FR-011, FR-012, FR-013, FR-014 |
| `crates/vox_core/Cargo.toml` | Modify — add `tracing-appender` dep | FR-007 |
| `crates/vox_core/src/vox_core.rs` | Modify — add `pub mod logging` | FR-007 |
| `crates/vox_core/src/logging.rs` | Create — logging init, log dir, cleanup | FR-007, FR-015 |
| `crates/vox_ui/src/theme.rs` | Implement — VoxTheme, ThemeColors, dark() | FR-005 |
| `crates/vox_ui/src/layout.rs` | Implement — spacing, radius, size modules | FR-006 |
| `crates/vox_ui/src/key_bindings.rs` | Implement — actions!, register_actions(), register_key_bindings() | FR-003, FR-004 |
| `crates/vox_ui/src/overlay_hud.rs` | Implement — OverlayHud with Render trait | FR-001, FR-009 |

### Key GPUI Patterns Used

| Pattern | API | Reference |
|---------|-----|-----------|
| Global state | `cx.set_global(state)` / `cx.global::<T>()` | Zed `app.rs:1500`, Tusk `main.rs:26` |
| Entity creation | `cx.new(\|cx\| OverlayHud::new(cx))` | Zed `app.rs`, Tusk `main.rs:72` |
| Window opening | `cx.open_window(options, \|window, cx\| ...)` | Zed `app.rs:946`, Tusk `main.rs:62` |
| Async spawn | `cx.spawn(\|cx\| async move { ... }).detach()` | Zed `main.rs:689` |
| Action registration | `cx.on_action(\|_: &Quit, cx\| cx.quit())` | Tusk `main.rs:87` |
| Key bindings | `cx.bind_keys([KeyBinding::new(...)])` | Tusk `key_bindings.rs:132` |
| Window close | `window.on_window_should_close(cx, \|_, cx\| { cx.defer(\|cx\| cx.quit()); false })` | Tusk `main.rs:66` |
| PopUp window | `WindowOptions { kind: WindowKind::PopUp, ... }` | Zed `platform.rs:1269` |

### Dependency Changes

| Crate | Version | Location | Reason |
|-------|---------|----------|--------|
| tracing-appender | 0.2 | Workspace + vox | Daily rotating log files (FR-007) |

All other dependencies already exist in workspace/crate Cargo.toml files:
- gpui, tracing, tracing-subscriber, dirs 5, anyhow, tokio, serde — all present in workspace/crate deps.
- tray-icon 0.19, global-hotkey 0.6 — present in vox_core/Cargo.toml; ALSO needed in vox/Cargo.toml since main.rs uses them directly for system tray and hotkey setup.

## Complexity Tracking

> No Constitution Check violations. No complexity justifications needed.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none) | — | — |
