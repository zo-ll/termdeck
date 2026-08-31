//! Types shared by the terminal engine and the user interface.

mod command;
mod engine;
mod project;
mod screen;
mod terminal;

pub use command::{ActionCommand, InputCommand, UserCommand};
pub use engine::{EngineCommand, EngineEvent, TerminalEngine};
pub use project::Project;
pub use screen::{CellStyle, Cursor, Rgb, ScreenCell, ScreenSize, TerminalFrame};
pub use terminal::{TerminalId, TerminalStatus};
