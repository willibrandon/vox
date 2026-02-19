# Research: Workspace Scaffolding

**Branch**: `001-workspace-scaffolding` | **Date**: 2026-02-19

## R-001: GPUI Revision Pin

**Decision**: Use rev `89e9ab97aa5d978351ee8a28d9cc35c272c530f5`

**Rationale**: This is the same revision Tusk uses in production. Verified via Tusk's `Cargo.lock` — resolves to `gpui` v0.2.2 and compiles successfully on Windows. The Zed repo HEAD is at `49acfd2602` (much newer), but using it would be untested. Tusk's rev is a known-good baseline.

**Alternatives considered**:
- Zed HEAD (`49acfd2602`): Newer but unverified on Windows for external consumers.
- Older Zed revs: No benefit over Tusk's proven rev.

**Workspace dependency declaration**:
```toml
gpui = { git = "https://github.com/zed-industries/zed", rev = "89e9ab97aa5d978351ee8a28d9cc35c272c530f5", package = "gpui" }
```

**Note**: Tusk uses edition 2021 with this rev. Vox targets edition 2024. Must verify the rev compiles under edition 2024 during build verification. If it fails, fall back to edition 2021 for the workspace and file an issue.

---

## R-002: whisper-rs Sourcing

**Decision**: Source from crates.io via standard `whisper-rs = "0.15.1"`

**Rationale**: Despite CLAUDE.md noting "Codeberg (not crates.io)", the crate IS published on crates.io (v0.15.1, published 2025-09-10, 27k+ downloads). The Codeberg reference is the source code repository, not the distribution channel. Using crates.io is the standard Rust approach and ensures reliable resolution via Cargo's registry.

**Alternatives considered**:
- Git dependency from Codeberg: Fragile (Codeberg availability), slower builds, harder to audit.
- Older version: No reason — 0.15.1 is latest stable with the API noted in CLAUDE.md.

**Feature flags**: `whisper-rs/cuda` and `whisper-rs/metal` are forwarded from `vox_core` feature flags.

---

## R-003: llama-cpp-2 Sourcing

**Decision**: Source from crates.io via `llama-cpp-2 = "0.1"`

**Rationale**: The `llama-cpp-2` crate (by utilityai) is available on crates.io. Latest version: 0.1.135. Using `"0.1"` SemVer range allows patch updates while staying within the 0.1.x line. Repository: `github.com/utilityai/llama-cpp-rs`.

**Critical warning**: Do NOT confuse with `llama-cpp-rs` 0.4 — completely different crate, incompatible APIs. The `llama-cpp-2` crate has nested types (`model::LlamaModel`) and requires `&LlamaBackend` as first arg to `load_from_file`.

**Alternatives considered**:
- Pinning `= 0.1.0` exactly: Too restrictive — would miss bug fixes. The 0.1.x series maintains API compatibility.
- `llama-cpp-rs` 0.4: Wrong crate entirely. Different author, different API.

---

## R-004: Rust Edition 2024 Compatibility

**Decision**: Use edition 2024 as specified. Fall back to 2021 only if GPUI rev fails to compile.

**Rationale**: The spec requires edition 2024 (FR-011). Tusk uses edition 2021, so the GPUI rev has not been tested under 2024. Edition 2024 changes are mostly around `async` closures and `impl Trait` in more positions — unlikely to break GPUI compilation, but must verify.

**Risk mitigation**: Build verification (FR-006 in spec) will catch any incompatibility. If edition 2024 causes GPUI compilation failure, options are:
1. Switch to edition 2021 (matches Tusk).
2. Find a newer GPUI rev that supports edition 2024.
3. Use edition 2024 only in vox crates, 2021 in workspace (per-crate edition override).

---

## R-005: Module Convention

**Decision**: Modern style — `audio.rs` as standalone file. No `mod.rs` files.

**Rationale**: Clarified during `/speckit.clarify` session. CLAUDE.md explicitly states "Never create files with `mod.rs` paths". For scaffolding, all modules are empty stubs as standalone `.rs` files. When a module later needs submodules, create `audio/` directory alongside `audio.rs`.

**Source**: Spec clarification session 2026-02-19.

---

## R-006: Library Entry Point Naming

**Decision**: Named files with `[lib] path` — `src/vox_core.rs` and `src/vox_ui.rs`.

**Rationale**: Clarified during `/speckit.clarify` session. CLAUDE.md says "prefer specifying the library root path in Cargo.toml using `[lib] path`". Matches GPUI convention (`gpui.rs` as entry point in the Zed repo).

**Cargo.toml pattern**:
```toml
[lib]
name = "vox_core"
path = "src/vox_core.rs"
```

---

## R-007: Dependency Version Verification

All dependencies verified as available on crates.io or git:

| Crate | Version | Status | Notes |
|-------|---------|--------|-------|
| gpui | git rev 89e9ab97 | Verified | Tusk Cargo.lock confirms |
| cpal | 0.17 | Available | Newer than Zed's 0.16, needed for SampleRate API |
| ringbuf | 0.4 | Available | Observer trait required |
| rubato | 1.0 | Available | Major API redesign from 0.16 |
| ort | 2.0.0-rc.11 | Available | Release candidate |
| whisper-rs | 0.15.1 | Available | crates.io, source on Codeberg |
| llama-cpp-2 | 0.1 | Available | utilityai, latest 0.1.135 |
| windows | 0.62 | Available | Newer than Zed's 0.61 |
| objc2 | 0.6 | Available | Tusk resolves 0.6.3 |
| objc2-core-graphics | 0.3 | Available | NOT Servo core-graphics |
| rusqlite | 0.38 | Available | With bundled feature |
| reqwest | 0.13 | Verify | Must confirm 0.13 exists on crates.io |
| tokio | 1.49 | Available | Tusk resolves 1.49.0 |
| global-hotkey | 0.6 | Available | — |
| tray-icon | 0.19 | Available | — |
| smallvec | 1.11 | Available | Tusk resolves 1.15.1 (compatible) |
| parking_lot | 0.12 | Available | Tusk resolves 0.12.5 |

**Risk**: `reqwest 0.13` — latest known stable is 0.12.x. If 0.13 does not exist at build time, use `0.12` with `stream` feature. Verify during build.

---

## R-008: Tusk Reference Patterns Applied

Key patterns from Tusk (`D:\SRC\tusk`) applied to Vox workspace design:

1. **Workspace structure**: `[workspace] members = ["crates/*"]` — Tusk uses glob, Vox spec requires explicit members. Use explicit per spec.
2. **GPUI dependency**: Declared in `[workspace.dependencies]` with `package = "gpui"` — must include the `package` key.
3. **Release profile**: Tusk uses `opt-level = 3, lto = "thin"`. Vox spec requires `opt-level = "s", lto = true` for size optimization. Follow Vox spec.
4. **Version**: Tusk uses `0.1.0` — matches Vox spec.
5. **core-text/core-graphics pinning**: Tusk forces `core-text = "=21.0.0"` and `core-graphics = "=0.24.0"` for macOS compatibility. Vox may need this if GPUI build fails on macOS. Note as risk.
