# Implementation Plan: Audio Debug Tap

**Branch**: `016-audio-debug-tap` | **Date**: 2026-02-27 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/016-audio-debug-tap/spec.md`
**Design Document**: [docs/audio-debug-tap-plan.md](../../docs/audio-debug-tap-plan.md)

## Summary

Save WAV files of captured audio at four pipeline stages (raw capture, post-resample, VAD segment, ASR input) for debugging audio quality, VAD boundary detection, resampling artifacts, and ASR input issues. A three-level setting (Off/Segments/Full) controls which tap points are active. Audio is written by a background thread via a bounded channel (256 messages, try_send, never blocks pipeline). Files are auto-cleaned (24h age, 500 MB cap).

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: hound 3.5 (WAV I/O, promoted from dev-dependencies), std::sync::mpsc (bounded channel), tokio::sync::broadcast (error notification via PipelineState)
**Storage**: WAV files in `data_dir/debug_audio/` (e.g. `%LOCALAPPDATA%/com.vox.app/debug_audio/`). Creation-time-based cleanup. No database involvement.
**Testing**: `cargo test -p vox_core --features cuda` (Windows) / `cargo test -p vox_core --features metal` (macOS). Unit tests in `audio/debug_tap.rs` module.
**Target Platform**: Windows (NTFS) + macOS (APFS). Both support `Metadata::created()` for file creation time.
**Project Type**: Three-crate Rust workspace (vox, vox_core, vox_ui)
**Performance Goals**: Zero latency impact on pipeline (all I/O on background thread, try_send ~1ns when Off, ~1-5μs when active). SC-003: ≤1ms delta over 100 utterances.
**Constraints**: 1 MB memory ceiling (bounded channel 256 × ~4 KB), 500 MB disk cap, 15 MB binary budget (feature adds ~15-20 KB = 0.1%). No blocking audio callback thread. No optional compilation (#[cfg] forbidden by constitution).
**Scale/Scope**: ~375 lines total (production + test in same module). 1 new file, 7 modified files.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|-----------|--------|-------|
| I | Local-Only Processing | PASS | All audio stays on disk locally. No network calls. WAV files written to local `data_dir/debug_audio/`. |
| II | Real-Time Latency Budget | PASS | try_send is non-blocking (~1ns atomic check when Off). Writer runs on background std::thread. Pipeline never waits for disk I/O. SC-003 verifies ≤1ms delta. |
| III | Full Pipeline — No Fallbacks | PASS | Debug tap is additive instrumentation, not a pipeline component. Pipeline operates identically with or without taps. No degraded modes introduced. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | Pure Rust implementation. hound is a Rust WAV library. Settings dropdown uses existing GPUI Select pattern. No web dependencies. |
| V | Zero-Click First Launch | PASS | Debug audio defaults to Off (FR-009). No setup required. No new first-launch steps. |
| VI | Scope Only Increases | PASS | Feature adds 4 tap points, 3-level setting, storage management, UI dropdown. No existing features removed or reduced. |
| VII | Public API Documentation | PASS | All `pub` items on `DebugAudioTap`, `DebugAudioLevel`, and public methods will have `///` doc comments. |
| VIII | No Test Skipping | PASS | All 9 unit tests + existing orchestrator tests run unconditionally. No `#[ignore]`, no `#[cfg(skip)]`. |
| IX | Explicit Commit Only | PASS | No commits without user instruction. |
| X | No Deferral | PASS | All 23 functional requirements implemented in a single pass. No items deferred. |
| XI | No Optional Compilation | PASS | hound promoted to required `[dependencies]`. No `optional = true`. No `#[cfg(feature)]` guards. Code always compiled. |
| XII | No Blame Attribution | PASS | N/A — new feature, no pre-existing code to blame. |
| XIII | No Placeholders | PASS | Every function will have real, working implementation. No `todo!()`, no stub returns. |

**Result**: All 13 gates PASS. No violations to track.

## Project Structure

### Documentation (this feature)

```text
specs/016-audio-debug-tap/
├── spec.md              # Feature specification (complete)
├── plan.md              # This file
├── research.md          # Phase 0: research findings
├── data-model.md        # Phase 1: entity/type definitions
├── quickstart.md        # Phase 1: implementation guide
├── contracts/           # Phase 1: API contracts
│   └── debug_tap_api.md # DebugAudioTap public interface
└── checklists/
    └── requirements.md  # Spec quality checklist (complete)
```

### Source Code (repository root)

```text
crates/vox_core/
├── Cargo.toml                          # MODIFY: hound dev-dep → dep
├── src/
│   ├── audio.rs                        # MODIFY: add `pub mod debug_tap;` declaration
│   ├── audio/
│   │   └── debug_tap.rs                # NEW: DebugAudioTap struct + writer thread + tests
│   ├── config.rs                       # MODIFY: add DebugAudioLevel enum + Settings field
│   ├── vad.rs                          # MODIFY: add tap calls to run_vad_loop + run_passthrough_loop
│   ├── pipeline/
│   │   └── orchestrator.rs             # MODIFY: add debug_tap field, tap_asr_input call, channel type change
│   └── state.rs                        # MODIFY: add debug_tap field to VoxState

crates/vox/
└── src/
    └── main.rs                         # MODIFY: create DebugAudioTap during init, pass to pipeline

crates/vox_ui/
└── src/
    └── settings_panel.rs               # MODIFY: add Debug Audio dropdown in Advanced section
```

**Structure Decision**: Existing three-crate workspace. New module `audio/debug_tap.rs` in vox_core alongside existing `audio/capture.rs`, `audio/resampler.rs`, `audio/ring_buffer.rs`. No new crates. The `audio.rs` module file already declares submodules — adding `pub mod debug_tap;` follows the established pattern.

## Complexity Tracking

> No constitution violations. Table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| *(none)* | | |
