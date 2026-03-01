//! Diagnostics listener accepting UDS connections for remote inspection.
//!
//! Provides [`DiagnosticsListener`] which binds a Unix Domain Socket and
//! accepts connections from `vox-tool` and `vox-mcp`. Each connection gets
//! a handler thread that processes JSON-RPC requests against [`VoxState`].
//!
//! Also defines [`DiagnosticsCommand`] for dispatching side-effect operations
//! (recording, screenshots) from handler threads to the GPUI foreground thread.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use vox_diag::net::uds::{UnixListener, UnixStream};
use vox_diag::protocol::{self, Response};

use crate::state::VoxState;

/// Maximum concurrent UDS connections.
const MAX_CONNECTIONS: u32 = 4;

/// Commands dispatched from diagnostics handler threads to the GPUI thread.
///
/// Each variant carries a oneshot reply channel so the handler thread can
/// block on the result before sending the response back to the UDS client.
pub enum DiagnosticsCommand {
    /// Start a recording session. Fails if already recording.
    StartRecording {
        /// Oneshot reply: Ok(()) on success, Err on failure.
        reply: std::sync::mpsc::Sender<Result<()>>,
    },
    /// Stop an active recording session. Fails if not recording.
    StopRecording {
        /// Oneshot reply: Ok(()) on success, Err on failure.
        reply: std::sync::mpsc::Sender<Result<()>>,
    },
    /// Capture a screenshot of the specified window.
    CaptureScreenshot {
        /// Which window to capture ("overlay" or "settings").
        window: String,
        /// Oneshot reply: Ok(png_bytes) on success, Err on failure.
        reply: std::sync::mpsc::Sender<Result<Vec<u8>>>,
    },
    /// Quit the application gracefully.
    Quit {
        /// Oneshot reply: Ok(()) on acknowledgement.
        reply: std::sync::mpsc::Sender<Result<()>>,
    },
}

/// Diagnostics listener that accepts UDS connections on a dedicated thread.
///
/// Binds a Unix Domain Socket at `~/.vox/sockets/{pid}.diagnostics.socket`
/// and spawns handler threads for each connection. Bounded to
/// [`MAX_CONNECTIONS`] concurrent connections.
pub struct DiagnosticsListener {
    /// Path to the bound socket file (deleted on shutdown).
    socket_path: PathBuf,
    /// Atomic flag signaling the accept loop to exit.
    shutdown: Arc<AtomicBool>,
    /// Join handle for the accept loop thread.
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl DiagnosticsListener {
    /// Start the diagnostics listener on a background thread.
    ///
    /// Creates the socket directory, cleans up stale sockets from crashed
    /// instances, binds the listener socket, and spawns the accept loop.
    pub fn start(state: VoxState, socket_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(socket_dir)?;
        cleanup_stale_sockets(socket_dir);

        let pid = std::process::id();
        let socket_path = socket_dir.join(format!("{pid}.diagnostics.socket"));

        // Remove leftover socket from a previous crash of this same PID
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;
        listener.set_nonblocking(true)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let active_connections = Arc::new(AtomicU32::new(0));

        let handle = {
            let shutdown = Arc::clone(&shutdown);
            let active_connections = Arc::clone(&active_connections);
            let state = state.clone();

            std::thread::Builder::new()
                .name("diagnostics-listener".into())
                .spawn(move || {
                    accept_loop(listener, state, shutdown, active_connections);
                })?
        };

        Ok(Self {
            socket_path,
            shutdown,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Shut down the listener and clean up the socket file.
    ///
    /// Idempotent — safe to call multiple times. Signals the accept loop
    /// to exit, joins the thread, and deletes the socket file.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);

        if let Ok(mut guard) = self.handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }

        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for DiagnosticsListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Accept loop running on a dedicated thread.
///
/// Uses nonblocking accept with 100ms sleep intervals so the loop
/// can be interrupted by the shutdown flag within ~100ms.
fn accept_loop(
    listener: UnixListener,
    state: VoxState,
    shutdown: Arc<AtomicBool>,
    active_connections: Arc<AtomicU32>,
) {
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let current = active_connections.load(Ordering::Acquire);
                if current >= MAX_CONNECTIONS {
                    reject_connection(stream);
                    continue;
                }

                active_connections.fetch_add(1, Ordering::AcqRel);
                let state = state.clone();
                let active = Arc::clone(&active_connections);

                std::thread::Builder::new()
                    .name("diagnostics-handler".into())
                    .spawn(move || {
                        handle_connection(stream, state);
                        active.fetch_sub(1, Ordering::AcqRel);
                    })
                    .ok();
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                tracing::warn!(%err, "diagnostics accept error");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Send a connection-limit error response and close the stream.
fn reject_connection(mut stream: UnixStream) {
    let response = Response::error(
        0,
        protocol::error_code::CONNECTION_LIMIT,
        String::from("connection limit reached"),
    );
    let _ = write_response(&mut stream, &response);
}

/// Handle a single UDS connection: read JSON lines, dispatch, write responses.
///
/// Most methods follow request/response. The "subscribe" method transitions
/// the connection into event-streaming mode — the handler takes ownership
/// of reader and writer and pushes events until the client disconnects.
///
/// Uses raw byte-at-a-time reads for the normal request/response loop (no
/// `BufReader`). `try_clone` is only used when entering subscribe mode, which
/// requires concurrent read + write on separate threads.
fn handle_connection(mut stream: UnixStream, state: VoxState) {
    loop {
        let line = match read_line_raw(&mut stream) {
            Ok(line) => line,
            Err(_) => break, // EOF or I/O error — client disconnected
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<vox_diag::protocol::Request>(trimmed) {
            Ok(req) => req,
            Err(err) => {
                let response = Response::error(
                    0,
                    protocol::error_code::INVALID_REQUEST,
                    format!("invalid JSON: {err}"),
                );
                if write_response(&mut stream, &response).is_err() {
                    break;
                }
                continue;
            }
        };

        if request.method == "subscribe" {
            // Subscribe needs concurrent read + write — split via try_clone.
            let read_stream = match stream.try_clone() {
                Ok(cloned) => cloned,
                Err(err) => {
                    tracing::warn!(%err, "failed to clone stream for subscribe");
                    return;
                }
            };
            let reader = BufReader::new(read_stream);
            let writer = BufWriter::new(stream);
            crate::diagnostics::handlers::handle_subscribe_streaming(
                &request, &state, reader, writer,
            );
            return; // connection done after subscribe ends
        }

        let response = dispatch(&request, &state);
        if write_response(&mut stream, &response).is_err() {
            break;
        }
    }
}

/// Read one line from the stream byte-by-byte until `\n`. No buffering.
fn read_line_raw(stream: &mut UnixStream) -> std::io::Result<String> {
    let mut buf = [0u8; 1];
    let mut line = Vec::new();
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                if line.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "connection closed",
                    ));
                }
                break;
            }
            Ok(_) => {
                if buf[0] == b'\n' {
                    break;
                }
                line.push(buf[0]);
            }
            Err(e) => return Err(e),
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Serialize and write a JSON response line. Returns Err on I/O failure.
fn write_response(writer: &mut UnixStream, response: &Response) -> std::io::Result<()> {
    let mut payload = serde_json::to_vec(response)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    payload.push(b'\n');
    writer.write_all(&payload)?;
    writer.flush()
}

/// Dispatch a parsed request to the appropriate handler.
///
/// Returns `Response::error(UNKNOWN_METHOD)` for unrecognized methods.
/// Handler implementations are added in Phase 3+ (T016-T031).
fn dispatch(request: &vox_diag::protocol::Request, state: &VoxState) -> Response {
    crate::diagnostics::handlers::dispatch(request, state)
}

/// Remove stale socket files from previous crashed instances.
///
/// For each `*.diagnostics.socket` file in the directory, extracts the PID
/// from the filename. If the PID is no longer running, deletes the socket.
fn cleanup_stale_sockets(socket_dir: &Path) {
    let entries = match std::fs::read_dir(socket_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if let Some(pid_str) = name.strip_suffix(".diagnostics.socket") {
            if let Ok(pid) = pid_str.parse::<u32>() {
                // Skip our own PID — we'll handle our own socket file separately
                if pid == std::process::id() {
                    continue;
                }
                if !is_pid_running(pid) {
                    let _ = std::fs::remove_file(entry.path());
                    tracing::info!(pid, "removed stale diagnostics socket");
                }
            }
        }
    }
}

/// Returns the default diagnostics socket directory path (`~/.vox/sockets/`).
///
/// The socket directory is under the user's home directory, not the
/// application data directory, so that `vox-tool` and `vox-mcp` can
/// discover it without knowing the platform data directory layout.
pub fn default_socket_dir() -> std::io::Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available"))?;
    Ok(home.join(".vox").join("sockets"))
}

/// Check whether a process with the given PID is currently running.
#[cfg(target_os = "windows")]
fn is_pid_running(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

/// Check whether a process with the given PID is currently running.
#[cfg(not(target_os = "windows"))]
fn is_pid_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
