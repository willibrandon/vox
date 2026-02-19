# Feature Specification: Workspace Scaffolding

**Feature Branch**: `001-workspace-scaffolding`
**Created**: 2026-02-19
**Status**: Draft
**Input**: Three-crate Cargo workspace forming the build foundation for all subsequent features

## Clarifications

### Session 2026-02-19

- Q: Module file convention — `mod.rs` (feature description) vs modern style `module.rs` (CLAUDE.md)? → A: Modern style — use `audio.rs` as module root, with `audio/` subdirectory for child modules when needed. No `mod.rs` files.
- Q: Library entry point naming — `lib.rs` (feature description) vs descriptive name (CLAUDE.md)? → A: Named files — `vox_core.rs` and `vox_ui.rs` with `[lib] path` in Cargo.toml. Matches GPUI convention.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Build the Project on Windows (Priority: P1)

A developer clones the Vox repository on Windows and runs `cargo build` with
GPU acceleration enabled. The three-crate workspace resolves all dependencies,
compiles with zero warnings, and produces a runnable binary — without requiring
any manual setup beyond standard prerequisites (Rust, CMake, CUDA, Visual
Studio Build Tools).

**Why this priority**: Every subsequent feature depends on a workspace that
compiles. If the foundation doesn't build, nothing else can proceed.

**Independent Test**: Run `cargo build -p vox --features vox_core/cuda` on a
clean checkout on Windows. Success means a binary is produced with zero
compiler warnings.

**Acceptance Scenarios**:

1. **Given** a fresh clone with Rust 1.85+, CMake 4.0+, CUDA 12.8+, and VS
   2022 Build Tools installed, **When** the developer runs
   `cargo build -p vox --features vox_core/cuda`, **Then** the build succeeds
   with zero warnings and produces a `vox` binary.
2. **Given** a successful build, **When** the developer runs `cargo test -p
   vox_core --features cuda`, **Then** the test harness runs (empty suite is
   acceptable) and reports success.
3. **Given** a successful first build, **When** the developer modifies any
   `.rs` file and rebuilds, **Then** the incremental rebuild completes in under
   10 seconds.

---

### User Story 2 - Build the Project on macOS (Priority: P1)

A developer clones the Vox repository on macOS and runs `cargo build` with
Metal acceleration enabled. The same workspace compiles cross-platform without
conditional hacks or manual flag toggling.

**Why this priority**: macOS is a first-class target. The workspace must
compile on both platforms from day one to prevent platform-specific drift.

**Independent Test**: Run `cargo build -p vox --features vox_core/metal` on
macOS with Xcode 26.x installed.

**Acceptance Scenarios**:

1. **Given** a fresh clone on macOS with Rust 1.85+, CMake 4.0+, and Xcode
   26.x installed, **When** the developer runs
   `cargo build -p vox --features vox_core/metal`, **Then** the build succeeds
   with zero warnings.
2. **Given** a successful build, **When** the developer runs `cargo test -p
   vox_core --features metal`, **Then** the test harness reports success.

---

### User Story 3 - Add Code to a Specific Crate (Priority: P2)

A developer begins implementing a new feature (e.g., audio capture) by adding
code to the appropriate crate. The workspace structure clearly separates
concerns: binary entry point, backend logic, and UI components are in distinct
crates with well-defined module boundaries.

**Why this priority**: Clear crate separation prevents tight coupling and
ensures parallel development. Developers need to know exactly where new code
belongs.

**Independent Test**: Open the project in an IDE and verify that each crate's
module structure matches the design document, with stub modules ready for
implementation.

**Acceptance Scenarios**:

1. **Given** the workspace is set up, **When** a developer adds a function to
   `vox_core/src/audio.rs`, **Then** it is accessible from the `vox` binary
   crate via `vox_core::audio::*` and from the `vox_ui` crate as well.
2. **Given** the workspace is set up, **When** a developer adds a UI component
   to `vox_ui/src/`, **Then** it is accessible from the `vox` binary crate via
   `vox_ui::*`.
3. **Given** the crate dependency graph (vox depends on vox_core and vox_ui;
   vox_ui depends on vox_core), **When** vox_core is modified, **Then** both
   vox_ui and vox rebuild; modifying vox_ui only triggers vox rebuild.

---

### User Story 4 - GPUI Rev Pin Verification (Priority: P2)

The workspace must pin a specific GPUI git revision that is verified to compile
on Windows. Tusk uses rev `89e9ab97aa5d978351ee8a28d9cc35c272c530f5` as a
known-good starting point, but this must be validated against actual
compilation.

**Why this priority**: GPUI is the UI framework for the entire app. A bad pin
causes all UI work to fail.

**Independent Test**: Build the `vox_ui` crate, which depends on GPUI, and
confirm the pinned revision compiles.

**Acceptance Scenarios**:

1. **Given** the workspace Cargo.toml pins a specific GPUI git revision,
   **When** the developer runs `cargo build -p vox_ui`, **Then** GPUI resolves
   and compiles successfully.
2. **Given** the pinned GPUI revision, **When** compared against the Tusk
   reference app, **Then** the revision is either the same known-good rev or a
   newer rev that has been verified to compile.

---

### Edge Cases

- What happens when CUDA is not installed but `--features vox_core/cuda` is
  passed? Build should fail with a clear error from the CUDA toolchain, not a
  cryptic linker error.
- What happens when both `cuda` and `metal` features are enabled
  simultaneously? This is an invalid configuration — build behavior is
  undefined (one platform only).
- What happens when `CMAKE_GENERATOR` is set to a VS version that CUDA doesn't
  support (e.g., VS 18 Insiders)? CUDA compilation fails. The environment
  variable documentation must warn about this.
- What happens when `llama-cpp-2` resolves to the wrong crate (`llama-cpp-rs`
  0.4 instead of `llama-cpp-2` 0.1)? Build may succeed but runtime behavior
  will be wrong. Cargo.toml must specify the correct crate name and version.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Workspace MUST define three crates — `vox` (binary), `vox_core`
  (library), and `vox_ui` (library) — as workspace members with shared version
  and edition settings.
- **FR-002**: The `vox` binary crate MUST depend on both `vox_core` and
  `vox_ui` via path dependencies. The `vox_ui` crate MUST depend on `vox_core`
  via path dependency.
- **FR-003**: The `vox_core` crate MUST declare feature flags for GPU
  acceleration: `cuda` (forwarded to `whisper-rs/cuda` and `llama-cpp-2/cuda`)
  and `metal` (forwarded to `whisper-rs/metal` and `llama-cpp-2/metal`).
- **FR-004**: All shared dependencies MUST be declared in the workspace root
  `[workspace.dependencies]` table and referenced via `.workspace = true` in
  member crates, ensuring version consistency.
- **FR-005**: Platform-specific dependencies MUST use `cfg` target gates:
  `windows` crate (Win32 features) for Windows, `objc2` + `objc2-core-graphics`
  for macOS.
- **FR-006**: The `vox_core` crate MUST declare module stubs for all pipeline
  components: audio, vad, asr, llm, injector, pipeline, dictionary, config,
  models, hotkey, and state. Modules MUST use modern Rust convention (e.g.,
  `audio.rs` as module root) — no `mod.rs` files.
- **FR-007**: The `vox_ui` crate MUST declare module stubs for all UI
  components: theme, layout, overlay_hud, waveform, workspace, settings_panel,
  history_panel, dictionary_panel, model_panel, log_panel, text_input, button,
  icon, and key_bindings. Same modern module convention as FR-006 — no
  `mod.rs` files.
- **FR-008**: The directory structure MUST include `assets/icons/`,
  `tests/audio_fixtures/`, top-level integration test files, and
  `scripts/` for model download scripts.
- **FR-009**: The `.gitignore` MUST exclude `/target`, `/models/`, model file
  extensions (`*.onnx`, `*.bin`, `*.gguf`), `.env`, and `*.log`.
- **FR-010**: The release profile MUST optimize for binary size (`opt-level =
  "s"`, LTO, symbol stripping, single codegen unit).
- **FR-011**: The workspace MUST use Rust edition 2024 and resolver version 2.

### Key Entities

- **Workspace Root (`Cargo.toml`)**: Defines the three workspace members,
  shared package metadata (version, edition, license), shared dependency
  versions, and release profile.
- **vox Crate**: Binary entry point. Depends on `vox_core` and `vox_ui`.
  Minimal shell that starts the application.
- **vox_core Crate**: Backend library (`src/vox_core.rs` entry point, declared
  via `[lib] path`). Contains all audio, ML, pipeline, and system integration
  modules. Feature-gated for CUDA/Metal GPU acceleration.
- **vox_ui Crate**: UI component library (`src/vox_ui.rs` entry point, declared
  via `[lib] path`). Contains all GPUI-based interface components. Depends on
  `vox_core` for state and data access.

### Dependency Pinning

All dependency versions specified in the feature description are binding.
Critical sourcing notes:

- `whisper-rs 0.15.1` — crates.io (source code on Codeberg)
- `llama-cpp-2 0.1` — the `utilityai` crate, NOT `llama-cpp-rs 0.4`
- `gpui` — pinned to a specific git revision from `zed-industries/zed`
- `ort 2.0.0-rc.11` — release candidate, with `load-dynamic` feature
- `reqwest 0.13` — with `stream` feature enabled
- `rusqlite 0.38` — with `bundled` feature (bundles SQLite)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A developer can go from fresh clone to successful build in under
  5 minutes on either supported platform.
- **SC-002**: The workspace compiles with zero compiler warnings on both
  Windows (CUDA) and macOS (Metal).
- **SC-003**: Incremental rebuild after modifying any `.rs` file completes in
  under 10 seconds.
- **SC-004**: `cargo test` runs successfully across all three crates (empty
  test suites are acceptable at this stage).
- **SC-005**: All 11 vox_core modules and 14 vox_ui modules are accessible
  from their respective dependents without compilation errors.
- **SC-006**: The release-mode binary (empty shell) is under 15 MB.

## Assumptions

- Developers have the documented prerequisites installed: Rust 1.85+,
  CMake 4.0+, and platform-specific toolchains (CUDA 12.8+ / VS 2022 on
  Windows, Xcode 26.x on macOS).
- On Windows, the `CMAKE_GENERATOR` and `CUDA_PATH` environment variables are
  set as persistent user environment variables per FR-007 of the feature
  description.
- The GPUI git revision from Tusk (`89e9ab97...`) is the starting point but
  may need to be updated if it doesn't compile on the current toolchain. The
  verified revision will be committed.
- No application logic, UI rendering, or ML inference is implemented in this
  feature. All modules are empty stubs.
