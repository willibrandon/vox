# Implementation Plan: Custom Dictionary

**Branch**: `010-custom-dictionary` | **Date**: 2026-02-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/010-custom-dictionary/spec.md`

## Summary

Refactor and extend the existing `DictionaryCache` in `crates/vox_core/src/dictionary.rs` to support full CRUD operations, command phrase exclusion, use count tracking, category filtering, search, and JSON import/export. The existing two-pass substitution algorithm (phrase-first, then single-word) is preserved and enhanced with command phrase filtering. Schema migration renames existing columns (`term`→`spoken`, `replacement`→`written`, `frequency`→`use_count`) and adds `category`, `is_command_phrase` columns plus indexes. The cache switches from immutable `Arc<HashMap>` internals to shared `Arc<RwLock<HashMap>>` so CRUD mutations from the UI are immediately visible to the pipeline without requiring `reload()`. VoxState gains a `DictionaryCache` field loaded at startup.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: rusqlite 0.38 (bundled SQLite ≥3.45), parking_lot (RwLock), serde/serde_json (import/export), anyhow, tracing — all already in vox_core/Cargo.toml
**Storage**: SQLite (vox.db — shared with transcripts table, created by Feature 009)
**Testing**: `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS)
**Target Platform**: Windows (CUDA/RTX 4090) + macOS (Metal/M4 Pro)
**Project Type**: Three-crate Rust workspace (vox, vox_core, vox_ui)
**Performance Goals**: Cache load <50ms (1000 entries), single substitution <1ms, full text (100 words) <5ms, CRUD <10ms, search <10ms
**Constraints**: All processing local-only, no network calls, no optional compilation
**Scale/Scope**: Up to 1000 dictionary entries typical, single-file refactor of existing dictionary.rs (~389→~750 lines)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design — still passing.*

| Principle | Status | Notes |
|---|---|---|
| I. Local-Only Processing | PASS | Dictionary is SQLite + in-memory cache. No network calls. |
| II. Real-Time Latency | PASS | In-memory cache for O(1) lookups during pipeline. Mutations rebuild optimized structures off the hot path. |
| III. Full Pipeline — No Fallbacks | PASS | Dictionary loads at startup as part of pipeline init. Empty dictionary is valid (no entries = no substitutions). |
| IV. Pure Rust / GPUI | PASS | All Rust. serde_json for import/export (already a dependency). |
| V. Zero-Click First Launch | PASS | Dictionary starts empty, no setup required. |
| VI. Scope Only Increases | PASS | Extending existing dictionary with CRUD, categories, command phrases, import/export, use tracking. |
| VII. Public API Documentation | PASS | All pub items will have `///` doc comments. |
| VIII. No Test Skipping | PASS | All 13 spec tests + existing 10 tests run unconditionally. |
| IX. Explicit Commit Only | PASS | No auto-commits. |
| X. No Deferral | PASS | All 6 user stories and 15 functional requirements addressed in this plan. |
| XI. No Optional Compilation | PASS | No optional deps or feature gates on required functionality. All deps already in Cargo.toml. |

All gates pass. No violations to justify.

## Project Structure

### Documentation (this feature)

```text
specs/010-custom-dictionary/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Phase 0: Technical decisions
├── data-model.md        # Phase 1: Entity definitions
├── quickstart.md        # Phase 1: Verification scenarios
├── contracts/
│   └── public-api.md    # Phase 1: Public API surface
├── checklists/
│   └── requirements.md  # Specification quality checklist
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/
├── src/
│   ├── dictionary.rs          # REFACTOR — DictionaryEntry, DictionaryCache, schema migration,
│   │                          #   CRUD, substitution, hints, import/export, use count, tests
│   ├── state.rs               # MODIFY — call migrate_schema() in init_database(),
│   │                          #   add DictionaryCache field to VoxState
│   ├── pipeline/
│   │   └── orchestrator.rs    # MODIFY — receive DictionaryCache from caller instead of empty(),
│   │                          #   call increment_use_counts() after substitution
│   └── vox_core.rs            # NO CHANGE — dictionary module already exported
└── Cargo.toml                 # NO CHANGE — all dependencies already present
```

**Structure Decision**: Single-file refactor of `dictionary.rs`. No new files or modules needed. The dictionary module is already exported from `vox_core.rs` and integrated into the pipeline. All dependencies (rusqlite, parking_lot, serde, serde_json, anyhow) are already in Cargo.toml.

## Key Architecture Decisions

### Cache Internals: Arc<RwLock<...>> for Shared Mutable State

The current `DictionaryCache` uses immutable `Arc<HashMap>` internals — cheap to clone but requires `reload()` to pick up changes. With CRUD operations, the cache needs mutation. The refactored design uses `Arc<RwLock<HashMap>>` so all clones (VoxState, Pipeline) share the same mutable state. CRUD updates are immediately visible to the pipeline.

```text
DictionaryCache (Clone via Arc)
├── db_path: Option<PathBuf>           — SQLite path (None for test-only caches)
├── entries: Arc<RwLock<HashMap>>      — All entries keyed by spoken (lowercase)
├── word_subs: Arc<RwLock<HashMap>>    — Single-word substitutions (excludes command phrases)
└── phrase_subs: Arc<RwLock<Vec>>      — Multi-word phrases sorted longest-first (excludes command phrases)
```

### Substitution: Two-Pass Algorithm Preserved

The existing two-pass algorithm is preserved:
1. **Phrase pass**: Replace multi-word phrases (longest-first, case-insensitive) — handles "New York City" before "New York"
2. **Word pass**: Replace single words via HashMap O(1) lookup

Enhancement: Both passes now skip entries where `is_command_phrase == true` (filtered out during `rebuild_substitution_maps()`). Both passes now collect matched entry IDs for use count tracking.

### apply_substitutions Return Type Change

`apply_substitutions` returns `SubstitutionResult { text: String, matched_ids: Vec<i64> }` instead of plain `String`. The `matched_ids` vector contains one entry per match occurrence (duplicates for repeated matches). The orchestrator calls `increment_use_counts(&result.matched_ids)` after substitution.

### Connection Management: Open Per-Operation

DictionaryCache stores `db_path` internally and opens a new `rusqlite::Connection` for each CRUD operation. SQLite connections are lightweight (~1ms). This avoids holding connections across async boundaries and matches the existing `load()`/`reload()` pattern.

## Complexity Tracking

No constitution violations. No complexity justifications needed.
