# Tasks: Audio Capture Pipeline

**Input**: Design documents from `/specs/002-audio-capture/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md

**Tests**: Required — spec explicitly defines unit and integration test tables.

**Organization**: Tasks are grouped by user story. Ring buffer is foundational (used by US1 capture and US2 resampling). Setup creates module structure and adds missing dependencies.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Exact file paths included in all descriptions

## Path Conventions

All work in the `vox_core` crate:
- `crates/vox_core/Cargo.toml` — crate manifest
- `crates/vox_core/src/audio.rs` — module root (exists, empty)
- `crates/vox_core/src/audio/` — submodule directory (to be created)
  - `capture.rs` — AudioCapture, AudioConfig, AudioDeviceInfo, list_input_devices()
  - `ring_buffer.rs` — AudioRingBuffer wrapper
  - `resampler.rs` — AudioResampler wrapper

---

## Phase 1: Setup (Dependencies & Module Structure)

**Purpose**: Add missing Cargo dependencies and create the audio submodule directory structure

- [ ] T001 Add `audioadapter = "0.2"` and `audioadapter-buffers = { version = "2.0", features = ["std"] }` to `crates/vox_core/Cargo.toml` under the `# Audio` section
- [ ] T002 Create directory `crates/vox_core/src/audio/` with three empty files: `capture.rs`, `ring_buffer.rs`, `resampler.rs`

---

## Phase 2: Foundational (Module Root & Ring Buffer)

**Purpose**: Declare submodules and implement the ring buffer — the shared communication channel between the audio callback thread (US1) and the processing thread (US2)

**CRITICAL**: No user story work can begin until this phase is complete

- [ ] T003 Update `crates/vox_core/src/audio.rs` with submodule declarations (`pub mod capture; pub mod ring_buffer; pub mod resampler;`) and public re-exports of key types (`AudioConfig`, `AudioCapture`, `AudioRingBuffer`, `AudioResampler`, `AudioDeviceInfo`, `list_input_devices`)
- [ ] T004 Implement `AudioRingBuffer::new(capacity: usize)` in `crates/vox_core/src/audio/ring_buffer.rs` — create `HeapRb::<f32>::new(capacity)`, call `rb.split()`, return `(HeapProd<f32>, HeapCons<f32>)`. Include `capacity_for_rate(sample_rate: u32) -> usize` helper that computes `(sample_rate * 2).next_power_of_two()`. Import `ringbuf::traits::*` for `Split` trait.
- [ ] T005 [P] Implement `AudioConfig` struct in `crates/vox_core/src/audio/capture.rs` with fields: `sample_rate: u32` (default 16000), `channels: u16` (default 1), `device_name: Option<String>` (default None). Include `Default` impl.
- [ ] T006 [P] Write `test_ring_buffer_basic` in `crates/vox_core/src/audio/ring_buffer.rs` — create buffer with capacity 1024, write 512 samples via `push_slice`, read via `pop_slice`, verify content matches. Use `ringbuf::traits::{Producer, Consumer, Observer}`.
- [ ] T007 [P] Write `test_ring_buffer_overflow` in `crates/vox_core/src/audio/ring_buffer.rs` — create buffer with capacity 64, write 128 samples, verify no panic, verify `push_slice` returns 64 (only first 64 written), read and verify those 64 are correct.
- [ ] T008 [P] Write `test_ring_buffer_concurrent` in `crates/vox_core/src/audio/ring_buffer.rs` — spawn producer thread writing 10000 sequential f32 values via `push_slice`, consumer thread reading via `pop_slice`, join both threads, verify all values received in order with no corruption.
- [ ] T009 Verify ring buffer tests pass: `cargo test -p vox_core --features cuda -- ring_buffer`

**Checkpoint**: Ring buffer is functional and tested — audio callback can write, processing thread can read

---

## Phase 3: User Story 1 — Capture Microphone Audio (Priority: P1) 🎯 MVP

**Goal**: Capture audio from the default microphone into the ring buffer. Audio samples flow continuously. Capture starts and stops cleanly.

**Independent Test**: Start capture, speak into microphone, verify samples appear in buffer within 100 ms. Stop capture, verify no hanging threads.

- [ ] T010 [US1] Implement `AudioCapture` struct in `crates/vox_core/src/audio/capture.rs` with fields: `stream: Option<cpal::Stream>`, `device_name: String`, `native_sample_rate: u32`, `error_flag: Arc<AtomicBool>`, `consumer: HeapCons<f32>`. Implement `AudioCapture::new(config: &AudioConfig)` — select device by name or default via `cpal::default_host()`, get `default_input_config()`, compute ring buffer capacity via `capacity_for_rate()`, create `AudioRingBuffer`, store consumer, return `Result<Self>`.
- [ ] T011 [US1] Implement `AudioCapture::start()` in `crates/vox_core/src/audio/capture.rs` — call `device.build_input_stream::<f32>()` with `&config.into()` (StreamConfig), data callback that extracts first channel via `step_by(channels)` and calls `producer.push_slice()`, error callback that sets `error_flag` on `StreamError::DeviceNotAvailable`. Call `stream.play()`. Store stream in `self.stream`.
- [ ] T012 [US1] Implement `AudioCapture::stop()`, `device_name()`, `native_sample_rate()`, `consumer()`, and `is_disconnected()` accessors in `crates/vox_core/src/audio/capture.rs` — `stop()` drops the stream by setting `self.stream = None`. `consumer()` returns `&mut HeapCons<f32>`. `is_disconnected()` reads the `error_flag`.
- [ ] T013 [P] [US1] Write `test_capture_to_buffer` in `crates/vox_core/src/audio/capture.rs` — create `AudioCapture` with default config, call `start()`, sleep 200ms, read from consumer via `pop_slice`, assert samples received (count > 0). Mark `#[ignore]` for CI (requires microphone hardware).
- [ ] T014 [P] [US1] Write `test_capture_stop_clean` in `crates/vox_core/src/audio/capture.rs` — create `AudioCapture`, start, stop, start again, stop again. Verify no panic, no hang. Wrap in `std::thread::spawn` with 2-second timeout to detect hangs. Mark `#[ignore]` for CI.
- [ ] T015 [US1] Verify capture tests pass (with microphone): `cargo test -p vox_core --features cuda -- capture -- --ignored --nocapture`

**Checkpoint**: User Story 1 complete — audio flows from microphone into ring buffer, capture starts/stops cleanly

---

## Phase 4: User Story 2 — Resample to Pipeline Format (Priority: P1)

**Goal**: Resample audio from device native rate (44.1/48/96 kHz) to 16 kHz mono f32 PCM. Bypass when already 16 kHz.

**Independent Test**: Feed known sine waves at 44.1 kHz and 48 kHz, verify 16 kHz output preserves frequency. Verify 16 kHz input returns None (bypass).

- [ ] T016 [US2] Implement `AudioResampler::new(input_rate: u32, output_rate: u32)` in `crates/vox_core/src/audio/resampler.rs` — return `None` if rates match. Otherwise create `Fft::<f32>::new(input_rate as usize, output_rate as usize, 1024, 1, 1, FixedSync::Input)`. Store resampler, rates, and chunk_size. Import from `rubato::{Resampler, Fft, FixedSync}`.
- [ ] T017 [US2] Implement `AudioResampler::process(&mut self, input: &[f32]) -> Result<Vec<f32>>` in `crates/vox_core/src/audio/resampler.rs` — wrap input in `SequentialSliceOfVecs::new()`, allocate output buffer via `process_all_needed_output_len()`, create output adapter, call `process_all_into_buffer()`, return output samples trimmed to `frames_written`. Import from `audioadapter_buffers::direct::SequentialSliceOfVecs`.
- [ ] T018 [P] [US2] Write `test_resampler_48000_to_16000` in `crates/vox_core/src/audio/resampler.rs` — generate 48000 samples of a 440 Hz sine wave at 48 kHz, resample to 16 kHz, verify output length is approximately 16000 samples (±5%), verify the dominant frequency in the output is still ~440 Hz (use zero-crossing count or simple frequency estimation).
- [ ] T019 [P] [US2] Write `test_resampler_44100_to_16000` in `crates/vox_core/src/audio/resampler.rs` — same as T018 but with 44100 Hz input rate and 44100 input samples.
- [ ] T020 [P] [US2] Write `test_resampler_16000_bypass` in `crates/vox_core/src/audio/resampler.rs` — call `AudioResampler::new(16000, 16000)`, assert it returns `None`.
- [ ] T021 [US2] Verify resampler tests pass: `cargo test -p vox_core --features cuda -- resampler`

**Checkpoint**: User Story 2 complete — audio is resampled to 16 kHz, 16 kHz bypass works, frequency preservation verified

---

## Phase 5: User Story 3 — Switch Microphones (Priority: P2)

**Goal**: Switch the active input device at runtime. Detect device disconnection and report it to the pipeline.

**Independent Test**: Start capture on device A, call switch_device for device B, verify capture resumes. Disconnect device, verify `is_disconnected()` returns true.

- [ ] T022 [US3] Implement `AudioCapture::switch_device(&mut self, device_name: Option<&str>) -> Result<()>` in `crates/vox_core/src/audio/capture.rs` — stop current stream, clear consumer buffer via `pop_slice` drain loop, select new device, get new config, rebuild ring buffer if native rate changed (update consumer), rebuild and start stream on new device, reset error_flag.
- [ ] T023 [US3] Implement `AudioCapture::needs_resampler_update(&self, current_resampler_rate: u32) -> bool` in `crates/vox_core/src/audio/capture.rs` — returns true if `native_sample_rate` differs from `current_resampler_rate`, signaling the caller to recreate the `AudioResampler` with the new rate.

**Checkpoint**: User Story 3 complete — devices can be switched at runtime, disconnection is detected

---

## Phase 6: User Story 4 — Enumerate Audio Devices (Priority: P2)

**Goal**: List all available input devices with name and default status for the settings UI.

**Independent Test**: Call `list_input_devices()`, verify at least one device returned on a system with a microphone.

- [ ] T024 [US4] Implement `AudioDeviceInfo` struct and `pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>>` in `crates/vox_core/src/audio/capture.rs` — iterate `host.input_devices()`, get name via `device.description()?.name().to_string()`, compare against `host.default_input_device()` to set `is_default`. Return the list.
- [ ] T025 [US4] Write `test_device_enumeration` in `crates/vox_core/src/audio/capture.rs` — call `list_input_devices()`, assert `Ok`, assert list is not empty, assert exactly one device has `is_default == true`. Mark `#[ignore]` for CI (requires audio hardware).

**Checkpoint**: User Story 4 complete — settings UI can display available input devices

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all success criteria

- [ ] T026 Verify zero compiler warnings: `cargo build -p vox_core --features cuda` must produce no warnings in any `audio/` module file
- [ ] T027 Run full audio test suite: `cargo test -p vox_core --features cuda -- audio` and verify all non-ignored tests pass
- [ ] T028 Validate `specs/002-audio-capture/quickstart.md` — run each build and test command listed, update if any steps are incorrect

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (deps and files must exist) — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 (ring buffer and AudioConfig must exist)
- **US2 (Phase 4)**: Depends on Phase 2 (ring buffer must exist for consumer concept). Can run in parallel with US1 (different file: resampler.rs)
- **US3 (Phase 5)**: Depends on US1 (AudioCapture must exist to add switch_device)
- **US4 (Phase 6)**: Depends on Phase 2 (capture.rs must exist). Can run in parallel with US1/US2 (adds independent function)
- **Polish (Phase 7)**: Depends on all user stories being complete

### User Story Dependencies

- **US1 (P1)**: Depends on Phase 2 — No dependencies on other stories
- **US2 (P1)**: Depends on Phase 2 — Independent of US1 (different file: resampler.rs vs capture.rs)
- **US3 (P2)**: Depends on US1 — adds `switch_device()` to existing `AudioCapture`
- **US4 (P2)**: Depends on Phase 2 — adds `AudioDeviceInfo` and `list_input_devices()` to capture.rs. Can be done after T005 (AudioConfig exists) or in parallel with US1

### Within Each Phase

- Phase 2: T003 (module declarations) must be first. T004 and T005 can be parallel (different files). T006–T008 depend on T004. T009 depends on T006–T008.
- Phase 3: T010 before T011 before T012. T013 and T014 depend on T012. T015 depends on T013–T014.
- Phase 4: T016 before T017. T018–T020 depend on T016–T017. T021 depends on T018–T020.
- Phase 5: T022 before T023. Both depend on US1 completion.
- Phase 6: T024 before T025.

### Parallel Opportunities

- Phase 1: T001 and T002 can run in parallel (different files)
- Phase 2: T004 and T005 can run in parallel after T003 (different files). T006, T007, T008 can all run in parallel (same file but independent test functions).
- Phase 3–4: US1 and US2 can run in parallel (capture.rs vs resampler.rs)
- Phase 5–6: NOT parallel — US3 modifies capture.rs which US4 also modifies. Execute sequentially.

---

## Parallel Example: Phase 2 Foundational

```text
# Step 1: Module root first (other files import from it)
Task T003: Update audio.rs with submodule declarations

# Step 2: Ring buffer and config in parallel (different files)
Task T004: Implement AudioRingBuffer in ring_buffer.rs
Task T005: Implement AudioConfig in capture.rs

# Step 3: Ring buffer tests in parallel (independent test functions)
Task T006: test_ring_buffer_basic
Task T007: test_ring_buffer_overflow
Task T008: test_ring_buffer_concurrent

# Step 4: Verify
Task T009: Run ring buffer tests
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (deps + directory)
2. Complete Phase 2: Foundational (module root + ring buffer + AudioConfig)
3. Complete Phase 3: US1 — Capture Microphone Audio
4. **STOP and VALIDATE**: `cargo test -p vox_core --features cuda -- audio` passes, samples flow from microphone to buffer

### Incremental Delivery

1. Phase 1 + 2 → Foundation ready (ring buffer tested, module compiles)
2. Phase 3: US1 → Audio capture works → **MVP complete**
3. Phase 4: US2 → Resampling works (can process audio to 16 kHz)
4. Phase 5: US3 → Device switching works
5. Phase 6: US4 → Device enumeration works (settings UI can list devices)
6. Phase 7: Polish → Zero warnings, full test suite, quickstart validated

### Single Developer (Current Setup)

1. Complete all phases sequentially: 1 → 2 → 3 → 4 → 5 → 6 → 7
2. US1 and US2 could be interleaved (different files) but sequential is simpler
3. Estimated: Phase 1–2 = small (file creation + ring buffer), Phase 3 = bulk of work (cpal integration), Phase 4 = moderate (rubato integration), Phase 5–6 = small (extensions to existing code)

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Hardware-dependent tests (capture, device enumeration) are marked `#[ignore]` for CI — run manually with `--ignored`
- Ring buffer overflow drops **newest** samples (ringbuf 0.4 SPSC behavior) — spec says "oldest" but the 2.7-second buffer makes this distinction irrelevant
- `HeapProd<f32>` has **no lifetime parameter** — it's `CachingProd<Arc<HeapRb<f32>>>`, inherently `'static`
- rubato 1.0 requires `audioadapter` + `audioadapter-buffers` crates (not in original Cargo.toml)
- No `mod.rs` files — modern Rust convention per CLAUDE.md
