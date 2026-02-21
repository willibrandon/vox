# API Contract: Pipeline Orchestration

**Module**: `crates/vox_core/src/pipeline/`

## PipelineState

The operational state of the pipeline, broadcast to all UI subscribers on every transition.

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineState {
    /// Pipeline is loaded and waiting for hotkey activation.
    Idle,

    /// Microphone is active, VAD is processing audio windows.
    Listening,

    /// A speech segment is being processed through ASR and/or LLM.
    /// `raw_text` is None until ASR completes, then Some(transcript).
    Processing { raw_text: Option<String> },

    /// Polished text is being injected into the focused application.
    Injecting { polished_text: String },

    /// A recoverable error occurred. Pipeline returns to Listening or Idle.
    Error { message: String },
}
```

## ActivationMode

Recording trigger behavior, persisted in user settings.

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivationMode {
    /// Hold hotkey to record, release to stop and process.
    HoldToTalk,
    /// Press once to start, press again to stop.
    Toggle,
    /// Double-press to enter continuous mode, single press to exit.
    HandsFree,
}

impl Default for ActivationMode {
    fn default() -> Self { Self::HoldToTalk }
}
```

## PipelineCommand

Commands sent from the PipelineController to the Pipeline's processing loop via an mpsc channel. This decouples hotkey handling from the async run loop, avoiding `&mut` aliasing between `run()` and hotkey handlers.

```rust
#[derive(Debug)]
pub enum PipelineCommand {
    /// Stop the pipeline after the current segment completes (FR-018).
    Stop,
}
```

## Pipeline

The pipeline orchestrator. Coordinates audio capture, VAD, ASR, LLM, dictionary substitution, and text injection into a single async flow.

All components must be loaded and operational before constructing a Pipeline (Constitution Principle III).

```rust
impl Pipeline {
    /// Create a new pipeline with all Send+Sync components.
    ///
    /// FR-002 requires 6 components (audio, VAD, ASR, LLM, injector, dictionary).
    /// Of these, Pipeline::new() takes the 4 that are Send+Sync (ASR, LLM,
    /// dictionary, transcript_store) plus VAD config (model path + config for
    /// deferred creation on the VAD thread, since SileroVad is NOT Send).
    /// The remaining 2 non-Send components are provided later:
    /// - AudioCapture: NOT Send — stays on caller's thread. Its ring buffer
    ///   consumer (HeapCons<f32>, which IS Send) is passed to start().
    /// - TextInjector: NOT Send — injection calls are made from the async
    ///   orchestrator task on the main thread (not from spawn_blocking).
    ///
    /// All 6 components MUST be loaded before the caller constructs a Pipeline.
    /// `command_rx` receives control commands (Stop) from the PipelineController.
    /// The caller keeps the corresponding `command_tx` (typically held by PipelineController).
    pub fn new(
        asr: AsrEngine,
        llm: PostProcessor,
        dictionary: DictionaryCache,
        transcript_store: TranscriptStore,
        state_tx: broadcast::Sender<PipelineState>,
        command_rx: mpsc::Receiver<PipelineCommand>,
        vad_model_path: PathBuf,
        vad_config: VadConfig,
    ) -> Self;

    /// Start the pipeline: spawn VAD thread using the provided audio consumer.
    ///
    /// The caller is responsible for creating and starting AudioCapture,
    /// then passing the owned ring buffer consumer here. AudioCapture itself
    /// stays on the caller's thread (it is NOT Send).
    ///
    /// `native_sample_rate` is the audio device's native rate (for resampler
    /// creation on the VAD thread). If it equals 16000, no resampling occurs.
    ///
    /// On success, broadcasts PipelineState::Listening. Between start()
    /// returning and run() beginning its select loop, the pipeline is in
    /// Listening state — the VAD thread is running and may enqueue segments,
    /// but they are buffered in the channel until run() starts draining them.
    /// The caller MUST call run() promptly after start() to avoid segment
    /// accumulation in the channel buffer.
    pub fn start(
        &mut self,
        consumer: HeapCons<f32>,
        native_sample_rate: u32,
    ) -> Result<()>;

    /// Main processing loop. Receives speech segments from the VAD thread
    /// and processes each through ASR → Dictionary → LLM → Injection.
    ///
    /// Uses `tokio::select!` to concurrently listen for:
    /// - Speech segments from the VAD thread (segment_rx)
    /// - Control commands from the PipelineController (command_rx)
    ///
    /// Segments are processed in strict FIFO order. A Stop command
    /// causes the loop to finish the current segment, then exit.
    pub async fn run(&mut self) -> Result<()>;

    /// Subscribe to pipeline state changes.
    ///
    /// Returns a broadcast receiver. Multiple subscribers are supported.
    /// If a subscriber falls behind, it receives the most recent state
    /// on next recv (latest-wins, no crash or deadlock).
    pub fn subscribe(&self) -> broadcast::Receiver<PipelineState>;

    /// Current pipeline state (latest broadcast value).
    pub fn state(&self) -> PipelineState;
}
```

## PipelineController

Translates hotkey events into pipeline start/stop commands based on the active ActivationMode. Communicates with Pipeline exclusively via the command channel — never holds `&mut Pipeline`.

```rust
impl PipelineController {
    /// Create a controller with a command channel sender.
    ///
    /// The corresponding receiver is passed to Pipeline::new().
    pub fn new(command_tx: mpsc::Sender<PipelineCommand>) -> Self;

    /// Handle hotkey press event.
    /// - HoldToTalk: sends Start (caller must call Pipeline::start externally)
    /// - Toggle: sends Start if idle, sends Stop if active
    /// - HandsFree: starts on double-press (within 300ms of last press)
    pub fn on_hotkey_press(&mut self);

    /// Handle hotkey release event.
    /// - HoldToTalk: sends Stop command
    /// - Toggle/HandsFree: no-op
    pub fn on_hotkey_release(&mut self);

    /// Force-stop by sending Stop command regardless of activation mode.
    pub fn force_stop(&mut self);

    /// Whether dictation is currently active.
    pub fn is_active(&self) -> bool;

    /// Current activation mode.
    pub fn mode(&self) -> ActivationMode;

    /// Set activation mode. Sends Stop first if dictation is active.
    /// The UI settings panel calls this method when the user changes the mode —
    /// there is no separate settings-layer indirection. This method both updates
    /// the controller's in-memory mode and persists the choice to the SQLite
    /// settings table (key "activation_mode").
    pub fn set_mode(&mut self, mode: ActivationMode);
}
```

## Startup Sequence

Shows how the caller wires Pipeline and PipelineController together:

```rust
// 1. Create shared channels
let (state_tx, _) = broadcast::channel::<PipelineState>(16);
let (command_tx, command_rx) = mpsc::channel::<PipelineCommand>(8);

// 2. Load all components (Constitution Principle III)
let asr = AsrEngine::new(&whisper_path, true)?;
let llm = PostProcessor::new(&qwen_path, true)?;
let dictionary = DictionaryCache::load(&dict_db_path)?;
let transcript_store = TranscriptStore::open(&transcript_db_path)?;

// 3. Create Pipeline and Controller
let mut pipeline = Pipeline::new(
    asr, llm, dictionary, transcript_store,
    state_tx, command_rx, vad_model_path, vad_config,
);
let mut controller = PipelineController::new(command_tx);

// 4. On hotkey activation:
let mut audio = AudioCapture::new(&audio_config)?;
audio.start()?;
let consumer = audio.take_consumer().expect("consumer available");
pipeline.start(consumer, audio.native_sample_rate())?;

// 5. Run the processing loop (async)
pipeline.run().await?;
```
