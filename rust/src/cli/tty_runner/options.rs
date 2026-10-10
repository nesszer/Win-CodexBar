//! `TtyCommandOptions`: terminal size, timeouts, scripted input and stop markers.

use super::*;

/// Options for running TTY commands
#[derive(Debug, Clone)]
pub struct TtyCommandOptions {
    /// Terminal rows (default: 50)
    pub rows: u16,
    /// Terminal columns (default: 160)
    pub cols: u16,
    /// Overall timeout in seconds (default: 20)
    pub timeout_secs: f64,
    /// Idle timeout - stop if no output for this duration (optional)
    pub idle_timeout_secs: Option<f64>,
    /// Working directory
    pub working_directory: Option<PathBuf>,
    /// Extra arguments to pass to the command
    pub extra_args: Vec<String>,
    /// Initial delay before sending script (default: 0.4s)
    pub initial_delay_secs: f64,
    /// Delay between script characters (default: 0s)
    pub script_char_delay_secs: f64,
    /// Delay between script lines (default: 0s)
    pub script_line_delay_secs: f64,
    /// Send enter/return every N seconds (optional)
    pub send_enter_every_secs: Option<f64>,
    /// Map of substrings to keys to send when detected
    pub send_on_substrings: HashMap<String, String>,
    /// Screen-aware responder, consulted on all output since its last answer
    pub screen_responder: Option<ScreenResponder>,
    /// Stop early when a URL is detected
    pub stop_on_url: bool,
    /// Stop early when any of these substrings are detected
    pub stop_on_substrings: Vec<String>,
    /// Settle time after stopping (default: 0.25s)
    pub settle_after_stop_secs: f64,
    /// Re-send the script at these offsets (seconds since launch) while none
    /// of `script_done_substrings` has appeared in the output yet. Interactive
    /// CLIs such as Claude Code drop keystrokes that arrive before their input
    /// widget is mounted, and that readiness delay varies between machines.
    pub script_retry_delays_secs: Vec<f64>,
    /// Case-insensitive markers that show the script was accepted (stops retries).
    pub script_done_substrings: Vec<String>,
    /// Case-insensitive markers that show the script text was at least echoed
    /// into the input widget; a retry is skipped while one is visible so the
    /// same text is not typed twice into a half-processed line.
    pub script_echo_substrings: Vec<String>,
    /// Shorter idle timeout used once a done marker is visible: the answer
    /// is on screen, so only trailing output is awaited (optional).
    pub idle_timeout_after_done_secs: Option<f64>,
    /// Environment variables to set (`Debug` renders only the entry count)
    pub env: ProcessEnvironment<HashMap<String, String>>,
}

impl Default for TtyCommandOptions {
    fn default() -> Self {
        Self {
            rows: 50,
            cols: 160,
            timeout_secs: 20.0,
            idle_timeout_secs: None,
            working_directory: None,
            extra_args: Vec::new(),
            initial_delay_secs: 0.4,
            script_char_delay_secs: 0.0,
            script_line_delay_secs: 0.0,
            send_enter_every_secs: None,
            send_on_substrings: HashMap::new(),
            screen_responder: None,
            stop_on_url: false,
            stop_on_substrings: Vec::new(),
            settle_after_stop_secs: 0.25,
            script_retry_delays_secs: Vec::new(),
            script_done_substrings: Vec::new(),
            script_echo_substrings: Vec::new(),
            idle_timeout_after_done_secs: None,
            env: ProcessEnvironment::default(),
        }
    }
}

impl TtyCommandOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, secs: f64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_idle_timeout(mut self, secs: f64) -> Self {
        self.idle_timeout_secs = Some(secs);
        self
    }

    pub fn with_initial_delay(mut self, secs: f64) -> Self {
        self.initial_delay_secs = secs;
        self
    }

    pub fn with_script_char_delay(mut self, secs: f64) -> Self {
        self.script_char_delay_secs = secs;
        self
    }

    pub fn with_script_line_delay(mut self, secs: f64) -> Self {
        self.script_line_delay_secs = secs;
        self
    }

    pub fn with_working_directory(mut self, dir: PathBuf) -> Self {
        self.working_directory = Some(dir);
        self
    }

    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    pub fn with_stop_on_url(mut self, stop: bool) -> Self {
        self.stop_on_url = stop;
        self
    }

    pub fn with_stop_on_substring(mut self, substring: impl Into<String>) -> Self {
        self.stop_on_substrings.push(substring.into());
        self
    }

    pub fn with_script_retries(
        mut self,
        delays_secs: Vec<f64>,
        done_substrings: Vec<String>,
        echo_substrings: Vec<String>,
    ) -> Self {
        self.script_retry_delays_secs = delays_secs;
        self.script_done_substrings = done_substrings
            .into_iter()
            .map(|marker| marker.to_lowercase())
            .collect();
        self.script_echo_substrings = echo_substrings
            .into_iter()
            .map(|marker| marker.to_lowercase())
            .collect();
        self
    }

    pub fn with_idle_timeout_after_done(mut self, secs: f64) -> Self {
        self.idle_timeout_after_done_secs = Some(secs);
        self
    }

    pub fn with_screen_responder(mut self, responder: ScreenResponder) -> Self {
        self.screen_responder = Some(responder);
        self
    }

    pub fn with_send_on_substring(
        mut self,
        trigger: impl Into<String>,
        keys: impl Into<String>,
    ) -> Self {
        self.send_on_substrings.insert(trigger.into(), keys.into());
        self
    }
}
