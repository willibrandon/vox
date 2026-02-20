# Research: Voice Activity Detection

**Feature**: 003-voice-activity-detection
**Date**: 2026-02-19

## R-001: ort 2.0.0-rc.11 API for ONNX Inference

### Decision
Use `ort` 2.0.0-rc.11 with `load-dynamic` feature for Silero VAD inference. The 2.0 API is significantly different from 1.x — no `Environment`, sessions use `Session::builder()?.commit_from_file()`, tensors use `Tensor::from_array((shape, data))`.

### Key API Patterns

**Session creation:**
```rust
use ort::session::{Session, builder::GraphOptimizationLevel};

let session = Session::builder()?
    .with_optimization_level(GraphOptimizationLevel::Level3)?
    .with_intra_threads(1)?
    .commit_from_file(model_path)?;
```

**Tensor creation (no ndarray needed):**
```rust
use ort::value::Tensor;

// f32 tensor with shape [1, 512]
let input = Tensor::from_array(([1usize, 512], audio_data.to_vec()))?;

// i64 tensor with shape [1] (sample rate)
let sr = Tensor::<i64>::from_array(([1usize], vec![16000_i64]))?;

// f32 tensor with shape [2, 1, 128] (hidden state)
let h = Tensor::from_array(([2usize, 1, 128], hidden_state.clone()))?;
```

**Running inference with named inputs:**
```rust
let outputs = session.run(ort::inputs! {
    "input" => input_tensor,
    "sr" => sr_tensor,
    "state" => h_tensor,
})?;
```

**Extracting outputs:**
```rust
let (_, output_data) = outputs["output"].try_extract_tensor::<f32>()?;
let speech_prob = output_data[0];

let (_, h_data) = outputs["stateN"].try_extract_tensor::<f32>()?;
new_hidden_state.copy_from_slice(h_data);
```

**Important: `session.run()` takes `&self` in ort 2.0** (changed from earlier RCs). Verify at compile time.

### Rationale
`ort` is already a dependency in vox_core's Cargo.toml with `load-dynamic` feature. The 2.0 API is cleaner than 1.x (no Environment boilerplate). The `load-dynamic` feature means ONNX Runtime shared library is loaded at runtime, keeping binary size small.

### Alternatives Considered
- **tract** (pure Rust ONNX): Slower inference, incomplete op coverage for Silero VAD's LSTM layers.
- **candle** (Hugging Face): Would require reimplementing Silero architecture in Rust. Unnecessary complexity.
- **Direct C FFI to onnxruntime**: `ort` already provides safe Rust bindings. No benefit to raw FFI.

## R-002: Silero VAD v5 Model Input/Output Names

### Decision
Use the official Silero VAD v5 ONNX model input/output tensor names. The official model uses:

| Role | Tensor Name | Shape | Type |
|------|------------|-------|------|
| Audio input | `input` | `[1, window_size]` | f32 |
| Sample rate | `sr` | `[1]` | i64 |
| Hidden state in | `state` | `[2, 1, 128]` | f32 |
| Speech probability | `output` | `[1, 1]` | f32 |
| Hidden state out | `stateN` | `[2, 1, 128]` | f32 |

**Note**: The feature spec uses `h`/`hn` for hidden state names. The actual official Silero VAD v5 model uses `state`/`stateN`. Implementation will use the actual model names. Add a runtime check that logs the actual input/output names on model load to catch any discrepancy.

### Rationale
Verified against the official Silero VAD repository:
- Python reference: `ort_inputs = {'input': x, 'state': self._state, 'sr': sr}`
- C++ reference: `input_node_names = {"input", "state", "sr"}; output_node_names = {"output", "stateN"}`

### Hidden State Lifecycle
- **Initialize**: `vec![0.0f32; 2 * 1 * 128]` (256 zeros)
- **Per-window**: Feed previous `stateN` output as next `state` input
- **Reset**: Fill with zeros when starting a new dictation session

## R-003: VAD Processing Thread Architecture

### Decision
The VAD processing loop runs as a dedicated thread (not a Tokio async task) that blocks on ring buffer reads with a small sleep interval. This matches the audio capture pattern from Feature 002.

### Rationale
- The ring buffer consumer (`HeapCons<f32>`) is synchronous — `pop_slice()` is non-blocking
- VAD inference via `ort` is synchronous CPU work (~0.5ms per window)
- Using a dedicated thread avoids Tokio executor overhead for tight-loop audio processing
- The only async boundary is the `mpsc::Sender<Vec<f32>>` to dispatch segments to ASR — `try_send()` is non-blocking from a sync context

### Processing Loop Pattern
```
loop {
    1. Check available samples: consumer.occupied_len()
    2. If >= 512 samples: pop into window buffer
    3. (Optional) Resample window to 16 kHz
    4. Feed 512 samples to SileroVad::process() → speech_prob
    5. Feed speech_prob to VadStateMachine::update() → Option<VadEvent>
    6. Feed samples + event to SpeechChunker::feed() → Option<Vec<f32>>
    7. If segment ready: segment_tx.try_send(segment)
    8. If < 512 samples: sleep 5ms (avoid busy-wait)
}
```

### Alternatives Considered
- **Tokio task with `tokio::time::sleep`**: Adds unnecessary async overhead for what is fundamentally a synchronous tight loop.
- **Callback-driven from audio thread**: Violates Constitution Principle II — no processing on the audio callback thread.
- **Crossbeam channel instead of tokio mpsc**: Would work, but `tokio::sync::mpsc` is already available and the ASR consumer will be async.

## R-004: Speech Padding Implementation

### Decision
The SpeechChunker maintains a circular "pre-buffer" of the last `speech_pad_ms` worth of samples (1,600 samples at 100ms × 16 kHz). When SpeechStart fires, the pre-buffer contents are prepended to the speech segment. When SpeechEnd fires, accumulation continues for an additional `speech_pad_ms` of samples before emitting.

### Rationale
- Pre-padding requires buffering audio *before* speech is detected — we can't go back in time
- A small circular buffer (1,600 × 4 bytes = 6.4 KB) is negligible memory cost
- Post-padding is simpler: just continue accumulating for the configured duration after SpeechEnd
- This matches the approach used in the official Silero VAD Python implementation

### Force-Segment Overlap
For segments exceeding 10 seconds that are force-segmented at `max_speech_ms`:
- The last 1 second (16,000 samples, 64 KB) of the emitted segment is copied to the start of the next segment's buffer
- This provides ASR stitching context without re-running VAD on the overlap

## R-005: Ring Buffer Read Strategy

### Decision
Use `ringbuf::traits::Consumer::pop_slice()` to read samples in batches. The processing loop reads all available samples into a local accumulation buffer, then extracts 512-sample windows from that buffer.

### Rationale
- `pop_slice()` is lock-free and returns immediately with however many samples are available
- Reading in batches (rather than exactly 512 at a time) handles the case where the audio callback writes at a different granularity than 512 samples
- A local accumulation buffer decouples the ring buffer read size from the VAD window size
- Must `use ringbuf::traits::Observer` for `occupied_len()` and `use ringbuf::traits::Consumer` for `pop_slice()`
