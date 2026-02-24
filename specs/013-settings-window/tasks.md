# Tasks: Settings Window & Panels

**Input**: Design documents from `/specs/013-settings-window/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/internal-interfaces.md, quickstart.md

**Tests**: Explicitly requested in spec.md Testing Requirements section — 5 unit tests included in their respective user story phases.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **vox_core**: `crates/vox_core/src/`
- **vox_ui**: `crates/vox_ui/src/`
- **vox (binary)**: `crates/vox/src/`

---

## Phase 1: Setup

**Purpose**: Add new types, module declarations, theme extensions, and layout constants needed by all subsequent phases.

- [ ] T001 Add module declarations: `pub mod toggle;`, `pub mod slider;`, `pub mod select;`, `pub mod hotkey_recorder;`, `pub mod scrollbar;` to crates/vox_ui/src/vox_ui.rs; add `pub mod log_sink;` to crates/vox_core/src/vox_core.rs

- [ ] T002 [P] Add window_x, window_y, window_width, window_height fields (all `Option<f32>` with `#[serde(default)]`) to Settings struct in crates/vox_core/src/config.rs

- [ ] T003 [P] Add ModelRuntimeInfo struct (state: ModelRuntimeState, vram_bytes: Option<u64>, benchmark: Option<BenchmarkResult>, custom_path: Option<PathBuf>) and ModelRuntimeState enum (Missing, Downloading, Downloaded, Loading, Loaded, Error(String)) to crates/vox_core/src/state.rs; add BenchmarkResult struct (metric_name: String, value: f64) to crates/vox_core/src/models.rs; add model_runtime: HashMap<String, ModelRuntimeInfo> field with model_runtime()/set_model_runtime() accessors and log_receiver: Option<LogReceiver> field with take_log_receiver() accessor to VoxState in crates/vox_core/src/state.rs

- [ ] T004 [P] Implement LogEntry struct (timestamp: String, level: LogLevel, target: String, message: String), LogLevel enum (Error, Warn, Info, Debug, Trace with PartialOrd ordering and Display impl), LogSink struct implementing tracing_subscriber::Layer (formats events into LogEntry, sends over mpsc::unbounded channel), and LogReceiver struct (wraps mpsc::UnboundedReceiver<LogEntry>) with LogSink::new() -> (LogSink, LogReceiver) constructor in new file crates/vox_core/src/log_sink.rs

- [ ] T005 [P] Extend ThemeColors in crates/vox_ui/src/theme.rs with 7 new color tokens: log_error (red), log_warn (amber), log_info (white/text), log_debug (gray/text_muted), log_trace (dim gray), scrollbar_thumb, scrollbar_track; add values to Dark, Light, and System theme variants in their respective constructor functions

- [ ] T006 [P] Add SIDEBAR_WIDTH (px(160.0)) and STATUS_BAR_HEIGHT (px(28.0)) constants to size module in crates/vox_ui/src/layout.rs

---

## Phase 2: Foundational (Shared UI Components)

**Purpose**: Reusable UI components used across multiple panels. MUST complete before any user story phase.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T007 Implement Scrollbar as a custom GPUI Element in crates/vox_ui/src/scrollbar.rs — Element trait with request_layout (Position::Absolute, full parent size), prepaint (compute track/thumb bounds from ScrollHandle offsets, insert hitbox), paint (paint_quad for track background and pill-shaped thumb, mouse event handlers for scroll wheel notify, click-to-jump on track, thumb drag with Capture phase for off-track movement); ScrollbarDragState type alias (Rc<Cell<Option<Pixels>>>) and new_drag_state() constructor; constants SCROLLBAR_WIDTH (8px), SCROLLBAR_PADDING (4px), MIN_THUMB_HEIGHT (25px); IntoElement impl

- [ ] T008 [P] Implement Button component (RenderOnce with #[derive(IntoElement)]) in crates/vox_ui/src/button.rs — label: SharedString, optional icon: Option<Icon>, variant: ButtonVariant enum (Primary, Secondary, Ghost, Danger) controlling background/text colors from ThemeColors, disabled: bool (grayed out, click suppressed), on_click: Box<dyn Fn> callback; hover and active visual states via .hover() and .active() style modifiers; renders as rounded div with padding, cursor_pointer when enabled

- [ ] T009 [P] Implement TextInput component (Entity<TextInput> + Render) in crates/vox_ui/src/text_input.rs — FocusHandle for focus management, content: String, placeholder: SharedString, on_change: Box<dyn Fn(String)> callback (fires on every keystroke), on_submit: Option<Box<dyn Fn(String)>> callback (fires on Enter key), border styling that changes on focus; keyboard event handling via on_key_down for character input, Backspace, Delete, Enter; renders as bordered div with text content or placeholder when empty

- [ ] T010 [P] Implement Icon rendering utilities in crates/vox_ui/src/icon.rs — Icon enum with variants for UI actions (Copy, Delete, Edit, Settings, Folder, Download, Retry, Check, Error, Warning, ChevronDown, Search, Clear, Play, Pause), render method returning impl IntoElement that renders the icon as a themed text glyph or SVG path with consistent sizing; IconElement RenderOnce wrapper for use as .child()

- [ ] T011 [P] Implement Toggle switch component (RenderOnce with #[derive(IntoElement)]) in crates/vox_ui/src/toggle.rs — enabled: bool state, label: SharedString, on_change: Box<dyn Fn(bool)> callback; renders as horizontal flex row with label text and pill-shaped track (36x20px) containing circular thumb (16px); thumb positioned left when off, right when on; track uses accent color when on, border/muted color when off; click anywhere on track or label triggers on_change(!enabled)

**Checkpoint**: Foundation ready — all shared UI components implemented. User story work can now begin.

---

## Phase 3: User Story 1 — Open and Navigate the Settings Workspace (Priority: P1) 🎯 MVP

**Goal**: Deliver a navigable workspace shell with sidebar, panel switching, status bar, singleton window management, and window position persistence. All five panels render as real entities connected to their data sources.

**Independent Test**: Open settings window via tray/overlay/keyboard, click each sidebar item and verify content area switches with sidebar highlight. Close window, reopen, verify position restored. Trigger open again while open and verify focus (no duplicate).

### Implementation for User Story 1

- [ ] T012 [US1] Implement VoxWorkspace entity in crates/vox_ui/src/workspace.rs — Panel enum (Settings, History, Dictionary, Model, Log) with Clone+Copy+PartialEq derives; VoxWorkspace struct with active_panel: Panel, settings_panel: Entity<SettingsPanel>, history_panel: Entity<HistoryPanel>, dictionary_panel: Entity<DictionaryPanel>, model_panel: Entity<ModelPanel>, log_panel: Entity<LogPanel>, focus_handle: FocusHandle, _subscriptions: Vec<Subscription>; new(window, cx) creates all 5 panel entities via cx.new(), sets active_panel to Panel::Settings, subscribes to VoxState global via cx.observe_global for re-render on state changes; Render impl: div().track_focus(&self.focus_handle).size_full().flex().flex_col() with top child div().flex_1().flex().flex_row() containing sidebar + active panel content (match on active_panel to render correct entity), and bottom child StatusBar

- [ ] T013 [US1] Implement sidebar rendering in crates/vox_ui/src/workspace.rs — render_sidebar(&self, cx) method returns div().flex().flex_col().w(SIDEBAR_WIDTH).h_full().bg(surface).border_r_1().border_color(border).p(spacing::SM).gap(spacing::XS) with 5 sidebar_item() children; sidebar_item(&self, label, panel, cx) returns div().id(label).px(spacing::MD).py(spacing::SM).rounded(radius::SM).cursor_pointer() with .when(is_active, bg accent + button_primary_text color) and .when(!is_active, text_muted + hover elevated_surface), .child(label), .on_click(cx.listener(move |this, _, _, cx| { this.active_panel = panel; cx.notify(); }))

- [ ] T014 [US1] Implement open_settings_window() with singleton pattern and window persistence in crates/vox_ui/src/workspace.rs — module-level static or VoxState field: Option<WindowHandle<VoxWorkspace>> for singleton check; if handle exists and valid, call handle.update(cx, |_, window, _| window.activate_window()) and return; otherwise read Settings window_x/y/width/height — if all Some, construct WindowBounds::Windowed(Bounds { origin, size }) with display bounds clamping (if saved position outside all displays, reset to centered); if any None, use Bounds::centered(None, size(px(800), px(600)), cx); WindowOptions with window_min_size Some(400x300), focus true, show true; cx.open_window returns WindowHandle, store it; register on_window_should_close that reads window.window_bounds(), saves origin.x/y and size.width/height to Settings via update_settings(), sets handle to None, returns true

- [ ] T015 [US1] Implement StatusBar as RenderOnce struct in crates/vox_ui/src/workspace.rs — struct StatusBar with pipeline_status: String, latency_ms: Option<u32>, vram_usage: Option<u64>, audio_device: String; constructor reads from cx.global::<VoxState>() to populate fields; RenderOnce impl: div().h(STATUS_BAR_HEIGHT).w_full().flex().items_center().px(spacing::MD).gap(spacing::LG).bg(surface).border_t_1().border_color(border).text_size(px(12.0)).text_color(text_muted) with children for each field separated by pipe characters, VRAM formatted as "X.X GB"

- [ ] T016 [P] [US1] Implement SettingsPanel entity in crates/vox_ui/src/settings_panel.rs — struct with scroll_handle: ScrollHandle, scrollbar_drag: ScrollbarDragState, input_devices: Vec<AudioDeviceInfo>, settings: Settings; new(window, cx) loads current Settings from VoxState via settings().clone(), enumerates audio devices via list_input_devices(); Render: div().size_full() with child div().id("settings-scroll").size_full().overflow_y_scroll().track_scroll(&self.scroll_handle).flex().flex_col().p(spacing::LG).gap(spacing::XL) containing 6 section rendering methods (render_audio_section through render_advanced_section — each renders section header div with bold title and descriptive subtitle), plus sibling Scrollbar::new(scroll_handle, entity_id, scrollbar_drag, scrollbar_thumb, scrollbar_track)

- [ ] T017 [P] [US1] Implement HistoryPanel entity in crates/vox_ui/src/history_panel.rs — struct with transcripts: Vec<TranscriptEntry>, search_query: String, scroll_handle: UniformListScrollHandle, total_count: usize, confirming_delete: Option<(String, Task<()>)>; new(window, cx) loads initial page from VoxState transcript_store().list(100, 0) and count(); Render: div().size_full().flex().flex_col() with header "Transcript History" and uniform_list("history-list", self.transcripts.len(), render_range) rendering each entry with timestamp (formatted created_at), polished_text truncated to 2 lines, target_app as small badge, latency as "Xms"; when transcripts is empty, render "No transcript history" centered message with muted text

- [ ] T018 [P] [US1] Implement DictionaryPanel entity in crates/vox_ui/src/dictionary_panel.rs — struct with entries: Vec<DictionaryEntry>, search_query: String, sort_field: SortField enum (Spoken, Category, UseCount), sort_ascending: bool, editing_entry: Option<i64>, new_spoken: String, new_written: String, new_category: String, confirming_delete: Option<(i64, Task<()>)>; new(window, cx) loads from VoxState dictionary().list(None); Render: div().size_full().flex().flex_col() with header "Custom Dictionary" and entry list showing spoken form → written form with category badge per row; when entries is empty, render "No dictionary entries. Add your first entry to get started." centered with muted text

- [ ] T019 [P] [US1] Implement ModelPanel entity in crates/vox_ui/src/model_panel.rs — struct tracking per-model display state from MODELS constant and VoxState model_runtime, plus download event subscription; new(window, cx) reads MODELS array and VoxState model_runtime() for each model name, subscribes to ModelDownloader events if available; Render: div().size_full().flex().flex_col() with header "Models" and one card div per model showing model name (bold), status text (Missing/Downloading/Downloaded/Loaded/Error), and file size from ModelInfo.size_bytes formatted as "X.X MB"

- [ ] T020 [P] [US1] Implement LogPanel entity with LogStore in crates/vox_ui/src/log_panel.rs — LogStore entity: entries: VecDeque<LogEntry> bounded to 10,000, EventEmitter<LogStoreEvent> with NewLogEntry variant; new(cx, receiver: LogReceiver) spawns foreground cx.spawn task that loops receiver.rx.recv().await, pushes entry, evicts front if over capacity, emits event; LogPanel struct: log_store: Entity<LogStore>, auto_scroll: bool (true), filter_level: LogLevel (Info), scroll_handle: UniformListScrollHandle, _subscription: Subscription; new(window, cx) takes LogReceiver from VoxState via take_log_receiver(), creates LogStore, subscribes to LogStoreEvent (cx.notify on new entries, scroll_to_item if auto_scroll); Render: div().size_full().flex().flex_col() with header "Application Logs" and uniform_list rendering entries with timestamp, level text color-coded (theme log_error/warn/info/debug/trace), target in muted text, message; auto-scrolls to last item on new entries when auto_scroll is true

- [ ] T021 [US1] Wire LogSink tracing layer and OpenSettings action — in crates/vox/src/main.rs: create LogSink::new() to get (sink, receiver), add sink as additional layer to existing tracing_subscriber setup, store receiver in VoxState via a new set_log_receiver() method; in crates/vox_ui/src/key_bindings.rs: replace existing log-only OpenSettings handler with call to open_settings_window(cx); in crates/vox_ui/src/overlay_hud.rs: wire "Open Settings" quick settings dropdown item to dispatch OpenSettings action

- [ ] T022 [US1] Write test_panel_switching unit test in crates/vox_ui/src/workspace.rs — using cx.new() to create a VoxWorkspace in a GPUI test harness, verify active_panel defaults to Panel::Settings, update active_panel to each Panel variant (History, Dictionary, Model, Log, Settings) and verify the value changes correctly, confirm cx.notify() triggers re-render

**Checkpoint**: Settings window opens from tray/overlay/keyboard, sidebar switches between 5 panels with highlight, status bar shows runtime info, window saves and restores position. All panels render real content from their data sources.

---

## Phase 4: User Story 2 — Configure Dictation Settings (Priority: P1)

**Goal**: Full Settings panel with all 6 sections, interactive controls for every setting, immediate persistence on change.

**Independent Test**: Open Settings panel, change each setting (select device, drag sliders, toggle switches, record hotkey, change theme), close and reopen window, verify all changes persisted. Adjust overlay opacity slider and verify real-time feedback on overlay HUD.

### Implementation for User Story 2

- [ ] T023 [P] [US2] Implement Slider component (Entity<Slider> + Render) in crates/vox_ui/src/slider.rs — struct with min: f32, max: f32, step: f32, value: f32, label: SharedString, on_change callback, dragging: bool; new(cx, min, max, step, initial, label, on_change); Render: horizontal flex row with label, value display (formatted to appropriate precision), and track div (h(4px), full width, rounded, themed); thumb div (16x16 circle, absolute positioned proportional to value); on_mouse_down on track: compute value from click position, set value, start drag; on_mouse_move (Capture phase): if dragging, update value clamped to min/max rounded to step; on_mouse_up: stop drag; each value change fires on_change callback

- [ ] T024 [P] [US2] Implement Select component (Entity<Select> + Render) in crates/vox_ui/src/select.rs — struct with options: Vec<SelectOption> (value: String, label: SharedString), selected: String, label: SharedString, open: bool, on_change callback, focus_handle: FocusHandle; new(cx, options, selected, label, on_change); Render: div with label above, clickable trigger div showing selected option label + ChevronDown icon; when open: absolute-positioned dropdown div below trigger with border/shadow, each option as hoverable row, click selects and closes; on_key_down: Escape closes, ArrowUp/Down navigates, Enter selects; click outside closes via focus loss detection

- [ ] T025 [P] [US2] Implement HotkeyRecorder component (Entity<HotkeyRecorder> + Render) in crates/vox_ui/src/hotkey_recorder.rs — struct with current_binding: String, recording: bool, on_change callback, focus_handle: FocusHandle; new(cx, current_binding, on_change); Render: bordered div showing current binding text; on click: set recording = true, render "Press a key..." with pulsing border; on_key_down while recording: capture keystroke as new binding string, set recording = false, fire on_change; Escape while recording: cancel, revert to previous binding

- [ ] T026 [US2] Implement render_audio_section() and render_vad_section() in crates/vox_ui/src/settings_panel.rs — Audio section: Select component for input_device (options built from input_devices Vec with device.name, is_default marked), Slider for noise_gate (min 0.0, max 1.0, step 0.01); VAD section: Slider for vad_threshold (min 0.0, max 1.0, step 0.01), Slider for min_silence_ms (min 0, max 2000, step 50, displayed as ms), Slider for min_speech_ms (min 0, max 2000, step 50, displayed as ms); each control created as Entity via cx.new() and stored in SettingsPanel struct fields

- [ ] T027 [US2] Implement render_hotkey_section() and render_llm_section() in crates/vox_ui/src/settings_panel.rs — Hotkey section: HotkeyRecorder for activation_hotkey, Toggle for hold_to_talk, Toggle for hands_free_double_press; LLM section: Slider for temperature (min 0.0, max 2.0, step 0.1), Toggle for remove_fillers, Toggle for course_correction, Toggle for punctuation; each Entity component created via cx.new() and stored in SettingsPanel struct fields

- [ ] T028 [US2] Implement render_appearance_section() and render_advanced_section() in crates/vox_ui/src/settings_panel.rs — Appearance section: Select for theme (System/Light/Dark options mapping to ThemeMode enum), Slider for overlay_opacity (min 0.0, max 1.0, step 0.05), Select for overlay_position (TopLeft/TopRight/BottomLeft/BottomRight mapping to OverlayPosition enum), Toggle for show_raw_transcript; Advanced section: TextInput for max_segment_ms (numeric), TextInput for overlap_ms (numeric), TextInput for command_prefix

- [ ] T029 [US2] Wire immediate settings persistence for all controls in crates/vox_ui/src/settings_panel.rs — each control's on_change callback calls cx.global::<VoxState>().update_settings(|s| s.field = new_value) for atomic JSON persistence; update local self.settings snapshot after write to reflect new value in render; cx.notify() to trigger re-render; theme changes additionally call cx.set_global::<VoxTheme>() to update app-wide theme immediately

- [ ] T030 [US2] Write test_settings_persistence unit test in crates/vox_ui/src/settings_panel.rs or crates/vox_core/src/config.rs — create VoxState with temp settings file, call update_settings(|s| s.noise_gate = 0.75), verify settings().noise_gate == 0.75, read the JSON file from disk and verify it contains the updated value

**Checkpoint**: All 6 settings sections fully functional with interactive controls. Every setting change persists immediately to JSON and takes effect without restart.

---

## Phase 5: User Story 3 — Review Transcript History (Priority: P2)

**Goal**: Full History panel with search, copy, delete (inline confirmation), clear all (modal), and virtualized rendering for 10,000+ entries.

**Independent Test**: Create transcripts via dictation, search by text in both raw and polished fields, copy entry to clipboard, delete single entry with inline confirm, clear all with modal confirm, scroll through 10,000+ entries at 60fps.

### Implementation for User Story 3

- [ ] T031 [US3] Implement search and detailed entry rendering in crates/vox_ui/src/history_panel.rs — add Entity<TextInput> for search at top of panel, on_change sets search_query and refreshes: if query non-empty call TranscriptStore::search(query), else call TranscriptStore::list(100, 0); update each entry row to show: formatted timestamp (created_at parsed and displayed as "YYYY-MM-DD HH:MM"), polished_text as primary content, target_app as muted badge, latency_ms as "Xms" suffix; conditionally show raw_text in smaller muted font below polished_text when cx.global::<VoxState>().settings().show_raw_transcript is true; add copy Button (Ghost variant, Copy icon) per entry that calls cx.write_to_clipboard(ClipboardItem::new_string(entry.polished_text.clone()))

- [ ] T032 [US3] Implement inline delete and "Clear All" in crates/vox_ui/src/history_panel.rs — per-entry Delete Button (Ghost, Danger color, Delete icon): on click, set confirming_delete = Some((entry.id.clone(), cx.spawn 5-second timer)); when confirming, replace delete button with "Confirm? [Yes] [No]" text buttons; Yes: call TranscriptStore::delete(&id), remove from local transcripts Vec, clear confirming_delete; No: clear confirming_delete; timer expiry: clear confirming_delete, cx.notify(); "Clear All" Button at panel top: on click call cx.prompt() for modal confirmation "Delete all transcript history? This cannot be undone."; on confirm: call TranscriptStore::clear_secure(), clear local transcripts Vec, update total_count to 0

- [ ] T033 [US3] Ensure virtualized rendering via uniform_list in crates/vox_ui/src/history_panel.rs — verify the entry list uses uniform_list("history-list", self.transcripts.len(), |this, range, window, cx| { ... }) returning Vec<impl IntoElement> for only the visible range; UniformListScrollHandle connected for programmatic scrolling; load more entries on scroll near bottom (pagination: when scroll offset approaches end, call TranscriptStore::list with increased offset to append more entries)

- [ ] T034 [US3] Write test_history_search unit test in crates/vox_ui/src/history_panel.rs or crates/vox_core/src/pipeline/transcript.rs — insert multiple TranscriptEntry records into TranscriptStore with distinct polished_text values, call search("specific query"), verify returned Vec contains only entries whose raw_text or polished_text contains the query string, verify case-insensitive matching

**Checkpoint**: History panel fully functional with search, copy, inline delete, modal clear all, and smooth virtualized scrolling at scale.

---

## Phase 6: User Story 4 — Manage Custom Dictionary (Priority: P2)

**Goal**: Full Dictionary panel with add, edit inline, delete (inline confirmation), search, sort, command phrase toggle, import/export JSON round-trip.

**Independent Test**: Add entries with spoken/written/category, edit inline, delete with confirm, search by all fields, sort by columns, toggle command phrase, export to JSON file, delete all, import from JSON, verify round-trip.

### Implementation for User Story 4

- [ ] T035 [US4] Implement add entry form in crates/vox_ui/src/dictionary_panel.rs — three Entity<TextInput> fields (new_spoken, new_written, new_category) rendered in a horizontal row at top of panel with an Add Button (Primary); on Add click: validate spoken is non-empty, call DictionaryCache::add(spoken, written, category, false), on success clear input fields and reload entries via list(None); on error (duplicate spoken form) display inline error message below the add form in status_error color

- [ ] T036 [US4] Implement inline editing and delete in crates/vox_ui/src/dictionary_panel.rs — per-entry Edit Button (Ghost, Edit icon): on click set editing_entry = Some(entry.id), swap that row to three editable Entity<TextInput> fields pre-filled with current values, show Confirm (Primary) and Cancel (Ghost) buttons; Confirm calls DictionaryCache::update(id, spoken, written, category, is_command_phrase), clears editing_entry, reloads entries; Cancel clears editing_entry; per-entry Delete Button (Ghost, Danger color): on click set confirming_delete = Some((entry.id, cx.spawn 5-second timer)), render "Confirm? [Yes] [No]"; Yes calls DictionaryCache::delete(id), removes from local entries; No/timeout clears confirming_delete

- [ ] T037 [US4] Implement search, sort, and command phrase toggle in crates/vox_ui/src/dictionary_panel.rs — Entity<TextInput> search field at panel top, on_change: if query non-empty call DictionaryCache::search(query), else call list(None), update local entries; column headers ("Spoken", "Written", "Category", "Uses") rendered as clickable divs with sort indicator arrow; click toggles sort_field and sort_ascending, apply entries.sort_by() locally using the selected field; per-entry Toggle for is_command_phrase, on_change calls DictionaryCache::update() with toggled value

- [ ] T038 [US4] Implement Import and Export actions in crates/vox_ui/src/dictionary_panel.rs — Export Button (Secondary) at panel header: on click cx.spawn(async move |cx| { let path = cx.prompt_for_new_path(&dirs::document_dir(), Some("dictionary.json")).await?; if let Some(path) = path { let json = DictionaryCache::export_json()?; std::fs::write(path, json)?; } }); Import Button (Secondary): on click cx.spawn(async move |cx| { let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false }).await?; if let Some(paths) = paths { let json = std::fs::read_to_string(&paths[0])?; let result = DictionaryCache::import_json(&json)?; /* display result.added, result.skipped as status message */ } }); after import, reload entries list

- [ ] T039 [US4] Write test_dictionary_add unit test in crates/vox_ui/src/dictionary_panel.rs or crates/vox_core/src/dictionary.rs — create DictionaryCache with temp database, call add("teh", "the", "typo", false), verify list(None) contains an entry with spoken="teh" and written="the"

- [ ] T040 [US4] Write test_dictionary_delete unit test in crates/vox_ui/src/dictionary_panel.rs or crates/vox_core/src/dictionary.rs — create DictionaryCache with temp database, add an entry, capture its id, call delete(id), verify list(None) no longer contains that entry

- [ ] T040a [US4] Write test_dictionary_round_trip unit test in crates/vox_core/src/dictionary.rs — create DictionaryCache with temp database, add 3 entries with distinct spoken/written/category/is_command_phrase values, call export_json() and capture the JSON string, call clear or delete all entries, call import_json(&json), verify list(None) returns 3 entries with identical spoken, written, category, and is_command_phrase fields matching the originals (validates SC-009 lossless round-trip)

**Checkpoint**: Dictionary panel fully functional with add, inline edit, delete, search, sort, command phrase toggle, JSON import/export.

---

## Phase 7: User Story 5 — Monitor and Manage Models (Priority: P3)

**Goal**: Full Model panel with detailed status display, download progress, retry, open folder, benchmark results, and model swap with pipeline stop/restart and error recovery.

**Independent Test**: View model statuses with file size and VRAM, trigger retry on failed download, open model folder in OS file manager, view benchmark metric per loaded model, swap model with valid GGUF (success path), swap with invalid file (error recovery path).

### Implementation for User Story 5

- [ ] T041 [US5] Implement detailed model status display and download progress in crates/vox_ui/src/model_panel.rs — render each model as a card div with: model name (bold, 16px), status badge (colored chip: Missing=text_muted, Downloading=accent, Downloaded=status_success, Loaded=status_success with Check icon, Error=status_error with Error icon), file size from ModelInfo.size_bytes formatted as "X.X MB", VRAM from ModelRuntimeInfo.vram_bytes formatted as "X.X GB" when Loaded; for Downloading state: progress bar div (full-width track with accent-filled portion proportional to bytes_downloaded/bytes_total), text label "X.X / Y.Y MB (Z%)"

- [ ] T042 [US5] Implement "Retry Download" and "Open Model Folder" actions in crates/vox_ui/src/model_panel.rs — Retry Download: Button (Secondary, Retry icon) rendered only for Error-state models, on click call VoxState model download mechanism for that specific model (set state to Downloading, trigger ModelDownloader), subscribe to download events to update progress; Open Model Folder: Button (Ghost, Folder icon) at panel header, on click spawn background command: on Windows `std::process::Command::new("explorer").arg(model_dir_path)`, on macOS `std::process::Command::new("open").arg(model_dir_path)`, create directory if it doesn't exist

- [ ] T043 [US5] Implement benchmark display in crates/vox_ui/src/model_panel.rs — for Loaded-state models, read BenchmarkResult from VoxState.model_runtime(model_name).benchmark; render metric_name and formatted value as a line below the status: "Real-time factor: 12.3x" for ASR, "Tokens/sec: 45.2" for LLM, "Inferences/sec: 1250" for VAD; if benchmark is None (not yet computed), show "Benchmark: pending" in muted text

- [ ] T044 [US5] Implement "Swap Model" action in crates/vox_ui/src/model_panel.rs — Button (Secondary, "Swap Model") per model card, on click cx.spawn(async move |handle, cx| { let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false }).await?; if let Some(paths) = paths { let selected = &paths[0]; validate extension (.gguf, .ggml, or .onnx matching model type); let was_active = check if pipeline is active via VoxState; if was_active, stop pipeline; copy selected file to model_dir() with original filename via std::fs::copy; let previous_filename = current model filename from Settings; update Settings model filename (whisper_model or llm_model) via update_settings(); attempt model reload (same loading path as main.rs init); on success: set ModelRuntimeInfo to Loaded, run benchmark, store result, restart pipeline if was_active; on failure: show error in model card, restore previous filename via update_settings(), reload original model, restart pipeline if was_active; } })

**Checkpoint**: Model panel fully functional with status display, progress bars, retry, open folder, benchmarks, and model swap with error recovery.

---

## Phase 8: User Story 6 — View Application Logs (Priority: P3)

**Goal**: Full Log panel with severity level filter, auto-scroll toggle control, copy per entry, and clear display action.

**Independent Test**: Generate log events at multiple levels, set filter to Error (only errors visible), set to Info (error+warn+info visible), toggle auto-scroll off (view stays put on new entries), copy entry to clipboard, clear display (new entries keep appearing).

### Implementation for User Story 6

- [ ] T045 [US6] Implement severity level filter in crates/vox_ui/src/log_panel.rs — row of 5 clickable level labels (Error, Warn, Info, Debug, Trace) or a Select dropdown at top of panel next to header; clicking a level sets filter_level; in uniform_list: compute filtered entries as self.log_store.read(cx).entries.iter().filter(|e| e.level <= self.filter_level).collect(), use filtered count for item_count, index into filtered list in render callback; update filtered view on filter change via cx.notify()

- [ ] T046 [US6] Implement auto-scroll toggle, copy, and clear actions in crates/vox_ui/src/log_panel.rs — Toggle component for auto_scroll next to filter controls, label "Auto-scroll", on_change sets self.auto_scroll; when auto_scroll is false, LogStoreEvent handler does NOT call scroll_to_item (view stays at current position); per-entry copy Button (Ghost, Copy icon) that formats entry as "[timestamp] [LEVEL] target: message" and calls cx.write_to_clipboard(ClipboardItem::new_string(formatted)); Clear Button (Ghost, Clear icon) at panel header, on click calls log_store.update(cx, |store, cx| { store.entries.clear(); cx.emit(LogStoreEvent::Cleared); cx.notify(); })

- [ ] T047 [US6] Validate high-throughput log rendering in crates/vox_ui/src/log_panel.rs — ensure uniform_list correctly renders filtered entries at 100+ entries/sec; verify LogStore polling task batches notifications (receive all available entries per poll cycle rather than one at a time); verify filter_level comparison uses correct ordering (Error < Warn < Info < Debug < Trace so e.level <= filter_level shows entries AT or ABOVE selected severity); verify color coding uses correct theme token per level (log_error for Error, log_warn for Warn, log_info for Info, log_debug for Debug, log_trace for Trace)

**Checkpoint**: Log panel fully functional with real-time display, level filter, auto-scroll toggle, copy, clear, and performant rendering under high throughput.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Final validation, zero warnings, comprehensive testing.

- [ ] T048 Verify zero compiler warnings — run `cargo build -p vox_ui` and `cargo build -p vox_core --features cuda` (Windows) or `cargo build -p vox_core --features metal` (macOS), fix any warnings including unused imports, dead code, missing `///` doc comments on all pub items per Constitution Principle VII

- [ ] T049 Run all automated tests — execute `cargo test -p vox_ui` and `cargo test -p vox_core --features cuda` (or `--features metal`), verify all 5 specified tests pass: test_panel_switching, test_settings_persistence, test_history_search, test_dictionary_add, test_dictionary_delete; fix any failures

- [ ] T050 Run quickstart.md manual test scenarios TS-001 through TS-030 — validate each scenario against expected behavior documented in specs/013-settings-window/quickstart.md; fix any discrepancies found during validation

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 completion (T001 module declarations needed for new files)
- **US1 (Phase 3)**: Depends on Phase 2 completion (shared UI components: Scrollbar, Button, TextInput, Icon, Toggle)
- **US2 (Phase 4)**: Depends on US1 T012-T016 (workspace shell + SettingsPanel entity exist)
- **US3 (Phase 5)**: Depends on US1 T012, T017 (workspace shell + HistoryPanel entity exists)
- **US4 (Phase 6)**: Depends on US1 T012, T018 (workspace shell + DictionaryPanel entity exists)
- **US5 (Phase 7)**: Depends on US1 T012, T019 (workspace shell + ModelPanel entity exists)
- **US6 (Phase 8)**: Depends on US1 T012, T020, T021 (workspace shell + LogPanel/LogStore + LogSink wiring)
- **Polish (Phase 9)**: Depends on all user stories being complete

### User Story Dependencies

- **US1 (P1)**: Depends on Foundational phase. **BLOCKS all other stories** — creates workspace shell and all initial panel entities.
- **US2 (P1)**: Depends on US1. Independent of US3, US4, US5, US6.
- **US3 (P2)**: Depends on US1. Independent of US2, US4, US5, US6.
- **US4 (P2)**: Depends on US1. Independent of US2, US3, US5, US6.
- **US5 (P3)**: Depends on US1. Independent of US2, US3, US4, US6.
- **US6 (P3)**: Depends on US1. Independent of US2, US3, US4, US5.

### Within Each User Story

- UI component dependencies (Slider, Select, HotkeyRecorder) built before panel sections that use them (US2)
- Data display tasks before interactive feature tasks
- Core functionality before edge case handling
- Tests written after the features they validate are implemented

### Parallel Opportunities

- **Phase 1**: T002-T006 run in parallel (5 different files, all depend only on T001)
- **Phase 2**: T007-T011 run in parallel (5 different files)
- **US1 (Phase 3)**: T016-T020 run in parallel (5 panel entities in 5 different files, all depend on T012-T015)
- **US2 (Phase 4)**: T023-T025 run in parallel (3 components in 3 different files)
- **Cross-Story**: After US1 completes, US2 through US6 can ALL run in parallel (each modifies only its own panel file)

---

## Parallel Example: Phase 2 (Foundational)

```text
# All 5 components in different files — full parallelization:
Task: T007 "Scrollbar Element in scrollbar.rs"
Task: T008 "Button component in button.rs"
Task: T009 "TextInput component in text_input.rs"
Task: T010 "Icon utilities in icon.rs"
Task: T011 "Toggle component in toggle.rs"
```

## Parallel Example: Phase 3 (US1 Panel Entities)

```text
# After T012-T015 (workspace shell) completes, launch 5 panel entities:
Task: T016 "SettingsPanel initial entity in settings_panel.rs"
Task: T017 "HistoryPanel initial entity in history_panel.rs"
Task: T018 "DictionaryPanel initial entity in dictionary_panel.rs"
Task: T019 "ModelPanel initial entity in model_panel.rs"
Task: T020 "LogPanel initial entity with LogStore in log_panel.rs"
```

## Parallel Example: Cross-Story (After US1)

```text
# After US1 completes, launch remaining stories in parallel:
Team A: US2 (T023-T030) — Settings panel controls and persistence
Team B: US3 (T031-T034) — History panel search, copy, delete, clear
Team C: US4 (T035-T040) — Dictionary panel CRUD and import/export
Team D: US5 (T041-T044) — Model panel status, benchmarks, swap
Team E: US6 (T045-T047) — Log panel filter, auto-scroll, copy, clear
```

---

## Implementation Strategy

### MVP First (US1 Only)

1. Complete Phase 1: Setup (T001-T006)
2. Complete Phase 2: Foundational UI components (T007-T011)
3. Complete Phase 3: US1 — Workspace shell with all panel entities (T012-T022)
4. **STOP and VALIDATE**: Settings window opens, sidebar switches 5 panels, status bar shows data, all panels render real content
5. This delivers a navigable workspace with working panels as the minimum viable increment

### Incremental Delivery

1. Setup + Foundational → Infrastructure ready
2. US1 → Workspace shell (MVP) — navigable, all panels render
3. US2 → Full Settings panel — all controls, immediate persistence
4. US3 → Full History panel — search, copy, delete, clear, virtualized
5. US4 → Full Dictionary panel — CRUD, search, sort, import/export
6. US5 → Full Model panel — status, benchmarks, swap
7. US6 → Full Log panel — filter, auto-scroll, copy, clear
8. Polish → Zero warnings, all tests pass, manual validation complete

### Sequential Execution (Single Developer)

Phase 1 → Phase 2 → Phase 3 (US1) → Phase 4 (US2) → Phase 5 (US3) → Phase 6 (US4) → Phase 7 (US5) → Phase 8 (US6) → Phase 9 (Polish)

Each phase is a complete, testable increment. After each user story completes, that panel is fully functional.

---

## Notes

- Every panel entity created in US1 has a real Render implementation connected to its real data source — no placeholders, no stubs, no todo!()
- US1 panels render their actual initial/empty state (e.g., "No transcript history" when database is empty) — this is the genuine application state, not a skeleton
- [P] tasks can run in different files without dependency conflicts
- [Story] labels map tasks to spec.md user stories for traceability
- Tests are placed in their story phases after the features they validate
- All file paths are relative to repository root (D:\SRC\vox\)
