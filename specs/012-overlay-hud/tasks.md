# Tasks: Overlay HUD

**Input**: Design documents from `/specs/012-overlay-hud/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Included — spec.md defines 6 unit tests across overlay and waveform modules.

**Organization**: Tasks grouped by user story (P1–P6) for independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- All file paths are relative to repository root

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: Core data model changes and state bridge infrastructure that ALL user stories depend on

**CRITICAL**: No user story work can begin until this phase is complete

- [X] T001 [P] Add `InjectionFailed { polished_text: String, error: String }` variant to `PipelineState` enum and update all existing match arms in `crates/vox_core/src/pipeline/state.rs`
- [X] T002 [P] Add `latest_rms: RwLock<f32>` field (default 0.0) with `latest_rms()` getter and `set_latest_rms(rms: f32)` setter (clamping to [0.0, 1.0]) to `VoxState` in `crates/vox_core/src/state.rs`
- [X] T003 [P] Add `status_loading: Hsla` (blue, `hsla(0.6, 0.8, 0.6, 1.0)`) and `status_injection_failed: Hsla` (amber, `hsla(0.15, 0.9, 0.6, 1.0)`) fields to `ThemeColors` struct and `VoxTheme::dark()` in `crates/vox_ui/src/theme.rs`, updating the test_theme_colors_valid test array. Note: `status_loading` (Loading) and `status_processing` (Processing) both use blue — ensure visually distinct hues (e.g., Loading lighter/desaturated) so users can differentiate the two states
- [X] T004 [P] Add `WAVEFORM_WIDTH: Pixels = px(340.0)`, `WAVEFORM_HEIGHT: Pixels = px(40.0)`, and `PROGRESS_BAR_HEIGHT: Pixels = px(6.0)` constants to `size` module in `crates/vox_ui/src/layout.rs`
- [X] T005 [P] Add `CopyInjectedText`, `RetryDownload`, `OpenModelFolder`, `DismissOverlay` actions to `actions!` macro and register logging handlers in `register_actions()` in `crates/vox_ui/src/key_bindings.rs`
- [X] T006 Restructure `OverlayHud` from zero-field struct to full state struct (readiness: `AppReadiness`, pipeline_state: `PipelineState`, waveform_samples: `VecDeque<f32>`, quick_settings_open: `bool`, showing_injected_fade: `bool`, focus_handle: `FocusHandle`, _subscriptions: `Vec<Subscription>`, _waveform_task: `Option<Task<()>>`, _fade_task: `Option<Task<()>>`), implement `OverlayDisplayState` bridge global struct with `Clone` + `Global` impls, and update `OverlayHud::new()` constructor to accept `window: &mut Window` parameter in `crates/vox_ui/src/overlay_hud.rs`
- [X] T007 Wire `OverlayDisplayState` bridge in `crates/vox/src/main.rs`: initialize `OverlayDisplayState` global after `VoxState` with initial readiness/pipeline_state, implement `update_overlay_state()` helper that reads `VoxState` and calls `cx.set_global(OverlayDisplayState { ... })`, call it after every `set_readiness()` and `set_pipeline_state()` in `try_initialize_pipeline()`, and update `open_overlay_window()` to pass `window` to `OverlayHud::new(window, cx)`

**Checkpoint**: Foundation ready — OverlayDisplayState bridge active, OverlayHud struct has full fields, all dependencies in place. User story implementation can now begin.

---

## Phase 2: User Story 1 — Seeing Dictation State at a Glance (Priority: P1) MVP

**Goal**: Display all pipeline states (idle, listening, processing, injected, error) with correct colored indicators and text labels in the overlay

**Independent Test**: Launch the app, observe the overlay in each pipeline state, verify correct indicator color and label for each state

### Tests for User Story 1

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T008 [US1] Write `test_overlay_state_idle` (verify idle renders gray indicator + hint text) and `test_overlay_state_processing` (verify processing renders blue indicator + raw transcript) unit tests in `crates/vox_ui/src/overlay_hud.rs`

### Implementation for User Story 1

- [X] T009 [US1] Implement `render_status_bar()` with colored indicator dot/icon (8px circle with state-appropriate color from theme), bold state label (IDLE/LISTENING/PROCESSING/INJECTED/ERROR/DOWNLOADING/LOADING/INJECTION FAILED), flex spacer, "Vox" title, dropdown arrow placeholder (▾), and menu button placeholder (≡) in `crates/vox_ui/src/overlay_hud.rs`
- [X] T010 [US1] Implement `on_state_changed()` handler: subscribe via `cx.observe_global::<OverlayDisplayState>()`, clone readiness and pipeline_state from display global, detect Listening transitions to start/stop waveform timer, detect Injecting transition to start fade timer AND drop `_fade_task = None` when transitioning AWAY from Injecting (prevents stale timer firing after state has moved on — spec edge case #4), call `cx.notify()` in `crates/vox_ui/src/overlay_hud.rs`
- [X] T011 [US1] Implement `render_content()` dispatch matching on (readiness, pipeline_state) to route to state-specific renderers. Readiness takes priority: when `Downloading`, check if ANY model's `DownloadProgress` is `Failed` — if so, route to `render_download_failed()`, otherwise route to `render_download_progress()`. When `Loading`, route to `render_loading()`. When `Ready`, dispatch on pipeline_state. Implement `render_idle_hint()` (shows "Press [hotkey] to start dictating" reading hotkey name from VoxState settings) and `render_processing()` (shows raw transcript text from Processing.raw_text or "Transcribing..." placeholder) in `crates/vox_ui/src/overlay_hud.rs`
- [X] T012 [US1] Implement `render_injected()` displaying polished text from Injecting.polished_text with green checkmark, `start_injection_fade()` using `cx.spawn()` with 2-second timer that sets `showing_injected_fade = false` and calls `cx.notify()`, and `render_error()` showing error message from Error.message with red warning text in `crates/vox_ui/src/overlay_hud.rs`
- [X] T013 [US1] Implement pulsing green dot animation for Listening state indicator using `with_animation("pulse", Animation::new(Duration::from_secs(1)).repeat(), |div, delta| div.opacity(0.4 + delta * 0.6))` in `render_status_bar()` in `crates/vox_ui/src/overlay_hud.rs`
- [X] T014 [US1] Update `Render::render()` to compose full overlay layout: outer div with `w(OVERLAY_WIDTH)`, `min_h(OVERLAY_HEIGHT)`, `bg(overlay_bg)`, `rounded(LG)`, `p(SM)`, `opacity(settings.overlay_opacity)`, `flex_col()`, `window_control_area(Drag)`, child `render_status_bar()`, child `render_content()` in `crates/vox_ui/src/overlay_hud.rs`

**Checkpoint**: User Story 1 fully functional — all pipeline states display with correct indicators, labels, and content. Tests pass.

---

## Phase 3: User Story 2 — Real-Time Audio Waveform (Priority: P2)

**Goal**: Display animated vertical bars during Listening state that respond to audio amplitude in real-time at 30fps

**Independent Test**: Activate dictation, speak into microphone, verify waveform bars animate proportionally to audio amplitude. Silence shows minimal bar height.

### Tests for User Story 2

- [X] T015 [P] [US2] Write `test_waveform_empty` (verify zero samples renders without crash — no division by zero) and `test_waveform_full` (verify 50 samples at various amplitudes produce correctly sized bars) unit tests in `crates/vox_ui/src/waveform.rs`

### Implementation for User Story 2

- [X] T016 [P] [US2] Implement `WaveformVisualizer` struct with `new(samples: Vec<f32>, bar_color: Hsla, baseline_color: Hsla)`, `IntoElement` impl, and full `Element` trait: `request_layout` returning fixed size (`WAVEFORM_WIDTH × WAVEFORM_HEIGHT`), `prepaint` no-op, `paint` iterating samples to call `window.paint_quad(fill(...))` per bar (width = bounds.width/N * 0.8, height = sample.clamp(0,1) * bounds.height max px(2.0), centered vertically, bar_color if height > 2px else baseline_color) in `crates/vox_ui/src/waveform.rs`
- [X] T017 [US2] Implement `start_waveform_animation()` spawning 33ms repeating timer via `cx.spawn()` that reads `VoxState.latest_rms()`, pushes to `waveform_samples: VecDeque<f32>` ring buffer (capacity 50, pop_front on overflow), calls `cx.notify()`; `stop_waveform_animation()` dropping `_waveform_task`; and `render_waveform()` creating `WaveformVisualizer::new(samples.iter().copied().collect(), theme.colors.waveform_active, theme.colors.waveform_inactive)` in `crates/vox_ui/src/overlay_hud.rs`
- [X] T018 [US2] Write `test_overlay_state_listening` (verify listening state renders waveform area) unit test in `crates/vox_ui/src/overlay_hud.rs`

**Checkpoint**: User Story 2 complete — waveform animates at 30fps during Listening state. Waveform and overlay tests pass.

---

## Phase 4: User Story 3 — Model Download Progress on First Launch (Priority: P3)

**Goal**: Show per-model download progress with name, percentage, byte count, and progress bar during first launch

**Independent Test**: Clear downloaded models, launch app, verify download progress appears for each model with accurate byte counts and progress bar

### Tests for User Story 3

- [X] T019 [US3] Write `test_overlay_state_download` (verify downloading state renders progress bar and model info) unit test in `crates/vox_ui/src/overlay_hud.rs`

### Implementation for User Story 3

- [X] T020 [US3] Implement `render_download_progress()` showing currently-downloading model name, percentage, byte count ("Whisper model: 43% (387 MB / 900 MB)"), and a visual progress bar div (filled portion using `w(fraction * WAVEFORM_WIDTH)` with `bg(status_downloading)`, unfilled with `bg(border)`, height `PROGRESS_BAR_HEIGHT`, rounded corners) in `crates/vox_ui/src/overlay_hud.rs`
- [X] T021 [US3] Implement `render_loading()` showing stage description text from Loading.stage (e.g., "Loading Whisper model onto GPU...") with `status_loading` color, and `render_download_failed()` receiving the `DownloadProgress::Failed { error, manual_url }` fields — show error description text, `manual_url` for reference, "Open Folder" button dispatching `OpenModelFolder` action, and "Retry Download" button dispatching `RetryDownload` action in `crates/vox_ui/src/overlay_hud.rs`

**Checkpoint**: User Story 3 complete — download progress, loading stages, and download failure all render correctly.

---

## Phase 5: User Story 4 — Injection Failure Recovery (Priority: P4)

**Goal**: When text injection fails, display the polished text with a Copy button so the user can recover their dictation

**Independent Test**: Simulate injection failure, verify overlay shows buffered text with Copy button, click Copy and verify text on clipboard

- [X] T022 [US4] Implement `render_injection_failed()` showing yellow warning icon, "INJECTION FAILED" label (already handled by status bar via InjectionFailed match arm), polished text from `InjectionFailed.polished_text`, error description, and a "Copy" button; implement `copy_to_clipboard()` using `cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()))`, show brief checkmark confirmation, then transition to Idle by setting `cx.global::<VoxState>().set_pipeline_state(PipelineState::Idle)` and immediately calling `cx.set_global(OverlayDisplayState { readiness: cx.global::<VoxState>().readiness(), pipeline_state: PipelineState::Idle })` to update the bridge (note: this is a permitted consumer-side bridge update per display-state-api contract) in `crates/vox_ui/src/overlay_hud.rs`

**Checkpoint**: User Story 4 complete — injection failure shows polished text with Copy button, clipboard copy works.

---

## Phase 6: User Story 5 — Overlay Position and Opacity Customization (Priority: P5)

**Goal**: Draggable overlay that remembers position across restarts, configurable opacity

**Independent Test**: Drag overlay to non-default position, relaunch app, verify overlay appears at saved position. Change opacity and verify visual change.

- [X] T023 [US5] Implement `on_position_changed()` handler: subscribe via `cx.observe_window_bounds(window, callback)` in `OverlayHud::new()`, callback reads `window.bounds()`, converts to `OverlayPosition::Custom { x, y }`, saves via `cx.global::<VoxState>().update_settings(|s| s.overlay_position = pos)` in `crates/vox_ui/src/overlay_hud.rs`
- [X] T024 [US5] Update `open_overlay_window()` in `crates/vox/src/main.rs` to read `VoxState.settings().overlay_position`, convert `Custom { x, y }` to `WindowBounds::Windowed(Bounds { origin, size })` with clamping to current display bounds (fall back to `Bounds::centered()` for non-Custom variants or if saved position is off-screen)

**Checkpoint**: User Story 5 complete — position persists across restarts, opacity applied from settings.

---

## Phase 7: User Story 6 — Quick Settings Access from Overlay (Priority: P6)

**Goal**: Dropdown with dictation toggle and language selector accessible from overlay status bar

**Independent Test**: Click dropdown arrow, verify quick settings appear. Toggle dictation, change language, verify changes take effect.

- [X] T025 [US6] Implement `render_quick_settings()` dropdown: when `quick_settings_open` is true, render anchored div below the ▾ button containing a dictation toggle (label shows "Pause" or "Resume", click dispatches `ToggleRecording`) and a language selector (shows current `settings.language`, click cycles or opens sub-list); add Escape key handler and click-outside-to-dismiss logic (set `quick_settings_open = false` + `cx.notify()`) in `crates/vox_ui/src/overlay_hud.rs`
- [X] T026 [US6] Wire ▾ dropdown button in `render_status_bar()` to toggle `quick_settings_open` via `cx.listener()`, wire ≡ menu button to dispatch `OpenSettings` action, and add `render_quick_settings()` as conditional child in `Render::render()` (`.when(self.quick_settings_open, |this| this.child(self.render_quick_settings(cx)))`) in `crates/vox_ui/src/overlay_hud.rs`

**Checkpoint**: User Story 6 complete — quick settings dropdown functional with dictation toggle and language selector.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Verification across all user stories, zero-warning build, full state coverage

- [X] T027 Run `cargo test -p vox_ui` and fix any failing tests — all 6 spec tests must pass: `test_overlay_state_idle`, `test_overlay_state_listening`, `test_overlay_state_processing`, `test_overlay_state_download`, `test_waveform_empty`, `test_waveform_full`
- [X] T028 Run `cargo build -p vox --features vox_core/cuda` and resolve any compiler warnings to achieve zero-warning build
- [ ] T029 Run quickstart.md validation — launch app and verify all 10+ overlay states render correctly per the verification steps (Downloading → Loading → Idle → Listening → Processing → Injected → Error → Injection Failed → Download Failed), verify drag persistence, verify quick settings dropdown

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — can start immediately. BLOCKS all user stories.
- **US1 (Phase 2)**: Depends on Phase 1 completion. MVP delivery target.
- **US2 (Phase 3)**: Depends on US1 (needs render_content dispatch and Listening state in status bar).
- **US3 (Phase 4)**: Depends on US1 (needs render_content dispatch and Downloading/Loading state routing).
- **US4 (Phase 5)**: Depends on US1 (needs render_content dispatch and InjectionFailed variant from T001).
- **US5 (Phase 6)**: Depends on US1 (needs overlay window and render infrastructure).
- **US6 (Phase 7)**: Depends on US1 (needs status bar structure for dropdown anchor).
- **Polish (Phase 8)**: Depends on all user stories being complete.

### User Story Dependencies

- **US1 (P1)**: Blocks US2, US3, US4, US5, US6 — all stories build on the core overlay infrastructure
- **US2 (P2)**: Independent after US1. Does not affect other stories.
- **US3 (P3)**: Independent after US1. Does not affect other stories.
- **US4 (P4)**: Independent after US1. Does not affect other stories.
- **US5 (P5)**: Independent after US1. Does not affect other stories.
- **US6 (P6)**: Independent after US1. Does not affect other stories.

### Within Each User Story

- Tests written and failing before implementation begins
- Status bar / state handling before content renderers
- Core rendering before animations and timers
- All story tasks are sequential (same file: `overlay_hud.rs`)

### Parallel Opportunities

**Phase 1** (maximum parallelism):
```
T001 ─┐
T002 ─┤
T003 ─┼── all 5 in parallel (different files)
T004 ─┤
T005 ─┘
      └──► T006 (depends on T001, T003, T004, T005) ──► T007 (depends on T006)
```

**Phase 3 — US2** (cross-file parallelism):
```
T015 (waveform.rs tests) ─┐
                           ├── parallel (different files)
T016 (waveform.rs impl)  ─┤
                           └──► T017 (overlay_hud.rs) ──► T018 (overlay_hud.rs)
```

**After US1 completion** (story-level parallelism):
```
US2 ─┐
US3 ─┼── theoretically parallel (independent stories)
US4 ─┤   BUT all modify overlay_hud.rs, so practically sequential
US5 ─┤
US6 ─┘
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Foundational (T001–T007)
2. Complete Phase 2: User Story 1 (T008–T014)
3. **STOP and VALIDATE**: All basic pipeline states render correctly
4. Build and verify zero warnings

### Incremental Delivery

1. Foundational → Foundation ready
2. US1 → Core overlay with all states (MVP)
3. US2 → Waveform visualization during Listening
4. US3 → Download/loading progress on first launch
5. US4 → Injection failure recovery with Copy
6. US5 → Position persistence + opacity
7. US6 → Quick settings dropdown
8. Polish → Full validation, zero warnings, all tests pass

### File Modification Summary

| File | Phase(s) | Changes |
|---|---|---|
| `crates/vox_core/src/pipeline/state.rs` | 1 | +InjectionFailed variant |
| `crates/vox_core/src/state.rs` | 1 | +latest_rms field + methods |
| `crates/vox_ui/src/theme.rs` | 1 | +2 status colors |
| `crates/vox_ui/src/layout.rs` | 1 | +3 size constants |
| `crates/vox_ui/src/key_bindings.rs` | 1 | +4 actions |
| `crates/vox_ui/src/overlay_hud.rs` | 1–7 | Full rewrite (~500 lines) |
| `crates/vox_ui/src/waveform.rs` | 3 | New Element impl (~100 lines) |
| `crates/vox/src/main.rs` | 1, 6 | +bridge init, +position restore |

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- US2–US6 are logically independent but share `overlay_hud.rs` — execute sequentially in priority order
- All overlay unit tests use GPUI headless rendering (no windowing system required)
- The `status_display()` helper function in current overlay_hud.rs is superseded by the new render_status_bar/render_content pattern — remove it during T009
