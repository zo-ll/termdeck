use super::TerminalId;

/// Input sent to the active terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputCommand {
    Bytes(Vec<u8>),
    Paste(String),
}

/// Commands handled by the outer Termdeck interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionCommand {
    SelectPrevious,
    SelectNext,
    SelectPosition(usize),
    Promote(TerminalId),
    ToggleZoom,
    ToggleScrollback,
    RespawnActive,
    ShowHelp,
    RequestQuit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserCommand {
    Input(InputCommand),
    Action(ActionCommand),
}
