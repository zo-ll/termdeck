use super::{NotifyKind, ScreenSize, TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus};

/// An engine-owned scrollback movement. `Up` moves toward older output, and
/// `Down` moves toward newer output; `Bottom` returns to live output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollCommand {
    Up(u16),
    Down(u16),
    Bottom,
}

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
    Scroll {
        terminal: TerminalId,
        command: ScrollCommand,
    },
    Respawn {
        terminal: TerminalId,
    },
    Shutdown,
}

/// State changes emitted by a terminal engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineEvent {
    /// A redraw signal. At the live tail it may carry cells identical to the
    /// previous frame, so consumers must not infer a content change from it.
    FrameReady(TerminalFrame),
    StatusChanged {
        terminal: TerminalId,
        status: TerminalStatus,
    },
    MetadataChanged {
        terminal: TerminalId,
        metadata: TerminalMetadata,
    },
    /// A terminal asked for the user's attention: a BEL out of the PTY, or an
    /// explicit `ctl notify` (#97). Additive, so an engine that raises none
    /// and a consumer that ignores them both stay correct.
    Notify {
        terminal: TerminalId,
        kind: NotifyKind,
    },
    /// A terminal's bounded input queue had no room, so the input was refused
    /// and nothing reached the PTY (#118). Returned from `dispatch`, not
    /// from `drain_events`: the frame pump only flushes what is already
    /// queued, so it never refuses. Additive like `Notify`: engines that
    /// never saturate never emit it, and consumers may ignore it.
    InputDropped { terminal: TerminalId },
    /// A terminal accepted input into its bounded queue, but the PTY could
    /// not flush every byte immediately. Returned from `dispatch` so ctl can
    /// distinguish queued input from input that reached the PTY at once.
    /// Consumers that do not need that acknowledgement may ignore it.
    InputQueued { terminal: TerminalId },
}

/// The narrow UI-to-engine seam. Implementations retain state and emit changes
/// after each UI command or while draining asynchronous engine work.
pub trait TerminalEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent>;
    /// Drains output and lifecycle events produced without a UI command.
    fn drain_events(&mut self) -> Vec<EngineEvent>;
    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame>;
    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus>;
    fn metadata(&self, terminal: &TerminalId) -> Option<&TerminalMetadata>;
    /// Returns up to `max` retained scrollback lines, oldest to newest.
    fn history_lines(&self, _terminal: &TerminalId, _max: usize) -> Option<Vec<String>> {
        None
    }
    /// Returns up to `max` lines from the terminal's active grid, oldest to
    /// newest. This is the main grid normally and the alternate grid while a
    /// full-screen application owns the pane.
    fn active_screen_lines(&self, _terminal: &TerminalId, _max: usize) -> Option<Vec<String>> {
        None
    }
}
