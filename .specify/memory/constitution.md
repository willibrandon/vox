<!--
  Sync Impact Report
  ===================
  Version change: 0.0.0 (template) → 1.0.0 (initial ratification)

  Modified principles: N/A (initial creation)

  Added sections:
    - Core Principles (5 principles derived from design document)
    - Performance & Resource Constraints
    - Development Workflow
    - Governance

  Removed sections: None

  Templates requiring updates:
    ✅ plan-template.md — Constitution Check section is generic; no update
        needed until first feature plan is generated
    ✅ spec-template.md — compatible; no constitution-specific sections
        require modification
    ✅ tasks-template.md — compatible; task phasing aligns with
        principles (setup → foundational → stories)
    ✅ agent-file-template.md — generic template; no updates needed
    ✅ checklist-template.md — generic template; no updates needed

  Deferred items: None
-->

# VoxFlow Constitution

## Core Principles

### I. Local-Only Processing (NON-NEGOTIABLE)

All audio capture, speech recognition, LLM inference, and text injection
MUST execute entirely on the user's device. No audio data, transcripts,
or telemetry may be transmitted over the network. The only permitted
network operation is the one-time model download during first-run setup.
Model files MUST be verified via SHA-256 checksum after download.

### II. Real-Time Latency Budget

End-to-end latency from utterance completion to text appearing in the
target application MUST remain below 500ms on the RTX 4090 and below
1000ms on the M4 Pro. Each pipeline stage has a fixed latency budget:

| Stage | RTX 4090 | M4 Pro |
|---|---|---|
| VAD decision | < 1 ms | < 1 ms |
| ASR (5s audio) | < 50 ms | < 150 ms |
| LLM post-processing (30 tok) | < 200 ms | < 550 ms |
| Text injection | < 30 ms | < 30 ms |

New features MUST NOT degrade these targets. Performance regressions
MUST be caught by the benchmark harness before merge.

### III. Lock-Free Audio Pipeline

The audio capture callback thread MUST never block. Communication
between the audio callback and processing thread MUST use a lock-free
SPSC ring buffer. The audio thread MUST run at real-time OS priority.
No heap allocation, no mutex acquisition, and no I/O is permitted on
the audio hot path.

### IV. Cross-Platform Parity

Windows (CUDA) and macOS (Metal) are equal first-class targets. Every
user-facing feature MUST work on both platforms. Platform-specific code
MUST be isolated behind `#[cfg(target_os = "...")]` gates with a
shared trait or interface. Feature flags (`cuda`, `metal`) control GPU
backend selection at compile time. A CPU-only fallback MUST exist for
all GPU-accelerated operations.

### V. Graceful Degradation

The pipeline MUST never hard-fail on a recoverable error. The
degradation chain is:

1. Full pipeline (GPU ASR + GPU LLM) — normal operation
2. Reduced pipeline (GPU ASR only) — if LLM fails, inject raw transcript
3. CPU pipeline (CPU ASR, no LLM) — if GPU fails entirely
4. Error state with user notification — only if ASR is completely
   unavailable

Each degradation level MUST be automatic and transparent to the user.
Audio device disconnection, model corruption, and GPU driver crashes
MUST be handled without application termination.

## Performance & Resource Constraints

Combined VRAM usage MUST remain below 6 GB on both target machines
(24 GB available each). System RAM usage MUST stay below 500 MB. CPU
usage MUST remain below 2% when idle (no active dictation) and below
20% during active dictation.

Binary size (excluding models) MUST remain below 15 MB. Model storage
is ~3.5 GB and is downloaded separately, never bundled in the binary.

Structured logging via the `tracing` crate is required for all pipeline
stages. Log output MUST include timing spans for each stage to enable
latency regression detection. Audio data MUST NOT appear in logs.

## Development Workflow

- Rust 2025 edition (1.84+) for the Tauri v2 backend; TypeScript 5.7
  with SolidJS for the frontend.
- `cargo tauri dev --features <cuda|metal>` for development;
  `cargo tauri build --features <cuda|metal>` for release.
- Frontend: `pnpm` as the package manager, Vite as the bundler.
- Tests run via `cargo test --features <cuda|metal>`. Integration
  tests that require ML models MUST be gated behind a `models`
  feature or `#[ignore]` attribute so CI can run without 3.5 GB of
  model files.
- Commits follow conventional commit format.
- All Tauri IPC commands MUST have typed request/response contracts
  shared between Rust and TypeScript.

## Governance

This constitution supersedes all other development guidance for VoxFlow.
Amendments require:

1. A documented rationale for the change.
2. Verification that no existing feature violates the amended principle.
3. Version bump following semver (MAJOR for principle
   removal/redefinition, MINOR for additions, PATCH for clarifications).
4. Update to the Sync Impact Report at the top of this file.

All code reviews MUST verify compliance with these principles.
Violations require explicit justification logged in the plan's
Complexity Tracking table.

**Version**: 1.0.0 | **Ratified**: 2026-02-04 | **Last Amended**: 2026-02-04
