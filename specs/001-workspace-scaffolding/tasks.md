# Tasks: Workspace Scaffolding

**Input**: Design documents from `/specs/001-workspace-scaffolding/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Not required — spec states "empty test suites are acceptable at this stage." Integration test stubs are part of directory structure (FR-008), not runnable tests.

**Organization**: Tasks are grouped by user story. Since scaffolding shares all implementation across stories, Phases 1–2 (file creation) are foundational, and Phases 3–6 verify each user story independently.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Exact file paths included in all descriptions

## Path Conventions

Three-crate workspace at repository root:
- `crates/vox/` — binary entry point
- `crates/vox_core/` — backend library
- `crates/vox_ui/` — GPUI UI library

---

## Phase 1: Setup (Directory Structure)

**Purpose**: Create the complete directory tree and repository configuration

- [ ] T001 Create directory structure: `crates/vox/src/`, `crates/vox_core/src/`, `crates/vox_ui/src/`, `assets/icons/`, `tests/audio_fixtures/`, `scripts/`
- [ ] T002 [P] Update `.gitignore` to exclude `/target`, `/models/`, `*.onnx`, `*.bin`, `*.gguf`, `.env`, `*.log` per FR-009

---

## Phase 2: Foundational (Cargo.toml Files and Source Stubs)

**Purpose**: Create all Cargo.toml manifests and source files so the workspace can compile

**CRITICAL**: No user story work can begin until this phase is complete

- [ ] T003 Create workspace root `Cargo.toml` with 3 members (`crates/vox`, `crates/vox_core`, `crates/vox_ui`), `[workspace.package]` (version 0.1.0, edition 2024, license MIT), `[workspace.dependencies]` for all shared deps (gpui rev `89e9ab97aa5d978351ee8a28d9cc35c272c530f5`, serde, tokio, anyhow, tracing, etc.), and `[profile.release]` (opt-level "s", lto true, strip symbols, codegen-units 1) per FR-001/FR-004/FR-010/FR-011
- [ ] T004 [P] Create `crates/vox_core/Cargo.toml` with `[lib]` path `src/vox_core.rs`, feature flags (cuda → whisper-rs/cuda + llama-cpp-2/cuda, metal → whisper-rs/metal + llama-cpp-2/metal), workspace deps (.workspace = true), crate-specific deps (cpal 0.17, ringbuf 0.4, rubato 1.0, ort 2.0.0-rc.11, whisper-rs 0.15.1, llama-cpp-2 0.1, rusqlite 0.38, reqwest 0.13 (if unavailable fall back to 0.12 with stream feature per research.md R-007), global-hotkey 0.6, tray-icon 0.19), and platform deps (windows 0.62 for cfg(windows), objc2 0.6 + objc2-core-graphics 0.3 for cfg(macos)) per FR-003/FR-005 and research.md R-001 through R-007
- [ ] T005 [P] Create `crates/vox_ui/Cargo.toml` with `[lib]` path `src/vox_ui.rs`, path dep on vox_core, workspace deps (gpui, serde, parking_lot), and smallvec 1.11 with union feature per data-model.md
- [ ] T006 [P] Create `crates/vox/Cargo.toml` with `[[bin]]` path `src/main.rs`, path deps on vox_core and vox_ui, workspace deps (gpui, serde, tokio, anyhow, tracing, tracing-subscriber) per FR-002
- [ ] T007 Create `crates/vox_core/src/vox_core.rs` with 11 `pub mod` declarations (audio, vad, asr, llm, injector, pipeline, dictionary, config, models, hotkey, state) and create 11 empty stub files in `crates/vox_core/src/` per FR-006 and research.md R-005/R-006
- [ ] T008 [P] Create `crates/vox_ui/src/vox_ui.rs` with 14 `pub mod` declarations (theme, layout, overlay_hud, waveform, workspace, settings_panel, history_panel, dictionary_panel, model_panel, log_panel, text_input, button, icon, key_bindings) and create 14 empty stub files in `crates/vox_ui/src/` per FR-007 and research.md R-005/R-006
- [ ] T009 [P] Create `crates/vox/src/main.rs` with minimal shell (`fn main()` printing app name)
- [ ] T010 [P] Create integration test stubs (`tests/test_vad.rs`, `tests/test_asr.rs`, `tests/test_llm.rs`, `tests/test_injector.rs`, `tests/test_pipeline_e2e.rs`) and model download scripts (`scripts/download-models.sh`, `scripts/download-models.ps1`) per FR-008

**Checkpoint**: All source files exist — workspace should be structurally complete

---

## Phase 3: User Story 1 — Build on Windows (Priority: P1) 🎯 MVP

**Goal**: Verify the workspace compiles on Windows with CUDA acceleration, produces a binary with zero warnings, and tests pass

**Independent Test**: Run `cargo build -p vox --features vox_core/cuda` on Windows with prerequisites installed

- [ ] T011 [US1] Run `cargo build -p vox --features vox_core/cuda` and resolve any dependency resolution or compilation errors in workspace Cargo.toml files
- [ ] T012 [US1] Verify build output has zero compiler warnings — if warnings exist, fix source files with `#[allow(...)]` only with justifying comment or proper code fixes
- [ ] T013 [US1] Run `cargo test -p vox_core --features cuda`, `cargo test -p vox`, and `cargo test -p vox_ui` to verify all three crates' test harnesses pass (SC-004, empty suites acceptable)

**Checkpoint**: User Story 1 complete — workspace builds and tests pass on Windows

---

## Phase 4: User Story 2 — Build on macOS (Priority: P1)

**Goal**: Verify the same workspace compiles on macOS with Metal acceleration

**Independent Test**: Run `cargo build -p vox --features vox_core/metal` on macOS with Xcode 26.x

**Note**: This phase cannot be verified on Windows. Tasks document what must be validated on a macOS machine or CI. Consider setting up GitHub Actions or a macOS remote runner to close this verification gap.

- [ ] T014 [US2] On macOS, run `cargo build -p vox --features vox_core/metal` and verify zero warnings. If GPUI build fails on macOS, add `core-text = "=21.0.0"` and `core-graphics = "=0.24.0"` patches to workspace Cargo.toml per research.md R-008
- [ ] T015 [US2] On macOS, run `cargo test -p vox_core --features metal`, `cargo test -p vox`, and `cargo test -p vox_ui` to verify all three crates' test harnesses pass (SC-004)

**Checkpoint**: User Story 2 complete — workspace builds cross-platform

---

## Phase 5: User Story 3 — Crate Structure for Development (Priority: P2)

**Goal**: Verify the workspace structure clearly separates concerns and all modules are accessible from dependent crates

**Independent Test**: Add a trivial function to a vox_core module, confirm it's accessible from vox and vox_ui

- [ ] T016 [US3] Verify all 11 vox_core modules are declared `pub mod` in `crates/vox_core/src/vox_core.rs` and accessible as `vox_core::audio`, `vox_core::vad`, etc.
- [ ] T017 [US3] Verify all 14 vox_ui modules are declared `pub mod` in `crates/vox_ui/src/vox_ui.rs` and accessible as `vox_ui::theme`, `vox_ui::layout`, etc.
- [ ] T018 [US3] Verify crate dependency graph: modify a vox_core stub, rebuild — both vox_ui and vox should recompile. Modify a vox_ui stub — only vox should recompile.

**Checkpoint**: User Story 3 complete — module structure matches design document

---

## Phase 6: User Story 4 — GPUI Rev Pin Verification (Priority: P2)

**Goal**: Confirm the pinned GPUI git revision compiles and matches the Tusk reference

**Independent Test**: Run `cargo build -p vox_ui` to verify GPUI resolves and compiles

- [ ] T019 [US4] Run `cargo build -p vox_ui` and verify GPUI rev `89e9ab97aa5d978351ee8a28d9cc35c272c530f5` resolves to gpui v0.2.2 and compiles under Rust edition 2024. If edition 2024 causes GPUI failure, fall back per research.md R-004 options.
- [ ] T020 [US4] Verify the pinned GPUI rev in `Cargo.toml` matches Tusk's `Cargo.lock` entry at `D:\SRC\tusk\Cargo.lock`

**Checkpoint**: User Story 4 complete — GPUI rev is verified and pinned

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all success criteria

- [ ] T021 Run `cargo build --release -p vox --features vox_core/cuda` and verify binary size < 15 MB (SC-006)
- [ ] T022 Verify incremental rebuild: modify any `.rs` file, run `cargo build -p vox --features vox_core/cuda`, confirm rebuild < 10 seconds (SC-003)
- [ ] T023 Validate `specs/001-workspace-scaffolding/quickstart.md` steps match actual build experience — update if any steps are incorrect
- [ ] T024 Time a clean build (`cargo clean && cargo build -p vox --features vox_core/cuda`) and verify it completes in under 5 minutes (SC-001)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (directory structure must exist first) — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 — first build verification
- **US2 (Phase 4)**: Depends on Phase 2 — can run in parallel with US1 (different machine)
- **US3 (Phase 5)**: Depends on Phase 3 (needs successful build to verify module access)
- **US4 (Phase 6)**: Implicitly verified by Phase 3 build success — explicit verification is confirmatory
- **Polish (Phase 7)**: Depends on Phase 3 completion

### User Story Dependencies

- **US1 (P1)**: Can start after Phase 2 — No dependencies on other stories
- **US2 (P1)**: Can start after Phase 2 — Independent of US1 (different platform)
- **US3 (P2)**: Benefits from US1 completion (confirmed build) but structurally independent
- **US4 (P2)**: Verified as side effect of US1 build — explicit check is supplementary

### Within Each Phase

- Phase 2 Cargo.toml files (T003–T006) should be written before source stubs (T007–T010)
- T003 (workspace root) should be written first — member crates reference workspace deps
- T007 and T008 (lib entry points) must exist before build — `pub mod` declarations need files
- T004–T006 are independent and can be written in parallel after T003

### Parallel Opportunities

- Phase 1: T001 and T002 can run in parallel
- Phase 2: T004, T005, T006 can run in parallel (after T003). T007, T008, T009, T010 can run in parallel (after their respective Cargo.toml)
- Phase 3–6: US1 and US2 can run in parallel on different machines
- Phase 5–6: US3 and US4 verification tasks can run in parallel

---

## Parallel Example: Phase 2 Foundational

```text
# Step 1: Write workspace root first (other Cargo.toml files depend on it)
Task T003: Create workspace root Cargo.toml

# Step 2: Write all crate Cargo.toml files in parallel
Task T004: Create vox_core Cargo.toml
Task T005: Create vox_ui Cargo.toml
Task T006: Create vox Cargo.toml

# Step 3: Write all source files in parallel
Task T007: Create vox_core entry point + 11 stubs
Task T008: Create vox_ui entry point + 14 stubs
Task T009: Create vox main.rs
Task T010: Create test stubs + scripts
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (directory structure, .gitignore)
2. Complete Phase 2: Foundational (all Cargo.toml + source stubs)
3. Complete Phase 3: US1 Build on Windows
4. **STOP and VALIDATE**: `cargo build` succeeds with zero warnings, `cargo test` passes
5. Workspace is usable — development of subsequent features can begin

### Incremental Delivery

1. Phase 1 + 2 → All files created
2. Phase 3: US1 → Windows build verified → **MVP complete**
3. Phase 4: US2 → macOS build verified (when macOS available)
4. Phase 5: US3 → Module structure confirmed
5. Phase 6: US4 → GPUI pin confirmed
6. Phase 7: Polish → Release build, incremental rebuild, quickstart validated

### Single Developer (Current Setup)

1. Complete all phases sequentially: 1 → 2 → 3 → 5 → 6 → 7
2. Phase 4 (macOS) deferred until macOS machine available or CI configured
3. Estimated: Phase 1–2 = bulk of work (file creation), Phase 3+ = build and fix cycle

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- US1 build success implicitly verifies US3 (module access) and US4 (GPUI compiles) — explicit phases provide traceability
- `reqwest 0.13` is a risk — if unavailable, fall back to `0.12` with `stream` feature (research.md R-007)
- Edition 2024 + GPUI is untested — if build fails, apply fallback per research.md R-004
- No `mod.rs` files anywhere — modern Rust convention per clarification session
- Library entry points are `vox_core.rs` and `vox_ui.rs` (not `lib.rs`) with `[lib] path` in Cargo.toml
