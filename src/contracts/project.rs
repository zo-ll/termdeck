use std::path::PathBuf;

use super::TerminalId;

/// A configured project and the terminal process it will own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    pub terminal: TerminalId,
    pub path: PathBuf,
    pub command: Vec<String>,
}
