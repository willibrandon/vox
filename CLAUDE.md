# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Vox — local-first intelligent voice dictation engine. Pure Rust, GPUI frontend, GPU-accelerated ML inference. Transforms speech into polished text injected into any application.

Pipeline: Audio Capture (cpal) → Ring Buffer → Silero VAD (ONNX) → Whisper ASR (whisper.cpp) → Qwen LLM post-processing (llama.cpp) → Text Injection (OS-level keystroke simulation).

Design document: `docs/design.md`. Constitution: `.specify/memory/constitution.md`.

## Constitution (All Principles Are Non-Negotiable)

Every change must comply with these 6 principles. Violations are rejected.

1. **Local-Only Processing** — All audio/ML processing on-device. No network calls except model download. No telemetry. SHA-256 checksum verification on downloaded models.
2. **Real-Time Latency Budget** — End-to-end < 300ms (RTX 4090), < 750ms (M4 Pro). No blocking on audio callback thread. ML inference on processing/GPU threads only.
3. **Full Pipeline — No Fallbacks** — VAD + ASR + LLM + Text Injection all required. No degraded modes, no optional components, no CPU fallbacks. Pipeline does not start until all components are loaded.
4. **Pure Rust / GPUI — No Web Tech** — No JavaScript, TypeScript, HTML, CSS, WebView, Node.js. Single static binary. UI calls Rust functions directly, no IPC serialization.
5. **Zero-Click First Launch** — Models auto-download concurrently on first launch. No setup wizards, no confirmation dialogs. Hotkey responds in every app state.
6. **Scope Only Increases** — No feature may be removed, deferred, made optional, deprioritized, or marked as a future version goal. Only scope increases are permitted. If it's in the design doc, it gets implemented.

## Performance Budgets (Binding)

| Resource | RTX 4090 | M4 Pro |
|---|---|---|
| End-to-end latency | < 300 ms | < 750 ms |
| VRAM / Unified Memory | < 6 GB | < 6 GB |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle / active) | < 2% / < 15% | < 2% / < 20% |
| Binary size (excl. models) | < 15 MB | < 15 MB |
| Incremental build | < 10 s | < 10 s |

## Build Commands

```bash
# Development
cargo run -p vox --features vox_core/cuda     # Windows (CUDA)
cargo run -p vox --features vox_core/metal     # macOS (Metal)

# Tests
cargo test -p vox_core --features cuda         # Windows
cargo test -p vox_core --features metal        # macOS

# Single test
cargo test -p vox_core test_name --features cuda -- --nocapture

# Release build
cargo build --release -p vox --features vox_core/cuda
```

Zero warnings required. `#[allow(...)]` only with justifying comment.

## Build Prerequisites

- Rust 1.85+ (2024 edition), CMake 4.0+
- **Windows**: Visual Studio 2022 Build Tools, CUDA 12.8+, cuDNN 9.x
- **Windows CUDA gotcha**: CUDA doesn't support VS 18 Insiders. `CMAKE_GENERATOR` must be set to `Visual Studio 17 2022`. Both `CMAKE_GENERATOR` and `CUDA_PATH` are persistent user env vars.
- **macOS**: Xcode 26.x + Command Line Tools. Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain`.
- **macOS GPUI gotcha**: GPUI's font-kit pulls `core-text 21.1` which depends on `core-graphics 0.25`, conflicting with font-kit's 0.24. Pin `core-text = "=21.0.0"` and `core-graphics = "=0.24.0"` in macOS deps (already done in `crates/vox/Cargo.toml`).
- No Node.js, pnpm, or any web toolchain.

## Architecture

Three-crate workspace:

- **`crates/vox/`** — Binary entry point. GPUI Application, window setup, system tray (`tray-icon`), global hotkeys (`global-hotkey`).
- **`crates/vox_core/`** — Backend. Audio pipeline, VAD, ASR, LLM, text injection, dictionary, settings, model download. Feature-gated: `cuda` and `metal`.
- **`crates/vox_ui/`** — GPUI UI components. Overlay HUD, settings panel, history, dictionary editor, model manager, log viewer.

GPUI patterns (from Zed): `Entity<T>` for state, `Render` trait for views, `cx.set_global()` for app-wide state, `div()` builder API, `Action` trait for keybindings.

## Reference Repositories

Two GPUI apps are cloned locally. Consult these before inventing new patterns.

- **Zed** (`D:\SRC\zed`) — The GPUI framework source (`crates/gpui/`). Authoritative reference for GPUI patterns: entity state, rendering, actions, window management.
- **Tusk** (`D:\SRC\tusk`) — Native PostgreSQL client, Rust/GPUI. Same three-crate workspace architecture as Vox. Reference for practical GPUI app patterns: settings, multi-panel layouts, list views, OS integrations.

## Pinned Dependency Versions

These are verified compatible. Using wrong versions will cause compile failures or runtime bugs.

| Crate | Version | Critical Notes |
|---|---|---|
| gpui | git rev `89e9ab97` (zed-industries/zed) | Resolves to gpui v0.2.2. Must use `package = "gpui"` in workspace dep. Matches Tusk pin. |
| cpal | 0.17 | `SampleRate` is `u32`. `device.description()` returns `DeviceDescription` struct — use `.name()`. Auto RT priority. |
| ringbuf | 0.4 | `occupied_len()` on `Observer` trait — must `use ringbuf::traits::Observer` |
| rubato | 1.0 | Major API redesign from 0.16. Use `AudioAdapter` trait + `SequentialSliceOfVecs` |
| ort | 2.0.0-rc.11 | RC but production-ready |
| whisper-rs | 0.15.1 | crates.io (source code on Codeberg). Flash attn disabled. `full_n_segments()` returns `c_int` not Result |
| llama-cpp-2 | 0.1 (utilityai) | **NOT `llama-cpp-rs` 0.4** — completely different crate. Types nested: `model::LlamaModel`. `load_from_file` needs `&LlamaBackend` first arg |
| windows | 0.62 | Win32 SendInput. Can't inject into elevated processes (UIPI) |
| objc2 | 0.6 | **NOT Servo `core-graphics`** (heading toward deprecation). Use `objc2-core-graphics` 0.3 |
| rusqlite | 0.38 | No `FromSql` for `chrono::DateTime<Utc>` — use `String` (ISO 8601) for timestamps |
| tokio | 1.49 | — |
| reqwest | 0.12 | 0.13 exists but is a separate semver line. Using 0.12 with `stream` feature. rustls default. |

## Thread Safety

- `WhisperContext` is **NOT** thread-safe → wrap in `Arc<Mutex<>>`. Create new `WhisperState` per transcription.
- `LlamaModel` is `Send+Sync` → `Arc`. `LlamaContext` is **NOT** → one per inference call.
- cpal audio callback is real-time — no allocations, no locks, no ML. Resampling on processing thread.
- macOS `CGEvent` has undocumented 20-char limit per call — must chunk text.

## Commit Style

Use `/vox.commit` command. Conventional commits (`type(scope): message`), imperative mood, no emojis, no AI attribution, no words like "comprehensive/robust/enhance/streamline/leverage".

## Spec-Kit Workflow

Feature specs live in `specs/NNN-feature-name/`. Commands: `/speckit.specify` → `/speckit.plan` → `/speckit.tasks` → `/speckit.implement`. Every plan must pass a Constitution Check against all 6 principles before implementation begins.

# Rust coding guidelines

* Prioritize code correctness and clarity. Speed and efficiency are secondary priorities unless otherwise specified.
* Do not write organizational or comments that summarize the code. Comments should only be written in order to explain "why" the code is written in some way in the case there is a reason that is tricky / non-obvious.
* Prefer implementing functionality in existing files unless it is a new logical component. Avoid creating many small files.
* Avoid using functions that panic like `unwrap()`, instead use mechanisms like `?` to propagate errors.
* Be careful with operations like indexing which may panic if the indexes are out of bounds.
* Never silently discard errors with `let _ =` on fallible operations. Always handle errors appropriately:
  - Propagate errors with `?` when the calling function should handle them
  - Use `.log_err()` or similar when you need to ignore errors but want visibility
  - Use explicit error handling with `match` or `if let Err(...)` when you need custom logic
  - Example: avoid `let _ = client.request(...).await?;` - use `client.request(...).await?;` instead
* When implementing async operations that may fail, ensure errors propagate to the UI layer so users get meaningful feedback.
* Never create files with `mod.rs` paths - prefer `src/some_module.rs` instead of `src/some_module/mod.rs`.
* When creating new crates, prefer specifying the library root path in `Cargo.toml` using `[lib] path = "...rs"` instead of the default `lib.rs`, to maintain consistent and descriptive naming (e.g., `gpui.rs` or `main.rs`).
* Avoid creative additions unless explicitly requested
* Use full words for variable names (no abbreviations like "q" for "queue")
* Use variable shadowing to scope clones in async contexts for clarity, minimizing the lifetime of borrowed references.
  Example:
  ```rust
  executor.spawn({
      let task_ran = task_ran.clone();
      async move {
          *task_ran.borrow_mut() = true;
      }
  });
  ```

# GPUI

GPUI is a UI framework which also provides primitives for state and concurrency management.

## Context

Context types allow interaction with global state, windows, entities, and system services. They are typically passed to functions as the argument named `cx`. When a function takes callbacks they come after the `cx` parameter.

* `App` is the root context type, providing access to global state and read and update of entities.
* `Context<T>` is provided when updating an `Entity<T>`. This context dereferences into `App`, so functions which take `&App` can also take `&Context<T>`.
* `AsyncApp` and `AsyncWindowContext` are provided by `cx.spawn` and `cx.spawn_in`. These can be held across await points.

## `Window`

`Window` provides access to the state of an application window. It is passed to functions as an argument named `window` and comes before `cx` when present. It is used for managing focus, dispatching actions, directly drawing, getting user input state, etc.

## Entities

An `Entity<T>` is a handle to state of type `T`. With `thing: Entity<T>`:

* `thing.entity_id()` returns `EntityId`
* `thing.downgrade()` returns `WeakEntity<T>`
* `thing.read(cx: &App)` returns `&T`.
* `thing.read_with(cx, |thing: &T, cx: &App| ...)` returns the closure's return value.
* `thing.update(cx, |thing: &mut T, cx: &mut Context<T>| ...)` allows the closure to mutate the state, and provides a `Context<T>` for interacting with the entity. It returns the closure's return value.
* `thing.update_in(cx, |thing: &mut T, window: &mut Window, cx: &mut Context<T>| ...)` takes a `AsyncWindowContext` or `VisualTestContext`. It's the same as `update` while also providing the `Window`.

Within the closures, the inner `cx` provided to the closure must be used instead of the outer `cx` to avoid issues with multiple borrows.

Trying to update an entity while it's already being updated must be avoided as this will cause a panic.

When  `read_with`, `update`, or `update_in` are used with an async context, the closure's return value is wrapped in an `anyhow::Result`.

`WeakEntity<T>` is a weak handle. It has `read_with`, `update`, and `update_in` methods that work the same, but always return an `anyhow::Result` so that they can fail if the entity no longer exists. This can be useful to avoid memory leaks - if entities have mutually recursive handles to each other they will never be dropped.

## Concurrency

All use of entities and UI rendering occurs on a single foreground thread.

`cx.spawn(async move |cx| ...)` runs an async closure on the foreground thread. Within the closure, `cx` is an async context like `AsyncApp` or `AsyncWindowContext`.

When the outer cx is a `Context<T>`, the use of `spawn` instead looks like `cx.spawn(async move |handle, cx| ...)`, where `handle: WeakEntity<T>`.

To do work on other threads, `cx.background_spawn(async move { ... })` is used. Often this background task is awaited on by a foreground task which uses the results to update state.

Both `cx.spawn` and `cx.background_spawn` return a `Task<R>`, which is a future that can be awaited upon. If this task is dropped, then its work is cancelled. To prevent this one of the following must be done:

* Awaiting the task in some other async context.
* Detaching the task via `task.detach()` or `task.detach_and_log_err(cx)`, allowing it to run indefinitely.
* Storing the task in a field, if the work should be halted when the struct is dropped.

A task which doesn't do anything but provide a value can be created with `Task::ready(value)`.

## Elements

The `Render` trait is used to render some state into an element tree that is laid out using flexbox layout. An `Entity<T>` where `T` implements `Render` is sometimes called a "view".

Example:

```
struct TextWithBorder(SharedString);

impl Render for TextWithBorder {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().border_1().child(self.0.clone())
    }
}
```

Since `impl IntoElement for SharedString` exists, it can be used as an argument to `child`. `SharedString` is used to avoid copying strings, and is either an `&'static str` or `Arc<str>`.

UI components that are constructed just to be turned into elements can instead implement the `RenderOnce` trait, which is similar to `Render`, but its `render` method takes ownership of `self`. Types that implement this trait can use `#[derive(IntoElement)]` to use them directly as children.

The style methods on elements are similar to those used by Tailwind CSS.

If some attributes or children of an element tree are conditional, `.when(condition, |this| ...)` can be used to run the closure only when `condition` is true. Similarly, `.when_some(option, |this, value| ...)` runs the closure when the `Option` has a value.

## Input events

Input event handlers can be registered on an element via methods like `.on_click(|event, window, cx: &mut App| ...)`.

Often event handlers will want to update the entity that's in the current `Context<T>`. The `cx.listener` method provides this - its use looks like `.on_click(cx.listener(|this: &mut T, event, window, cx: &mut Context<T>| ...)`.

## Actions

Actions are dispatched via user keyboard interaction or in code via `window.dispatch_action(SomeAction.boxed_clone(), cx)` or `focus_handle.dispatch_action(&SomeAction, window, cx)`.

Actions with no data defined with the `actions!(some_namespace, [SomeAction, AnotherAction])` macro call. Otherwise the `Action` derive macro is used. Doc comments on actions are displayed to the user.

Action handlers can be registered on an element via the event handler `.on_action(|action, window, cx| ...)`. Like other event handlers, this is often used with `cx.listener`.

## Notify

When a view's state has changed in a way that may affect its rendering, it should call `cx.notify()`. This will cause the view to be rerendered. It will also cause any observe callbacks registered for the entity with `cx.observe` to be called.

## Entity events

While updating an entity (`cx: Context<T>`), it can emit an event using `cx.emit(event)`. Entities register which events they can emit by declaring `impl EventEmittor<EventType> for EntityType {}`.

Other entities can then register a callback to handle these events by doing `cx.subscribe(other_entity, |this, other_entity, event, cx| ...)`. This will return a `Subscription` which deregisters the callback when dropped.  Typically `cx.subscribe` happens when creating a new entity and the subscriptions are stored in a `_subscriptions: Vec<Subscription>` field.

## Recent API changes

GPUI has had some changes to its APIs. Always write code using the new APIs:

* `spawn` methods now take async closures (`AsyncFn`), and so should be called like `cx.spawn(async move |cx| ...)`.
* Use `Entity<T>`. This replaces `Model<T>` and `View<T>` which no longer exist and should NEVER be used.
* Use `App` references. This replaces `AppContext` which no longer exists and should NEVER be used.
* Use `Context<T>` references. This replaces `ModelContext<T>` which no longer exists and should NEVER be used.
* `Window` is now passed around explicitly. The new interface adds a `Window` reference parameter to some methods, and adds some new "*_in" methods for plumbing `Window`. The old types `WindowContext` and `ViewContext<T>` should NEVER be used.
