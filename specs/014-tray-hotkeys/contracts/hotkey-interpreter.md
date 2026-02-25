# API Contract: HotkeyInterpreter

**Module**: `crates/vox_core/src/hotkey_interpreter.rs`
**Visibility**: Public (consumed by `crates/vox/src/main.rs`)

## Types

```rust
/// Recording activation mode chosen by the user.
///
/// Determines how hotkey press/release events translate into
/// start/stop recording actions. Persisted in settings.json
/// as a kebab-case string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationMode {
    /// Hold the hotkey to record; release to stop.
    /// This is the default mode.
    HoldToTalk,
    /// Press once to start recording; press again to stop.
    Toggle,
    /// Double-press (within 300ms) to start continuous
    /// VAD-segmented recording; single press to stop.
    HandsFree,
}

/// Action produced by the hotkey interpreter after processing
/// a press or release event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    /// No action. Waiting for potential double-press, or
    /// the event was not actionable in the current mode.
    None,
    /// Begin a new recording session.
    StartRecording,
    /// End the current recording session and process
    /// remaining audio.
    StopRecording,
    /// Begin continuous VAD-segmented recording (hands-free).
    /// Each speech segment is processed independently.
    StartHandsFree,
}

/// Stateful interpreter that maps hotkey press/release events
/// to recording actions based on the active activation mode.
///
/// This is a pure state machine with no OS dependencies, no
/// timers, and no async. Safe to use in unit tests.
pub struct HotkeyInterpreter { /* mode, last_press_time */ }
```

## Public API

```rust
impl HotkeyInterpreter {
    /// Create a new interpreter with the given activation mode.
    pub fn new(mode: ActivationMode) -> Self;

    /// Process a key press event.
    ///
    /// `is_recording` indicates whether a recording session is
    /// currently active. The interpreter does not track this
    /// internally — the caller (main.rs) knows the authoritative
    /// recording state from RecordingSession.
    ///
    /// Returns the action to take based on the current mode.
    pub fn on_press(&mut self, is_recording: bool) -> HotkeyAction;

    /// Process a key release event.
    ///
    /// Only meaningful in hold-to-talk mode. In all other modes,
    /// returns HotkeyAction::None.
    pub fn on_release(&mut self, is_recording: bool) -> HotkeyAction;

    /// Change the activation mode.
    ///
    /// Takes effect on the next event. Does not affect any
    /// in-progress recording session.
    pub fn set_mode(&mut self, mode: ActivationMode);

    /// Get the current activation mode.
    pub fn mode(&self) -> ActivationMode;
}

impl Default for ActivationMode {
    /// Returns HoldToTalk as the default activation mode (FR-006).
    fn default() -> Self { ActivationMode::HoldToTalk }
}
```

## Behavioral Contract

### Hold-to-Talk Mode

| Event | is_recording | Result |
|-------|-------------|--------|
| Press | false | StartRecording |
| Press | true | StartRecording (new session replaces old) |
| Release | true | StopRecording |
| Release | false | None |

### Toggle Mode

| Event | is_recording | Result |
|-------|-------------|--------|
| Press | false | StartRecording |
| Press | true | StopRecording |
| Release | any | None |

### Hands-Free Mode

| Event | is_recording | Time Since Last Press | Result |
|-------|-------------|----------------------|--------|
| Press | false | N/A (first press ever) | None (record timestamp) |
| Press | false | < 300ms | StartHandsFree |
| Press | false | >= 300ms | None (record new timestamp) |
| Press | true | any | StopRecording |
| Release | any | any | None |

## Testability

The interpreter is designed for deterministic unit testing:

- **No OS dependencies**: Pure Rust, no FFI, no global state
- **No timers**: Uses `Instant` from std, which can be tested by controlling timing between calls
- **No async**: All methods are synchronous and return immediately
- **No side effects**: Returns `HotkeyAction` values; the caller decides what to do

Test categories:
1. Hold-to-talk: press starts, release stops
2. Toggle: press toggles, release ignored
3. Hands-free: double-press starts, single press stops, lone press discarded
4. Mode switching: mode change takes effect on next event
5. Edge cases: rapid presses, press during processing, mode change while recording

# API Contract: Tray Management

**Module**: `crates/vox/src/tray.rs`
**Visibility**: Crate-internal (used within `crates/vox/`)

## Types

```rust
/// Visual state of the system tray icon, derived from the
/// combination of AppReadiness and PipelineState.
#[derive(Clone, Debug)]
pub enum TrayIconState {
    /// Gray icon. Pipeline ready, no active recording.
    Idle,
    /// Green icon. Microphone active, VAD processing audio.
    Listening,
    /// Blue icon. ASR/LLM inference in progress.
    Processing,
    /// Orange icon. Models downloading or loading onto GPU.
    Downloading { tooltip_detail: String },
    /// Red icon. An error occurred.
    Error { message: String },
}

/// Command sent to the tray polling task to update the icon.
pub enum TrayUpdate {
    /// Update icon and tooltip to reflect new state.
    SetState(TrayIconState),
}
```

## Functions

```rust
/// Decode all five tray icon variants from embedded PNG bytes.
/// Called once at startup. Returns a struct holding the five
/// pre-decoded Icon values.
pub fn decode_all_tray_icons() -> TrayIcons;

/// Derive the TrayIconState from the current AppReadiness
/// and PipelineState combination.
pub fn derive_tray_state(
    readiness: &AppReadiness,
    pipeline_state: &PipelineState,
) -> TrayIconState;

/// Create the expanded 6-item tray context menu.
/// Returns the Menu and the MenuItemIds needed for event matching.
pub fn create_tray_menu() -> (Menu, TrayMenuIds);

/// Get the tooltip string for a TrayIconState.
pub fn tooltip_for_state(state: &TrayIconState) -> String;
```

## TrayMenuIds

```rust
/// Menu item IDs used to match incoming MenuEvents to actions.
pub struct TrayMenuIds {
    pub toggle_recording: MenuId,
    pub settings: MenuId,
    pub toggle_overlay: MenuId,
    pub about: MenuId,
    pub quit: MenuId,
}
```
