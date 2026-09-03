use std::fmt;

/// Stable identity for a configured terminal.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TerminalId(String);

impl TerminalId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Process lifecycle state, independent of the PTY implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalStatus {
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { message: String },
}

/// A wall-clock time represented as milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp {
    pub unix_millis: u64,
}

/// A non-negative elapsed duration in milliseconds.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Elapsed {
    pub millis: u64,
}

/// Information available while a terminal process is alive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub uptime: Elapsed,
}

/// The selected engine-owned scrollback viewport.
///
/// `lines_above` and `lines_below` count retained lines outside the current
/// frame. The UI displays the frame it receives and asks the engine to move
/// this viewport; it never indexes or mutates a terminal history buffer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollbackPosition {
    pub lines_above: u32,
    pub lines_below: u32,
}

/// Chrome data that changes independently of terminal pixels.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalMetadata {
    /// Cumulative output bytes written by the terminal process.
    pub bytes_written: u64,
    /// Elapsed time since the most recent output, or `None` before any output.
    pub output_idle: Option<Elapsed>,
    /// Timestamp of the most recent process exit. The exit code lives only in
    /// [`TerminalStatus::Exited`].
    pub last_exit_at: Option<Timestamp>,
    pub restarted_at: Option<Timestamp>,
    pub process: Option<ProcessInfo>,
    pub scrollback: ScrollbackPosition,
    /// Whether the terminal shows its alternate screen: a full-screen app
    /// (vim, less, claude) owns the grid, so termdeck scrollback is inert
    /// there and the wheel belongs to the app (#74).
    pub alt_screen: bool,
    /// Whether the app enabled mouse reporting: with it the app wants the
    /// wheel as SGR mouse reports, without it as cursor keys (#74).
    pub mouse_reporting: bool,
}
