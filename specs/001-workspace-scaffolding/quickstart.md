# Quickstart: Workspace Scaffolding

**Branch**: `001-workspace-scaffolding` | **Date**: 2026-02-19

## Prerequisites

### Both Platforms
- Rust 1.85+ (`rustup update`)
- CMake 4.0+

### Windows
- Visual Studio 2022 Build Tools (C++ workload)
- CUDA Toolkit 12.8+ with cuDNN 9.x
- Environment variables (persistent user-level, not session-scoped):
  ```
  CMAKE_GENERATOR=Visual Studio 17 2022
  CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8
  ```
- **Warning**: CUDA does not support VS 18 Insiders. `CMAKE_GENERATOR` must point to VS 17 2022.

### macOS
- Xcode 26.x + Command Line Tools
- Metal Toolchain component: `xcodebuild -downloadComponent MetalToolchain`

## Build Commands

```bash
# Windows — build with CUDA
cargo build -p vox --features vox_core/cuda

# macOS — build with Metal
cargo build -p vox --features vox_core/metal

# Run tests (Windows)
cargo test -p vox_core --features cuda

# Run tests (macOS)
cargo test -p vox_core --features metal

# Release build (Windows)
cargo build --release -p vox --features vox_core/cuda
```

## Verification

After building, verify:

1. **Zero warnings**: Build output should show no compiler warnings.
2. **Binary exists**: `target/debug/vox` (or `vox.exe` on Windows) is produced.
3. **Tests pass**: `cargo test` reports success (empty test suites are expected at this stage).
4. **Incremental build**: Modify any `.rs` file, rebuild — should complete in under 10 seconds.

## Project Layout

```
vox/
├── Cargo.toml              # Workspace root
├── Cargo.lock              # Generated on first build
├── CLAUDE.md               # AI agent instructions
├── crates/
│   ├── vox/                # Binary — minimal shell
│   ├── vox_core/           # Backend — 11 module stubs
│   └── vox_ui/             # UI — 14 module stubs
├── assets/icons/           # Icon assets
├── tests/                  # Integration test stubs
│   └── audio_fixtures/     # Test audio files
└── scripts/                # Model download scripts
```

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| CUDA compilation fails | Wrong `CMAKE_GENERATOR` | Set to `Visual Studio 17 2022` (not VS 18) |
| Linker errors on Windows | Missing VS Build Tools | Install C++ workload from VS Installer |
| GPUI fails to resolve | Git network issue | Check internet connection, retry `cargo build` |
| `whisper-rs` build error | Missing CMake | Install CMake 4.0+ and ensure it's on PATH |
| macOS code signing error | Xcode not configured | Run `xcode-select --install` |
| Metal shader compilation fails | Missing Metal toolchain | Run `xcodebuild -downloadComponent MetalToolchain` |
| `core-text`/`core-graphics` version conflict | GPUI font-kit dependency split | Pin `core-text = "=21.0.0"` and `core-graphics = "=0.24.0"` in macOS deps |
