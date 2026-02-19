<!--
  Sync Impact Report
  ===================
  Version change: 1.0.0 → 1.1.0 (MINOR — new principle added)

  Modified principles: None

  Added sections:
    - Principle VI: Scope Only Increases

  Removed sections: None

  Templates requiring updates:
    ✅ plan-template.md — Constitution Check section is generic;
        Principle VI will be checked via the Compliance Review
        list below. No template file change needed.
    ✅ spec-template.md — compatible; no changes needed
    ✅ tasks-template.md — compatible; no changes needed
    ✅ agent-file-template.md — compatible; no changes needed
    ✅ checklist-template.md — compatible; no changes needed

  Deferred items: None
-->

# Vox Constitution

**Version**: 1.1.0
**Ratified**: 2026-02-19
**Last Amended**: 2026-02-19

## Core Principles

### I. Local-Only Processing (NON-NEGOTIABLE)

All audio capture, speech recognition, LLM inference, and text
injection MUST execute entirely on the user's device. No audio data,
transcripts, or telemetry may be transmitted over the network. The
only permitted network operation is model download during first-run
setup. Model files MUST be verified via SHA-256 checksum after
download.

### II. Real-Time Latency Budget

End-to-end latency from utterance completion to text appearing in
the target application MUST remain below 300ms on the RTX 4090 and
below 750ms on the M4 Pro. No pipeline stage may introduce blocking
waits on the audio callback thread. Resampling and ML inference MUST
execute on processing/GPU threads, never the audio callback.

### III. Full Pipeline — No Fallbacks

The complete pipeline (VAD + ASR + LLM + Text Injection) MUST be
present and operational before dictation activates. There are no
degraded modes, no optional components, no CPU fallbacks, and no
scope reduction. Every component is required. If any component is
unavailable, the pipeline does not start — the app stays in the
downloading or loading state until all components are ready.

### IV. Pure Rust / GPUI — No Web Tech

The entire application MUST be implemented in Rust using GPUI for
the UI layer. No JavaScript, TypeScript, HTML, CSS, WebView,
Node.js, or any web toolchain. Single static binary. No IPC
serialization between frontend and backend — the UI calls Rust
functions directly.

### V. Zero-Click First Launch

On first launch, all required models MUST download automatically
and concurrently with progress shown in the overlay HUD. No setup
wizards, no confirmation dialogs, no "click to download" buttons.
The user installs, launches, waits for download, and dictates.
Nothing to configure unless they choose to. The hotkey MUST respond
in every app state — if the pipeline is not ready, the overlay
shows why (download progress, loading state, error with guidance).

### VI. Scope Only Increases (NON-NEGOTIABLE)

Scope reduction is forbidden. No feature, component, or capability
defined in the design document may be removed, deferred, made
optional, deprioritized, or marked as a future version goal. Only
scope increases are permitted. If a task is specified, it MUST be
implemented — not stubbed, not skipped, not replaced with a simpler
alternative. This applies to all artifacts: design documents, feature
specs, implementation plans, and task lists. Any proposal that
reduces scope relative to what has been defined is rejected.

## Performance & Resource Constraints

These budgets are derived from the design document (Section 13) and
are binding. Any implementation that exceeds these limits is a bug.

| Resource | RTX 4090 | M4 Pro |
|---|---|---|
| End-to-end latency (utterance → text) | < 300 ms | < 750 ms |
| VRAM / Unified Memory | < 6 GB | < 6 GB |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle) | < 2% | < 2% |
| CPU (active dictation) | < 15% | < 20% |
| Binary size (excluding models) | < 15 MB | < 15 MB |
| Incremental build time | < 10 s | < 10 s |

## Development Workflow

- **Build**: `cargo run -p vox --features vox_core/cuda` (Windows)
  or `cargo run -p vox --features vox_core/metal` (macOS).
- **Test**: `cargo test -p vox_core --features cuda` (Windows)
  or `cargo test -p vox_core --features metal` (macOS).
- **Edition**: Rust 2024 (1.85+). CMake 4.0+.
- **Zero warnings**: The codebase MUST compile with zero warnings.
  `#[allow(...)]` is acceptable only with a justifying comment.
- **Feature specs**: Live in `specs/NNN-feature-name/`.
- **Constitution location**: `.specify/memory/constitution.md`.

## Governance

### Amendment Procedure

1. Propose a change by editing this file on a feature branch.
2. The Sync Impact Report (HTML comment at top) MUST be updated
   to reflect the change.
3. Version bump follows semantic versioning:
   - **MAJOR**: Principle removed, redefined, or made negotiable.
   - **MINOR**: New principle added or existing one expanded.
   - **PATCH**: Wording clarification, typo fix.
4. `LAST_AMENDED` date MUST be updated to the amendment date.

### Compliance Review

Every feature plan (`/speckit.plan`) MUST include a Constitution
Check section that gates Phase 0 research. The check verifies:
- Principle I: No network calls beyond model download.
- Principle II: Latency budget met on both target machines.
- Principle III: All pipeline components present and required.
- Principle IV: No web dependencies introduced.
- Principle V: No manual setup steps added to first launch.
- Principle VI: No features removed, deferred, or made optional.

Violations MUST be documented in the plan's Complexity Tracking
table with justification and a simpler alternative that was rejected.
