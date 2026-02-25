# Data Model: System Tray & Global Hotkeys

**Feature**: 014-tray-hotkeys
**Date**: 2026-02-24

## Entities

### ActivationMode

Represents the user's chosen recording trigger behavior. Persisted in settings. Determines how hotkey press/release events map to start/stop recording actions.

| Variant | Description | Hotkey Behavior |
|---------|-------------|-----------------|
| HoldToTalk | Default mode. Press-and-hold to record. | Press → start, Release → stop |
| Toggle | Tap once to start, tap again to stop. | Press → start, Press again → stop |
| HandsFree | Double-press for continuous VAD-segmented recording. | Double-press → start continuous, Single press → stop |

**Serialization**: Lowercase kebab-case string in settings.json: `"hold-to-talk"`, `"toggle"`, `"hands-free"`.

**Default**: `HoldToTalk` (FR-006).

**Location**: `crates/vox_core/src/hotkey_interpreter.rs`

### HotkeyAction

The outcome of a hotkey event after interpretation through the active activation mode. Produced by `HotkeyInterpreter` and consumed by the action dispatch layer in main.rs.

| Variant | Description |
|---------|-------------|
| None | No action. Occurs when waiting for potential double-press in hands-free mode, or on release events in toggle/hands-free modes. |
| StartRecording | Begin a new recording session. Pipeline creates AudioCapture, spawns VAD thread, enters Listening state. |
| StopRecording | End the current recording session. Drop command channel, pipeline processes remaining audio, transitions to Idle. |
| StartHandsFree | Begin continuous VAD-segmented recording. Dispatches the same pipeline action as StartRecording (VAD auto-segmentation is the default pipeline behavior). The distinct variant exists so the caller can differentiate in logging or future pipeline changes. |

**Location**: `crates/vox_core/src/hotkey_interpreter.rs`

### HotkeyInterpreter

Stateful interpreter that maps hotkey press/release events to recording actions based on the active activation mode. Pure synchronous state machine with no OS dependencies, timers, or async.

| Field | Type | Description |
|-------|------|-------------|
| mode | ActivationMode | Current activation mode (from settings) |
| last_press_time | Instant | Timestamp of most recent key press (used for hands-free double-press detection) |

**Invariants**:
- The interpreter is purely synchronous — no timers, no async, no OS calls
- The interpreter does not track whether recording is active; the caller passes `is_recording: bool` as a parameter
- Mode changes via `set_mode()` take effect on the next hotkey event (the current recording session, if any, continues under the old mode's rules until it completes)
- The interpreter is not `Send` or `Sync` (holds `Instant` which is fine, but there's no need for cross-thread access — it lives on the GPUI foreground thread)

**Location**: `crates/vox_core/src/hotkey_interpreter.rs`

### TrayIconState

Maps the combined application state (readiness + pipeline) to a tray icon visual. Used by the tray management code to select the correct icon and tooltip.

| State | Icon Asset | Color | Tooltip |
|-------|-----------|-------|---------|
| Idle | tray-idle.png | Gray | "Vox — Idle" |
| Listening | tray-listening.png | Green | "Vox — Listening..." |
| Processing | tray-processing.png | Blue | "Vox — Processing..." |
| Downloading | tray-downloading.png | Orange | "Vox — Downloading models..." |
| Loading | tray-downloading.png | Orange | "Vox — Loading models..." |
| Error | tray-error.png | Red | "Vox — Error: {message}" |

**Derivation from AppReadiness + PipelineState**:

| AppReadiness | PipelineState | TrayIconState |
|-------------|---------------|---------------|
| Downloading { .. } | * | Downloading |
| Loading { .. } | * | Loading |
| Error { message } | * | Error(message) |
| Ready | Idle | Idle |
| Ready | Listening | Listening |
| Ready | Processing { .. } | Processing |
| Ready | Injecting { .. } | Processing |
| Ready | InjectionFailed { .. } | Error(error) |
| Ready | Error { message } | Error(message) |

**Location**: `crates/vox/src/tray.rs`

### TrayUpdate

Message sent from the state-forwarding code to the tray polling task to trigger icon/tooltip updates.

| Variant | Fields | Description |
|---------|--------|-------------|
| SetState | TrayIconState | Update icon and tooltip to reflect new state |

**Communication**: Sent via `std::sync::mpsc::Sender<TrayUpdate>`. Received in the tray polling loop alongside `MenuEvent` polling.

**Location**: `crates/vox/src/tray.rs`

## Settings Changes

### Settings Fields (Hotkey Section)

```json
{
  "activation_hotkey": "Ctrl+Shift+Space",
  "activation_mode": "hold-to-talk"
}
```

Replaces the old `hold_to_talk: bool` and `hands_free_double_press: bool` fields with a single `activation_mode` string.

## State Transitions

### HotkeyInterpreter State Machine

#### Hold-to-Talk Mode

```
               Press              Release
  [Not Recording] ────→ StartRecording   [Recording] ────→ StopRecording
  [Recording]     ────→ StartRecording   [Not Recording] ────→ None
                   (new session;                          (no-op)
                    old continues
                    processing)
```

#### Toggle Mode

```
               Press                Release
  [Not Recording] ────→ StartRecording    * ────→ None
  [Recording]     ────→ StopRecording          (always ignored)
```

#### Hands-Free Mode

```
               Press (not recording)
  [elapsed >= 300ms] ────→ None (record timestamp, wait for second press)
  [elapsed <  300ms] ────→ StartHandsFree (double-press detected)

               Press (recording)
  [any timing]       ────→ StopRecording

               Release
  * ────→ None (always ignored in hands-free)
```

### Tray State Reactive Updates

```
  AppReadiness change ──┐
                        ├──→ derive TrayIconState ──→ send TrayUpdate ──→ set_icon + set_tooltip
  PipelineState change ─┘
```

Triggered from:
1. `initialize_pipeline()` — readiness transitions (Downloading → Loading → Ready / Error)
2. State-forwarding GPUI task — pipeline broadcasts (Listening → Processing → Idle / Error)
3. Injection fade timer — pipeline state reset (Injecting → Idle after 2s)
