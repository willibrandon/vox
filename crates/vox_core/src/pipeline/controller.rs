//! Hotkey-to-command translation for pipeline activation modes.
//!
//! The `PipelineController` translates hotkey press/release events into
//! pipeline start/stop commands based on the active `ActivationMode`.
//! Communicates with `Pipeline::run()` exclusively via an mpsc command
//! channel — never holds `&mut Pipeline`.

use std::time::Instant;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::state::PipelineCommand;

/// Recording trigger behavior, persisted in user settings.
///
/// Determines how hotkey events map to pipeline start/stop. The default is
/// HoldToTalk, which is the most intuitive for first-time users.
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
    fn default() -> Self {
        Self::HoldToTalk
    }
}

impl ActivationMode {
    /// Convert to string for SQLite storage.
    fn as_str(&self) -> &'static str {
        match self {
            Self::HoldToTalk => "hold_to_talk",
            Self::Toggle => "toggle",
            Self::HandsFree => "hands_free",
        }
    }
}

/// Translates hotkey events into pipeline commands based on activation mode.
///
/// Communicates with Pipeline exclusively via the command channel — never
/// holds `&mut Pipeline`. The corresponding `mpsc::Receiver` is passed to
/// `Pipeline::new()`.
pub struct PipelineController {
    command_tx: mpsc::Sender<PipelineCommand>,
    mode: ActivationMode,
    is_active: bool,
    last_press_time: Option<Instant>,
}

impl PipelineController {
    /// Create a controller with a command channel sender.
    ///
    /// The corresponding receiver is passed to Pipeline::new().
    pub fn new(command_tx: mpsc::Sender<PipelineCommand>) -> Self {
        Self {
            command_tx,
            mode: ActivationMode::default(),
            is_active: false,
            last_press_time: None,
        }
    }

    /// Handle hotkey press event.
    ///
    /// - HoldToTalk: marks active (caller responsible for starting pipeline)
    /// - Toggle: if active sends Stop, else marks active (caller starts pipeline)
    /// - HandsFree: checks for double-press; if detected marks active, if already
    ///   active and single press sends Stop
    pub fn on_hotkey_press(&mut self) {
        match self.mode {
            ActivationMode::HoldToTalk => {
                self.is_active = true;
            }
            ActivationMode::Toggle => {
                if self.is_active {
                    self.send_stop();
                    self.is_active = false;
                } else {
                    self.is_active = true;
                }
            }
            ActivationMode::HandsFree => {
                if self.is_double_press() {
                    self.is_active = true;
                    self.last_press_time = None;
                } else if self.is_active {
                    self.send_stop();
                    self.is_active = false;
                } else {
                    self.last_press_time = Some(Instant::now());
                }
            }
        }
    }

    /// Handle hotkey release event.
    ///
    /// - HoldToTalk: sends Stop command if active
    /// - Toggle/HandsFree: no-op
    pub fn on_hotkey_release(&mut self) {
        match self.mode {
            ActivationMode::HoldToTalk => {
                if self.is_active {
                    self.send_stop();
                    self.is_active = false;
                }
            }
            ActivationMode::Toggle | ActivationMode::HandsFree => {}
        }
    }

    /// Force-stop by sending Stop command regardless of activation mode.
    pub fn force_stop(&mut self) {
        self.send_stop();
        self.is_active = false;
    }

    /// Whether dictation is currently active.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Current activation mode.
    pub fn mode(&self) -> ActivationMode {
        self.mode.clone()
    }

    /// Set activation mode. Sends Stop first if dictation is active.
    ///
    /// Persists the choice to the SQLite settings table. The current segment
    /// completes fully per FR-018 before the mode change takes effect.
    pub fn set_mode(&mut self, mode: ActivationMode, db_path: &std::path::Path) -> Result<()> {
        if self.is_active {
            self.force_stop();
        }
        self.mode = mode.clone();

        // Persist to SQLite settings table
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open settings database at {}", db_path.display()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        )
        .context("failed to create settings table")?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('activation_mode', ?1)",
            rusqlite::params![mode.as_str()],
        )
        .context("failed to persist activation mode")?;

        Ok(())
    }

    /// Check if the current press is a double-press (within 300ms exclusive).
    fn is_double_press(&mut self) -> bool {
        if let Some(last) = self.last_press_time {
            let elapsed = last.elapsed();
            if elapsed.as_millis() < 300 {
                return true;
            }
        }
        false
    }

    /// Send a Stop command via the channel.
    fn send_stop(&self) {
        // try_send is fine here — command channel has capacity 8 and commands
        // are consumed near-instantly. If it fails, the pipeline is likely
        // already shutting down.
        let _ = self.command_tx.try_send(PipelineCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_controller() -> (PipelineController, mpsc::Receiver<PipelineCommand>) {
        let (tx, rx) = mpsc::channel(8);
        (PipelineController::new(tx), rx)
    }

    #[test]
    fn test_hold_to_talk_press_release() {
        let (mut ctrl, mut rx) = make_controller();
        ctrl.on_hotkey_press();
        assert!(ctrl.is_active());
        ctrl.on_hotkey_release();
        assert!(!ctrl.is_active());
        // Should have sent Stop on release
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_toggle_mode() {
        let (mut ctrl, mut rx) = make_controller();
        ctrl.mode = ActivationMode::Toggle;

        ctrl.on_hotkey_press();
        assert!(ctrl.is_active());
        // No Stop sent yet
        assert!(rx.try_recv().is_err());

        ctrl.on_hotkey_press();
        assert!(!ctrl.is_active());
        // Stop sent on second press
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_toggle_release_is_noop() {
        let (mut ctrl, mut rx) = make_controller();
        ctrl.mode = ActivationMode::Toggle;

        ctrl.on_hotkey_press();
        ctrl.on_hotkey_release();
        // Release shouldn't send Stop in Toggle mode
        assert!(ctrl.is_active());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_double_press_detection_within_window() {
        let (mut ctrl, _rx) = make_controller();
        ctrl.mode = ActivationMode::HandsFree;

        // First press — records time
        ctrl.on_hotkey_press();
        assert!(!ctrl.is_active()); // Not active after single press

        // Second press immediately — double press
        ctrl.on_hotkey_press();
        assert!(ctrl.is_active());
    }

    #[test]
    fn test_double_press_detection_outside_window() {
        let (mut ctrl, _rx) = make_controller();
        ctrl.mode = ActivationMode::HandsFree;

        ctrl.last_press_time = Some(Instant::now() - std::time::Duration::from_millis(300));
        ctrl.on_hotkey_press();
        // 300ms exactly is single press (exclusive boundary)
        assert!(!ctrl.is_active());
    }

    #[test]
    fn test_double_press_detection_well_outside_window() {
        let (mut ctrl, _rx) = make_controller();
        ctrl.mode = ActivationMode::HandsFree;

        ctrl.last_press_time = Some(Instant::now() - std::time::Duration::from_millis(301));
        ctrl.on_hotkey_press();
        assert!(!ctrl.is_active());
    }

    #[test]
    fn test_force_stop() {
        let (mut ctrl, mut rx) = make_controller();
        ctrl.is_active = true;
        ctrl.force_stop();
        assert!(!ctrl.is_active());
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_set_mode_persistence() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("settings.db");
        let (mut ctrl, _rx) = make_controller();

        ctrl.set_mode(ActivationMode::Toggle, &db_path)
            .expect("set_mode");
        assert_eq!(ctrl.mode(), ActivationMode::Toggle);

        // Verify persisted
        let conn = Connection::open(&db_path).expect("open");
        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'activation_mode'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(value, "toggle");
    }

    #[test]
    fn test_set_mode_stops_active_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("settings.db");
        let (mut ctrl, mut rx) = make_controller();

        ctrl.is_active = true;
        ctrl.set_mode(ActivationMode::HandsFree, &db_path)
            .expect("set_mode");

        assert!(!ctrl.is_active());
        // Should have sent Stop
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_hands_free_single_press_stops() {
        let (mut ctrl, mut rx) = make_controller();
        ctrl.mode = ActivationMode::HandsFree;
        ctrl.is_active = true;

        ctrl.on_hotkey_press();
        assert!(!ctrl.is_active());
        assert!(rx.try_recv().is_ok());
    }
}
