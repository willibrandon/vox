# Implementation Plan: System Tray & Global Hotkeys

**Branch**: `014-tray-hotkeys` | **Date**: 2026-02-24 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/014-tray-hotkeys/spec.md`

## Summary

Extend the existing system tray and global hotkey infrastructure (from 011-gpui-app-shell) with three activation modes (hold-to-talk, toggle, hands-free), dynamic tray icon states reflecting pipeline readiness, an expanded six-item context menu, runtime hotkey remapping, and universal hotkey response in every application state. Default hotkey remains Ctrl+Shift+Space.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: gpui (git rev 89e9ab97), tray-icon 0.19, global-hotkey 0.6, png 0.17, windows 0.62
**Storage**: JSON (settings.json) — activation mode persisted as kebab-case string
**Testing**: cargo test -p vox_core (unit tests for HotkeyInterpreter)
**Target Platform**: Windows (CUDA) + macOS (Metal)
**Project Type**: Three-crate Rust workspace (vox, vox_core, vox_ui)
**Performance Goals**: Hotkey event → action < 5ms, tray icon update < 10ms, double-press window = 300ms
**Constraints**: Zero compiler warnings, all tests pass unconditionally
**Scale/Scope**: ~500 new lines across 6 files, 2 new modules, 1 new asset

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|-----------|--------|-------|
| I | Local-Only Processing | PASS | No network calls added |
| II | Real-Time Latency Budget | PASS | Hotkey < 5ms (polling reduced to 5ms), tray < 10ms |
| III | Full Pipeline — No Fallbacks | PASS | No pipeline changes; all components remain required |
| IV | Pure Rust / GPUI — No Web Tech | PASS | tray-icon and global-hotkey are Rust FFI wrappers |
| V | Zero-Click First Launch | PASS | Hotkey responds in every state with visible overlay feedback |
| VI | Scope Only Increases | PASS | Adding 3 activation modes, 3 menu items, dynamic icons |
| VII | Public API Documentation | PASS | All new pub items will have `///` doc comments |
| VIII | No Test Skipping | PASS | Unit tests for HotkeyInterpreter run unconditionally |
| IX | Explicit Commit Only | PASS | No auto-commits |
| X | No Deferral | PASS | All 24 functional requirements addressed in this plan |
| XI | No Optional Compilation | PASS | No feature flags added; platform code uses `#[cfg(target_os)]` |
| XII | No Blame Attribution | PASS | N/A |
| XIII | No Placeholders | PASS | All code will be real, working implementation |

**Post-Phase 1 re-check**: All gates remain PASS. No new concerns from data model or contract design.

## Project Structure

### Documentation (this feature)

```text
specs/014-tray-hotkeys/
├── spec.md
├── plan.md              # This file
├── research.md          # Phase 0: key technical decisions
├── data-model.md        # Phase 1: entities and state machines
├── quickstart.md        # Phase 1: developer verification guide
├── contracts/
│   └── hotkey-interpreter.md  # Internal API contract
└── tasks.md             # Phase 2 (/speckit.tasks — not created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/vox_core/src/
  hotkey_interpreter.rs          # NEW: ActivationMode, HotkeyAction, HotkeyInterpreter
  config.rs                      # MODIFIED: replace bool fields with activation_mode

crates/vox/src/
  main.rs                        # MODIFIED: activation mode dispatch, dynamic tray,
                                 #           expanded menu, universal hotkey response,
                                 #           5ms polling, press/release handling
  tray.rs                        # NEW: TrayIconState, TrayUpdate, icon decoding,
                                 #       state derivation, menu creation

crates/vox_ui/src/
  settings_panel.rs              # MODIFIED: activation mode dropdown replaces bool toggles

assets/icons/
  tray-downloading.png           # NEW: 32×32 RGBA orange icon for download state

crates/vox_core/tests/
  hotkey_interpreter_tests.rs    # NEW: unit tests for all activation modes
```

**Structure Decision**: Extends the existing three-crate workspace. `hotkey_interpreter.rs` in vox_core keeps the state machine testable and OS-independent. `tray.rs` in vox extracts tray management from main.rs as it grows from a static icon to a state-reactive system.

## Complexity Tracking

No Constitution violations. No complexity justifications needed.
