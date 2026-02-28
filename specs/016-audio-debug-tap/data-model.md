# Data Model: Audio Debug Tap

**Feature**: 016-audio-debug-tap
**Date**: 2026-02-27

## Types

### DebugAudioLevel (New enum in config.rs)

Three-valued setting controlling which tap points are active. Persisted in `settings.json`.

```rust
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DebugAudioLevel {
    #[default]
    Off,       // discriminant 0 — no files written
    Segments,  // discriminant 1 — per-utterance files only (vad_segment + asr_input)
    Full,      // discriminant 2 — all 4 taps (streaming + per-utterance)
}
```

**Serialization**: kebab-case in JSON (`"off"`, `"segments"`, `"full"`).
**Runtime representation**: Stored as `AtomicU8` (discriminant value) for lock-free reads on the hot path.

### DebugAudioMessage (New enum in audio/debug_tap.rs)

Internal message protocol between tap call sites and the writer thread.

| Variant | Fields | Sent by | When |
|---------|--------|---------|------|
| `StartSession` | `session_id: u64`, `raw_sample_rate: u32`, `timestamp: String` | `start_session()` | Recording begins |
| `AppendRaw` | `Vec<f32>` | `tap_raw()` | Each ring buffer read (Full only) |
| `AppendResampled` | `Vec<f32>` | `tap_resampled()` | Each resampled chunk (Full only) |
| `VadSegment` | `session_id: u64`, `segment_index: u32`, `samples: Vec<f32>` | `tap_vad_segment()` | VAD emits utterance |
| `AsrInput` | `session_id: u64`, `segment_index: u32`, `samples: Vec<f32>` | `tap_asr_input()` | Orchestrator pads segment |
| `EndSession` | *(none)* | `end_session()` | Recording stops |

### DebugAudioTap (New struct in audio/debug_tap.rs)

Core struct shared across threads via `Arc<DebugAudioTap>`.

| Field | Type | Thread Safety | Purpose |
|-------|------|--------------|---------|
| `level` | `AtomicU8` | Lock-free read/write | Current debug level (Off=0, Segments=1, Full=2) |
| `sender` | `std::sync::mpsc::SyncSender<DebugAudioMessage>` | `Send + Sync` | Bounded channel (256) to writer thread |
| `session_counter` | `AtomicU64` | Lock-free increment | Monotonic session ID, reset on level change |
| `segment_counter` | `AtomicU32` | Lock-free increment | Per-session segment index, reset on start_session |
| `drop_count` | `AtomicU64` | Lock-free increment | Total try_send failures (diagnostic counter) |
| `write_error` | `Arc<AtomicBool>` | Lock-free read/write | Set by writer on I/O failure, suppresses streaming taps |
| `writer_handle` | `Mutex<Option<JoinHandle<()>>>` | Mutex for take() | Background writer thread handle |
| `debug_audio_dir` | `PathBuf` | Immutable after creation | `data_dir/debug_audio/` |
| `state_tx` | `Mutex<Option<broadcast::Sender<PipelineState>>>` | Mutex for swap | Error notification to overlay (set per recording session) |

### Settings field addition (config.rs)

```rust
// In Settings struct, under Advanced category:
#[serde(default)]
pub debug_audio: DebugAudioLevel,
```

**Default**: `DebugAudioLevel::Off` (via `#[serde(default)]` + enum's `#[default]`).
**Backward compatible**: Missing field in existing `settings.json` files deserializes to `Off`.

## File Entities (on disk)

### Debug Audio File

WAV files in `data_dir/debug_audio/`. Named by convention:

```
session-{NNN}_{ISO-timestamp}_{tap-type}[-{segment-NNN}].wav
```

| Component | Format | Example |
|-----------|--------|---------|
| Session ID | Zero-padded 3 digits | `session-001` |
| Timestamp | ISO 8601, colons replaced with dashes | `2026-02-27T10-30-45` |
| Tap type | One of: `raw-capture`, `post-resample`, `vad-segment`, `asr-input` | `raw-capture` |
| Segment index | Optional, zero-padded 3 digits | `-001` (only for per-segment taps) |

**WAV format**: mono, 32-bit float PCM. Sample rate varies by tap (native for raw-capture, 16000 for others).

### Streaming vs Per-Segment Files

| Category | Taps | Files per session | Lifecycle |
|----------|------|-------------------|-----------|
| Streaming | `raw-capture`, `post-resample` | 1 each (continuous) | Created on StartSession, appended to, finalized on EndSession |
| Per-segment | `vad-segment`, `asr-input` | 1 per utterance each | Created, written, finalized immediately per segment |

## Relationships

```
VoxState
  └── Arc<DebugAudioTap>  ──────────────┐
        │                                │
        ├── (shared with) Pipeline ──────┤
        │     └── tap_asr_input()        │
        │                                │
        └── (shared with) VAD thread ────┤
              ├── tap_raw()              │
              ├── tap_resampled()        │
              └── tap_vad_segment()      │
                                         ▼
                               Writer Thread (owns Receiver)
                                 ├── raw WavWriter (Option)
                                 ├── resample WavWriter (Option)
                                 └── per-segment WavWriters (ephemeral)
```

## State Transitions

### DebugAudioLevel

```
Off ──(user sets Segments)──→ Segments
Off ──(user sets Full)──────→ Full
Segments ──(user sets Off)──→ Off (sends EndSession if mid-recording)
Segments ──(user sets Full)─→ Full
Full ──(user sets Off)──────→ Off (sends EndSession if mid-recording)
Full ──(user sets Segments)─→ Segments
```

### Writer Thread Session State

```
Idle ──(StartSession)──→ Active (raw_writer + resample_writer open)
Idle ──(VadSegment/AsrInput without StartSession)──→ Active (auto-session created, then message processed)
Idle ──(AppendRaw/AppendResampled)──→ Idle (dropped, logged once at debug level — no streaming writers to write to)
Active ──(AppendRaw/AppendResampled)──→ Active (samples written)
Active ──(VadSegment/AsrInput)──→ Active (new file created + finalized)
Active ──(EndSession)──→ Idle (writers finalized)
Active ──(I/O error)──→ ErrorState (write_error flag set, writers dropped)
ErrorState ──(StartSession)──→ attempt dir recreation ──→ Active (flag cleared, fresh attempt)
ErrorState ──(StartSession + dir recreation fails)──→ ErrorState (notify overlay, log error)
```
