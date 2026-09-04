use std::path::PathBuf;

use super::TerminalId;

/// A configured project and the terminal process it will own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    pub terminal: TerminalId,
    pub path: PathBuf,
    pub command: Vec<String>,
    /// Whether `command` came from the workspace default and may receive the
    /// supported interactive-shell hook. Explicit pane commands stay literal.
    pub shell_hook: bool,
}
