# Quickstart: Overlay HUD

**Feature**: 012-overlay-hud
**Date**: 2026-02-22

## Build

```bash
# Windows (CUDA)
cargo run -p vox --features vox_core/cuda

# macOS (Metal)
cargo run -p vox --features vox_core/metal
```

## Test

```bash
# Run all vox_ui tests (includes overlay and waveform tests)
cargo test -p vox_ui

# Run specific overlay tests
cargo test -p vox_ui test_overlay

# Run specific waveform tests
cargo test -p vox_ui test_waveform

# Run with output
cargo test -p vox_ui test_overlay -- --nocapture
```

## Verify Overlay States

After launching the app, the overlay displays these states in sequence:

### Startup Sequence (automatic)
1. **DOWNLOADING** — Orange ↓ indicator. Per-model progress bars appear. Wait for all three models to download.
2. **LOADING** — Blue spinner. Stage messages cycle: "Loading ASR model...", "Loading LLM model...".
3. **IDLE** — Gray dot. Hint text: "Press [CapsLock] to start dictating".

### Dictation Cycle (press hotkey)
4. **LISTENING** — Green pulsing dot. Waveform bars animate with voice. Speak into microphone.
5. **PROCESSING** — Blue spinner. Raw transcript text appears.
6. **INJECTED** — Green checkmark. Polished text appears, fades after 2 seconds.
7. Returns to **LISTENING** (if hotkey still held) or **IDLE**.

### Error States (simulate)
8. **ERROR** — Red warning icon. Error message with guidance.
9. **INJECTION FAILED** — Yellow warning icon. Polished text visible with Copy button. Click Copy to recover text.
10. **DOWNLOAD FAILED** — Red warning icon. Model path shown with "Open Folder" / "Retry Download" buttons.

### Overlay Interaction
- **Drag**: Click and drag the overlay to reposition. Position persists across restarts.
- **Quick Settings**: Click ▾ dropdown arrow. Toggle dictation, change language.
- **Settings**: Click ≡ menu button. Opens full settings panel.
- **Opacity**: Change `overlay_opacity` in settings (0.0–1.0, default 0.85).

## File Locations

| File | Purpose |
|---|---|
| `crates/vox_ui/src/overlay_hud.rs` | OverlayHud view + OverlayDisplayState global |
| `crates/vox_ui/src/waveform.rs` | WaveformVisualizer custom Element |
| `crates/vox_ui/src/theme.rs` | Theme colors (status_loading, status_injection_failed) |
| `crates/vox_ui/src/layout.rs` | Layout constants (WAVEFORM_WIDTH, WAVEFORM_HEIGHT) |
| `crates/vox_ui/src/key_bindings.rs` | Actions (CopyInjectedText, RetryDownload, etc.) |
| `crates/vox_core/src/state.rs` | VoxState (latest_rms field) |
| `crates/vox_core/src/pipeline/state.rs` | PipelineState (InjectionFailed variant) |
| `crates/vox/src/main.rs` | State bridge: VoxState → OverlayDisplayState |

## Performance Verification

| Metric | Target | How to verify |
|---|---|---|
| Overlay render time | < 2ms/frame | GPUI frame timing in debug output |
| State update → render | < 16ms | Observe state transition responsiveness |
| Waveform update rate | 30fps | Visual smoothness during dictation |
| Waveform bar count | 50 bars | Count visible bars during Listening |
