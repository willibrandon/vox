# Implementation Plan: Workspace Scaffolding

**Branch**: `001-workspace-scaffolding` | **Date**: 2026-02-19 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-workspace-scaffolding/spec.md`

## Summary

Create the three-crate Cargo workspace (`vox`, `vox_core`, `vox_ui`) with all dependency declarations, feature flags (cuda/metal), module stubs (11 backend + 14 UI), and supporting directory structure. No application logic — pure build foundation. Must compile with zero warnings on Windows (CUDA) and macOS (Metal).

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: GPUI (git rev 89e9ab97), cpal 0.17, whisper-rs 0.15.1, llama-cpp-2 0.1, ort 2.0.0-rc.11
**Storage**: N/A (scaffolding only, no runtime storage)
**Testing**: cargo test (empty test suites acceptable)
**Target Platform**: Windows 10+ (CUDA 12.8+), macOS (Metal via Xcode 26.x)
**Project Type**: Three-crate Cargo workspace (binary + 2 libraries)
**Performance Goals**: Clean build < 5 min, incremental rebuild < 10s, release binary < 15 MB
**Constraints**: Zero compiler warnings, no web dependencies, Rust edition 2024, resolver v2
**Scale/Scope**: 3 crates, 11 vox_core modules, 14 vox_ui modules, ~30 external dependencies

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Rationale |
|-----------|--------|-----------|
| I. Local-Only Processing | PASS | No network calls. Dependency declarations only. |
| II. Real-Time Latency Budget | N/A | No runtime code in this feature. Stubs only. |
| III. Full Pipeline — No Fallbacks | N/A | No pipeline code. Module stubs declare the structure for all pipeline components (audio, vad, asr, llm, injector, pipeline). |
| IV. Pure Rust / GPUI — No Web Tech | PASS | Rust-only workspace. GPUI declared as dependency. No JS/TS/HTML/CSS. |
| V. Zero-Click First Launch | N/A | No first-launch behavior. |
| VI. Scope Only Increases | PASS | All 11 vox_core modules and 14 vox_ui modules from the design doc are present. No modules omitted. |

**Gate result: PASS** — No violations. Proceed to Phase 0.

## Project Structure

### Documentation (this feature)

```text
specs/001-workspace-scaffolding/
├── plan.md              # This file
├── research.md          # Phase 0: dependency verification
├── data-model.md        # Phase 1: crate/module structure
├── quickstart.md        # Phase 1: developer setup guide
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
crates/
├── vox/                          # Binary entry point
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── vox_core/                     # Backend library
│   ├── Cargo.toml
│   └── src/
│       ├── vox_core.rs           # Lib entry point ([lib] path)
│       ├── audio.rs              # Audio capture pipeline stub
│       ├── vad.rs                # Voice activity detection stub
│       ├── asr.rs                # Speech recognition stub
│       ├── llm.rs                # LLM post-processing stub
│       ├── injector.rs           # Text injection stub
│       ├── pipeline.rs           # Pipeline orchestration stub
│       ├── dictionary.rs         # Custom dictionary stub
│       ├── config.rs             # Configuration stub
│       ├── models.rs             # Model management stub
│       ├── hotkey.rs             # Hotkey handling stub
│       └── state.rs              # Application state stub
└── vox_ui/                       # GPUI UI library
    ├── Cargo.toml
    └── src/
        ├── vox_ui.rs             # Lib entry point ([lib] path)
        ├── theme.rs              # Theme system stub
        ├── layout.rs             # Layout primitives stub
        ├── overlay_hud.rs        # Overlay HUD stub
        ├── waveform.rs           # Waveform visualizer stub
        ├── workspace.rs          # Workspace container stub
        ├── settings_panel.rs     # Settings panel stub
        ├── history_panel.rs      # History panel stub
        ├── dictionary_panel.rs   # Dictionary editor stub
        ├── model_panel.rs        # Model manager stub
        ├── log_panel.rs          # Log viewer stub
        ├── text_input.rs         # Text input component stub
        ├── button.rs             # Button component stub
        ├── icon.rs               # Icon component stub
        └── key_bindings.rs       # Key bindings stub

assets/
└── icons/                        # Icon assets directory

tests/
├── audio_fixtures/               # Test audio files
├── test_vad.rs                   # VAD integration test stub
├── test_asr.rs                   # ASR integration test stub
├── test_llm.rs                   # LLM integration test stub
├── test_injector.rs              # Injector integration test stub
└── test_pipeline_e2e.rs          # End-to-end pipeline test stub

scripts/
├── download-models.sh            # Model download (Unix)
└── download-models.ps1           # Model download (Windows)
```

**Structure Decision**: Three-crate Cargo workspace matching the Tusk reference architecture. Modern module convention per clarification (no `mod.rs` files). Named library entry points (`vox_core.rs`, `vox_ui.rs`) per CLAUDE.md convention.

## Complexity Tracking

No constitution violations detected. Table not applicable.
