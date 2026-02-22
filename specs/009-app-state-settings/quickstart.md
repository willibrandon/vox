# Quickstart: Application State & Settings

**Feature Branch**: `009-app-state-settings`
**Date**: 2026-02-21

## Build

```bash
# Windows (CUDA)
cargo build -p vox_core --features cuda

# macOS (Metal)
cargo build -p vox_core --features metal
```

## Test

```bash
# Run all vox_core tests
cargo test -p vox_core --features cuda      # Windows
cargo test -p vox_core --features metal     # macOS

# Run specific Feature 009 tests
cargo test -p vox_core test_settings --features cuda -- --nocapture
cargo test -p vox_core test_vox_state --features cuda -- --nocapture
cargo test -p vox_core test_transcript --features cuda -- --nocapture
cargo test -p vox_core test_app_readiness --features cuda -- --nocapture
```

## Verification Scenarios

### 1. VoxState Initialization (US1)

```rust
// Unit test: create VoxState from a temporary directory
let dir = tempfile::tempdir()?;
let state = VoxState::new(dir.path())?;

// Verify: settings loaded with defaults
assert_eq!(state.settings().vad_threshold, 0.5);

// Verify: database exists
assert!(dir.path().join("vox.db").exists());

// Verify: settings file created
assert!(dir.path().join("settings.json").exists());

// Verify: readiness starts at Downloading
assert!(matches!(state.readiness(), AppReadiness::Downloading { .. }));
```

### 2. Settings Round-Trip (US2)

```rust
let dir = tempfile::tempdir()?;

// Save non-default settings
let mut settings = Settings::default();
settings.vad_threshold = 0.8;
settings.theme = ThemeMode::Light;
settings.save(dir.path())?;

// Reload and verify
let loaded = Settings::load(dir.path())?;
assert_eq!(loaded.vad_threshold, 0.8);
assert_eq!(loaded.theme, ThemeMode::Light);
```

### 3. Corrupt Settings Recovery (US1)

```rust
let dir = tempfile::tempdir()?;

// Write corrupt JSON
std::fs::write(dir.path().join("settings.json"), "not json{{")?;

// Load should return defaults, not crash
let settings = Settings::load(dir.path())?;
assert_eq!(settings.vad_threshold, 0.5);  // default
```

### 4. Transcript CRUD (US3)

```rust
let dir = tempfile::tempdir()?;
let state = VoxState::new(dir.path())?;

// Save
let entry = TranscriptEntry {
    id: "test-1".into(),
    raw_text: "hello world".into(),
    polished_text: "Hello, world.".into(),
    target_app: "VSCode".into(),
    duration_ms: 2000,
    latency_ms: 150,
    created_at: "2026-02-21T10:00:00Z".into(),
};
state.save_transcript(&entry)?;

// List
let results = state.get_transcripts(10, 0)?;
assert_eq!(results.len(), 1);

// Search
let found = state.search_transcripts("hello")?;
assert_eq!(found.len(), 1);

// Delete single
state.delete_transcript("test-1")?;
assert_eq!(state.get_transcripts(10, 0)?.len(), 0);
```

### 5. Secure Delete (US5)

```rust
let dir = tempfile::tempdir()?;
let state = VoxState::new(dir.path())?;

// Save some transcripts
for i in 0..5 {
    state.save_transcript(&make_entry(&format!("id-{i}")))?;
}

// Clear history (secure delete)
state.clear_history()?;

// Verify: no transcripts remain
assert_eq!(state.get_transcripts(100, 0)?.len(), 0);

// Verify: database file is smaller after VACUUM
// (VACUUM reclaims free pages)
```

### 6. AppReadiness State Machine (US4)

```rust
let dir = tempfile::tempdir()?;
let state = VoxState::new(dir.path())?;

// Initial state
assert!(matches!(state.readiness(), AppReadiness::Downloading { .. }));

// Transition to Loading
state.set_readiness(AppReadiness::Loading {
    stage: "Loading Whisper model".into(),
});
assert!(matches!(state.readiness(), AppReadiness::Loading { .. }));

// Transition to Ready
state.set_readiness(AppReadiness::Ready);
assert!(matches!(state.readiness(), AppReadiness::Ready));
```

### 7. Settings Forward/Backward Compatibility (Edge Case)

```rust
// Forward compatibility: extra fields are ignored
let json = r#"{"vad_threshold": 0.7, "future_field": true}"#;
let settings: Settings = serde_json::from_str(json)?;
assert_eq!(settings.vad_threshold, 0.7);

// Backward compatibility: missing fields get defaults
let json = r#"{"vad_threshold": 0.7}"#;
let settings: Settings = serde_json::from_str(json)?;
assert_eq!(settings.min_silence_ms, 500);  // default
```

## Performance Validation

```rust
use std::time::Instant;

// Settings load < 10ms
let start = Instant::now();
let settings = Settings::load(dir.path())?;
assert!(start.elapsed().as_millis() < 10);

// Settings save < 10ms
let start = Instant::now();
settings.save(dir.path())?;
assert!(start.elapsed().as_millis() < 10);

// Transcript save < 5ms
let start = Instant::now();
state.save_transcript(&entry)?;
assert!(start.elapsed().as_millis() < 5);
```

## Key Files

| File | Purpose |
|------|---------|
| `crates/vox_core/src/state.rs` | VoxState, AppReadiness, Global impl |
| `crates/vox_core/src/config.rs` | Settings, OverlayPosition, ThemeMode |
| `crates/vox_core/src/pipeline/transcript.rs` | TranscriptStore (extended) |
| `crates/vox_core/Cargo.toml` | gpui dependency addition |
