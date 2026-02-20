# Research: Speech Recognition (ASR)

**Feature**: 004-speech-recognition
**Date**: 2026-02-19

## R-001: whisper-rs 0.15.1 API Surface

**Decision**: Use whisper-rs 0.15.1 from crates.io (source on Codeberg/GitHub).
**Rationale**: Already in Cargo.toml with `cuda` and `metal` feature gates. Provides safe Rust FFI bindings over whisper.cpp.
**Alternatives considered**: Direct whisper.cpp C bindings (too much unsafe code), candle-whisper (pure Rust but slower, no whisper.cpp optimizations).

### API Findings (from source code reading)

These findings come from reading the actual whisper-rs 0.15.1 source at `~/.cargo/registry/src/index.crates.io-*/whisper-rs-0.15.1/src/`.

#### WhisperContext

- **File**: `whisper_ctx_wrapper.rs`
- Wraps `Arc<WhisperInnerContext>` internally (already reference-counted)
- `new_with_params(path: &str, parameters: WhisperContextParameters) -> Result<Self, WhisperError>`
- `create_state(&self) -> Result<WhisperState, WhisperError>` — takes `&self`, not `&mut self`
- `WhisperInnerContext` holds a raw pointer (`*mut whisper_rs_sys::whisper_context`)

#### WhisperState

- **File**: `whisper_state.rs`
- Has `unsafe impl Send for WhisperState {}` and `unsafe impl Sync for WhisperState {}`
- `full(&mut self, params: FullParams, data: &[f32]) -> Result<c_int, WhisperError>`
  - Returns `Result<c_int>`, NOT `Result<()>`
  - **CRITICAL**: Returns `Err(WhisperError::NoSamples)` when `data.is_empty()`
- `full_n_segments(&self) -> c_int` — returns plain `c_int`, NOT `Result`
- `get_segment(i: c_int) -> Option<WhisperSegment<'_>>` — safe segment access
- `as_iter(&self) -> WhisperStateSegmentIterator<'_>` — iterator over segments

#### WhisperSegment

- **File**: `whisper_state/segment.rs`
- `to_str(&self) -> Result<&str, WhisperError>` — get segment text
- `to_str_lossy(&self) -> Result<Cow<'_, str>, WhisperError>` — lossy UTF-8
- `start_timestamp(&self) -> i64` and `end_timestamp(&self) -> i64`
- `no_speech_probability(&self) -> f32`

#### FullParams

- **File**: `whisper_params.rs`
- `new(sampling_strategy: SamplingStrategy) -> FullParams`
- `set_suppress_nst(bool)` — suppresses non-speech tokens. **NOT** `set_suppress_non_speech_tokens`
- `set_print_progress(bool)` — defaults to **true**; must explicitly set to false
- `set_no_speech_thold(f32)` — no-speech threshold
- `set_language(Option<&str>)`, `set_no_context(bool)`, `set_single_segment(bool)`, `set_n_threads(c_int)`

#### SamplingStrategy

- `Greedy { best_of: c_int }` — fastest, best_of=1 for our use
- `BeamSearch { beam_size: c_int, patience: c_float }` — not needed

## R-002: Design Doc vs. Actual API Discrepancies

Five discrepancies found between the design doc (`docs/design.md` §4.3) and the actual whisper-rs 0.15.1 API:

| # | Design Doc Says | Actual API | Impact |
|---|----------------|-----------|--------|
| 1 | `state.full_get_segment_text(i)?` | `state.get_segment(i).map(\|s\| s.to_str())` or iterator | Must use segment/iterator API |
| 2 | `set_suppress_non_speech_tokens(true)` | `set_suppress_nst(true)` | Different method name |
| 3 | Empty audio returns empty text | `state.full()` returns `Err(WhisperError::NoSamples)` | Must catch this error and return `Ok(String::new())` |
| 4 | `state.full()` returns `Result<()>` | Returns `Result<c_int, WhisperError>` | Must handle return value |
| 5 | `set_print_progress` not mentioned | Defaults to `true`, prints to stdout | Must set to `false` explicitly |

## R-003: Thread Safety Model

**Decision**: Wrap `WhisperContext` in `Arc<Mutex<WhisperContext>>`.
**Rationale**: `WhisperContext` wraps `Arc<WhisperInnerContext>` which holds a raw pointer. While the docs.rs listing shows `Copy` + `Send` + `Sync`, the underlying `whisper_rs_sys::whisper_context` is a C struct that is NOT thread-safe for concurrent access. Creating a `WhisperState` internally calls `whisper_init_state()` which accesses shared context state. The Mutex serializes all access to the context, including state creation and inference.

**Pattern**: `AsrEngine { ctx: Arc<Mutex<WhisperContext>> }` with `#[derive(Clone)]` for cheap cloning via `Arc::clone`.

**Per-transcription state**: A new `WhisperState` is created for each `transcribe()` call via `ctx.create_state()`. The state is dropped after the call. This prevents cross-utterance contamination per spec FR-004/FR-008.

## R-004: Empty/Silent Audio Handling

**Decision**: Catch `WhisperError::NoSamples` and return `Ok(String::new())`.
**Rationale**: Spec FR-003 requires empty string (not error) for silent input. The whisper-rs API returns an error for empty input. Our wrapper must handle this at the boundary.

**Additional check**: For audio that has samples but is purely silence, Whisper will produce segments with high `no_speech_probability()`. We let Whisper's internal `no_speech_thold` (set to 0.6) handle this — segments below the threshold produce empty or whitespace-only text that we trim.

## R-005: Force-Segment Stitching Strategy

**Decision**: Word-level overlap deduplication using longest common subsequence (LCS) on word tokens.
**Rationale**: When the VAD force-segments long speech at 30 seconds with 1-second overlap, both the tail of segment N and the head of segment N+1 will transcribe the same ~1 second of audio. The overlap produces similar (but not always identical) word sequences. LCS on whitespace-split tokens identifies the shared region and removes duplicates.

**Algorithm**:
1. Split previous segment's trailing text and next segment's leading text into word tokens
2. Find the longest common subsequence of words between the tail of previous and head of next
3. Trim the duplicate words from the beginning of the next segment
4. Concatenate with a space

**Alternatives considered**: Character-level diff (too fragile with punctuation), timestamp-based alignment (requires segment-level timing which adds complexity), fixed word count overlap (unreliable because speech rate varies).

## R-006: GPU Backend Configuration

**Decision**: Feature-gated GPU support via existing `cuda` and `metal` features in Cargo.toml.
**Rationale**: `whisper-rs/cuda` and `whisper-rs/metal` are already wired in the feature gates. `WhisperContextParameters::use_gpu(true)` enables GPU at runtime. Flash attention disabled by default (do not enable it).

**Build dependencies**: whisper-rs-sys builds whisper.cpp from source via CMake. On Windows, requires `CMAKE_GENERATOR="Visual Studio 17 2022"` and CUDA Toolkit. On macOS, requires Xcode + Metal Toolchain.

## R-007: Model File

**Decision**: Whisper Large V3 Turbo with Q5_0 quantization (`ggml-large-v3-turbo-q5_0.bin`).
**Rationale**: 6x faster than Large V3 with ~1% WER degradation. ~900 MB on disk, ~1.8 GB VRAM. Fits within the 6 GB total VRAM budget alongside future LLM model.

**Test fixture**: The full model (~900 MB) is too large for CI. Tests marked `#[ignore]` require the model file at a configurable path. For development, the model lives at a path supplied to `AsrEngine::new()`.
