# Tasks: System Tray & Global Hotkeys

**Input**: Design documents from `/specs/014-tray-hotkeys/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/hotkey-interpreter.md, quickstart.md

**Tests**: Unit tests for HotkeyInterpreter explicitly requested in spec (Testing Requirements section) and plan (`hotkey_interpreter_tests.rs`).

**Organization**: Tasks grouped by user story priority. US1+US5 (both P1) combined as MVP — they share the same hotkey event handler code path. US3 (hands-free) requires no unique implementation beyond the foundational HotkeyInterpreter — its tasks are covered by T003, T009, and T010.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story (US1–US6) from spec.md

## Phase 1: Setup

**Purpose**: New files, module declarations, and assets needed before implementation

- [x] T001 [P] Create orange 32×32 RGBA tray icon at `assets/icons/tray-downloading.png` matching the style of existing tray icons (use `png` crate to generate programmatically, or create a static asset with the same pixel dimensions and RGBA format as `tray-idle.png`)
- [x] T002 [P] Add `pub mod hotkey_interpreter;` declaration to `crates/vox_core/src/vox_core.rs` and add `mod tray;` declaration to `crates/vox/src/main.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types and state machine that ALL user stories depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T003 [P] Implement `ActivationMode` enum (HoldToTalk/Toggle/HandsFree with serde kebab-case + Default), `HotkeyAction` enum (None/StartRecording/StopRecording/StartHandsFree), and `HotkeyInterpreter` struct with `new()`, `on_press(is_recording) -> HotkeyAction`, `on_release(is_recording) -> HotkeyAction`, `set_mode()`, `mode()` per behavioral contract tables in `contracts/hotkey-interpreter.md` in `crates/vox_core/src/hotkey_interpreter.rs`
- [x] T004 [P] Implement `TrayIconState` enum, `TrayUpdate` enum, `TrayIcons` struct (five pre-decoded `Icon` values), `TrayMenuIds` struct, and helper functions `decode_all_tray_icons()`, `derive_tray_state()`, `create_tray_menu()`, `tooltip_for_state()` per tray management contract in `contracts/hotkey-interpreter.md` in `crates/vox/src/tray.rs`
- [x] T005 Write unit tests for HotkeyInterpreter covering: hold-to-talk press→StartRecording and release→StopRecording, toggle first-press→start and second-press→stop, hands-free double-press(<300ms)→StartHandsFree and single-press-while-active→StopRecording, lone single-press→None after 300ms, press-during-recording→StartRecording (new session), mode switching mid-session, and release-when-not-recording→None in `crates/vox_core/tests/hotkey_interpreter_tests.rs`
- [x] T006 Replace `hold_to_talk: bool` and `hands_free_double_press: bool` fields with `activation_mode: ActivationMode` in Settings struct — add `#[serde(default)]` so existing settings.json files missing the new field deserialize correctly, update `Default` impl, remove old field references in `crates/vox_core/src/config.rs`

**Checkpoint**: `cargo test -p vox_core --features cuda` passes all HotkeyInterpreter unit tests. `cargo build -p vox --features vox_core/cuda` compiles with new tray.rs module and updated Settings. All five tray icons decode without error.

---

## Phase 3: User Story 1 — Hold-to-Talk Dictation + User Story 5 — Hotkey Feedback in Non-Ready States (Priority: P1) 🎯 MVP

**Goal**: Hold-to-talk activation works (press=start, release=stop) from any application. Hotkey always produces visible overlay feedback in every app state — downloading, loading, error, ready.

**Independent Test**: Press and hold Ctrl+Shift+Space → overlay shows "Listening..." → speak → release → text injected. Press hotkey during model download → overlay shows "Models downloading... X%". Press hotkey during error → overlay shows error message.

### Implementation

- [x] T007 [US1][US5] Rewrite hotkey event loop in `crates/vox/src/main.rs` — instantiate `HotkeyInterpreter` with mode from `VoxState.settings().activation_mode`, reduce polling timer from 50ms to 5ms, handle `GlobalHotKeyEvent.state` field (`HotKeyState::Pressed` → `interpreter.on_press()`, `HotKeyState::Released` → `interpreter.on_release()`), read `is_recording` from `VoxState` recording session state. Handle hotkey registration failure: on macOS, if Input Monitoring permission is denied, show error overlay with guidance to enable it in System Settings (FR-013); handle the macOS permission prompt that fires on first registration (FR-024)
- [x] T008 [US1][US5] Add universal hotkey response before interpreter dispatch in `crates/vox/src/main.rs` — on every hotkey event, check `AppReadiness` first: if `Downloading`/`Loading`/`Error`, show overlay (ensure overlay window is visible) and skip interpreter; if `Ready`, proceed to interpreter → action dispatch. Read current `activation_mode` from VoxState and call `interpreter.set_mode()` before each event
- [x] T009 [US1] Wire `HotkeyAction` variants to pipeline actions in `crates/vox/src/main.rs` — `StartRecording`/`StartHandsFree` → dispatch `ToggleRecording` (start new recording session), `StopRecording` → stop current recording session (drop command channel), `None` → no-op. Replace the existing simple `ToggleRecording` dispatch with this mode-aware logic

**Checkpoint**: Hold-to-talk works end-to-end (press=start, release=stop). Pressing hotkey during download/loading/error shows overlay with status. Polling interval is 5ms. `cargo build` zero warnings.

---

## Phase 4: User Story 2 — Toggle Dictation + User Story 4 — Dynamic Tray Status Awareness (Priority: P2)

**Goal**: Users can select toggle activation mode in settings. Tray icon dynamically reflects pipeline state (idle/listening/processing/downloading/error). Tray context menu expanded from 3 to 6 items.

**Independent Test (US2)**: Open Settings → set mode to Toggle → press Ctrl+Shift+Space once → "Listening..." → speak → press again → text injected. **Independent Test (US4)**: Observe tray icon change idle(gray)→listening(green)→processing(blue)→idle(gray). Right-click tray → 6 menu items all functional.

### Implementation

- [x] T010 [P] [US2] Replace `hold_to_talk` and `hands_free_double_press` boolean toggles with an activation mode dropdown selector (Hold-to-Talk / Toggle / Hands-Free) in the hotkey section of `crates/vox_ui/src/settings_panel.rs` — on change callback updates `VoxState.settings.activation_mode` via `update_settings()`, interpreter reads new mode on next hotkey event
- [x] T011 [P] [US4] Create `std::sync::mpsc::channel::<TrayUpdate>()` in `crates/vox/src/main.rs` — pass `Sender` to the state-forwarding task and `Receiver` to the tray polling task. In state-forwarding task, after updating `OverlayDisplayState`, call `derive_tray_state()` and send `TrayUpdate::SetState` on every `AppReadiness` or `PipelineState` change
- [x] T012 [US4] Implement dynamic icon switching in tray polling loop in `crates/vox/src/main.rs` — poll `TrayUpdate` receiver with `try_recv()` alongside existing `MenuEvent` polling, on `SetState` call `tray_icon.set_icon()` with the matching pre-decoded icon from `TrayIcons` and `tray_icon.set_tooltip()` with `tooltip_for_state()`
- [x] T013 [US4] Replace 3-item tray menu with 6-item menu using `create_tray_menu()` from `tray.rs` in `crates/vox/src/main.rs` — wire new menu events: Toggle Recording → start/stop recording (simple toggle bypassing activation mode), Settings → `OpenSettings` action, Show/Hide Overlay → `ToggleOverlay` action, About Vox → show version info, Quit → `cx.quit()`
- [x] T014 [US4] Add dynamic menu item text updates in `crates/vox/src/main.rs` — when recording state changes, call `toggle_item.set_text("Stop Recording")` or `toggle_item.set_text("Start Recording")` on the Toggle Recording menu item. Update alongside tray icon state changes in the polling loop

**Checkpoint**: Toggle mode works via settings dropdown. Tray icon updates within 10ms of state transitions. All 6 menu items functional. Menu text reflects recording state.

---

## Phase 5: User Story 6 — Hotkey Remapping (Priority: P3)

**Goal**: Users can remap the hotkey at runtime; new binding takes effect immediately without restart.

**Independent Test**: Open Settings → click hotkey recorder → press Ctrl+Shift+D → close settings → press Ctrl+Shift+D in any app → recording starts. Press old Ctrl+Shift+Space → nothing happens.

**Note**: User Story 3 (Hands-Free Dictation) requires no additional implementation. The `HotkeyInterpreter` (T003) handles double-press detection, action dispatch (T009) maps `StartHandsFree`, and the settings dropdown (T010) includes the Hands-Free option. Hands-free mode is testable after Phase 4 completion.

### Implementation

- [x] T015 [US6] Implement hotkey re-registration channel in `crates/vox/src/main.rs` — create `std::sync::mpsc::channel::<String>()` for new hotkey strings, poll `Receiver` in the hotkey event loop with `try_recv()`, on receiving new hotkey string: unregister old hotkey via `manager.unregister()`, parse new string with `.parse::<HotKey>()`, register new hotkey via `manager.register()`, update `Arc<AtomicU32>` with new `hotkey.id()` for event matching. On Windows, if CapsLock is the new hotkey, suppress its normal toggle behavior by not calling `CallNextHookEx` (FR-023)
- [x] T016 [US6] Wire settings panel hotkey recorder on-change callback to send new hotkey string through re-registration channel in `crates/vox/src/main.rs` — pass the `Sender<String>` clone to the settings window setup so the `HotkeyRecorder` callback can send the new binding when the user records a new key combination

**Checkpoint**: Hotkey remapping works at runtime. Old binding deregistered immediately. New binding works in any application. All three activation modes work with remapped hotkey.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all user stories

- [x] T017 Verify zero compiler warnings with `cargo build -p vox --features vox_core/cuda`
- [x] T018 Run full unit test suite with `cargo test -p vox_core --features cuda` — all HotkeyInterpreter tests pass
- [ ] T019 Execute quickstart.md manual verification checklist for all 5 areas: activation modes (hold-to-talk, toggle, hands-free), dynamic tray icons, tray context menu, hotkey remapping, universal hotkey response

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (T002 module declarations) — BLOCKS all user stories
- **US1+US5 (Phase 3)**: Depends on Phase 2 completion (T003 interpreter, T004 tray types, T006 config)
- **US2+US4 (Phase 4)**: Depends on Phase 3 (main.rs hotkey wiring must exist before adding tray channel)
- **US6 (Phase 5)**: Depends on Phase 4 (main.rs must have expanded event loop before adding re-registration)
- **Polish (Phase 6)**: Depends on all previous phases

### Within-Phase Parallelism

| Phase | Parallel Groups | Sequential Chain |
|-------|----------------|-----------------|
| Phase 1 | T001 ∥ T002 | — |
| Phase 2 | T003 ∥ T004 | T005 after T003, T006 after T003 |
| Phase 3 | — | T007 → T008 → T009 (same file, each builds on previous) |
| Phase 4 | T010 ∥ T011 | T012 → T013 → T014 (same file, sequential) |
| Phase 5 | — | T015 → T016 (sequential dependency) |
| Phase 6 | — | T017 → T018 → T019 |

### User Story Coverage Map

| User Story | Primary Tasks | Also Covered By |
|-----------|--------------|----------------|
| US1 Hold-to-Talk (P1) | T007, T008, T009 | T003 (interpreter), T006 (settings) |
| US2 Toggle (P2) | T010 | T003 (interpreter mode), T007-T009 (wiring) |
| US3 Hands-Free (P3) | — (no unique tasks) | T003 (double-press detection), T009 (StartHandsFree dispatch), T010 (settings dropdown) |
| US4 Dynamic Tray (P2) | T011, T012, T013, T014 | T004 (tray types), T001 (icon asset) |
| US5 Hotkey Feedback (P1) | T007, T008 | — |
| US6 Hotkey Remapping (P3) | T015, T016 | — |

---

## Parallel Example: Phase 2

```text
# Launch simultaneously (different crates, no dependencies):
Task T003: "Implement HotkeyInterpreter in crates/vox_core/src/hotkey_interpreter.rs"
Task T004: "Implement tray types in crates/vox/src/tray.rs"

# After T003 completes, launch simultaneously:
Task T005: "Write unit tests in crates/vox_core/tests/hotkey_interpreter_tests.rs"
Task T006: "Update Settings in crates/vox_core/src/config.rs"
```

## Parallel Example: Phase 4

```text
# Launch simultaneously (different crates):
Task T010: "Activation mode dropdown in crates/vox_ui/src/settings_panel.rs"
Task T011: "TrayUpdate channel wiring in crates/vox/src/main.rs"
```

---

## Implementation Strategy

### MVP First (Phases 1 + 2 + 3 = US1 + US5)

1. **Setup**: Create icon asset, add module declarations
2. **Foundational**: Build HotkeyInterpreter (all modes) + tray types + update settings + unit tests
3. **US1 + US5**: Wire interpreter into main.rs with press/release handling and universal response
4. **STOP and VALIDATE**: Hold-to-talk works, hotkey responds in all app states, unit tests pass

### Incremental Delivery

1. Phases 1–3 → **MVP**: Hold-to-talk + universal hotkey feedback (P1 stories)
2. Phase 4 → **P2**: Toggle mode selection + dynamic tray icons + expanded menu
3. Phase 5 → **P3**: Hotkey remapping at runtime + hands-free verified working
4. Phase 6 → **Polish**: Zero warnings, full test pass, quickstart validation

---

## Notes

- US3 (Hands-Free) has no dedicated phase because it requires zero unique implementation — the HotkeyInterpreter handles double-press detection (T003), main.rs dispatches `StartHandsFree` (T009), and the settings dropdown includes the option (T010). The pipeline already auto-segments via VAD.
- `StartHandsFree` and `StartRecording` dispatch the same pipeline action — VAD auto-segmentation is the default pipeline behavior. The distinction exists in `HotkeyAction` for potential future differentiation.
- The HotkeyInterpreter reads current `activation_mode` from `VoxState` on each event (no channel needed for mode changes — the GPUI foreground task has direct access to the global).
- The `is_recording` parameter for `on_press()`/`on_release()` comes from `VoxState` recording session state (whether `RecordingSession` exists).
- All main.rs modifications accumulate — each phase builds on the previous phase's code.
- T001 icon asset: 32×32 RGBA PNG in orange, matching the style/dimensions of existing tray-idle.png (398 bytes).
