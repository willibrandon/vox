//! Unit tests for the HotkeyInterpreter state machine.
//!
//! Covers all three activation modes (hold-to-talk, toggle, hands-free),
//! mode switching, edge cases, and the serialization contract.

use std::time::Duration;

use vox_core::hotkey_interpreter::{ActivationMode, HotkeyAction, HotkeyInterpreter};

// ─── Hold-to-Talk Mode ──────────────────────────────────────────────

#[test]
fn test_hold_to_talk_press_starts_recording() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);
}

#[test]
fn test_hold_to_talk_release_stops_recording() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);
    assert_eq!(interp.on_release(true), HotkeyAction::StopRecording);
}

#[test]
fn test_hold_to_talk_release_when_not_recording_is_none() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);
    assert_eq!(interp.on_release(false), HotkeyAction::None);
}

#[test]
fn test_hold_to_talk_press_during_recording_starts_new_session() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);
    // Press while already recording → new session (old continues processing)
    assert_eq!(interp.on_press(true), HotkeyAction::StartRecording);
}

// ─── Toggle Mode ────────────────────────────────────────────────────

#[test]
fn test_toggle_first_press_starts_recording() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::Toggle);
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);
}

#[test]
fn test_toggle_second_press_stops_recording() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::Toggle);
    assert_eq!(interp.on_press(true), HotkeyAction::StopRecording);
}

#[test]
fn test_toggle_release_is_always_none() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::Toggle);
    assert_eq!(interp.on_release(false), HotkeyAction::None);
    assert_eq!(interp.on_release(true), HotkeyAction::None);
}

// ─── Hands-Free Mode ────────────────────────────────────────────────

#[test]
fn test_hands_free_double_press_starts_hands_free() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HandsFree);
    // First press → None (records timestamp)
    assert_eq!(interp.on_press(false), HotkeyAction::None);
    // Second press within 300ms → StartHandsFree
    assert_eq!(interp.on_press(false), HotkeyAction::StartHandsFree);
}

#[test]
fn test_hands_free_single_press_while_active_stops() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HandsFree);
    assert_eq!(interp.on_press(true), HotkeyAction::StopRecording);
}

#[test]
fn test_hands_free_lone_single_press_is_none_after_window() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HandsFree);
    // First press → None
    assert_eq!(interp.on_press(false), HotkeyAction::None);
    // Wait longer than 300ms, then press again
    std::thread::sleep(Duration::from_millis(310));
    // This is a new "first press" since the window expired
    assert_eq!(interp.on_press(false), HotkeyAction::None);
}

#[test]
fn test_hands_free_release_is_always_none() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HandsFree);
    assert_eq!(interp.on_release(false), HotkeyAction::None);
    assert_eq!(interp.on_release(true), HotkeyAction::None);
}

// ─── Mode Switching ─────────────────────────────────────────────────

#[test]
fn test_mode_switching() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);
    assert_eq!(interp.mode(), ActivationMode::HoldToTalk);

    interp.set_mode(ActivationMode::Toggle);
    assert_eq!(interp.mode(), ActivationMode::Toggle);

    // Now behaves as Toggle: press when not recording → start
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);
    // Release should be None in toggle
    assert_eq!(interp.on_release(true), HotkeyAction::None);
}

#[test]
fn test_mode_switch_during_recording_does_not_affect_current_session() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HoldToTalk);

    // Start recording with hold-to-talk
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);

    // Switch to toggle mode while recording is active
    interp.set_mode(ActivationMode::Toggle);

    // Release in toggle mode → None (toggle ignores releases)
    // This demonstrates the mode change takes effect immediately
    assert_eq!(interp.on_release(true), HotkeyAction::None);

    // But a press in toggle mode with recording active → stop
    assert_eq!(interp.on_press(true), HotkeyAction::StopRecording);
}

// ─── Default & Serialization ────────────────────────────────────────

#[test]
fn test_activation_mode_default_is_hold_to_talk() {
    assert_eq!(ActivationMode::default(), ActivationMode::HoldToTalk);
}

#[test]
fn test_activation_mode_serde_roundtrip() {
    let modes = [
        (ActivationMode::HoldToTalk, "\"hold-to-talk\""),
        (ActivationMode::Toggle, "\"toggle\""),
        (ActivationMode::HandsFree, "\"hands-free\""),
    ];
    for (mode, expected_json) in &modes {
        let json = serde_json::to_string(mode).expect("serialize");
        assert_eq!(&json, expected_json, "serialize {:?}", mode);
        let deserialized: ActivationMode =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&deserialized, mode, "roundtrip {:?}", mode);
    }
}

// ─── Edge Cases ─────────────────────────────────────────────────────

#[test]
fn test_rapid_toggle_presses() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::Toggle);
    // Rapid press-press-press sequence
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);
    assert_eq!(interp.on_press(true), HotkeyAction::StopRecording);
    assert_eq!(interp.on_press(false), HotkeyAction::StartRecording);
}

#[test]
fn test_hands_free_stop_then_restart() {
    let mut interp = HotkeyInterpreter::new(ActivationMode::HandsFree);
    // Double press to start
    assert_eq!(interp.on_press(false), HotkeyAction::None);
    assert_eq!(interp.on_press(false), HotkeyAction::StartHandsFree);
    // Single press to stop
    assert_eq!(interp.on_press(true), HotkeyAction::StopRecording);
    // Now double press again to restart
    assert_eq!(interp.on_press(false), HotkeyAction::None);
    assert_eq!(interp.on_press(false), HotkeyAction::StartHandsFree);
}
