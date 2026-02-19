# Data Model: Workspace Scaffolding

**Branch**: `001-workspace-scaffolding` | **Date**: 2026-02-19

This feature has no traditional data entities (no database, no API). The "data model" is the crate dependency graph and module structure.

## Crate Dependency Graph

```
vox (binary)
├── depends on: vox_core (path)
├── depends on: vox_ui (path)
└── depends on: gpui, serde, tokio, anyhow, tracing, tracing-subscriber

vox_ui (library)
├── depends on: vox_core (path)
├── depends on: gpui, serde, parking_lot, smallvec
└── 14 public modules

vox_core (library)
├── feature flags: cuda, metal
├── depends on: serde, serde_json, tokio, anyhow, tracing, parking_lot, uuid
├── depends on: cpal, ringbuf, rubato (audio)
├── depends on: ort, whisper-rs, llama-cpp-2 (ML)
├── depends on: windows [cfg(windows)]
├── depends on: objc2, objc2-core-graphics [cfg(macos)]
├── depends on: rusqlite, reqwest (storage/network)
├── depends on: global-hotkey, tray-icon (system integration)
└── 11 public modules
```

## Module Inventory

### vox_core (11 modules)

| Module | Future Responsibility | Will Need Submodules |
|--------|----------------------|---------------------|
| `audio` | Audio capture, ring buffer, resampling | Yes (capture, buffer, resample) |
| `vad` | Voice activity detection (Silero ONNX) | Yes (engine, state_machine) |
| `asr` | Whisper ASR engine | Yes (engine, streaming) |
| `llm` | LLM post-processing (llama.cpp) | Yes (engine, prompt) |
| `injector` | OS-level text injection | Yes (platform-specific) |
| `pipeline` | Pipeline orchestration | Yes (state, channels) |
| `dictionary` | Custom dictionary storage | Possibly |
| `config` | Settings and configuration | Possibly |
| `models` | Model download and management | Yes (download, validation) |
| `hotkey` | Global hotkey handling | No (single-file) |
| `state` | Application state | No (single-file) |

### vox_ui (14 modules)

| Module | Future Responsibility | Will Need Submodules |
|--------|----------------------|---------------------|
| `theme` | Color palette, theming | No |
| `layout` | Spacing, sizing constants | No |
| `overlay_hud` | Overlay HUD window | Possibly |
| `waveform` | Custom waveform element | No |
| `workspace` | Main workspace container | Yes |
| `settings_panel` | Settings UI | Possibly |
| `history_panel` | Transcript history | Possibly |
| `dictionary_panel` | Dictionary editor UI | Possibly |
| `model_panel` | Model manager UI | Possibly |
| `log_panel` | Log viewer UI | No |
| `text_input` | Text input component | No |
| `button` | Button component | No |
| `icon` | Icon enum and component | No |
| `key_bindings` | Action definitions | No |

## Workspace Dependency Sharing

All shared dependencies use `[workspace.dependencies]` with `.workspace = true` in member crates:

| Dependency | Used By | Notes |
|------------|---------|-------|
| gpui | vox, vox_ui | Git dependency, pinned rev |
| serde | vox, vox_core, vox_ui | With `derive` feature |
| serde_json | vox_core | — |
| tokio | vox, vox_core | Multi-thread runtime |
| anyhow | vox, vox_core | Error handling |
| tracing | vox, vox_core | Logging |
| tracing-subscriber | vox | With env-filter |
| parking_lot | vox_core, vox_ui | Sync primitives |
| uuid | vox_core | With v4, serde features |
| smallvec | vox_ui | With union feature |

## Feature Flag Forwarding

```
vox_core::cuda  → whisper-rs/cuda + llama-cpp-2/cuda
vox_core::metal → whisper-rs/metal + llama-cpp-2/metal
```

Enabled from the binary crate via: `cargo build -p vox --features vox_core/cuda`
