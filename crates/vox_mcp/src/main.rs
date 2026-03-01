use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{
        CallToolResult, Content, ErrorData, Implementation,
        ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use vox_diag::client::DiagnosticsClient;

/// Parameters for the `vox_settings_get` tool.
#[derive(Deserialize, JsonSchema)]
struct SettingsGetParams {
    /// Setting key to read. Omit to get all settings.
    key: Option<String>,
}

/// Parameters for the `vox_settings_set` tool.
#[derive(Deserialize, JsonSchema)]
struct SettingsSetParams {
    /// Setting key to write.
    key: String,
    /// New value (number, string, boolean, or object).
    value: serde_json::Value,
}

/// Parameters for the `vox_logs` tool.
#[derive(Deserialize, JsonSchema)]
struct LogsParams {
    /// Number of log entries to retrieve (default: 50).
    count: Option<u32>,
    /// Minimum log level: error, warn, info, debug, trace.
    level: Option<String>,
}

/// Parameters for the `vox_inject_audio` tool.
#[derive(Deserialize, JsonSchema)]
struct InjectAudioParams {
    /// Absolute path to a WAV audio file on disk.
    path: String,
}

/// Parameters for the `vox_screenshot` tool.
#[derive(Deserialize, JsonSchema)]
struct ScreenshotParams {
    /// Window to capture: "overlay" (default) or "settings".
    window: Option<String>,
}

/// Parameters for the `vox_transcripts` tool.
#[derive(Deserialize, JsonSchema)]
struct TranscriptsParams {
    /// Number of recent transcripts to retrieve (default: 10).
    count: Option<u32>,
}

/// MCP server exposing Vox diagnostics as tools for AI assistants.
///
/// Forwards each MCP tool call to the corresponding diagnostics protocol
/// method over a fresh UDS connection. Nine tools are exposed; `subscribe`
/// is intentionally excluded per FR-032 — streaming events are not
/// compatible with the MCP request/response model.
pub struct VoxMcp {
    /// Target PID for connecting to a specific Vox instance.
    /// `None` means auto-discover. Updated by `vox_launch`.
    target_pid: Mutex<Option<u32>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl VoxMcp {
    /// Create a new MCP server, optionally targeting a specific Vox PID.
    pub fn new(pid: Option<u32>) -> Self {
        Self {
            target_pid: Mutex::new(pid),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get Vox application status: pipeline state, GPU info, loaded models, audio device, and last transcription latency")]
    fn vox_status(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.call_diag("status", None)?;
        Ok(CallToolResult::success(vec![Content::text(pretty(&result)?)]))
    }

    #[tool(description = "Read Vox settings. Omit key to get all settings, or specify a key to read one")]
    fn vox_settings_get(
        &self,
        Parameters(params): Parameters<SettingsGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let req = match params.key {
            Some(key) => json!({"action": "get", "key": key}),
            None => json!({"action": "get"}),
        };
        let result = self.call_diag("settings", Some(req))?;
        Ok(CallToolResult::success(vec![Content::text(pretty(&result)?)]))
    }

    #[tool(description = "Write a Vox setting. Value can be a number, string, boolean, or object")]
    fn vox_settings_set(
        &self,
        Parameters(params): Parameters<SettingsSetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let req = json!({"action": "set", "key": params.key, "value": params.value});
        self.call_diag("settings", Some(req))?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Read recent log entries from the running Vox instance")]
    fn vox_logs(
        &self,
        Parameters(params): Parameters<LogsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut req = json!({});
        if let Some(count) = params.count {
            req["count"] = json!(count);
        }
        if let Some(level) = params.level {
            req["min_level"] = json!(level);
        }
        let result = self.call_diag("logs", Some(req))?;
        Ok(CallToolResult::success(vec![Content::text(pretty(&result)?)]))
    }

    #[tool(description = "Start a recording session in Vox")]
    fn vox_record_start(&self) -> Result<CallToolResult, ErrorData> {
        self.call_diag("record", Some(json!({"action": "start"})))?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Stop the current recording session in Vox")]
    fn vox_record_stop(&self) -> Result<CallToolResult, ErrorData> {
        self.call_diag("record", Some(json!({"action": "stop"})))?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Inject a WAV audio file into the Vox pipeline for testing. Returns the transcript result with raw text, polished text, and latency")]
    fn vox_inject_audio(
        &self,
        Parameters(params): Parameters<InjectAudioParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.call_diag("inject_audio", Some(json!({"path": params.path})))?;
        Ok(CallToolResult::success(vec![Content::text(pretty(&result)?)]))
    }

    #[tool(description = "Capture a screenshot of a Vox window as a PNG image")]
    fn vox_screenshot(
        &self,
        Parameters(params): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let window = params.window.unwrap_or_else(|| "overlay".into());
        let result = self.call_diag("screenshot", Some(json!({"window": window})))?;
        let data = result["data"]
            .as_str()
            .ok_or_else(|| {
                ErrorData::internal_error("missing 'data' in screenshot response", None)
            })?;
        Ok(CallToolResult::success(vec![Content::image(data, "image/png")]))
    }

    #[tool(description = "Read recent transcripts from Vox with raw text, polished text, and latency")]
    fn vox_transcripts(
        &self,
        Parameters(params): Parameters<TranscriptsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut req = json!({});
        if let Some(count) = params.count {
            req["count"] = json!(count);
        }
        let result = self.call_diag("transcripts", Some(req))?;
        Ok(CallToolResult::success(vec![Content::text(pretty(&result)?)]))
    }

    #[tool(description = "Launch the Vox application. Starts the Vox process in the background and waits for it to become reachable via diagnostics socket. Returns the PID of the launched process.")]
    fn vox_launch(&self) -> Result<CallToolResult, ErrorData> {
        let vox_path = find_vox_binary()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let child = std::process::Command::new(&vox_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                ErrorData::internal_error(
                    format!("failed to launch {}: {e}", vox_path.display()),
                    None,
                )
            })?;

        let pid = child.id();

        // Wait for the diagnostics socket file to appear (up to 30s).
        let socket_dir = dirs::home_dir()
            .ok_or_else(|| ErrorData::internal_error("no home directory", None))?
            .join(".vox")
            .join("sockets");
        let socket_path = socket_dir.join(format!("{pid}.diagnostics.socket"));

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
            if socket_path.exists() {
                // Target this PID for subsequent tool calls
                if let Ok(mut guard) = self.target_pid.lock() {
                    *guard = Some(pid);
                }
                return Ok(CallToolResult::success(vec![Content::text(
                    format!("Vox launched and ready (PID {pid})")
                )]));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(
            format!("Vox launched (PID {pid}) but diagnostics socket not yet available. It may still be loading models.")
        )]))
    }

    #[tool(description = "Quit the running Vox application gracefully")]
    fn vox_quit(&self) -> Result<CallToolResult, ErrorData> {
        match self.call_diag("quit", None) {
            Ok(_) => {
                if let Ok(mut guard) = self.target_pid.lock() {
                    *guard = None;
                }
                Ok(CallToolResult::success(vec![Content::text("Vox is shutting down")]))
            }
            Err(_) => {
                Err(ErrorData::internal_error("Vox is not running", None))
            }
        }
    }

    #[tool(description = "List all running Vox instances. Returns PID and socket path for each discovered instance")]
    fn vox_list(&self) -> Result<CallToolResult, ErrorData> {
        let instances = vox_diag::client::discover_instances()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let list: Vec<serde_json::Value> = instances
            .iter()
            .map(|i| json!({"pid": i.pid, "socket": i.socket_path.display().to_string()}))
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            pretty(&json!({"instances": list, "count": instances.len()}))?
        )]))
    }
}

impl VoxMcp {
    /// Send a request to the diagnostics socket and return the result value.
    ///
    /// Creates a fresh connection for each call — same pattern the CLI uses.
    /// The connection is dropped after the response is received.
    fn call_diag(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ErrorData> {
        let pid = {
            let guard = self
                .target_pid
                .lock()
                .map_err(|e| ErrorData::internal_error(format!("lock poisoned: {e}"), None))?;
            *guard
        };

        let mut client = DiagnosticsClient::connect_auto_or_pid(pid).map_err(|e| {
            ErrorData::internal_error(
                format!("Vox is not running or not reachable: {e}"),
                None,
            )
        })?;

        client.request(method, params).map_err(|e| {
            ErrorData::internal_error(e.to_string(), None)
        })
    }
}

/// Pretty-print a JSON value.
fn pretty(value: &serde_json::Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

/// Locate the `vox` binary.
///
/// Search order: sibling of the current executable (same directory as `vox-mcp`),
/// then fall back to `PATH` lookup.
fn find_vox_binary() -> anyhow::Result<PathBuf> {
    let exe_name = if cfg!(windows) { "vox.exe" } else { "vox" };

    // Check sibling of this binary (both live in target/debug or target/release)
    if let Ok(self_exe) = std::env::current_exe() {
        if let Some(dir) = self_exe.parent() {
            let sibling = dir.join(exe_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }

    anyhow::bail!("could not find '{exe_name}' next to vox-mcp — build Vox first with `cargo build -p vox`")
}

#[tool_handler]
impl ServerHandler for VoxMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "vox-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            instructions: Some(
                "Control and inspect a running Vox dictation engine instance. \
                 Tools: status, settings, logs, record, inject audio, screenshot, transcripts."
                    .into(),
            ),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // MCP uses stdout for protocol framing — direct tracing to stderr
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let pid = parse_pid_arg();
    let service = VoxMcp::new(pid)
        .serve(stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

/// Parse `--pid <PID>` from command-line arguments.
fn parse_pid_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--pid" {
            return args.get(i + 1).and_then(|s| s.parse().ok());
        }
        i += 1;
    }
    None
}
