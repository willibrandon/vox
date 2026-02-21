# Research: Model Management

**Branch**: `008-model-management` | **Date**: 2026-02-21

## Decision 1: HTTP Streaming Approach

**Decision**: Use reqwest `Response::chunk()` method for streaming downloads.

**Rationale**: The `.chunk()` method returns `Option<Bytes>` directly without requiring Stream traits. This avoids adding `futures-util` as a dependency solely for `StreamExt::next()`. The download loop becomes a simple `while let Some(chunk) = response.chunk().await?` pattern. Since Vox already depends on reqwest 0.12 with the `stream` feature, no additional dependencies are needed.

**Alternatives considered**:
- `bytes_stream()` + `futures_util::StreamExt` — More composable with the futures ecosystem but requires adding `futures-util` as a dependency for a single trait method. Rejected to minimize dependency surface.
- `bytes()` (buffer entire response) — Would buffer up to 1.6 GB in memory. Violates streaming requirement. Rejected.

## Decision 2: File I/O Strategy

**Decision**: Use `std::fs::File` inside `tokio::task::spawn_blocking` for the write loop. Hash and write each chunk in the same blocking task, fed via a bounded `tokio::sync::mpsc` channel from the async download stream.

**Rationale**: `tokio::fs::File` dispatches every `write_all` call to the blocking thread pool via `spawn_blocking` internally, creating overhead for many small writes. More critically, when `tokio::fs::File::write_all()` completes, the data has only been delivered to the tokio runtime — not necessarily to the kernel. A `tokio::fs::rename` after write completion could move a file whose data has not been flushed. By owning the `std::fs::File` directly in a single blocking task, we guarantee that `file.sync_all()` truly flushes to disk before signaling completion to the async side.

**Alternatives considered**:
- `tokio::fs::File` + `BufWriter` + explicit `flush().await` — Simpler code but the flush semantics are unclear (tokio's File flush only ensures the write buffer is submitted to spawn_blocking, not that the OS has written to disk). For atomic writes where we rename after verification, we need real fsync guarantees. Rejected.
- Writing directly from the async stream without spawn_blocking — Mixes async I/O with blocking file I/O, which can starve the tokio runtime if the disk is slow. Rejected.

## Decision 3: SHA-256 Hashing Strategy

**Decision**: Compute SHA-256 hash inline during download — each chunk is fed to `Sha256::update()` as it arrives, before writing to disk. The hash is finalized after the last chunk.

**Rationale**: Avoids re-reading the entire file after download. For a 1.6 GB model, re-reading would add several seconds and duplicate all I/O. Inline hashing adds negligible overhead (SHA-256 throughput on modern CPUs with SHA-NI extensions exceeds 2 GB/s). The `sha2` crate's `Digest` trait supports incremental updates via `update(&chunk)`.

**Alternatives considered**:
- Hash after download by re-reading the file — Wastes I/O and time. For 1.6 GB, adds 2-5 seconds depending on disk speed. Rejected.
- Use `ring` crate instead of `sha2` — `ring` provides constant-time comparison but is a heavier dependency with C code. `sha2` is pure Rust and sufficient since we're comparing hashes, not doing cryptographic operations. Rejected.

## Decision 4: Progress Throttling

**Decision**: Use `std::time::Instant::elapsed()` check per chunk, emitting a broadcast event only when >= 500ms have elapsed since the last emission. Always emit a final 100% event after the last chunk.

**Rationale**: Simple and zero-overhead. No extra tasks, timers, or channels needed. The 500ms throttle prevents UI thrashing (FR-007) while keeping progress responsive. The final event guarantees the UI shows completion.

**Alternatives considered**:
- `tokio::time::interval(500ms)` with `select!` — More complex, requires a separate timer task, and risks missing the final progress update. Rejected.
- Percentage-based throttling (emit every 1%) — Irregular timing, poor UX on fast connections where chunks arrive rapidly. Rejected.

## Decision 5: Concurrency Model

**Decision**: `tokio::spawn` per model download with a `Vec<JoinHandle<Result<()>>>`. Await all handles sequentially to collect results.

**Rationale**: Independent failure isolation — if one download panics, the others continue. With only 3 concurrent downloads, no semaphore is needed. Each task owns its own cloned `Client` handle (reqwest Client uses an internal connection pool, so cloning is cheap). The `JoinHandle` vector lets us await all downloads and report per-model success/failure.

**Alternatives considered**:
- `futures::future::join_all` — Runs all futures on the same task, so a panic in one kills all. Also requires adding `futures` crate. Rejected.
- Sequential downloads — Violates FR-004 (concurrent downloads required). Rejected.
- `tokio::task::JoinSet` — Slightly more ergonomic but unordered completion makes per-model error reporting harder. Rejected for simplicity.

## Decision 6: Progress Event Distribution

**Decision**: `tokio::sync::broadcast::channel::<DownloadEvent>(16)` for distributing progress events to multiple consumers.

**Rationale**: Broadcast channels support multiple independent receivers (overlay HUD, model panel, log viewer) without coupling. Capacity 16 is sufficient — with 3 downloads emitting at most every 500ms, that's 6 events/second, giving ~2.6 seconds of buffer. `RecvError::Lagged` is acceptable for progress events since they're idempotent; a receiver that misses updates just sees a jump in the progress bar.

**Alternatives considered**:
- `tokio::sync::watch` — Only stores the latest value, losing intermediate events like Started/Failed. Rejected.
- `tokio::sync::mpsc` — Single consumer only. Multiple UI components need to observe progress independently. Rejected.
- Callback-based approach — Couples the downloader to specific UI components. Rejected.

## Decision 7: Atomic File Writes

**Decision**: Download to `{filename}.tmp` in the same directory, verify SHA-256, then `std::fs::rename` to the final filename. Delete `.tmp` on verification failure.

**Rationale**: `std::fs::rename` is atomic on the same filesystem on all platforms (POSIX `rename(2)`, Windows `MoveFileExW`). Since both files are in the same models directory, cross-filesystem rename failures cannot occur. The `.tmp` suffix follows the pattern already established in the bash download script (`scripts/download-models.sh`).

**Alternatives considered**:
- `tempfile::NamedTempFile::persist()` — Would require promoting `tempfile` from dev-dependency to dependency. The manual `.tmp` + rename pattern is simpler and matches existing conventions. Rejected.
- Writing directly to the final filename — Partial files would be mistaken for valid models. Violates FR-006. Rejected.

## Decision 8: Platform Path Resolution

**Decision**: Use `dirs` crate v5 for `data_local_dir()` (Windows) and `data_dir()` (macOS).

**Rationale**: `dirs` is the standard Rust crate for platform-specific directory resolution. It correctly handles `%LOCALAPPDATA%` on Windows and `~/Library/Application Support` on macOS. The app identifier `com.vox.app` is appended manually to match the spec's path convention.

**Alternatives considered**:
- `std::env::var("LOCALAPPDATA")` + manual path construction — Platform-specific, error-prone, doesn't handle macOS. Rejected.
- `directories` crate (with `ProjectDirs`) — More opinionated about directory naming (uses reverse-domain conventions that may not match our path structure). Rejected for explicit control.
- Hardcoded paths — Not portable. Rejected.

## Decision 9: Magic Byte Validation

**Decision**: Read the first 4 bytes of a file and match against known magic bytes:
- GGUF: `0x47 0x47 0x55 0x46` ("GGUF") — Qwen LLM and modern GGML models
- GGML variants: `0x67 0x67 0x6D 0x6C` ("ggml"), `0x67 0x67 0x6D 0x66` ("ggmf"), `0x67 0x67 0x6A 0x74` ("ggjt") — Whisper ASR models
- ONNX: If not GGUF/GGML, check for protobuf wire format (first byte `0x08` = field 1, varint type) — Silero VAD

**Rationale**: File extension is unreliable for model swapping (user could rename a file). Magic bytes are definitive. The format uniquely maps to model type (ONNX = VAD, GGML = ASR, GGUF = LLM), enabling automatic slot detection during model swapping.

**Alternatives considered**:
- Extension-based detection — Unreliable. A user could rename a .gguf to .onnx. Rejected.
- Full protobuf parsing for ONNX — Overly complex for a simple format check. The protobuf wire format check is sufficient. Rejected.
- Requiring explicit model type in filename — Poor UX for model swapping. Rejected.

## Decision 10: Model Registry (Verified Data)

**Decision**: Use the following verified model data from `scripts/download-models.sh`:

| Model | Filename | URL | SHA-256 | Size |
|-------|----------|-----|---------|------|
| Silero VAD v5 | `silero_vad_v5.onnx` | `https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx` | `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` | ~1.1 MB |
| Whisper Large V3 Turbo Q5_0 | `ggml-large-v3-turbo-q5_0.bin` | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` | `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2` | ~900 MB |
| Qwen 2.5 3B Instruct Q4_K_M | `qwen2.5-3b-instruct-q4_k_m.gguf` | `https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf` | `626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d` | ~1.6 GB |

**Rationale**: These SHA-256 hashes are from the existing verified download script that has been used successfully in development. The Qwen URL uses the official `Qwen/` HuggingFace organization (not the community `bartowski/` mirror) for reliability.

## Decision 11: Directory Polling Architecture

**Decision**: Polling runs as a `tokio::spawn` task with a `tokio::time::interval(Duration::from_secs(5))` loop. The task checks for model file existence on each tick and emits `DownloadEvent::DetectedOnDisk` via the broadcast channel when a file is detected. The task cancels itself when all models are present.

**Rationale**: A 5-second poll interval (FR-012) is lightweight (one directory listing every 5 seconds). Using a background task with a broadcast channel keeps the polling decoupled from the UI. The task self-terminates to satisfy FR-014 (no background activity after all models present).

**Alternatives considered**:
- Filesystem watcher (`notify` crate) — Cross-platform filesystem watchers are unreliable (inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows). Each has different edge cases and requires an additional dependency. A 5-second poll is simple, reliable, and meets the spec requirement. Rejected.
- Manual timer in the UI layer — Couples polling logic to the UI. Rejected.

## Decision 12: Open Folder Implementation

**Decision**: Use `std::process::Command` with platform-specific commands:
- Windows: `explorer.exe <path>`
- macOS: `open <path>`

**Rationale**: These are the standard OS commands for opening a directory in the file explorer. No additional dependencies needed. The command is fire-and-forget (`spawn()`, not `output()`).

## Decision 13: reqwest Client Configuration

**Decision**: Single `reqwest::Client` instance with default configuration (rustls TLS, automatic redirect following, no timeout).

**Rationale**: reqwest 0.12 defaults to rustls (no native-tls dependency needed). Redirect following is automatic (handles CDN redirects for HuggingFace/GitHub). No download timeout is set because model files are large (up to 1.6 GB) and connection speed varies widely — a fixed timeout would fail on slow connections. The HTTP response status is checked before streaming.

**Alternatives considered**:
- Per-download timeout — A 1.6 GB file on a 10 Mbps connection takes ~20 minutes. Any reasonable timeout would either be too short for slow connections or too long to be useful. Rejected.
- Connection timeout only — Could add a 30-second connection timeout to detect unreachable servers faster. Worth considering but not strictly required by the spec.
