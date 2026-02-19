# Feature 001: Workspace Scaffolding

**Status:** Not Started
**Dependencies:** None (first feature)
**Design Reference:** Sections 6 (Project Structure), 7 (Build System), 3 (Technology Stack)
**Estimated Scope:** Cargo workspace, crate structure, all Cargo.toml files, build configuration

---

## Overview

Set up the three-crate Cargo workspace that forms the foundation for every subsequent feature. This is pure project structure — no application logic, no UI, no ML. By the end, `cargo build` and `cargo test` must succeed on both platforms with all three crates compiling (as empty shells with correct dependency declarations).

---

## Requirements

### FR-001: Cargo Workspace Root

Create `Cargo.toml` at repository root defining the workspace:

```toml
[workspace]
members = ["crates/vox", "crates/vox_core", "crates/vox_ui"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "TBD" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1.49", features = ["rt-multi-thread", "sync", "time", "macros"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
parking_lot = "0.12"
uuid = { version = "1", features = ["v4", "serde"] }

[profile.release]
opt-level = "s"
lto = true
strip = "symbols"
codegen-units = 1
```

**GPUI rev pinning:** The exact GPUI revision must be determined by testing against the Zed repo at `D:\SRC\zed`. Tusk uses rev `89e9ab97aa5d978351ee8a28d9cc35c272c530f5` as a known-good starting point. Verify this rev compiles on Windows before committing.

### FR-002: vox Crate (Binary Entry Point)

`crates/vox/Cargo.toml`:

```toml
[package]
name = "vox"
version.workspace = true
edition.workspace = true

[[bin]]
name = "vox"
path = "src/main.rs"

[dependencies]
vox_core = { path = "../vox_core" }
vox_ui = { path = "../vox_ui" }
gpui.workspace = true
serde.workspace = true
tokio.workspace = true
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

`crates/vox/src/main.rs` — minimal shell:

```rust
fn main() {
    println!("Vox — voice dictation engine");
}
```

### FR-003: vox_core Crate (Backend)

`crates/vox_core/Cargo.toml`:

```toml
[package]
name = "vox_core"
version.workspace = true
edition.workspace = true

[features]
default = []
cuda = ["whisper-rs/cuda", "llama-cpp-2/cuda"]
metal = ["whisper-rs/metal", "llama-cpp-2/metal"]

[dependencies]
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
anyhow.workspace = true
tracing.workspace = true
parking_lot.workspace = true
uuid.workspace = true

# Audio
cpal = "0.17"
ringbuf = "0.4"
rubato = "1.0"

# ML
ort = { version = "2.0.0-rc.11", features = ["load-dynamic"] }
whisper-rs = "0.15.1"
llama-cpp-2 = "0.1"

# Platform — Windows
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.62", features = [
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
] }

# Platform — macOS
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-core-graphics = "0.3"

# Storage & networking
rusqlite = { version = "0.38", features = ["bundled"] }
reqwest = { version = "0.13", features = ["stream"] }

# Hotkey & tray
global-hotkey = "0.6"
tray-icon = "0.19"
```

`crates/vox_core/src/lib.rs` — module stubs:

```rust
pub mod audio;
pub mod vad;
pub mod asr;
pub mod llm;
pub mod injector;
pub mod pipeline;
pub mod dictionary;
pub mod config;
pub mod models;
pub mod hotkey;
pub mod state;
```

Each submodule starts as an empty `mod.rs` file.

### FR-004: vox_ui Crate (GPUI Components)

`crates/vox_ui/Cargo.toml`:

```toml
[package]
name = "vox_ui"
version.workspace = true
edition.workspace = true

[dependencies]
gpui.workspace = true
vox_core = { path = "../vox_core" }
serde.workspace = true
parking_lot.workspace = true
smallvec = { version = "1.11", features = ["union"] }
```

`crates/vox_ui/src/lib.rs` — module stubs:

```rust
pub mod theme;
pub mod layout;
pub mod overlay_hud;
pub mod waveform;
pub mod workspace;
pub mod settings_panel;
pub mod history_panel;
pub mod dictionary_panel;
pub mod model_panel;
pub mod log_panel;
pub mod text_input;
pub mod button;
pub mod icon;
pub mod key_bindings;
```

### FR-005: Directory Structure

Create the full directory tree from design doc section 6:

```
vox/
├── Cargo.toml
├── Cargo.lock
├── CLAUDE.md
├── crates/
│   ├── vox/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   ├── vox_core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs
│   │       ├── pipeline/
│   │       │   └── mod.rs
│   │       ├── audio/
│   │       │   └── mod.rs
│   │       ├── vad/
│   │       │   └── mod.rs
│   │       ├── asr/
│   │       │   └── mod.rs
│   │       ├── llm/
│   │       │   └── mod.rs
│   │       ├── injector/
│   │       │   └── mod.rs
│   │       ├── dictionary/
│   │       │   └── mod.rs
│   │       ├── config/
│   │       │   └── mod.rs
│   │       ├── hotkey.rs
│   │       └── models.rs
│   └── vox_ui/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
├── assets/
│   └── icons/
├── tests/
│   ├── audio_fixtures/
│   ├── test_vad.rs
│   ├── test_asr.rs
│   ├── test_llm.rs
│   ├── test_injector.rs
│   └── test_pipeline_e2e.rs
└── scripts/
    ├── download-models.sh
    └── download-models.ps1
```

### FR-006: Build Verification

Both commands must succeed with zero warnings:

```bash
# Windows
cargo build -p vox --features vox_core/cuda
cargo test -p vox_core --features cuda

# macOS
cargo build -p vox --features vox_core/metal
cargo test -p vox_core --features metal
```

### FR-007: Environment Variable Documentation

Document required environment variables for Windows CUDA builds:

- `CMAKE_GENERATOR=Visual Studio 17 2022` — CUDA does not support VS 18 Insiders
- `CUDA_PATH` — Points to CUDA Toolkit installation

Both must be persistent user environment variables (not session-scoped).

### FR-008: .gitignore

Extend `.gitignore` to cover:

```
/target
/models/
*.onnx
*.bin
*.gguf
.env
*.log
```

Models are downloaded at runtime, never committed.

---

## Dependency Verification

Before marking this feature complete, verify these crate versions resolve and compile:

| Crate | Version | Verification |
|---|---|---|
| gpui | git rev TBD | `cargo build -p vox_ui` succeeds |
| cpal | 0.17 | `cargo build -p vox_core` succeeds |
| ringbuf | 0.4 | Same |
| rubato | 1.0 | Same |
| ort | 2.0.0-rc.11 | Same |
| whisper-rs | 0.15.1 | Same (Codeberg source) |
| llama-cpp-2 | 0.1 | Same (utilityai, NOT llama-cpp-rs) |
| windows | 0.62 | Windows only |
| objc2 | 0.6 | macOS only |
| rusqlite | 0.38 | Same |
| reqwest | 0.13 | Same |
| tokio | 1.49 | Same |
| global-hotkey | 0.6 | Same |
| tray-icon | 0.19 | Same |

**Critical:** `llama-cpp-2` is the `utilityai` crate, NOT `llama-cpp-rs` 0.4 — these are completely different crates with incompatible APIs. The `whisper-rs` crate is from Codeberg, not crates.io.

---

## Acceptance Criteria

- [ ] Workspace compiles on Windows with `--features vox_core/cuda`
- [ ] Workspace compiles on macOS with `--features vox_core/metal`
- [ ] Zero compiler warnings
- [ ] All three crates have correct inter-dependencies
- [ ] `cargo test` passes (empty test suite is acceptable)
- [ ] Directory structure matches design doc section 6
- [ ] `.gitignore` excludes models, target, logs

---

## Testing Requirements

- `cargo build` with each feature flag succeeds
- `cargo test` passes on both platforms
- Incremental rebuild after touching a `.rs` file completes in < 10 seconds

---

## Performance Targets

| Metric | Target |
|---|---|
| Clean build | < 5 minutes |
| Incremental build | < 10 seconds |
| Binary size (empty shell, release) | < 15 MB |
