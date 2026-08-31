use super::{ScreenSize, TerminalFrame, TerminalId, TerminalStatus};

/// Commands the UI can issue to a terminal engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineCommand {
    Input {
        terminal: TerminalId,
        bytes: Vec<u8>,
    },
    Resize {
        terminal: TerminalId,
        size: ScreenSize,
    },
    Respawn {
        terminal: TerminalId,
    },
    Shutdown,
}

/// State changes emitted by a terminal engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineEvent {
    FrameReady(TerminalFrame),
    StatusChanged {
        terminal: TerminalId,
        status: TerminalStatus,
    },
}

/// The narrow UI-to-engine seam. Implementations retain state and emit changes
/// after each UI command.
pub trait TerminalEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent>;
    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame>;
    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus>;
}
