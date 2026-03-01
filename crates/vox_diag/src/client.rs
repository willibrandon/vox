use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::net::uds::UnixStream;
use crate::protocol::{ErrorInfo, Response};

/// Client for communicating with a running Vox diagnostics listener over UDS.
///
/// Shared by both `vox-tool` (CLI) and `vox-mcp` (MCP server). Handles socket
/// discovery, request/response serialization, and line-based protocol framing.
///
/// Uses a single `UnixStream` with byte-at-a-time reads (no `BufReader`)
/// to avoid any buffering interactions with the socket.
pub struct DiagnosticsClient {
    stream: UnixStream,
    next_id: AtomicU64,
}

impl DiagnosticsClient {
    /// Connect to a diagnostics socket at the given path.
    pub fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .with_context(|| format!("failed to connect to {}", path.display()))?;
        Ok(Self {
            stream,
            next_id: AtomicU64::new(1),
        })
    }

    /// Auto-discover and connect to a running Vox instance.
    ///
    /// Scans `~/.vox/sockets/` for `*.diagnostics.socket` files. Connects if
    /// exactly one is found. Returns an error listing PIDs if multiple are found.
    pub fn connect_auto() -> Result<Self> {
        let socket_dir = socket_dir()?;
        let sockets = discover_sockets(&socket_dir)?;

        match sockets.len() {
            0 => bail!("No running Vox instance found"),
            1 => Self::connect(&sockets[0]),
            _ => {
                let pids: Vec<String> = sockets
                    .iter()
                    .filter_map(|p| extract_pid_from_socket(p))
                    .map(|pid| pid.to_string())
                    .collect();
                bail!(
                    "Multiple Vox instances found (PIDs: {}). Use --pid <PID> to specify which instance.",
                    pids.join(", ")
                )
            }
        }
    }

    /// Connect to a specific PID's socket, or auto-discover if no PID given.
    pub fn connect_auto_or_pid(pid: Option<u32>) -> Result<Self> {
        match pid {
            Some(pid) => {
                let socket_dir = socket_dir()?;
                let path = socket_dir.join(format!("{pid}.diagnostics.socket"));
                Self::connect(&path)
            }
            None => Self::connect_auto(),
        }
    }

    /// Send a request and read the response, returning the result value.
    ///
    /// Returns an error if the server responds with an error payload.
    pub fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = if let Some(params) = params {
            serde_json::json!({"id": id, "method": method, "params": params})
        } else {
            serde_json::json!({"id": id, "method": method})
        };

        let mut payload = serde_json::to_vec(&request)
            .context("failed to serialize request")?;
        payload.push(b'\n');

        self.stream.write_all(&payload)
            .context("failed to write request")?;
        self.stream.flush()
            .context("failed to flush request")?;

        let line = self.read_line()
            .context("failed to read response")?;

        let response: Response = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse response: {line}"))?;

        if let Some(error) = response.error {
            bail!("diagnostics error {}: {}", error.code, error.message);
        }

        response.result.context("response had neither result nor error")
    }

    /// Read one raw line from the underlying stream.
    ///
    /// Reads one byte at a time until `\n` is found. No internal buffering.
    /// Used by the CLI subscribe command to read event notifications after the
    /// initial subscribe response.
    pub fn read_line(&mut self) -> Result<String> {
        let mut buf = [0u8; 1];
        let mut line = Vec::new();
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    if line.is_empty() {
                        bail!("diagnostics connection closed");
                    }
                    break;
                }
                Ok(_) => {
                    if buf[0] == b'\n' {
                        break;
                    }
                    line.push(buf[0]);
                }
                Err(e) => {
                    return Err(e).context("failed to read from diagnostics socket");
                }
            }
        }
        // Strip trailing \r if present
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        String::from_utf8(line).context("response was not valid UTF-8")
    }
}

/// Deserialize a `Response` from raw JSON (used by MCP/CLI when they need
/// to inspect error details directly).
impl Response {
    /// Parse a JSON line into a Response.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to parse Response JSON")
    }
}

/// Derive `Deserialize` for Response so client can parse server responses.
/// (Serialize is already derived in protocol.rs for the server side.)
impl<'de> serde::Deserialize<'de> for Response {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct ResponseHelper {
            id: u64,
            result: Option<Value>,
            error: Option<ErrorInfo>,
        }
        let helper = ResponseHelper::deserialize(deserializer)?;
        Ok(Response {
            id: helper.id,
            result: helper.result,
            error: helper.error,
        })
    }
}

/// Returns `~/.vox/sockets/`.
fn socket_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".vox").join("sockets"))
}

/// Returns the default socket path for the current process.
pub fn socket_path_for_pid(pid: u32) -> Result<PathBuf> {
    let dir = socket_dir()?;
    Ok(dir.join(format!("{pid}.diagnostics.socket")))
}

/// Ensure the socket directory exists.
pub fn ensure_socket_dir() -> Result<PathBuf> {
    let dir = socket_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create socket directory: {}", dir.display()))?;
    Ok(dir)
}

/// Discovered Vox instance with PID and socket path.
pub struct VoxInstance {
    /// Process ID of the running Vox instance.
    pub pid: u32,
    /// Path to the diagnostics socket.
    pub socket_path: PathBuf,
}

/// Discover all live Vox instances by scanning the socket directory.
///
/// Returns a list of instances with their PIDs and socket paths.
/// Cleans up stale sockets from crashed processes.
pub fn discover_instances() -> Result<Vec<VoxInstance>> {
    let socket_dir = socket_dir()?;
    let sockets = discover_sockets(&socket_dir)?;
    Ok(sockets
        .into_iter()
        .filter_map(|path| {
            let pid = extract_pid_from_socket(&path)?;
            Some(VoxInstance { pid, socket_path: path })
        })
        .collect())
}

/// Discover live diagnostics sockets, cleaning up stale ones.
fn discover_sockets(socket_dir: &Path) -> Result<Vec<PathBuf>> {
    if !socket_dir.exists() {
        return Ok(Vec::new());
    }

    let mut live = Vec::new();

    let entries = std::fs::read_dir(socket_dir)
        .with_context(|| format!("failed to read socket directory: {}", socket_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.ends_with(".diagnostics.socket") => {}
            _ => continue,
        };

        // Check if the PID is still alive
        if let Some(pid) = extract_pid_from_socket(&path) {
            if !is_pid_alive(pid) {
                // Stale socket — delete it
                let _ = std::fs::remove_file(&path);
                continue;
            }
        }

        // Try connecting to verify the socket is responsive
        match UnixStream::connect(&path) {
            Ok(_stream) => {
                // Connection succeeded — socket is live
                drop(_stream);
                live.push(path);
            }
            Err(_) => {
                // Can't connect — stale socket, clean it up
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    Ok(live)
}

/// Extract PID from a socket filename like `12345.diagnostics.socket`.
fn extract_pid_from_socket(path: &Path) -> Option<u32> {
    path.file_name()?
        .to_str()?
        .strip_suffix(".diagnostics.socket")?
        .parse()
        .ok()
}

/// Check if a process with the given PID is still running.
#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
            true
        }
        Err(_) => false,
    }
}

#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    // signal 0 checks process existence without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
