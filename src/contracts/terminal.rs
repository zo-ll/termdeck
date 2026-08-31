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
