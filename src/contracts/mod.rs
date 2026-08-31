//! Types shared by the terminal engine and the user interface.

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
    Elapsed, ProcessInfo, ScrollbackPosition, TerminalId, TerminalMetadata, TerminalStatus,
    Timestamp,
};
