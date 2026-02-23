<!--
  Sync Impact Report
  ===================
  Version change: 1.8.0 → 1.9.0 (MINOR — Principle XII added)

  Modified principles: None

  Added sections:
    - Principle XII: No Blame Attribution (NON-NEGOTIABLE)
      Forbids claiming any encountered problem is a "pre-existing
      issue," deflecting ownership to other features/sessions, or
      using provenance as a reason not to fix something. If Claude
      encounters a broken thing, Claude fixes it — no commentary
      about whose fault it is.

  Removed sections: None

  Templates requiring updates:
    ✅ plan-template.md — no changes needed; Constitution Check
        uses dynamic gate list from constitution
    ✅ spec-template.md — no changes needed
    ✅ tasks-template.md — no changes needed
    ✅ agent-file-template.md — no changes needed
    ✅ checklist-template.md — no changes needed

  Companion updates:
    ⚠ CLAUDE.md — Principle 12 summary needs adding to
        constitution list (manual follow-up)

  Follow-up TODOs:
    - Update CLAUDE.md constitution summary to include Principle 12
-->

# Vox Constitution

**Version**: 1.9.0
**Ratified**: 2026-02-19
**Last Amended**: 2026-02-22

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

### VII. Public API Documentation

Every public item in the codebase MUST have a Rust-idiomatic doc
comment (`///`). This applies to all `pub` structs, enums, traits,
functions, methods, type aliases, constants, and modules (`//!`
for module-level docs). Doc comments MUST describe what the item
does and why a caller would use it — not restate the type signature.
Omit doc comments only on trait implementations where the trait's
own documentation is sufficient. `#[allow(missing_docs)]` is
forbidden without a justifying comment approved in code review.

### VIII. No Test Skipping (NON-NEGOTIABLE)

Every test in the codebase MUST run unconditionally on every
`cargo test` invocation. The `#[ignore]` attribute, `#[cfg(skip)]`
guards, conditional compilation to disable tests, and any other
mechanism that prevents a test from executing are absolutely
forbidden. No test guards of any kind are permitted. If a test
requires external resources (model files, hardware, fixtures),
those resources MUST be present in the development environment.
If a test fails, it MUST be fixed — not skipped, not guarded,
not deferred, not commented out. Any test that exists in the
codebase MUST pass on every test run. Violations of this
principle result in immediate project reset.

### IX. Explicit Commit Only (NON-NEGOTIABLE)

Git commits MUST only be created when the user explicitly
instructs Claude to commit. Claude MUST NEVER create a git
commit, amend a commit, or run any git command that modifies
repository history on its own initiative. Staging files (`git add`)
for inspection is permitted, but `git commit` MUST NOT execute
without a direct, unambiguous instruction from the user. This
applies regardless of task completion state — finishing an
implementation does not imply permission to commit. Violations
of this principle are treated as unauthorized repository
modifications.

### X. No Deferral (NON-NEGOTIABLE)

Claude MUST NEVER defer any work item, decision, clarification,
action, analysis finding, or output item to a later phase, future
session, or subsequent command. Every identified gap, ambiguity,
missing requirement, incomplete analysis, or unresolved question
MUST be addressed immediately in the current workflow step.

This prohibition applies to ALL outputs Claude produces — code,
specifications, analysis reports, coverage tables, summary tables,
clarification reports, checklists, and any other artifact. No row
in any table, no bullet in any list, no category in any report
may carry a status of "Deferred," "Outstanding," "Pending review,"
"Better suited for later," or any synonym.

The following are absolutely forbidden:

- Labeling any item as "Deferred" in any output artifact
- Using the status "Outstanding" for items within scope
- Writing "better suited for planning" or "planning concern"
- Writing "better addressed in a later pass" or any variation
- Recommending that an issue be "addressed during planning"
  when it was identified during clarification
- Postponing resolution of a partial or missing specification
  category to any future step
- Suggesting that an incomplete item "can be handled later"
- Marking any work as "out of scope for this pass" when it was
  within the scope of the current command
- Using hedging language: "low impact," "not critical enough,"
  "exceeds question quota," "low priority — can wait"
- Producing coverage/analysis tables with anything other than
  "Clear" or "Resolved" — if a category is not Clear, Claude
  MUST resolve it immediately (ask a question, make a decision,
  or fill the gap)

If a gap is identified, it MUST be resolved NOW — by asking a
clarification question, making an informed decision, or
implementing the solution. There is no "later." There is no
"deferred." There is no hedge. Violations of this principle
result in immediate deletion of all generated work.

### XI. No Optional Compilation (NON-NEGOTIABLE)

When the design specifies that a component uses a crate or
implements a trait, that dependency MUST be added as a required
(non-optional) dependency. Making required dependencies optional
via Cargo feature flags, wrapping required implementations in
`#[cfg(feature = ...)]` guards, or using `optional = true` in
Cargo.toml for dependencies that the design mandates is absolutely
forbidden. This is deferral disguised as configurability — hedging
whether the implementation will actually work by providing an
escape hatch to compile without it.

Legitimate uses of Cargo feature flags are strictly limited to:

- Platform-specific backends (`cuda`, `metal`) that are mutually
  exclusive by target hardware
- Enabling vendor-specific build-time codegen (e.g., CUDA kernels)

If the design says a struct implements a trait, that `impl` is
unconditional. If the design says a module uses a crate, that
crate is a required dependency. No `optional = true`. No
`#[cfg(feature = "...")]` on required functionality. If it is
in the design, it is compiled — always, unconditionally.
Violations of this principle result in immediate deletion of
all generated work.

### XII. No Blame Attribution (NON-NEGOTIABLE)

Claude MUST NEVER claim that a problem is a "pre-existing issue,"
attribute a bug to a different feature, blame another session, or
use the provenance of a defect as a reason not to fix it. When
Claude encounters something broken — a corrupt file, a missing
validation, a logic error, an incomplete implementation — Claude
fixes it. Period. No commentary about when, where, or by whom the
problem was introduced.

The following are absolutely forbidden:

- Saying "this is a pre-existing issue" or any variation
- Saying "this was introduced by Feature NNN" or blaming another
  feature, task, session, or prior implementation
- Saying "this is not something introduced by the current work"
  as justification for not fixing it
- Saying "this is a bug in [other module/crate/feature]" without
  immediately fixing it
- Using provenance or attribution as a reason to defer, skip, or
  deprioritize a fix
- Diagnosing a problem and then suggesting the user fix it
  themselves because it "belongs to" a different scope
- Any form of "not my problem" deflection regardless of how the
  problem originated

If Claude sees broken code, Claude fixes it. If Claude encounters
a corrupt file, Claude repairs or replaces it. If Claude discovers
a missing validation that causes a runtime failure, Claude adds the
validation. The origin of the problem is irrelevant — only the fix
matters. Violations of this principle result in immediate deletion
of all generated work.

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

## Reference Repositories

Two GPUI applications are cloned locally and MUST be used as
implementation references when building Vox features.

- **Zed** (`D:\SRC\zed`) — The Zed code editor. Source of the GPUI
  framework itself (`crates/gpui/`). Reference for GPUI patterns:
  `Entity<T>` state management, `Render` trait, `div()` builder API,
  `Action` keybindings, window management, and context menus. The
  authoritative source for how GPUI is meant to be used.

- **Tusk** (`D:\SRC\tusk`) — A native PostgreSQL client built with
  Rust and GPUI. Shares the same three-crate workspace architecture
  as Vox (binary + core + UI). Reference for practical GPUI app
  patterns: settings management, multi-panel layouts, list views,
  and OS-level integrations in a non-editor context.

When implementing GPUI UI components, consult Zed for framework-level
patterns and Tusk for application-level patterns before inventing new
approaches.

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
- Principle VII: All public items have doc comments.
- Principle VIII: No tests skipped, ignored, or guarded.
- Principle IX: No commits without explicit user instruction.
- Principle X: No work items deferred to later phases.
- Principle XI: No optional/feature-gated required dependencies.
- Principle XII: No blame attribution or ownership deflection.

Violations MUST be documented in the plan's Complexity Tracking
table with justification and a simpler alternative that was rejected.
