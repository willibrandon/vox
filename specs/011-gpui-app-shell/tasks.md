# Tasks: GPUI Application Shell

**Input**: Design documents from `/specs/011-gpui-app-shell/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/public-api.md, quickstart.md

**Tests**: Included — spec.md explicitly requests unit tests for theme, layout, and logging.

**Organization**: Tasks are grouped by user story. US5 (Theme) and US6 (Logging) are P3 priority but implemented first because they are foundational prerequisites — the theme system must exist before the overlay HUD renders, and logging is the first call in main().

> **Constitution VII**: All `pub` items in implementation tasks (T005–T014) MUST have `///` doc comments. All modules MUST have `//!` module-level docs. This is non-negotiable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add new dependencies and declare the new logging module.

- [X] T001 Add `tracing-appender = "0.2"` to `[workspace.dependencies]` in Cargo.toml (root)
- [X] T002 [P] Add `tracing-appender.workspace = true` to `[dependencies]` in crates/vox_core/Cargo.toml (logging.rs lives in vox_core)
- [X] T003 [P] Add `pub mod logging;` declaration to crates/vox_core/src/vox_core.rs
- [X] T004 [P] Add `tray-icon = "0.19"` and `global-hotkey = "0.6"` to `[dependencies]` in crates/vox/Cargo.toml (main.rs uses these directly for system tray and hotkey setup)

---

## Phase 2: US5 — Themed Visual Appearance (Priority: P3, foundational)

**Goal**: Define the complete dark theme color palette (28 HSLA colors) and layout constant scales (spacing, radius, size) that all UI components reference.

**Why first**: Although P3 priority, the theme and layout systems are prerequisites for the overlay HUD window (US1). The window cannot be themed without VoxTheme, and cannot be sized without layout constants.

**Independent Test**: Create `VoxTheme::dark()`, verify all 28 HSLA values are in 0.0..=1.0 range, verify `overlay_bg.a < 1.0`. Verify spacing constants are ordered XS < SM < MD < LG < XL.

- [X] T005 [P] [US5] Implement VoxTheme struct, ThemeColors struct with 28 color fields, `impl Global for VoxTheme`, and `VoxTheme::dark()` constructor with all dark theme HSLA values per data-model.md, plus `#[cfg(test)]` tests (test_theme_colors_valid, test_overlay_bg_transparent) in crates/vox_ui/src/theme.rs
- [X] T006 [P] [US5] Implement `spacing` sub-module (XS/SM/MD/LG/XL Pixels constants), `radius` sub-module (SM/MD/LG/PILL), `size` sub-module (OVERLAY_WIDTH/OVERLAY_HEIGHT/SETTINGS_WIDTH/SETTINGS_HEIGHT), plus `#[cfg(test)]` tests (test_spacing_scale, test_radius_scale, test_overlay_dimensions) in crates/vox_ui/src/layout.rs

**Checkpoint**: `cargo test -p vox_ui` — theme and layout tests pass.

---

## Phase 3: US6 — Application Logging (Priority: P3, foundational)

**Goal**: Structured logging with daily file rotation and 7-day retention. Platform-specific log directories. Environment-variable-configurable verbosity.

**Why second**: `init_logging()` is the very first call in `main()`, before GPUI even starts. Must exist before the entry point.

**Independent Test**: Call `log_dir()` and verify path contains "com.vox.app". Create temp directory with dated log files, call `cleanup_old_logs()`, verify only files >7 days old are deleted.

- [X] T007 [US6] Implement `LoggingGuard` struct wrapping `WorkerGuard`, `pub fn init_logging() -> LoggingGuard` with daily `RollingFileAppender` + non-blocking writer + env-filter (VOX_LOG > RUST_LOG > default), `pub fn log_dir() -> PathBuf` with platform-conditional paths, `pub fn cleanup_old_logs(dir: &Path, retention_days: u32)` with date-based file scanning, plus `#[cfg(test)]` tests (test_log_dir_platform, test_cleanup_old_logs) in crates/vox_core/src/logging.rs

**Checkpoint**: `cargo test -p vox_core --features cuda` — logging tests pass.

---

## Phase 4: US3 — Actions and Keyboard Shortcuts (Priority: P2)

**Goal**: Define all 7 application actions and provide registration functions for action handlers and keyboard shortcuts.

**Independent Test**: Verify `actions!` macro compiles, `register_actions()` and `register_key_bindings()` accept `&mut App`.

- [X] T008 [US3] Implement `actions!(vox, [ToggleRecording, StopRecording, ToggleOverlay, OpenSettings, Quit, CopyLastTranscript, ClearHistory])`, `register_actions(cx: &mut App)` with handlers for Quit (cx.quit()), ToggleRecording, OpenSettings (placeholder log), and `register_key_bindings(cx: &mut App)` with platform-conditional ctrl/cmd bindings (ToggleOverlay, OpenSettings, Quit) in crates/vox_ui/src/key_bindings.rs

**Checkpoint**: `cargo build -p vox_ui` — key_bindings module compiles without warnings.

---

## Phase 5: US1 — Instant Application Launch (Priority: P1)

**Goal**: The overlay HUD window view — a minimal Render implementation that displays application state using theme colors.

**Independent Test**: Verify OverlayHud implements Render, reads VoxState readiness, and returns a styled element tree.

**Depends on**: US5 (T005, T006) for VoxTheme and layout size constants.

- [X] T009 [US1] Implement `OverlayHud` struct, `OverlayHud::new(cx: &mut Context<Self>)`, and `impl Render for OverlayHud` that reads `cx.global::<VoxState>().readiness()` and `cx.global::<VoxTheme>()`, displays status text ("Downloading models...", "Loading pipeline...", "Ready") with appropriate theme status colors, uses `overlay_bg` for background, and sizes to `OVERLAY_WIDTH x OVERLAY_HEIGHT` in crates/vox_ui/src/overlay_hud.rs

**Checkpoint**: `cargo build -p vox_ui` — overlay HUD module compiles without warnings.

---

## Phase 6: US4 — System Tray Icons (Priority: P2)

**Goal**: Tray icon assets for 4 application states, embedded in the binary.

- [X] T010 [US4] Create four 32x32 RGBA tray icon PNGs — tray-idle.png (gray), tray-listening.png (green), tray-processing.png (blue), tray-error.png (red) — as solid colored squares matching VoxTheme status colors, generated programmatically via the `image` crate (dev-dependency) or hand-crafted, saved to assets/icons/ and embedded via `include_bytes!`

**Checkpoint**: Icon files exist at assets/icons/ and are valid 32x32 RGBA PNGs.

---

## Phase 7: Application Entry Point (US1 + US2 + US3 + US4 Integration)

**Goal**: Full working `main()` that wires all systems together. Window opens <100ms, pipeline inits in background, tray icon appears, global hotkey responds.

**Depends on**: ALL previous phases (T001–T010).

- [X] T011 [US1] Implement `main()` function skeleton in crates/vox/src/main.rs — call `init_logging()` (hold guard), `Application::new().run()`, inside run callback: compute platform-specific data directory via `dirs::data_local_dir().unwrap().join("com.vox.app")` (Windows) or equivalent macOS path, create `VoxState::new(&data_dir)` and `VoxTheme::dark()` as globals via `cx.set_global()`, call `register_actions(cx)` and `register_key_bindings(cx)`, implement `open_overlay_window(cx)` using `WindowKind::PopUp` + `WindowBackgroundAppearance::Transparent` + `TitlebarOptions { appears_transparent: true }` + `Bounds::centered()` + `focus: false` + `is_resizable: false`, register `window.on_window_should_close` with deferred quit, call `cx.activate(true)`
- [X] T012 [US3] Add `setup_global_hotkey(cx: &mut App)` function to crates/vox/src/main.rs — create `GlobalHotKeyManager`, register CapsLock hotkey from `Settings::activation_hotkey`, spawn crossbeam `GlobalHotKeyEvent::receiver()` polling task (50ms timer) that dispatches ToggleRecording action on press, log warning if registration fails
- [X] T013 [US4] Add `setup_system_tray(cx: &mut App)` function to crates/vox/src/main.rs — build `Menu` with Toggle Recording / Settings / Quit items, create `TrayIconBuilder` with idle icon from `include_bytes!`, tooltip "Vox — Voice Dictation", spawn crossbeam `MenuEvent::receiver()` polling task (50ms timer, combined with hotkey polling) that dispatches matching actions
- [X] T014 [US2] Add `async fn initialize_pipeline(mut cx: AsyncApp)` to crates/vox/src/main.rs — check missing models via `check_missing_models()`, if missing set readiness to `Downloading` and call `ModelDownloader::download_missing()`, then set readiness to `Loading` and load pipeline components (VAD/ASR/LLM), then set readiness to `Ready`; on failure, add `Error { message: String }` variant to `AppReadiness` enum in crates/vox_core/src/state.rs if not already present, and set readiness to `Error` with descriptive message; wire into main() via `cx.spawn(|cx| async move { initialize_pipeline(cx).await }).detach()`

**Checkpoint**: `cargo build -p vox --features vox_core/cuda` compiles without warnings. Application launches and shows a window.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Validation and quality assurance across all stories.

- [X] T015 Run `cargo build -p vox --features vox_core/cuda` and verify zero compiler warnings in crates/vox/src/main.rs
- [X] T016 Run `cargo build -p vox_ui` and verify zero compiler warnings across theme.rs, layout.rs, key_bindings.rs, overlay_hud.rs
- [X] T017 Run `cargo test -p vox_ui` and `cargo test -p vox_core --features cuda` to verify all unit tests pass (theme, layout, logging)
- [X] T018 Validate quickstart.md scenarios VS-001 through VS-004 (theme colors valid, layout constants ordered, log directory platform, log retention cleanup) via test execution

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **US5 (Phase 2)**: Depends on Setup — theme.rs and layout.rs are independent of each other [P]
- **US6 (Phase 3)**: Depends on Setup (T002 for tracing-appender dep, T003 for module declaration) — can run in parallel with US5
- **US3 (Phase 4)**: Depends on Setup — can run in parallel with US5 and US6
- **US1 (Phase 5)**: Depends on US5 (theme + layout for rendering)
- **US4 (Phase 6)**: Depends on Setup — can run in parallel with US1
- **Entry Point (Phase 7)**: Depends on ALL previous phases — sequential T011 → T012 → T013 → T014
- **Polish (Phase 8)**: Depends on all implementation phases

### User Story Dependencies

- **US5 (Theme/Layout)**: No dependencies on other stories — foundational
- **US6 (Logging)**: No dependencies on other stories — foundational
- **US3 (Actions/Keys)**: No dependencies on other stories — standalone module
- **US1 (Launch)**: Depends on US5 (theme for rendering), US3 (actions for registration), US6 (logging for init)
- **US2 (Pipeline Init)**: Depends on US1 (window must be open first)
- **US4 (System Tray)**: Depends on icon assets (T010), no story dependencies

### Within Phase 7 (main.rs)

- T011 MUST be first (creates the file skeleton)
- T012, T013 can follow T011 in any order (different functions)
- T014 MUST be last (depends on window being open from T011)

### Parallel Opportunities

**Maximum parallelism after Setup**:
```
T005 (theme.rs) ──┐
T006 (layout.rs) ──┼──→ T009 (overlay_hud.rs) ──→ T011 (main skeleton)
T007 (logging.rs) ─┤                                 ↓
T008 (key_bindings.rs)                            T012 (global hotkey)
T010 (tray icons) ───────────────────────────→ T013 (system tray)
                                                  ↓
                                              T014 (pipeline init)
```

---

## Parallel Example: Phase 2 + 3 + 4

```text
# These four tasks can ALL run in parallel (different files, no dependencies):
Task T005: "Implement VoxTheme in crates/vox_ui/src/theme.rs"
Task T006: "Implement layout constants in crates/vox_ui/src/layout.rs"
Task T007: "Implement logging in crates/vox_core/src/logging.rs"
Task T008: "Implement key_bindings in crates/vox_ui/src/key_bindings.rs"
```

---

## Implementation Strategy

### MVP First (US5 + US6 + US1)

1. Complete Phase 1: Setup (add deps)
2. Complete Phase 2: US5 — Theme + Layout (foundational)
3. Complete Phase 3: US6 — Logging (foundational)
4. Complete Phase 5: US1 — Overlay HUD
5. Implement T011 from Phase 7: main() skeleton with window
6. **STOP and VALIDATE**: App launches and shows themed overlay HUD

### Incremental Delivery

1. Setup + US5 + US6 → Foundation ready
2. Add US3 (actions) + US1 (overlay HUD) → Window with keyboard shortcuts (MVP!)
3. Add US4 (system tray) → Full tray integration
4. Add US2 (pipeline init) → Background model loading
5. Polish → Zero warnings, all tests pass

### Single Developer Sequence

Since this is a single-developer project, execute phases sequentially:
1. Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5 → Phase 6 → Phase 7 → Phase 8
2. Within Phase 2, T005 and T006 can be done in either order
3. Phase 7 tasks MUST be sequential (same file)

---

## Notes

- [P] tasks = different files, no dependencies on each other
- [US*] labels map to spec.md user stories for traceability
- US5/US6 (P3) precede US1/US3/US4 (P1/P2) because they are foundational prerequisites
- Phase 7 is the integration phase where main.rs ties all modules together
- All tests use `#[cfg(test)]` inline modules — no separate test files
- Tests run via `cargo test -p vox_ui` and `cargo test -p vox_core --features cuda`
- Icon PNGs are embedded via `include_bytes!` for single-binary distribution
- tracing-appender is a dep of vox_core (not vox) because logging.rs lives in vox_core
- tray-icon and global-hotkey are deps of BOTH vox_core and vox — vox_core declares them for re-export/wiring, vox/main.rs uses them directly for system tray and hotkey setup
