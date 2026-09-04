//! Presentation layer for the master-and-preview-stack interface.
//!
//! This module renders one screen from owned application types plus the
//! interface's own [`DeckState`]. It has no event loop, no key decoding, and
//! no terminal backend of its own: the caller supplies a Ratatui [`Frame`], a
//! [`TerminalEngine`] to read from, and the state to render.
//!
//! The public surface is the renderer, its state, its key handling, the window
//! it scrolls the preview list through, and the frozen contracts only.
//! Reference fixtures and `FakeEngine` are test-only and never reach a release
//! build.

mod chrome;
mod deck;
use chrome::*;
#[cfg(test)]
mod fixture;
mod input;
mod state;

#[cfg(test)]
mod tests;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};

pub use input::{Input, Key, Reaction};
pub mod picker;
pub use picker::{
    Browse, Entry, EntryKind, FsBrowse, Hit, Instance, Listing, Open, Picker, PickerReaction,
    PickerState, Sheet, SheetHit, SheetState,
};
pub use state::{
    DEFAULT_MASTER_RATIO, DeckState, MAX_MASTER_RATIO, MIN_MASTER_RATIO, Modal, NOTIFY_WINDOW,
    Notifications, Notify, TOAST_WINDOW,
};

use crate::contracts::{
    CellContent, CellStyle, Cursor, Elapsed, NotifyKind, Project, Rgb, ScreenSize, TerminalEngine,
    TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
};

/// Accepted palette. Every value comes from the design export's palette board.
mod palette {
    use ratatui::style::Color;

    pub const CANVAS: Color = Color::Rgb(0x0e, 0x10, 0x13);
    pub const STATUS_BG: Color = Color::Rgb(0x12, 0x15, 0x1a);
    pub const IDLE_BORDER: Color = Color::Rgb(0x23, 0x28, 0x30);
    pub const ACCENT: Color = Color::Rgb(0x2d, 0xd4, 0xa7);
    pub const MASTER_FG: Color = Color::Rgb(0xd7, 0xdb, 0xe0);
    pub const PREVIEW_FG: Color = Color::Rgb(0x8a, 0x92, 0x9c);
    pub const HINT: Color = Color::Rgb(0x4d, 0x55, 0x5f);
    pub const SUCCESS: Color = Color::Rgb(0x98, 0xc3, 0x79);
    pub const WARNING: Color = Color::Rgb(0xd8, 0xa6, 0x57);
    pub const ERROR: Color = Color::Rgb(0xe0, 0x6c, 0x6c);
    /// Title separators. Present in the export, absent from its palette list.
    pub const SEPARATOR: Color = Color::Rgb(0x3a, 0x42, 0x4c);
    /// Muted text: exited panes, preview paths, the master command.
    pub const MUTED: Color = Color::Rgb(0x6d, 0x75, 0x80);
    /// A just-demoted preview: a lighter border, a one-shade-lighter
    /// background, and a brighter name than an ordinary preview. Export
    /// screen 02; absent from the palette list.
    pub const DEMOTED_BORDER: Color = Color::Rgb(0x2a, 0x30, 0x38);
    pub const DEMOTED_BG: Color = Color::Rgb(0x10, 0x13, 0x17);
    pub const DEMOTED_FG: Color = Color::Rgb(0xa8, 0xb0, 0xba);
    /// Narrow-mode pane-strip chip background. Export screen 04.
    pub const CHIP_BG: Color = Color::Rgb(0x18, 0x1c, 0x22);
    /// The interface behind an open modal. The supplement recedes it by
    /// dimming foreground only: pane content drops to the separator value and
    /// the key hints one step further, while the borders fall back to idle.
    pub const UNDER_FG: Color = SEPARATOR;
    pub const UNDER_HINT: Color = Color::Rgb(0x2a, 0x30, 0x38);
}

use palette::*;

/// Columns between the master and the preview stack.
const GUTTER: u16 = 2;
/// Rows in one preview, including its borders.
const PREVIEW_HEIGHT: u16 = 12;
/// Rows in one preview once the stack is narrower than [`WIDE_COLUMNS`].
const COMPACT_PREVIEW_HEIGHT: u16 = 9;
/// Rows in a collapsed preview: the export folds it to a single title row.
const COLLAPSED_HEIGHT: u16 = 1;
/// At or above this width the stack takes its share of `master_ratio`.
const WIDE_COLUMNS: u16 = 120;
/// Below this width the stack is not usable and the deck falls back to
/// master-only mode with the compact pane strip and status line.
const NARROW_COLUMNS: u16 = 100;
/// Between [`NARROW_COLUMNS`] and [`WIDE_COLUMNS`] the stack is fixed.
const COMPACT_STACK: u16 = 34;
/// Horizontal pane padding, inside the border.
const PADDING: u16 = 2;
/// Output within this window counts as recent activity.
const ACTIVE_WINDOW: Elapsed = Elapsed { millis: 30_000 };
/// Cells in the activity meter.
const METER_CELLS: u64 = 6;
/// Help overlay size: the supplement's 60 columns by 21 rows, grown a row at
/// a time for the collapse binding, the stack-paging keys, the split
/// divider's keys, the runtime-add sheet and the close binding.
const HELP_SIZE: (u16, u16) = (60, 26);
/// Quit confirmation size, from the supplement: 52 columns by 10 rows.
const QUIT_SIZE: (u16, u16) = (52, 10);
/// The notification toast (#97) borrows the quit confirmation's size language
/// and nothing else: it is drawn without dimming the interface and captures
/// no key, because a background completion must never interrupt typing.
const TOAST_SIZE: (u16, u16) = (52, 10);
/// How many notifications the toast lists before it starts counting them.
const TOAST_ROWS: usize = 4;
/// Column the help overlay's descriptions start at.
const HELP_KEYS: usize = 17;
/// The keys that page the preview list, as the stack footer states them.
const PAGE_KEYS: &str = "^g pgup/pgdn";
/// The status bar's runtime-add affordance. Clicking it opens the same sheet
/// `^g a` does.
const ADD_AFFORDANCE: &str = "+";
/// The per-pane close affordance (#84): the pointer's half of `^g x`.
///
/// A multiplication sign, not the `✕` [`status_glyph`] gives an exited pane:
/// the two never mean the same thing, so they are never the same glyph. They
/// are told apart three ways over — a different character, the hint colour
/// against the status dot's [`ERROR`], and the pane's right edge against the
/// title's left.
const CLOSE_AFFORDANCE: &str = "×";
/// The mark a pane asking for attention wears (#97): in the toast's stead on
/// a flashing strip, and in the censuses that keep hidden panes accounted for
/// once the toast has been dismissed.
const NOTIFY_MARK: &str = "!";

/// How a single pane is dressed. Every pane draws the same chrome; the kind
/// selects the colours and how much of the title the pane has room to say.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pane {
    /// Master beside a visible stack.
    Master,
    /// Master with the stack hidden by zoom.
    Zoomed,
    /// Master in narrow fallback: no stack, so no room for the full title.
    Compact,
    Preview,
    /// The preview that the most recent promotion demoted.
    Demoted,
    /// A preview whose terminal has asked for attention (#97): the demotion
    /// idiom's lifted background, with the warning border a held pane wears,
    /// because this one is asking rather than settling.
    Notify,
    DragMasterSource,
    DragMasterTarget,
    DragPreviewSource,
    DragPreviewTarget,
}

impl Pane {
    fn base(self) -> Self {
        match self {
            Self::DragMasterSource | Self::DragMasterTarget => Self::Master,
            Self::DragPreviewSource | Self::DragPreviewTarget => Self::Preview,
            pane => pane,
        }
    }

    fn dragging(self, target: bool) -> Self {
        match (self.master(), target) {
            (true, false) => Self::DragMasterSource,
            (true, true) => Self::DragMasterTarget,
            (false, false) => Self::DragPreviewSource,
            (false, true) => Self::DragPreviewTarget,
        }
    }

    /// Whether this pane holds the active terminal.
    fn master(self) -> bool {
        matches!(self.base(), Self::Master | Self::Zoomed | Self::Compact)
    }

    /// Whether the pane has room for the full title: complete path, state
    /// label, command, and the right-aligned slot.
    fn wide(self) -> bool {
        matches!(self.base(), Self::Master | Self::Zoomed)
    }
}

/// One drawn child of the preview stack, top to bottom.
///
/// Produced once per render by [`Deck::stack_layout`] and consumed by both the
/// renderer and the pointer hit test, so the two can never disagree about
/// where a preview sits — heights stopped being uniform once a preview could
/// be folded, and the list stopped starting at its first entry once it could
/// be scrolled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StackSlot {
    position: usize,
    rect: Rect,
    collapsed: bool,
}

/// Which part of the preview list the stack column is showing.
///
/// The column holds whole previews at their fixed heights, so a list longer
/// than the column is paged rather than scrolled by rows: the window states
/// where it starts, how many previews it drew, and how long the list is. Every
/// hit test and every indicator reads it, so they cannot disagree.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StackWindow {
    /// Index into [`DeckState::stack`] of the first preview drawn.
    pub offset: usize,
    /// How many previews the column drew.
    pub visible: usize,
    /// How many previews the list holds.
    pub total: usize,
    /// The furthest offset that still ends the window on the last preview.
    limit: usize,
}

impl StackWindow {
    /// The offset after scrolling `items` slots down the list, negative for
    /// up, clamped so the window never scrolls past the last preview.
    pub fn scrolled(&self, items: isize) -> usize {
        self.offset.saturating_add_signed(items).min(self.limit)
    }

    /// The offset after paging by whole windows, which is what the keyboard
    /// moves.
    pub fn paged(&self, pages: isize) -> usize {
        self.scrolled(pages.saturating_mul(self.visible.max(1) as isize))
    }

    /// Whether the list is longer than the window — the only state in which
    /// the column draws a scroll indicator at all.
    pub fn overflows(&self) -> bool {
        self.total > self.visible
    }

    /// Previews hidden above the window.
    fn above(&self) -> usize {
        self.offset
    }

    /// Previews hidden below it.
    fn below(&self) -> usize {
        self.total.saturating_sub(self.offset + self.visible)
    }
}

/// The chosen screen arrangement for one render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Layout {
    Stacked {
        stack: u16,
        preview: u16,
    },
    Zoom,
    /// No stacked previews: the master takes the full body, with no
    /// divider, gutter, or stack chrome. Geometrically zoom's twin, but
    /// semantically distinct — the status row and key hints read it as an
    /// ordinary deck (census, `+ add`, full keys), never as a zoom.
    Single,
    Narrow,
}

/// Everything the renderer needs that does not belong to the engine.
pub struct Deck<'a> {
    pub workspace: &'a str,
    pub projects: &'a [Project],
    /// Which terminal is master, how the rest stack, and whether zoom is on.
    pub state: &'a DeckState,
    /// What the terminals have asked for and has not been seen (#97). Read
    /// only: the renderer flashes, lists and marks, and changes nothing.
    pub notifies: &'a Notifications,
    /// Share of the width given to the master pane.
    pub master_ratio: f64,
    /// Wall clock used to age exit timestamps and the demotion highlight.
    pub now: Timestamp,
}
