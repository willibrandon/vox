use std::path::PathBuf;
use std::process;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use vox_diag::client::DiagnosticsClient;

/// CLI tool for interacting with a running Vox instance.
///
/// Connects to the diagnostics socket of a running Vox process and
/// executes commands. Auto-discovers the instance unless `--pid` is given.
#[derive(Parser)]
#[command(name = "vox-tool", version, about)]
struct Cli {
    /// PID of the Vox instance to connect to.
    ///
    /// If omitted, auto-discovers a running instance. Errors if multiple
    /// instances are running without this flag.
    #[arg(long, global = true)]
    pid: Option<u32>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show application status (pipeline, GPU, models, audio).
    Status,

    /// Read or write settings.
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },

    /// Show recent log entries.
    Logs {
        /// Number of entries to retrieve (default: 50).
        #[arg(long, short = 'n')]
        count: Option<u32>,

        /// Minimum log level (error, warn, info, debug, trace).
        #[arg(long, short)]
        level: Option<String>,
    },

    /// Start or stop recording.
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },

    /// Inject a WAV audio file into the pipeline.
    Inject {
        /// Path to a WAV file.
        path: PathBuf,
    },

    /// Capture a window screenshot.
    Screenshot {
        /// Window to capture: "overlay" (default) or "settings".
        #[arg(long, short, default_value = "overlay")]
        window: String,

        /// Save PNG to file instead of printing base64.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// Subscribe to live pipeline events.
    Subscribe {
        /// Comma-separated event types: pipeline_state, audio_rms, transcript.
        #[arg(long, short, default_value = "pipeline_state,transcript")]
        events: String,
    },

    /// Show recent transcripts.
    Transcripts {
        /// Number of entries to retrieve (default: 10).
        #[arg(long, short = 'n')]
        count: Option<u32>,
    },

    /// Launch the Vox application in the background.
    Launch,

    /// Quit the running Vox application gracefully.
    Quit,

    /// List all running Vox instances.
    List,
}

#[derive(Subcommand)]
enum SettingsAction {
    /// Get all settings or a specific key.
    Get {
        /// Setting key to read (omit for all settings).
        key: Option<String>,
    },
    /// Set a setting value.
    Set {
        /// Setting key.
        key: String,
        /// New value (JSON literal: number, string, bool).
        value: String,
    },
}

#[derive(Subcommand)]
enum RecordAction {
    /// Start a recording session.
    Start,
    /// Stop the current recording session.
    Stop,
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(cli) {
        eprintln!("error: {err:#}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    // These commands don't need an existing connection
    match cli.command {
        Command::Launch => return cmd_launch(),
        Command::List => return cmd_list(),
        _ => {}
    }

    let mut client = DiagnosticsClient::connect_auto_or_pid(cli.pid)?;

    match cli.command {
        Command::Status => cmd_status(&mut client),
        Command::Settings { action } => {
            let action = action.unwrap_or(SettingsAction::Get { key: None });
            cmd_settings(&mut client, action)
        }
        Command::Logs { count, level } => cmd_logs(&mut client, count, level),
        Command::Record { action } => cmd_record(&mut client, action),
        Command::Inject { path } => cmd_inject(&mut client, path),
        Command::Screenshot { window, output } => cmd_screenshot(&mut client, window, output),
        Command::Subscribe { events } => cmd_subscribe(&mut client, events),
        Command::Transcripts { count } => cmd_transcripts(&mut client, count),
        Command::Quit => cmd_quit(&mut client),
        Command::Launch | Command::List => unreachable!(),
    }
}

fn cmd_status(client: &mut DiagnosticsClient) -> Result<()> {
    let result = client.request("status", None)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn cmd_settings(client: &mut DiagnosticsClient, action: SettingsAction) -> Result<()> {
    match action {
        SettingsAction::Get { key } => {
            let params = match key {
                Some(key) => json!({"action": "get", "key": key}),
                None => json!({"action": "get"}),
            };
            let result = client.request("settings", Some(params))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        SettingsAction::Set { key, value } => {
            let parsed_value: serde_json::Value = serde_json::from_str(&value)
                .unwrap_or_else(|_| serde_json::Value::String(value));
            let params = json!({"action": "set", "key": key, "value": parsed_value});
            let _result = client.request("settings", Some(params))?;
            println!("ok");
        }
    }
    Ok(())
}

fn cmd_logs(
    client: &mut DiagnosticsClient,
    count: Option<u32>,
    level: Option<String>,
) -> Result<()> {
    let mut params = json!({});
    if let Some(count) = count {
        params["count"] = json!(count);
    }
    if let Some(level) = level {
        params["min_level"] = json!(level);
    }

    let result = client.request("logs", Some(params))?;
    let entries = result["entries"]
        .as_array()
        .context("expected 'entries' array in response")?;

    for entry in entries {
        let timestamp = entry["timestamp"].as_str().unwrap_or("");
        let level = entry["level"].as_str().unwrap_or("???");
        let target = entry["target"].as_str().unwrap_or("");
        let message = entry["message"].as_str().unwrap_or("");
        println!("{timestamp} [{level:>5}] {target}: {message}");
    }
    Ok(())
}

fn cmd_record(client: &mut DiagnosticsClient, action: RecordAction) -> Result<()> {
    let action_str = match action {
        RecordAction::Start => "start",
        RecordAction::Stop => "stop",
    };
    let _result = client.request("record", Some(json!({"action": action_str})))?;
    println!("ok");
    Ok(())
}

fn cmd_inject(client: &mut DiagnosticsClient, path: PathBuf) -> Result<()> {
    let path_str = path
        .to_str()
        .context("path contains non-UTF-8 characters")?;

    let result = client.request("inject_audio", Some(json!({"path": path_str})))?;

    let raw = result["raw_transcript"].as_str().unwrap_or("");
    let polished = result["polished_text"].as_str().unwrap_or("");
    let latency = result["latency_ms"].as_u64().unwrap_or(0);

    println!("Raw:      {raw}");
    println!("Polished: {polished}");
    println!("Latency:  {latency}ms");
    Ok(())
}

fn cmd_screenshot(
    client: &mut DiagnosticsClient,
    window: String,
    output: Option<PathBuf>,
) -> Result<()> {
    let result = client.request("screenshot", Some(json!({"window": window})))?;
    let data = result["data"]
        .as_str()
        .context("expected 'data' field in screenshot response")?;

    match output {
        Some(path) => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .context("failed to decode base64 PNG data")?;
            std::fs::write(&path, &bytes)
                .with_context(|| format!("failed to write {}", path.display()))?;
            println!("Saved {} bytes to {}", bytes.len(), path.display());
        }
        None => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .context("failed to decode base64 PNG data")?;
            println!("PNG: {} bytes (use --output to save)", bytes.len());
        }
    }
    Ok(())
}

fn cmd_subscribe(client: &mut DiagnosticsClient, events: String) -> Result<()> {
    let event_list: Vec<&str> = events.split(',').map(|s| s.trim()).collect();
    let params = json!({"events": event_list});

    // Send subscribe request and get ack
    let result = client.request("subscribe", Some(params))?;
    let subscribed = result["subscribed"]
        .as_array()
        .context("expected 'subscribed' array in response")?;
    let names: Vec<&str> = subscribed
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    eprintln!("Subscribed to: {}", names.join(", "));
    eprintln!("Press Ctrl+C to stop.");

    // Read event lines until disconnect or Ctrl+C
    loop {
        let line = match client.read_line() {
            Ok(line) => line,
            Err(_) => break,
        };

        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<vox_diag::protocol::Event>(&line) {
            Ok(event) => {
                let data = serde_json::to_string(&event.data).unwrap_or_default();
                println!("[{}] {data}", event.event);
            }
            Err(_) => {
                println!("{line}");
            }
        }
    }

    Ok(())
}

fn cmd_transcripts(client: &mut DiagnosticsClient, count: Option<u32>) -> Result<()> {
    let mut params = json!({});
    if let Some(count) = count {
        params["count"] = json!(count);
    }

    let result = client.request("transcripts", Some(params))?;
    let entries = result["entries"]
        .as_array()
        .context("expected 'entries' array in response")?;

    for entry in entries {
        let timestamp = entry["timestamp"].as_str().unwrap_or("");
        let raw = entry["raw"].as_str().unwrap_or("");
        let polished = entry["polished"].as_str().unwrap_or("");
        let latency = entry["latency_ms"].as_u64().unwrap_or(0);
        println!("{timestamp}  [{latency}ms]");
        println!("  Raw:      {raw}");
        println!("  Polished: {polished}");
        println!();
    }
    Ok(())
}

fn cmd_list() -> Result<()> {
    let instances = vox_diag::client::discover_instances()?;

    if instances.is_empty() {
        println!("No running Vox instances found.");
        return Ok(());
    }

    println!("{:<8} {}", "PID", "SOCKET");
    for instance in &instances {
        println!("{:<8} {}", instance.pid, instance.socket_path.display());
    }
    println!("\n{} instance(s)", instances.len());
    Ok(())
}

fn cmd_launch() -> Result<()> {
    let vox_path = find_vox_binary()?;

    let child = std::process::Command::new(&vox_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch {}", vox_path.display()))?;

    let pid = child.id();
    eprintln!("Launched Vox (PID {pid}), waiting for diagnostics socket...");

    let socket_dir = dirs::home_dir()
        .context("no home directory")?
        .join(".vox")
        .join("sockets");
    let socket_path = socket_dir.join(format!("{pid}.diagnostics.socket"));

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if socket_path.exists() {
            println!("Vox launched and ready (PID {pid})");
            return Ok(());
        }
    }

    println!("Vox launched (PID {pid}) but diagnostics socket not yet available. It may still be loading models.");
    Ok(())
}

fn cmd_quit(client: &mut DiagnosticsClient) -> Result<()> {
    client.request("quit", None)?;
    println!("Vox is shutting down");
    Ok(())
}

/// Locate the `vox` binary next to this executable.
fn find_vox_binary() -> Result<PathBuf> {
    let exe_name = if cfg!(windows) { "vox.exe" } else { "vox" };

    if let Ok(self_exe) = std::env::current_exe() {
        if let Some(dir) = self_exe.parent() {
            let sibling = dir.join(exe_name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }

    anyhow::bail!("could not find '{exe_name}' next to vox-tool — build Vox first with `cargo build -p vox`")
}
