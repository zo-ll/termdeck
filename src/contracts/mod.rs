//! Types shared by the terminal engine and the user interface.

/// The default number of retained terminal history lines.
pub const DEFAULT_SCROLLBACK: usize = 10_000;

mod command;
mod engine;
mod project;
mod screen;
mod terminal;

pub use command::{ActionCommand, InputCommand, UserCommand};
pub use engine::{EngineCommand, EngineEvent, ScrollCommand, TerminalEngine};
pub use project::Project;
pub use screen::{
    CellContent, CellStyle, CellWidth, Cursor, Rgb, ScreenCell, ScreenSize, TerminalFrame,
};
pub use terminal::{
    Elapsed, NotifyKind, ProcessInfo, ScrollbackPosition, TerminalId, TerminalMetadata,
    TerminalStatus, Timestamp,
};
