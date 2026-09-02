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

#[cfg(test)]
mod fixture;
mod input;
mod state;

use std::path::Path;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};

pub use input::{Input, Key, Reaction};
pub use state::{DEFAULT_MASTER_RATIO, DeckState, MAX_MASTER_RATIO, MIN_MASTER_RATIO, Modal};

use crate::contracts::{
    CellContent, CellStyle, Cursor, Elapsed, Project, Rgb, TerminalEngine, TerminalFrame,
    TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
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
/// Help overlay size: the supplement's 60 columns by 21 rows, one row taller
/// for the collapse binding, one more for the stack-paging keys and one more
/// for the split divider's keys.
const HELP_SIZE: (u16, u16) = (60, 24);
/// Quit confirmation size, from the supplement: 52 columns by 10 rows.
const QUIT_SIZE: (u16, u16) = (52, 10);
/// Column the help overlay's descriptions start at.
const HELP_KEYS: usize = 17;
/// The keys that page the preview list, as the stack footer states them.
const PAGE_KEYS: &str = "^g pgup/pgdn";

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
    Stacked { stack: u16, preview: u16 },
    Zoom,
    Narrow,
}

/// Everything the renderer needs that does not belong to the engine.
pub struct Deck<'a> {
    pub workspace: &'a str,
    pub projects: &'a [Project],
    /// Which terminal is master, how the rest stack, and whether zoom is on.
    pub state: &'a DeckState,
    /// Home directory used to abbreviate project paths.
    pub home: Option<&'a Path>,
    /// Share of the width given to the master pane.
    pub master_ratio: f64,
    /// Wall clock used to age exit timestamps and the demotion highlight.
    pub now: Timestamp,
}

impl Deck<'_> {
    /// Draws the whole screen: master, preview stack, key hints, status row.
    pub fn render(&self, engine: &dyn TerminalEngine, frame: &mut Frame) {
        let area = frame.area();
        Block::new()
            .style(Style::new().bg(CANVAS))
            .render(area, frame.buffer_mut());
        if area.width < GUTTER + 4 || area.height < 4 {
            return;
        }

        // Rows: body, one blank row, then the status row.
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let layout = self.layout(body);
        // The drawn panes, so an open modal knows which cells are pane chrome
        // and which are the key hints between them.
        let panes = match layout {
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                self.draw_active(
                    engine,
                    frame.buffer_mut(),
                    master,
                    self.master_pane(Pane::Master),
                );
                let mut panes = vec![master];
                panes.extend(self.stack(
                    engine,
                    frame.buffer_mut(),
                    Rect {
                        x: master.width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                ));
                if adjustable(body) {
                    self.divider(frame.buffer_mut(), body, master.x + master.width);
                }
                panes
            }
            Layout::Zoom => {
                self.draw_active(engine, frame.buffer_mut(), body, Pane::Zoomed);
                vec![body]
            }
            Layout::Narrow => vec![self.narrow(engine, frame.buffer_mut(), body)],
        };
        self.status_row(
            engine,
            frame.buffer_mut(),
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
            layout,
        );
        // A modal takes focus: the interface recedes behind it and the master
        // gives up both its accent border and its cursor.
        match self.state.modal() {
            Some(modal) => {
                dim(frame.buffer_mut(), body, &panes);
                self.modal(frame.buffer_mut(), area, modal);
            }
            None => self.place_cursor(engine, frame, panes[0]),
        }
    }

    /// Returns the terminal in the pane under `pointer`, excluding chrome
    /// outside the pane rectangles.
    pub fn terminal_at(&self, area: Rect, pointer: Position) -> Option<&TerminalId> {
        self.position_at(area, pointer)
            .and_then(|position| self.projects.get(position))
            .map(|project| &project.terminal)
    }

    /// Returns the configured position in the pane under `pointer`.
    ///
    /// A collapsed preview still answers here, so promoting or swapping it by
    /// pointer keeps working; it is the caller's business to notice that a
    /// folded pane has no viewport to scroll.
    pub fn position_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let active = || {
            self.state
                .active()
                .filter(|position| self.projects.get(*position).is_some())
        };
        match self.layout(body) {
            Layout::Zoom if body.contains(pointer) => active(),
            Layout::Narrow => {
                let master = Rect {
                    y: body.y + 2,
                    height: body.height.saturating_sub(2),
                    ..body
                };
                master.contains(pointer).then(active).flatten()
            }
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                if master.contains(pointer) {
                    return active();
                }
                self.stack_layout(
                    Rect {
                        x: master.width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .into_iter()
                .find(|slot| slot.rect.contains(pointer))
                .map(|slot| slot.position)
            }
            _ => None,
        }
    }

    /// The configured position whose disclosure marker sits under `pointer`.
    ///
    /// The marker owns two cells at the head of a stack pane's title: the
    /// export draws `▾ ` inside an open preview's top border and `▸ ` at the
    /// head of a collapsed strip, one column further left because the strip
    /// has no border to inset past.
    ///
    /// Every stack pane carries a marker, folded or not, so the cells are
    /// live from the first frame — that is the affordance a fresh run has to
    /// offer, and it takes precedence over drag and double-click there.
    pub fn marker_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let Layout::Stacked { stack, preview } = self.layout(body) else {
            return None;
        };
        let master = body.width - GUTTER - stack;
        self.stack_layout(
            Rect {
                x: master + GUTTER,
                width: stack,
                ..body
            },
            preview,
        )
        .into_iter()
        .find(|slot| {
            // An open pane insets its title past the border; a strip does not.
            let head = slot.rect.x + if slot.collapsed { PADDING } else { PADDING + 1 };
            pointer.y == slot.rect.y && (pointer.x == head || pointer.x == head + 1)
        })
        .map(|slot| slot.position)
    }

    /// The part of the preview list the stack column is currently showing.
    ///
    /// The caller pages the list through this: the window knows how far it can
    /// scroll, which only the rendered geometry can say. A hidden stack has no
    /// window, and its offset never moves.
    pub fn stack_window(&self, area: Rect) -> StackWindow {
        if area.width < GUTTER + 4 || area.height < 4 {
            return StackWindow::default();
        }
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let Layout::Stacked { preview, .. } = self.layout(body) else {
            return StackWindow::default();
        };
        self.stack_window_of(&self.stack_items(), body.height.saturating_sub(1), preview)
    }

    /// Whether `pointer` sits on the stack column's own chrome rather than on
    /// a preview: the gutter beside it, the blank rows between previews, the
    /// empty column below them, and the footer row.
    ///
    /// That is where the wheel pages the list. Over a preview the wheel still
    /// belongs to that preview's viewport, so the two gestures never fight.
    pub fn stack_scroll_at(&self, area: Rect, pointer: Position) -> bool {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return false;
        }
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let Layout::Stacked { stack, preview } = self.layout(body) else {
            return false;
        };
        let column = Rect {
            x: body.width - GUTTER - stack,
            width: GUTTER + stack,
            ..body
        };
        column.contains(pointer)
            && !self
                .stack_layout(
                    Rect {
                        x: body.width - stack,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .iter()
                .any(|slot| slot.rect.contains(pointer))
    }

    /// The column the split divider is drawn in, while the split can move.
    ///
    /// It is the gutter column beside the master, so the divider takes no
    /// columns from either pane and leaves the column beside the stack to the
    /// scroll track (#34b). Below [`WIDE_COLUMNS`] the export fixes the stack
    /// width, so there is nothing to move and there is no divider.
    fn divider_of(&self, area: Rect) -> Option<(Rect, u16)> {
        if area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = Rect {
            height: area.height - 2,
            ..area
        };
        let Layout::Stacked { stack, .. } = self.layout(body) else {
            return None;
        };
        adjustable(body).then(|| (body, body.x + body.width - GUTTER - stack))
    }

    /// Whether `pointer` is on the divider, and so starts a resize rather than
    /// a pane drag. The divider sits in the gutter, which belongs to no pane,
    /// so the two gestures never contend for the same cell.
    pub fn divider_at(&self, area: Rect, pointer: Position) -> bool {
        self.divider_of(area)
            .is_some_and(|(body, column)| pointer.x == column && body.contains(pointer))
    }

    /// The split that puts the divider under `column`: the inverse of the
    /// layout, so dragging to a column and releasing leaves the divider under
    /// the pointer.
    ///
    /// The master keeps every column left of the divider, the gutter takes
    /// the next two and the stack takes the rest, so the master's share is
    /// `column + GUTTER`. The caller clamps it — [`DeckState`] owns the range.
    pub fn ratio_at(&self, area: Rect, column: u16) -> Option<f64> {
        let (body, _) = self.divider_of(area)?;
        let master = column.saturating_sub(body.x).min(body.width);
        Some(f64::from(master + GUTTER) / f64::from(body.width))
    }

    /// A draggable pane must have a visible master-and-stack counterpart.
    pub fn swap_position_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        let body = Rect {
            height: area.height.saturating_sub(2),
            ..area
        };
        matches!(self.layout(body), Layout::Stacked { .. })
            .then(|| self.position_at(area, pointer))
            .flatten()
    }

    fn master_pane(&self, pane: Pane) -> Pane {
        match self.state.active() {
            Some(position) if self.state.dragged() == Some(position) => pane.dragging(false),
            Some(position) if self.state.drag_target() == Some(position) => pane.dragging(true),
            _ => pane,
        }
    }

    /// Narrow fallback wins over zoom: below a usable preview width the stack
    /// is already hidden, so zoom has nothing left to hide.
    fn layout(&self, body: Rect) -> Layout {
        let preview = if body.width >= WIDE_COLUMNS {
            PREVIEW_HEIGHT
        } else {
            COMPACT_PREVIEW_HEIGHT
        };
        // The stack needs one whole preview plus the hint row below it.
        if body.width < NARROW_COLUMNS || body.height < preview + 2 {
            return Layout::Narrow;
        }
        if self.state.zoomed() {
            return Layout::Zoom;
        }
        Layout::Stacked {
            stack: stack_width(body.width, self.master_ratio),
            preview,
        }
    }

    /// The previews the list holds, top to bottom, dropping any configured
    /// position the workspace has no project for.
    fn stack_items(&self) -> Vec<usize> {
        self.state
            .stack()
            .iter()
            .copied()
            .filter(|position| self.projects.get(*position).is_some())
            .collect()
    }

    /// What one preview costs the column before any freed rows are handed out:
    /// a fold is a single title row, everything else is a whole preview.
    fn item_height(&self, position: usize, preview: u16) -> u16 {
        if self.state.collapsed(position) {
            COLLAPSED_HEIGHT
        } else {
            preview
        }
    }

    /// Which previews the column can show from the stored offset.
    ///
    /// `budget` is the stack area less its footer row, and every preview costs
    /// its height plus the blank row under it. The window takes previews from
    /// the offset while they fit, so the column always holds whole previews;
    /// the offset itself is clamped to the last window that still ends on the
    /// final preview, which is found by filling the same budget backwards.
    fn stack_window_of(&self, items: &[usize], budget: u16, preview: u16) -> StackWindow {
        let limit = items.len() - self.fill(items.iter().rev(), budget, preview);
        let offset = self.state.stack_offset().min(limit);
        StackWindow {
            offset,
            visible: self.fill(items[offset..].iter(), budget, preview),
            total: items.len(),
            limit,
        }
    }

    /// How many of `positions` fit in `budget` rows, each costing its height
    /// plus the blank row that follows it.
    fn fill<'a>(
        &self,
        positions: impl Iterator<Item = &'a usize>,
        budget: u16,
        preview: u16,
    ) -> usize {
        let mut used = 0;
        let mut count = 0;
        for position in positions {
            let cost = self.item_height(*position, preview) + 1;
            if used + cost > budget {
                break;
            }
            used += cost;
            count += 1;
        }
        count
    }

    /// Places the stack's children, top to bottom, from the scrolled window.
    ///
    /// A collapsed preview gives up every row but its title, and those rows go
    /// straight to the previews still open: the export's "freed rows
    /// redistribute to the panes still open, so one open preview grows to fill
    /// the column". The remainder goes to the topmost open panes so the column
    /// stays deterministic.
    ///
    /// When the whole list fits, each fold hands over exactly what it gave up,
    /// so the stack's used height never changes and a fold can never overflow
    /// a stack that fitted before it. When the list is longer than the column,
    /// the folds have already bought room for further previews, so what is
    /// handed out is only the room the window has left over.
    fn stack_layout(&self, area: Rect, preview: u16) -> Vec<StackSlot> {
        let items = self.stack_items();
        let budget = area.height.saturating_sub(1);
        let window = self.stack_window_of(&items, budget, preview);
        let drawn = &items[window.offset..window.offset + window.visible];
        let used: u16 = drawn
            .iter()
            .map(|position| self.item_height(*position, preview) + 1)
            .sum();
        let folded = drawn
            .iter()
            .filter(|position| self.state.collapsed(**position))
            .count();
        let open = window.visible - folded;
        let freed = (folded as u16 * (preview - COLLAPSED_HEIGHT)).min(budget - used);
        let (share, mut remainder) = if open > 0 {
            (freed / open as u16, freed % open as u16)
        } else {
            (0, 0)
        };

        let mut slots = Vec::with_capacity(window.visible);
        let mut top = area.y;
        for position in drawn.iter().copied() {
            let collapsed = self.state.collapsed(position);
            let height = if collapsed {
                COLLAPSED_HEIGHT
            } else {
                let extra = u16::from(remainder > 0);
                remainder = remainder.saturating_sub(1);
                preview + share + extra
            };
            slots.push(StackSlot {
                position,
                rect: Rect {
                    y: top,
                    height,
                    ..area
                },
                collapsed,
            });
            top += height + 1;
        }
        slots
    }

    fn draw_active(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        pane: Pane,
    ) {
        let Some(position) = self.state.active() else {
            return;
        };
        let Some(project) = self.projects.get(position) else {
            return;
        };
        self.draw_pane(engine, buffer, area, project, position, pane);
    }

    /// Draws the previews top to bottom and returns the rects they took.
    fn stack(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        preview: u16,
    ) -> Vec<Rect> {
        let demoted = self.state.demoted(self.now);
        let window =
            self.stack_window_of(&self.stack_items(), area.height.saturating_sub(1), preview);
        let mut drawn = Vec::new();
        for slot in self.stack_layout(area, preview) {
            let Some(project) = self.projects.get(slot.position) else {
                continue;
            };
            if slot.collapsed {
                // A strip has no border to carry a drag or demotion highlight,
                // and already sits on the demoted background.
                self.draw_strip(engine, buffer, slot.rect, project, slot.position);
            } else {
                let kind = if self.state.dragged() == Some(slot.position) {
                    Pane::Preview.dragging(false)
                } else if self.state.drag_target() == Some(slot.position) {
                    Pane::Preview.dragging(true)
                } else if demoted == Some(slot.position) {
                    Pane::Demoted
                } else {
                    Pane::Preview
                };
                self.draw_pane(engine, buffer, slot.rect, project, slot.position, kind);
            }
            drawn.push(slot.rect);
        }
        self.scroll_track(buffer, area, window);
        buffer.set_line(
            area.x + 1,
            area.y + area.height - 1,
            &Line::from(self.stack_hints(demoted, window, area.width - 1)),
            area.width - 1,
        );
        drawn
    }

    /// The draggable divider between the master and the stack.
    ///
    /// A `│` in the separator colour for its whole height, with a three-cell
    /// grip at the middle in the hint colour: the affordance says both where
    /// the split is and that it can be taken hold of. While it is held the
    /// whole divider takes the accent, so the drag states itself the way a
    /// pane drag does.
    fn divider(&self, buffer: &mut Buffer, body: Rect, column: u16) {
        let middle = body.y + body.height / 2;
        let grip = middle.saturating_sub(1)..=middle + 1;
        for row in body.y..body.y + body.height {
            let Some(cell) = buffer.cell_mut((column, row)) else {
                continue;
            };
            let held = grip.contains(&row);
            cell.set_symbol(if held { "┃" } else { "│" }).set_style(
                Style::new()
                    .fg(match (self.state.resizing(), held) {
                        (true, _) => ACCENT,
                        (false, true) => HINT,
                        (false, false) => SEPARATOR,
                    })
                    .bg(CANVAS),
            );
        }
    }

    /// A one-column track in the gutter beside the stack, drawn only while the
    /// list is longer than the window.
    ///
    /// It takes no room from the previews, so a stack that fits looks exactly
    /// as the export draws it. The thumb's length and position are the
    /// window's share of the list, which states both how much is hidden and
    /// where the window sits in it.
    fn scroll_track(&self, buffer: &mut Buffer, area: Rect, window: StackWindow) {
        let track = usize::from(area.height.saturating_sub(1));
        if !window.overflows() || track == 0 || area.x == 0 {
            return;
        }
        let length = (window.visible * track)
            .div_ceil(window.total)
            .clamp(1, track);
        // Anchored at whichever end the window has reached, so "there is
        // nothing further down" is never a rounding question.
        let top = match window.below() {
            0 => track - length,
            _ => (window.offset * track / window.total).min(track - length),
        };
        for row in 0..track {
            let held = (top..top + length).contains(&row);
            let Some(cell) = buffer.cell_mut((area.x - 1, area.y + row as u16)) else {
                continue;
            };
            cell.set_symbol(if held { "┃" } else { "│" }).set_style(
                Style::new()
                    .fg(if held { HINT } else { IDLE_BORDER })
                    .bg(CANVAS),
            );
        }
    }

    /// A collapsed preview: one row, no box, on the export's `#101317`.
    ///
    /// `▸ {n} {name} · {dot} · {tail}`, where the tail is the pane's last
    /// meaningful line. The box goes and takes the cwd, the right-hand
    /// activity slot, the viewport, the exit footer and the scrollback marker
    /// with it; all of them come back when the pane expands.
    fn draw_strip(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        project: &Project,
        position: usize,
    ) {
        Block::new()
            .style(Style::new().bg(DEMOTED_BG))
            .render(area, buffer);
        let status = self.status(engine, project);
        let metadata = self.metadata(engine, project);
        let (glyph, glyph_colour) = status_glyph(&status, &metadata);
        let separator = Style::new().fg(SEPARATOR).bg(DEMOTED_BG);

        let mut spans = vec![
            Span::styled("▸ ", Style::new().fg(HINT).bg(DEMOTED_BG)),
            Span::styled(
                format!("{} {}", position + 1, project.terminal),
                Style::new().fg(PREVIEW_FG).bg(DEMOTED_BG),
            ),
            Span::styled(" · ", separator),
            Span::styled(glyph, Style::new().fg(glyph_colour).bg(DEMOTED_BG)),
            Span::styled(" · ", separator),
        ];
        // The strip sits two columns in, per the export's `padding:0 2ch`, and
        // keeps the same inset on the right.
        let width = area.width.saturating_sub(2 * PADDING);
        let taken: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let (tail, colour) = self.strip_tail(engine, project, &status, &metadata);
        spans.push(Span::styled(
            clip(&tail, (width as usize).saturating_sub(taken)),
            Style::new().fg(colour).bg(DEMOTED_BG),
        ));
        buffer.set_line(area.x + PADDING, area.y, &Line::from(spans), width);
    }

    /// The strip's trailing text: what the pane would say if it had one line
    /// left. An exit outranks an idle age, which outranks the live output.
    fn strip_tail(
        &self,
        engine: &dyn TerminalEngine,
        project: &Project,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
    ) -> (String, Color) {
        match status {
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => (
                status_label(status, false).unwrap_or_else(|| "exited".to_owned()),
                ERROR,
            ),
            TerminalStatus::Starting => ("starting".to_owned(), MUTED),
            TerminalStatus::Running => match metadata.output_idle {
                Some(idle) if idle >= ACTIVE_WINDOW => {
                    (format!("idle {}", age(idle.millis)), MUTED)
                }
                _ => (
                    engine
                        .frame(&project.terminal)
                        .map(last_line)
                        .unwrap_or_default(),
                    MUTED,
                ),
            },
        }
    }

    /// The stack footer names the promotion that just happened and the key
    /// that undoes it; otherwise it states what the window hides, the fold
    /// census, or the promotion keys.
    fn stack_hints(
        &self,
        demoted: Option<usize>,
        window: StackWindow,
        width: u16,
    ) -> Vec<Span<'static>> {
        if self.state.scrollback() {
            return vec![
                Span::styled("scrollback", Style::new().fg(WARNING)),
                Span::styled(" · ", Style::new().fg(HINT)),
                Span::styled("esc", Style::new().fg(PREVIEW_FG)),
                Span::styled(" returns to live", Style::new().fg(HINT)),
            ];
        }
        let promoted = self
            .state
            .active()
            .and_then(|position| self.projects.get(position));
        // A demotion outranks both censuses: it clears itself after ~1.5s and
        // whichever of them applies comes back.
        //
        // Hidden previews outrank folded ones. A fold is already declared by
        // its own strip, its marker and the status row, while a preview the
        // window has scrolled past says nothing about itself anywhere else.
        if demoted.is_none() && window.overflows() {
            return self.overflow_hints(window, width);
        }
        let collapsed = self.state.collapsed_count();
        if demoted.is_none() && collapsed > 0 {
            return vec![
                Span::styled(format!("{collapsed} collapsed · "), Style::new().fg(HINT)),
                Span::styled("^g c", Style::new().fg(PREVIEW_FG)),
                Span::styled(" expand all", Style::new().fg(HINT)),
            ];
        }
        match (demoted, promoted) {
            (Some(previous), Some(project)) => vec![
                Span::styled("promoted ", Style::new().fg(HINT)),
                Span::styled(project.terminal.to_string(), Style::new().fg(ACCENT)),
                Span::styled(" · ", Style::new().fg(HINT)),
                Span::styled(format!("^g {}", previous + 1), Style::new().fg(PREVIEW_FG)),
                Span::styled(" back", Style::new().fg(HINT)),
            ],
            _ => vec![
                Span::styled("ctrl+g ", Style::new().fg(HINT)),
                Span::styled("1-4", Style::new().fg(PREVIEW_FG)),
                Span::styled(" promote · ", Style::new().fg(HINT)),
                Span::styled("j/k", Style::new().fg(PREVIEW_FG)),
                Span::styled(" cycle", Style::new().fg(HINT)),
            ],
        }
    }

    /// `↑ 2 more · ↓ 3 more · ^g pgup/pgdn`, naming only the end that has
    /// something behind it. The keys are stated whenever the footer has the
    /// columns for them, and dropped first when it does not — the status
    /// bar's own rule.
    fn overflow_hints(&self, window: StackWindow, width: u16) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT);
        let key = Style::new().fg(PREVIEW_FG);
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (arrow, hidden) in [("↑", window.above()), ("↓", window.below())] {
            if hidden == 0 {
                continue;
            }
            if !spans.is_empty() {
                spans.push(Span::styled(" · ", hint));
            }
            spans.push(Span::styled(format!("{arrow} {hidden} "), key));
            spans.push(Span::styled("more", hint));
        }
        let taken: usize = spans.iter().map(Span::width).sum();
        if taken + PAGE_KEYS.chars().count() + 3 <= usize::from(width) {
            spans.push(Span::styled(" · ", hint));
            spans.push(Span::styled(PAGE_KEYS, key));
        }
        spans
    }

    /// Master-only fallback: a one-line pane strip on the first row, a blank
    /// row, then the master for the rest of the body. Returns the master rect.
    fn narrow(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, body: Rect) -> Rect {
        self.strip(engine, buffer, Rect { height: 1, ..body });
        let master = Rect {
            y: body.y + 2,
            height: body.height.saturating_sub(2),
            ..body
        };
        self.draw_active(engine, buffer, master, Pane::Compact);
        master
    }

    /// `1 frontend` `2 backend` · `3 app ✕1` · `4 worker ○`, in configured
    /// order so positions stay learnable when the stack is gone.
    fn strip(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, area: Rect) {
        let active = self.state.active();
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (position, project) in self.projects.iter().enumerate() {
            let master = active == Some(position);
            // The export separates chips, but never right after the accent
            // chip, which already stands apart.
            if position > 0 && active != Some(position - 1) {
                spans.push(Span::styled("·", Style::new().fg(HINT)));
            }
            let status = self.status(engine, project);
            let metadata = self.metadata(engine, project);
            let label = format!(
                " {} {}{} ",
                position + 1,
                project.terminal,
                chip_tag(&status, &metadata)
            );
            let style = if master {
                Style::new()
                    .fg(CANVAS)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(chip_colour(&status, &metadata)).bg(CHIP_BG)
            };
            spans.push(Span::styled(label, style));
        }
        buffer.set_line(
            area.x + 1,
            area.y,
            &Line::from(spans),
            area.width.saturating_sub(1),
        );
    }

    /// Draws the open overlay, centred on the canvas at the supplement's size.
    ///
    /// The box takes the accent border the master has just given up, and it
    /// clears the cells beneath it: the reviewed dimming rule recedes the
    /// interface, it does not show through the modal.
    fn modal(&self, buffer: &mut Buffer, area: Rect, modal: Modal) {
        let (width, height) = match modal {
            Modal::Help => HELP_SIZE,
            Modal::Quit => QUIT_SIZE,
        };
        let rect = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width: width.min(area.width),
            height: height.min(area.height),
        };
        if rect.width < 2 * PADDING + 4 || rect.height < 3 {
            return;
        }
        Clear.render(rect, buffer);
        let style = Style::new().fg(ACCENT).bg(CANVAS);
        Block::bordered()
            .style(Style::new().bg(CANVAS))
            .border_style(style)
            .render(rect, buffer);

        let left = rect.x + 1 + PADDING;
        let right = rect.x + rect.width - 2 - PADDING;
        let width = right - left + 1;
        let (name, slot, lines) = match modal {
            Modal::Help => ("help", Some("^g ?"), help_lines()),
            Modal::Quit => ("quit", None, quit_lines(self.projects.len())),
        };
        buffer.set_line(
            left - 1,
            rect.y,
            &Line::from(clear_around(
                vec![Span::styled(name, Style::new().fg(ACCENT))],
                style,
            )),
            width + 2,
        );
        if let Some(slot) = slot {
            let slot_width = slot.chars().count() as u16;
            buffer.set_line(
                right - slot_width,
                rect.y,
                &Line::from(clear_around(
                    vec![Span::styled(slot, Style::new().fg(HINT))],
                    style,
                )),
                slot_width + 2,
            );
        }
        for (row, line) in lines.iter().enumerate().take(rect.height as usize - 2) {
            buffer.set_line(left, rect.y + 1 + row as u16, line, width);
        }
    }

    /// Draws one bordered pane: border, title chrome, terminal cells, footer.
    fn draw_pane(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        project: &Project,
        position: usize,
        pane: Pane,
    ) {
        let id = &project.terminal;
        let status = self.status(engine, project);
        let metadata = self.metadata(engine, project);
        let exited = matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        );

        let (border, background) = match pane {
            Pane::Preview => (IDLE_BORDER, CANVAS),
            Pane::Demoted => (DEMOTED_BORDER, DEMOTED_BG),
            // A held pane turns warning-coloured; its only valid counterpart
            // gets the accent and a quiet lifted background.
            Pane::DragMasterSource | Pane::DragPreviewSource => (WARNING, CANVAS),
            Pane::DragMasterTarget | Pane::DragPreviewTarget => (ACCENT, DEMOTED_BG),
            _ => (ACCENT, CANVAS),
        };
        Block::bordered()
            .style(Style::new().bg(background))
            .border_style(Style::new().fg(border).bg(background))
            .render(area, buffer);

        // Content columns, also the columns the title chrome aligns to.
        let left = area.x + 1 + PADDING;
        let right = area.x + area.width - 2 - PADDING;
        let content = Rect {
            x: left,
            y: area.y + 1,
            width: right - left + 1,
            height: area.height - 2,
        };

        // Title chrome aligns with the content columns and clears one border
        // cell on each side, matching the export's 2-column title inset.
        let border_style = Style::new().fg(border).bg(background);
        let slot = self.right_slot(&status, &metadata, pane);
        let slot_width: u16 = slot.iter().map(|span| span.width() as u16).sum();
        let title = self.title(
            project,
            position,
            &status,
            &metadata,
            pane,
            content.width.saturating_sub(slot_width + 2),
        );
        buffer.set_line(
            left - 1,
            area.y,
            &Line::from(clear_around(title, border_style)),
            content.width + 2,
        );
        if slot_width > 0 {
            buffer.set_line(
                right - slot_width,
                area.y,
                &Line::from(clear_around(slot, border_style)),
                slot_width + 2,
            );
        }

        let default_fg = if pane.master() {
            MASTER_FG
        } else if exited {
            MUTED
        } else {
            PREVIEW_FG
        };
        let mut viewport = content;
        if pane.master() && self.state.scrollback() {
            // The mode owns the pane foot while it is active.
            viewport.height = content.height.saturating_sub(2);
            self.scroll_footer(buffer, content, background);
        } else if exited {
            viewport.height = content.height.saturating_sub(2);
            self.exit_footer(buffer, content, &status, &metadata, background);
        } else if !pane.master() && metadata.scrollback.lines_above > 0 {
            viewport.height = content.height.saturating_sub(1);
            self.scroll_marker(buffer, content, &metadata, background);
        }
        if let Some(terminal) = engine.frame(id) {
            draw_terminal(buffer, viewport, terminal, default_fg, background);
        }
    }

    /// `> {n} {name} · {cwd} · {dot} {state} · {cmd}` for a full-width master,
    /// and `{n} {name} · {cwd} · {dot}` for a preview or a compact master.
    fn title(
        &self,
        project: &Project,
        position: usize,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        pane: Pane,
        budget: u16,
    ) -> Vec<Span<'static>> {
        let (master, wide) = (pane.master(), pane.wide());
        let number = position + 1;
        let name = project.terminal.to_string();
        let separator = if wide { "  ·  " } else { " · " };
        let (glyph, glyph_colour) = status_glyph(status, metadata);
        let mut spans = Vec::new();
        // Every stack pane declares its disclosure state, folded or not: the
        // open `▾` is the only thing on a fresh frame that says the stack
        // folds at all, and it is the cell the pointer toggles.
        if !master {
            spans.push(Span::styled("▾ ", Style::new().fg(HINT)));
        }
        if master {
            spans.push(Span::styled(
                format!("> {number} {name}"),
                Style::new().fg(ACCENT),
            ));
        } else {
            spans.push(Span::styled(
                format!("{number} {name}"),
                Style::new().fg(if pane.base() == Pane::Demoted {
                    DEMOTED_FG
                } else {
                    PREVIEW_FG
                }),
            ));
        }
        spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));

        let fixed: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let tail = separator.chars().count() + glyph.chars().count();
        let path_budget = (budget as usize).saturating_sub(fixed + tail);
        let path = self.path(&project.path, wide, path_budget);
        spans.push(Span::styled(
            path,
            Style::new().fg(if wide { PREVIEW_FG } else { MUTED }),
        ));
        spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));
        spans.push(Span::styled(
            glyph,
            Style::new().fg(if master && glyph_colour == SUCCESS {
                ACCENT
            } else {
                glyph_colour
            }),
        ));
        if let Some(label) = status_label(status, wide) {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::new().fg(glyph_colour),
            ));
        }
        if wide {
            spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));
            spans.push(Span::styled(
                self.command(project, metadata, pane),
                Style::new().fg(MUTED),
            ));
        }
        spans
    }

    /// Zoom has the width to spend on the process behind the command.
    fn command(&self, project: &Project, metadata: &TerminalMetadata, pane: Pane) -> String {
        let command = project.command.join(" ");
        match (pane.base(), metadata.process) {
            (Pane::Zoomed, Some(process)) => format!(
                "{command} · pid {} · up {}",
                process.pid,
                age(process.uptime.millis)
            ),
            _ => command,
        }
    }

    /// A full-width master shows the complete path; every other pane keeps the
    /// repository name only.
    fn path(&self, path: &Path, wide: bool, budget: usize) -> String {
        let full = abbreviate(path, self.home);
        if wide && full.chars().count() <= budget {
            return full;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(full);
        clip(&format!("…/{name}"), budget)
    }

    /// The right-aligned slot: activity meter, idle age, exit age, and the
    /// MASTER or ZOOM tag. A compact master has no room for it.
    fn right_slot(
        &self,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        pane: Pane,
    ) -> Vec<Span<'static>> {
        if pane.master() && self.state.scrollback() {
            return vec![Span::styled(
                " SCROLL ",
                Style::new()
                    .fg(CANVAS)
                    .bg(WARNING)
                    .add_modifier(Modifier::BOLD),
            )];
        }
        if pane.base() == Pane::Compact {
            return Vec::new();
        }
        let colour = match pane.base() {
            Pane::Master | Pane::Zoomed => ACCENT,
            Pane::Demoted => MUTED,
            _ => HINT,
        };
        let text = match status {
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => metadata
                .last_exit_at
                .map(|at| {
                    format!(
                        "{} ago",
                        age(self.now.unix_millis.saturating_sub(at.unix_millis))
                    )
                })
                .unwrap_or_else(|| "exited".to_owned()),
            _ => match metadata.output_idle {
                Some(idle) if idle >= ACTIVE_WINDOW => format!("idle {}", age(idle.millis)),
                Some(idle) => meter(idle.millis),
                None => meter(0),
            },
        };
        match pane.base() {
            Pane::Zoomed => vec![
                Span::styled(
                    " ZOOM ",
                    Style::new()
                        .fg(CANVAS)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {text}"), Style::new().fg(ACCENT)),
            ],
            Pane::Master => vec![
                Span::styled(text, Style::new().fg(colour)),
                Span::styled(" MASTER", Style::new().fg(ACCENT)),
            ],
            _ => vec![Span::styled(text, Style::new().fg(colour))],
        }
    }

    /// Rule plus the navigation keys at the pane foot, reusing the accepted
    /// exited-pane pattern. Scroll position is stated once, in the status row,
    /// so the footer carries the keys alone.
    fn scroll_footer(&self, buffer: &mut Buffer, content: Rect, background: Color) {
        if content.height < 2 {
            return;
        }
        buffer.set_line(
            content.x,
            content.y + content.height - 2,
            &Line::styled(
                "─".repeat(content.width as usize),
                Style::new().fg(SEPARATOR).bg(background),
            ),
            content.width,
        );
        let hint = Style::new().fg(HINT).bg(background);
        let key = Style::new().fg(PREVIEW_FG).bg(background);
        let mut spans = Vec::new();
        // The last entry is help, which stays in the status bar.
        for (index, (name, label)) in SCROLLBACK_HINTS[..4].iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" · ", hint));
            }
            spans.push(Span::styled(*name, key));
            spans.push(Span::styled(format!(" {label}"), hint));
        }
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::from(spans),
            content.width,
        );
    }

    /// `↑ 214 lines above · ^g [` on a pane holding a detached viewport. A
    /// promoted terminal keeps its scroll position, so a demoted preview says
    /// how far back it is sitting.
    fn scroll_marker(
        &self,
        buffer: &mut Buffer,
        content: Rect,
        metadata: &TerminalMetadata,
        background: Color,
    ) {
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::styled(
                format!("↑ {} lines above · ^g [", metadata.scrollback.lines_above),
                Style::new().fg(HINT).bg(background),
            ),
            content.width,
        );
    }

    /// Rule plus `exited · code 1 · {time} · r restart` at the pane foot.
    fn exit_footer(
        &self,
        buffer: &mut Buffer,
        content: Rect,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        background: Color,
    ) {
        if content.height < 2 {
            return;
        }
        let rule = "─".repeat(content.width as usize);
        buffer.set_line(
            content.x,
            content.y + content.height - 2,
            &Line::styled(rule, Style::new().fg(ERROR).bg(background)),
            content.width,
        );
        let reason = match status {
            TerminalStatus::Exited { code: Some(code) } => format!("exited · code {code}"),
            TerminalStatus::Exited { code: None } => "exited".to_owned(),
            _ => "failed".to_owned(),
        };
        let mut spans = vec![Span::styled(reason, Style::new().fg(ERROR).bg(background))];
        if let Some(at) = metadata.last_exit_at {
            spans.push(Span::styled(
                format!(" · {} · ", clock(at)),
                Style::new().fg(HINT).bg(background),
            ));
        } else {
            spans.push(Span::styled(" · ", Style::new().fg(HINT).bg(background)));
        }
        spans.push(Span::styled(
            "r restart",
            Style::new().fg(PREVIEW_FG).bg(background),
        ));
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::from(spans),
            content.width,
        );
    }

    fn status_row(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        layout: Layout,
    ) {
        Block::new()
            .style(Style::new().bg(STATUS_BG))
            .render(area, buffer);
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let workspace = Span::styled(
            format!(" {} ", self.workspace),
            Style::new()
                .fg(CANVAS)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        );

        let left = if layout == Layout::Narrow {
            // The compact line trades the terminal census for the reason the
            // stack is missing.
            Line::from(vec![
                workspace,
                // The row below the body, so `y + height` is the row count.
                Span::styled(format!("  {}×{}  ", area.width, area.y + area.height), hint),
                Span::styled("stack hidden", Style::new().fg(WARNING).bg(STATUS_BG)),
            ])
        } else {
            let total = self.projects.len();
            let active = self
                .state
                .active()
                .and_then(|position| self.projects.get(position).zip(Some(position)))
                .map(|(project, position)| format!("> {} {}", position + 1, project.terminal))
                .unwrap_or_default();
            let mut spans = vec![
                workspace,
                Span::styled(format!("  {total} terminal{}   ", plural(total)), hint),
                Span::styled(active, Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
            ];
            if self.state.scrollback() {
                spans.push(Span::styled("  ·  ", hint));
                spans.push(Span::styled(
                    "SCROLLBACK",
                    Style::new().fg(WARNING).bg(STATUS_BG),
                ));
                if let Some((line, total)) = self.scroll_position(engine) {
                    spans.push(Span::styled(format!("  ·  line {line}/{total}"), hint));
                }
            } else if layout == Layout::Zoom {
                spans.push(Span::styled("  ·  hidden: ", hint));
                spans.extend(self.hidden_summary(engine));
            } else if self.state.collapsed_count() > 0 {
                // The fold census replaces the stack count outright: the
                // export states it alone, with no exited summary after it.
                let collapsed = self.state.collapsed_count();
                spans.push(Span::styled(
                    format!("  ·  {} open  ·  ", self.state.stack().len() - collapsed),
                    hint,
                ));
                spans.push(Span::styled(
                    format!("{collapsed} collapsed"),
                    Style::new().fg(WARNING).bg(STATUS_BG),
                ));
            } else {
                spans.push(Span::styled(
                    format!("  ·  {} stacked", total.saturating_sub(1)),
                    hint,
                ));
                spans.extend(self.exited_summary(engine));
            }
            Line::from(spans)
        };
        let taken = left.width() as u16;
        buffer.set_line(area.x + PADDING, area.y, &left, area.width);

        // Keys outlive their labels: take the widest form that still clears
        // the left text by a two-column gap.
        for right in self.key_hints(layout) {
            let width = right.width() as u16;
            if taken + width + 2 * PADDING + 2 <= area.width {
                buffer.set_line(area.x + area.width - PADDING - width, area.y, &right, width);
                break;
            }
        }
    }

    /// `line 2217/2431`: the last visible line, and every retained line.
    fn scroll_position(&self, engine: &dyn TerminalEngine) -> Option<(u32, u32)> {
        let project = self
            .state
            .active()
            .and_then(|position| self.projects.get(position))?;
        let frame = engine.frame(&project.terminal)?;
        let scrollback = self.metadata(engine, project).scrollback;
        let line = scrollback.lines_above + u32::from(frame.size.rows);
        Some((line, line + scrollback.lines_below))
    }

    /// `1 exited`, or `all running` when every terminal is alive.
    fn exited_summary(&self, engine: &dyn TerminalEngine) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let exited = self
            .projects
            .iter()
            .filter(|project| {
                matches!(
                    engine.status(&project.terminal),
                    Some(TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. })
                )
            })
            .count();
        if exited == 0 {
            return vec![Span::styled("  ·  all running", hint)];
        }
        vec![
            Span::styled("  ·  ", hint),
            Span::styled(
                format!("{exited} exited"),
                Style::new().fg(ERROR).bg(STATUS_BG),
            ),
        ]
    }

    /// `2● 3● 4○` — zoom hides the previews, so the status row keeps them
    /// accounted for.
    fn hidden_summary(&self, engine: &dyn TerminalEngine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for position in self.state.stack().iter().copied() {
            let Some(project) = self.projects.get(position) else {
                continue;
            };
            if !spans.is_empty() {
                spans.push(Span::styled(" ", Style::new().fg(HINT).bg(STATUS_BG)));
            }
            let status = self.status(engine, project);
            let metadata = self.metadata(engine, project);
            let (glyph, colour) = status_glyph(&status, &metadata);
            spans.push(Span::styled(
                format!("{}{glyph}", position + 1),
                Style::new().fg(colour).bg(STATUS_BG),
            ));
        }
        spans
    }

    /// Candidate hint rows, widest first. The status bar drops shortcut
    /// labels before keys and then collapses, per the export's responsive
    /// rule; the caller takes the first one that fits.
    fn key_hints(&self, layout: Layout) -> Vec<Line<'static>> {
        // A modal or mode states its own keys: nothing else is reachable while
        // it is open.
        if let Some(modal) = self.state.modal() {
            return vec![modal_hints(modal)];
        }
        if self.state.scrollback() {
            return vec![
                self.hints(&SCROLLBACK_HINTS, layout, true),
                self.hints(&SCROLLBACK_HINTS, layout, false),
            ];
        }
        let collapsed = Line::from(vec![
            Span::styled("^g", Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
            Span::styled(
                " j/k · 1-4 · z · [ · ? · q",
                Style::new().fg(HINT).bg(STATUS_BG),
            ),
        ]);
        if layout == Layout::Narrow {
            return vec![collapsed];
        }
        // While a fold is in play the row advertises the key that undoes it.
        // The export drops the scrollback hint and seats collapse ahead of
        // zoom rather than in the slot scrollback vacated.
        //
        // Zoom hides the stack outright, so the fold is inert and unstated
        // there — the spec's "the zoom status line ... says nothing about
        // collapse". Since #39 every run starts folded, so without this the
        // zoomed row would trade its live `^g [` for an inert `^g c`.
        let folded: [(&str, &str); 6] = [
            KEY_HINTS[0],
            KEY_HINTS[1],
            ("^g c", "collapse"),
            KEY_HINTS[2],
            KEY_HINTS[4],
            KEY_HINTS[5],
        ];
        let entries = if self.state.collapsed_count() > 0 && layout != Layout::Zoom {
            folded
        } else {
            KEY_HINTS
        };
        vec![
            self.hints(&entries, layout, true),
            self.hints(&entries, layout, false),
            collapsed,
        ]
    }

    fn hints(&self, entries: &[(&str, &str)], layout: Layout, labels: bool) -> Line<'static> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let key = Style::new().fg(PREVIEW_FG).bg(STATUS_BG);
        let mut spans = Vec::new();
        for (index, (name, label)) in entries.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled("  ", hint));
            }
            // An active mode names itself in accent: zoom while zoomed, and
            // collapse while any preview is folded.
            let folded = self.state.collapsed_count() > 0 && *name == "^g c";
            let zoom = layout == Layout::Zoom && *name == "^g z";
            spans.push(Span::styled(
                (*name).to_owned(),
                if zoom || folded {
                    Style::new().fg(ACCENT).bg(STATUS_BG)
                } else {
                    key
                },
            ));
            if labels {
                spans.push(Span::styled(
                    format!(" {}", if zoom { "unzoom" } else { *label }),
                    if zoom || folded { key } else { hint },
                ));
            }
        }
        Line::from(spans)
    }

    fn place_cursor(&self, engine: &dyn TerminalEngine, frame: &mut Frame, master: Rect) {
        // The scrollback viewport is detached from the live tail, so there is
        // no cursor to draw.
        if self.state.scrollback() {
            return;
        }
        let Some(project) = self
            .state
            .active()
            .and_then(|position| self.projects.get(position))
        else {
            return;
        };
        let Some(terminal) = engine.frame(&project.terminal) else {
            return;
        };
        let Cursor {
            column,
            row,
            visible: true,
        } = terminal.cursor
        else {
            return;
        };
        let x = master.x + 1 + PADDING + column;
        let y = master.y + 1 + row;
        if x < master.x + master.width - 1 && y < master.y + master.height - 1 {
            frame.set_cursor_position(Position::new(x, y));
        }
    }

    fn status(&self, engine: &dyn TerminalEngine, project: &Project) -> TerminalStatus {
        engine
            .status(&project.terminal)
            .cloned()
            .unwrap_or(TerminalStatus::Starting)
    }

    fn metadata(&self, engine: &dyn TerminalEngine, project: &Project) -> TerminalMetadata {
        engine
            .metadata(&project.terminal)
            .cloned()
            .unwrap_or_default()
    }
}

/// Surrounds title chrome with one blank border cell on each side.
fn clear_around(spans: Vec<Span<'static>>, style: Style) -> Vec<Span<'static>> {
    let mut padded = vec![Span::styled(" ", style)];
    padded.extend(spans);
    padded.push(Span::styled(" ", style));
    padded
}

/// Recedes the interface behind an open modal. The reviewed rule dims
/// foreground only, so the two accepted background values are never
/// supplemented with a scrim: pane borders fall back to idle, pane content
/// drops to [`UNDER_FG`], and the key hints between the panes drop further.
fn dim(buffer: &mut Buffer, body: Rect, panes: &[Rect]) {
    for y in body.y..body.y + body.height {
        for x in body.x..body.x + body.width {
            let position = Position::new(x, y);
            let colour = match panes.iter().find(|pane| pane.contains(position)) {
                Some(pane) if on_border(*pane, position) => IDLE_BORDER,
                Some(_) => UNDER_FG,
                None => UNDER_HINT,
            };
            if let Some(cell) = buffer.cell_mut(position) {
                cell.fg = colour;
            }
        }
    }
}

fn on_border(pane: Rect, position: Position) -> bool {
    position.x == pane.x
        || position.x + 1 == pane.x + pane.width
        || position.y == pane.y
        || position.y + 1 == pane.y + pane.height
}

/// The help overlay's body, in the supplement's order: a blank row, then each
/// section heading with its bindings, then the way out.
fn help_lines() -> Vec<Line<'static>> {
    let key = Style::new().fg(PREVIEW_FG);
    let label = Style::new().fg(MUTED);
    let mut lines = vec![Line::default()];
    for (name, description) in HELP {
        if description.is_empty() {
            if lines.len() > 1 {
                lines.push(Line::default());
            }
            lines.push(Line::styled(
                name,
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{name:<HELP_KEYS$}"), key),
            Span::styled(description, label),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("esc  ", key),
        Span::styled("close", Style::new().fg(HINT)),
    ]));
    lines
}

/// The quit confirmation's body. It states how many terminals close, not which
/// ones, and confines the warning colour to the consequence line.
fn quit_lines(terminals: usize) -> Vec<Line<'static>> {
    let key = Style::new().fg(PREVIEW_FG);
    let label = Style::new().fg(MUTED);
    // The supplement's ten-column gaps, less one: a drawn border costs a whole
    // cell where the supplement's hairline border costs none.
    let gap = Span::styled(" ".repeat(9), Style::new().fg(HINT));
    vec![
        Line::default(),
        Line::styled("quit termdeck?", Style::new().fg(MASTER_FG)),
        Line::default(),
        Line::styled(
            format!("{terminals} terminal{} will be closed.", plural(terminals)),
            key,
        ),
        Line::styled("SIGTERM, then SIGKILL after 2s.", Style::new().fg(WARNING)),
        Line::default(),
        Line::from(vec![
            Span::styled("y  ", key),
            Span::styled("quit", label),
            gap.clone(),
            Span::styled("n  ", key),
            Span::styled("cancel", label),
            gap,
            Span::styled("esc  ", key),
            Span::styled("cancel", label),
        ]),
    ]
}

/// The status row while a modal is open: it names the modal and its answers.
fn modal_hints(modal: Modal) -> Line<'static> {
    let hint = Style::new().fg(HINT).bg(STATUS_BG);
    let key = Style::new().fg(PREVIEW_FG).bg(STATUS_BG);
    match modal {
        Modal::Help => Line::from(vec![
            Span::styled("^g ?", Style::new().fg(ACCENT).bg(STATUS_BG)),
            Span::styled(" help open", key),
            Span::styled("  ·  ", hint),
            Span::styled("esc", key),
            Span::styled(" close", hint),
        ]),
        Modal::Quit => Line::from(vec![
            Span::styled("confirm quit", Style::new().fg(WARNING).bg(STATUS_BG)),
            Span::styled("  ·  ", hint),
            Span::styled("y", key),
            Span::styled(" quit  ", hint),
            Span::styled("n", key),
            Span::styled(" cancel", hint),
        ]),
    }
}

/// The help overlay's bindings, from the plan. An empty description marks a
/// section heading.
const HELP: [(&str, &str); 16] = [
    ("NAVIGATE", ""),
    ("^g j  ^g k", "promote next / previous"),
    ("^g ↓  ^g ↑", "same, with arrow keys"),
    ("^g 1 … ^g 4", "promote by configured position"),
    ("VIEW", ""),
    ("^g z", "toggle zoom"),
    ("^g c", "collapse / expand previews"),
    ("^g pgup/pgdn", "page the preview stack"),
    ("^g -  ^g =", "narrow / widen the master"),
    ("^g [", "enter scrollback mode"),
    ("TERMINAL", ""),
    ("^g r", "respawn active terminal"),
    ("^g ^g", "send a literal ^g"),
    ("SESSION", ""),
    ("^g ?", "this help"),
    ("^g q", "quit termdeck"),
];

/// Navigation keys captured while scrollback mode is active. The first four
/// are the pane footer; the whole list is the status bar.
const SCROLLBACK_HINTS: [(&str, &str); 5] = [
    ("j/k ↑↓", "line"),
    ("pgup/pgdn", "page"),
    ("g/G", "ends"),
    ("esc", "live"),
    ("^g ?", "help"),
];

const KEY_HINTS: [(&str, &str); 6] = [
    ("^g j/k", "switch"),
    ("^g 1-4", "select"),
    ("^g z", "zoom"),
    ("^g [", "scroll"),
    ("^g ?", "help"),
    ("^g q", "quit"),
];

/// Whether the split can move at this width. Below [`WIDE_COLUMNS`] the export
/// fixes the stack at [`COMPACT_STACK`], so the ratio has nothing to say and
/// the divider is neither drawn nor draggable.
fn adjustable(body: Rect) -> bool {
    body.width >= WIDE_COLUMNS
}

/// At or above [`WIDE_COLUMNS`] the stack takes the ceiling of the non-master
/// share so the master never overruns: at 144 columns with the default 0.70
/// this is the design's 44. Below that the export fixes it instead.
fn stack_width(width: u16, master_ratio: f64) -> u16 {
    if width < WIDE_COLUMNS {
        return COMPACT_STACK;
    }
    // The ceiling, but not fooled by a share that is a hair above a whole
    // column: the divider hands back the exact ratio for the column the
    // pointer is on, and without this the last bit of that division would
    // sometimes round the split one column past where it was dropped.
    let share = f64::from(width) * (1.0 - master_ratio) - 1e-9;
    let stack = share.ceil().max(0.0) as u16;
    stack.clamp(1, width.saturating_sub(GUTTER + 1))
}

/// The last non-blank row of a frame, as plain text. This is what a collapsed
/// preview shows for a terminal that is neither exited nor idle.
fn last_line(frame: &TerminalFrame) -> String {
    (0..frame.size.rows)
        .rev()
        .map(|row| {
            (0..frame.size.columns)
                .filter_map(
                    |column| match frame.cell(column, row).map(|cell| &cell.content) {
                        Some(CellContent::Glyph { text, .. }) => Some(text.as_str()),
                        Some(CellContent::Empty) => Some(" "),
                        _ => None,
                    },
                )
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn status_glyph(status: &TerminalStatus, metadata: &TerminalMetadata) -> (&'static str, Color) {
    match status {
        TerminalStatus::Starting => ("○", WARNING),
        TerminalStatus::Running => match metadata.output_idle {
            Some(idle) if idle >= ACTIVE_WINDOW => ("○", HINT),
            _ => ("●", SUCCESS),
        },
        TerminalStatus::Exited { code: Some(0) } => ("✓", SUCCESS),
        TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => ("✕", ERROR),
    }
}

/// A full-width master always names its state; a narrower pane only names an
/// exit.
fn status_label(status: &TerminalStatus, wide: bool) -> Option<String> {
    match status {
        TerminalStatus::Starting => wide.then(|| "starting".to_owned()),
        TerminalStatus::Running => wide.then(|| "running".to_owned()),
        TerminalStatus::Exited { code: Some(code) } => Some(format!("exit {code}")),
        TerminalStatus::Exited { code: None } => Some("exited".to_owned()),
        TerminalStatus::Failed { .. } => Some("failed".to_owned()),
    }
}

/// A strip chip states only what an ordinary running terminal does not need:
/// an exit code, or the idle ring.
fn chip_tag(status: &TerminalStatus, metadata: &TerminalMetadata) -> String {
    match status {
        TerminalStatus::Exited { code: Some(code) } => format!(" ✕{code}"),
        TerminalStatus::Exited { code: None } | TerminalStatus::Failed { .. } => " ✕".to_owned(),
        _ => match status_glyph(status, metadata) {
            ("●", _) => String::new(),
            (glyph, _) => format!(" {glyph}"),
        },
    }
}

fn chip_colour(status: &TerminalStatus, metadata: &TerminalMetadata) -> Color {
    match status {
        TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => ERROR,
        _ => match status_glyph(status, metadata) {
            ("●", _) => PREVIEW_FG,
            _ => MUTED,
        },
    }
}

/// `[####··]`, emptying one cell per fifth of the activity window.
fn meter(idle_millis: u64) -> String {
    let step = ACTIVE_WINDOW.millis / METER_CELLS;
    let fill = METER_CELLS
        .saturating_sub(idle_millis / step)
        .min(METER_CELLS) as usize;
    format!(
        "[{}{}]",
        "#".repeat(fill),
        "·".repeat(METER_CELLS as usize - fill)
    )
}

fn age(millis: u64) -> String {
    let seconds = millis / 1_000;
    match seconds {
        ..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3_600),
    }
}

/// `HH:MM:SS` in UTC. Termdeck has no time-zone database dependency.
fn clock(at: Timestamp) -> String {
    let seconds = at.unix_millis / 1_000 % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn abbreviate(path: &Path, home: Option<&Path>) -> String {
    home.and_then(|home| path.strip_prefix(home).ok())
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|| path.display().to_string())
}

/// Right-first truncation with an ellipsis, per the export's truncation rule.
fn clip(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    match budget {
        0 => String::new(),
        _ => text
            .chars()
            .take(budget - 1)
            .chain(std::iter::once('…'))
            .collect(),
    }
}

/// Copies engine-owned cells into the buffer, clipping to the viewport.
fn draw_terminal(
    buffer: &mut Buffer,
    area: Rect,
    terminal: &TerminalFrame,
    default_fg: Color,
    background: Color,
) {
    let clipped = terminal.size.columns > area.width;
    for row in 0..area.height.min(terminal.size.rows) {
        for column in 0..area.width.min(terminal.size.columns) {
            let Some(cell) = terminal.cell(column, row) else {
                continue;
            };
            let Some(target) = buffer.cell_mut((area.x + column, area.y + row)) else {
                continue;
            };
            let style = cell_style(&cell.style, default_fg, background);
            match &cell.content {
                CellContent::Glyph { text, .. } => {
                    target.set_symbol(text);
                }
                CellContent::Empty | CellContent::Continuation => {
                    target.set_symbol(" ");
                }
            }
            target.set_style(style);
        }
        if clipped && area.width > 0 {
            buffer[(area.x + area.width - 1, area.y + row)]
                .set_symbol("…")
                .set_style(Style::new().fg(default_fg).bg(background));
        }
    }
}

fn cell_style(style: &CellStyle, default_fg: Color, background: Color) -> Style {
    let mut result = Style::new()
        .fg(style.foreground.map_or(default_fg, colour))
        .bg(style.background.map_or(background, colour));
    for (enabled, modifier) in [
        (style.bold, Modifier::BOLD),
        (style.dim, Modifier::DIM),
        (style.italic, Modifier::ITALIC),
        (style.underline, Modifier::UNDERLINED),
        (style.inverse, Modifier::REVERSED),
    ] {
        if enabled {
            result = result.add_modifier(modifier);
        }
    }
    result
}

const fn colour(rgb: Rgb) -> Color {
    Color::Rgb(rgb.red, rgb.green, rgb.blue)
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::{Position, Rect},
    };

    use super::{
        ACCENT, CHIP_BG, DEMOTED_BG, DEMOTED_BORDER, Deck, DeckState, ERROR, HINT, IDLE_BORDER,
        SEPARATOR, STATUS_BG, UNDER_FG, UNDER_HINT, WARNING, fixture,
    };
    use crate::{
        contracts::{ActionCommand, Project, TerminalEngine, TerminalId, TerminalStatus},
        engine::FakeEngine,
    };

    /// Renders one reference canvas at the given size.
    fn render(
        engine: &FakeEngine,
        state: &DeckState,
        size: (u16, u16),
    ) -> (Buffer, Option<Position>) {
        let projects = fixture::projects();
        let deck = Deck {
            workspace: "idp",
            projects: &projects,
            state,
            home: Some(fixture::home()),
            master_ratio: state.master_ratio(),
            now: fixture::NOW,
        };
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| deck.render(engine as &dyn TerminalEngine, frame))
            .unwrap();
        let cursor = terminal.get_cursor_position().ok();
        (terminal.backend().buffer().clone(), cursor)
    }

    /// The accepted 144x42 reference canvas with the first terminal as master.
    fn reference() -> (Buffer, Option<Position>) {
        render(&fixture::frontend_active(), &DeckState::new(4), (144, 42))
    }

    /// Open previews own three bands of the column; the folded default is
    /// covered by `a_folded_strip_is_hit_tested_but_has_nothing_to_scroll`.
    #[test]
    fn pane_hit_testing_follows_the_rendered_layout() {
        let projects = fixture::projects();
        let state = &expanded(4);
        let deck = Deck {
            workspace: "idp",
            projects: &projects,
            state,
            home: Some(fixture::home()),
            master_ratio: state.master_ratio(),
            now: fixture::NOW,
        };
        let area = Rect::new(0, 0, 144, 42);

        let terminal = |pointer| deck.terminal_at(area, pointer).map(ToString::to_string);
        assert_eq!(terminal(Position::new(10, 10)).as_deref(), Some("frontend"));
        assert_eq!(terminal(Position::new(110, 5)).as_deref(), Some("backend"));
        assert_eq!(terminal(Position::new(110, 18)).as_deref(), Some("app"));
        assert_eq!(terminal(Position::new(110, 31)).as_deref(), Some("worker"));
        assert_eq!(
            terminal(Position::new(99, 10)),
            None,
            "the gutter is not a pane"
        );
        assert_eq!(
            terminal(Position::new(10, 40)),
            None,
            "the hint row is not a pane"
        );
        assert_eq!(deck.swap_position_at(area, Position::new(10, 10)), Some(0));
        assert_eq!(
            deck.swap_position_at(Rect::new(0, 0, 84, 22), Position::new(10, 10)),
            None,
            "a hidden stack has no swap target"
        );
    }

    #[test]
    fn dragging_marks_the_source_and_only_valid_drop_target() {
        // The drop target's border chrome is what this reads, so the previews
        // are open.
        let mut state = expanded(4);
        assert!(state.begin_drag(1));
        state.update_drag(Some(0));

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // The held preview is warning-coloured, while the master is lifted as
        // the valid drop target using the accepted accent palette.
        assert_eq!(buffer[(100u16, 0u16)].fg, WARNING);
        assert_eq!(buffer[(0u16, 0u16)].fg, ACCENT);
        assert_eq!(buffer[(5u16, 1u16)].bg, DEMOTED_BG);
    }

    fn text(buffer: &Buffer) -> String {
        let area = buffer.area();
        (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Compares against a committed snapshot. Set `TERMDECK_BLESS` to rewrite
    /// the snapshots after a reviewed visual change.
    fn assert_snapshot(name: &str, buffer: &Buffer) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ui/testdata")
            .join(format!("{name}.txt"));
        let rendered = text(buffer);
        if std::env::var_os("TERMDECK_BLESS").is_some() {
            std::fs::write(&path, format!("{rendered}\n")).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap();
        assert_eq!(rendered, expected.trim_end_matches('\n'), "{name}");
    }

    fn promote(position: usize) -> DeckState {
        let mut state = DeckState::new(4);
        state.apply(
            &ActionCommand::SelectPosition(position),
            &fixture::projects(),
            fixture::NOW,
        );
        state
    }

    fn zoomed() -> DeckState {
        let mut state = DeckState::new(4);
        state.apply(
            &ActionCommand::ToggleZoom,
            &fixture::projects(),
            fixture::NOW,
        );
        state
    }

    /// The export's screen 05: app and worker folded to their title rows.
    /// Since #39 every preview starts folded, so the one open preview is what
    /// this has to ask for; the rendered state is the same as before.
    fn collapsed() -> DeckState {
        let mut state = DeckState::new(4);
        assert!(state.toggle_collapse(1));
        state
    }

    /// A deck with every preview open. Since #39 that is no longer the state
    /// a run starts in, so a test whose subject is open-preview chrome or
    /// geometry asks for it rather than leaning on the default.
    fn expanded(terminals: usize) -> DeckState {
        let mut state = DeckState::new(terminals);
        assert!(state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 0);
        state
    }

    #[test]
    fn collapsed_stack_matches_the_reference_canvas() {
        let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));

        assert_snapshot("collapsed-stack", &buffer);
    }

    /// Screen 05 measured: the one open preview takes both folds' rows, so it
    /// runs from row 0 to row 33 and the strips sit on rows 35 and 37.
    #[test]
    fn folded_previews_hand_their_rows_to_the_one_still_open() {
        let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));

        assert_eq!(buffer[(100u16, 0u16)].symbol(), "┌");
        assert_eq!(
            buffer[(100u16, 33u16)].symbol(),
            "└",
            "12 + 2 x 11 = 34 rows"
        );
        assert_eq!(buffer[(102u16, 35u16)].symbol(), "▸");
        assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
        // The strips sit on the export's #101317, and carry no border.
        assert_eq!(buffer[(100u16, 35u16)].bg, DEMOTED_BG);
        assert!(text(&buffer).contains("2 collapsed · ^g c expand all"));
    }

    /// One fold among three previews: 11 freed rows split 6/5, the remainder
    /// going to the topmost open pane.
    #[test]
    fn freed_rows_split_evenly_with_the_remainder_going_to_the_top() {
        let mut state = DeckState::new(4);
        state.toggle_collapse(1);
        state.toggle_collapse(2);
        assert_eq!(state.collapsed_count(), 1, "worker alone is left folded");

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // 18 rows, then 17, then the strip.
        assert_eq!(buffer[(100u16, 17u16)].symbol(), "└");
        assert_eq!(buffer[(100u16, 19u16)].symbol(), "┌");
        assert_eq!(buffer[(100u16, 35u16)].symbol(), "└");
        assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
    }

    /// Every fold hands over exactly the rows it gave up, so the stack always
    /// ends on the same row whatever the mix.
    #[test]
    fn folding_never_changes_the_height_the_stack_uses() {
        // Walked from the folded default outwards, one expansion at a time.
        let mut state = DeckState::new(4);

        for opened in 0..4 {
            let folds = 3 - opened;
            let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
            // The stack's own columns, on the row the footer always owns.
            let footer = (100..144)
                .map(|x| buffer[(x, 39u16)].symbol())
                .collect::<String>();
            let expected = if folds == 0 {
                "ctrl+g 1-4 promote · j/k cycle".to_owned()
            } else {
                format!("{folds} collapsed · ^g c expand all")
            };
            assert_eq!(
                footer.trim(),
                expected,
                "the footer stays on row 39 with {folds} folded"
            );
            // The row above it stays blank, so nothing has overrun.
            assert_eq!(buffer[(102u16, 38u16)].symbol(), " ");
            if opened < 3 {
                state.toggle_collapse(opened + 1);
            }
        }
    }

    /// Issues #32 and #39: a fresh run has to show the affordance, and since
    /// #39 the state it shows is folded. Every stack pane carries `▸` before
    /// anything is expanded, and the master carries no marker at all.
    #[test]
    fn the_disclosure_markers_are_drawn_before_anything_is_expanded() {
        let (plain, _) = render(&fixture::frontend_active(), &DeckState::new(4), (144, 42));
        let rendered = text(&plain);

        assert!(rendered.contains("▸ 2 backend"), "{rendered}");
        assert!(rendered.contains("▸ 3 app"), "{rendered}");
        assert!(rendered.contains("▸ 4 worker"), "{rendered}");
        assert_eq!(rendered.matches('▸').count(), 3, "one per stacked preview");
        assert!(!rendered.contains("▾"), "nothing is expanded yet");
        assert!(
            !rendered.contains("▸ > 1 frontend"),
            "the master never folds, so it never claims a marker"
        );

        // Expanding every preview is what turns them over.
        let (open, _) = render(&fixture::frontend_active(), &expanded(4), (144, 42));
        let opened = text(&open);
        assert_eq!(opened.matches('▾').count(), 3, "one per stacked preview");
        assert!(!opened.contains("▸"), "nothing is folded any more");
    }

    /// The markers track each preview's own state once folds are in play.
    #[test]
    fn each_marker_states_its_own_panes_fold() {
        let (folded, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
        let rendered = text(&folded);

        assert!(rendered.contains("▾ 2 backend"), "the open pane opens");
        assert!(rendered.contains("▸ 3 app"), "the folded panes close");
        assert!(rendered.contains("▸ 4 worker"), "{rendered}");
    }

    /// An exit outranks the live output, so a folded exited pane says so.
    #[test]
    fn a_folded_pane_states_its_exit_and_its_idle_age() {
        let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
        let rendered = text(&buffer);

        assert!(rendered.contains("▸ 3 app · ✕ · exit 1"), "{rendered}");
        assert!(rendered.contains("▸ 4 worker · ○ · idle 6m"), "{rendered}");
    }

    /// A pane that is neither exited nor idle falls back to its last output.
    #[test]
    fn a_folded_running_pane_shows_its_last_output_line() {
        // Backend is folded from the first frame since #39.
        let state = DeckState::new(4);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // The tail takes what the 44-column strip has left and clips.
        assert!(
            text(&buffer).contains("▸ 2 backend · ● · 12:06:09 /api/termina…"),
            "{}",
            text(&buffer)
        );
    }

    /// The census and the key hint both switch over, and the exited summary
    /// gives up its place, exactly as screen 05 states it.
    #[test]
    fn the_status_bar_counts_the_folds_instead_of_the_stack() {
        let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
        let rendered = text(&buffer);

        assert!(
            rendered.contains("> 1 frontend  ·  1 open  ·  2 collapsed"),
            "{rendered}"
        );
        assert!(!rendered.contains("3 stacked"));
        assert!(
            !rendered.contains("1 exited"),
            "the fold census stands alone"
        );
        assert!(rendered.contains("^g c collapse"));
        assert!(
            !rendered.contains("^g [ scroll"),
            "collapse takes scroll's slot"
        );
    }

    /// A folded strip still answers the hit test, so promoting or swapping it
    /// by pointer keeps working; the caller decides it has nothing to scroll.
    #[test]
    fn a_folded_strip_is_hit_tested_but_has_nothing_to_scroll() {
        let projects = fixture::projects();
        let state = collapsed();
        let deck = Deck {
            workspace: "idp",
            projects: &projects,
            state: &state,
            home: Some(fixture::home()),
            master_ratio: state.master_ratio(),
            now: fixture::NOW,
        };
        let area = Rect::new(0, 0, 144, 42);

        let position = deck.position_at(area, Position::new(110, 35));

        assert_eq!(position, Some(2), "the strip on row 35 is app");
        assert!(state.collapsed(2), "so the wheel skips it");
        assert_eq!(deck.position_at(area, Position::new(110, 10)), Some(1));
        assert!(!state.collapsed(1), "the open preview still scrolls");
    }

    fn deck_for<'a>(projects: &'a [Project], state: &'a DeckState) -> Deck<'a> {
        Deck {
            workspace: "idp",
            projects,
            state,
            home: Some(fixture::home()),
            master_ratio: state.master_ratio(),
            now: fixture::NOW,
        }
    }

    /// The two cells the export draws the disclosure marker in, on an open
    /// preview's top border and at the head of each strip.
    #[test]
    fn the_disclosure_marker_is_hit_tested_in_its_own_two_cells() {
        let projects = fixture::projects();
        let state = collapsed();
        let deck = deck_for(&projects, &state);
        let area = Rect::new(0, 0, 144, 42);
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // An open preview insets its title past the border: columns 103-104.
        assert_eq!(buffer[(103u16, 0u16)].symbol(), "▾");
        assert_eq!(deck.marker_at(area, Position::new(103, 0)), Some(1));
        assert_eq!(deck.marker_at(area, Position::new(104, 0)), Some(1));
        // A strip has no border to inset past: columns 102-103.
        assert_eq!(buffer[(102u16, 35u16)].symbol(), "▸");
        assert_eq!(deck.marker_at(area, Position::new(102, 35)), Some(2));
        assert_eq!(deck.marker_at(area, Position::new(103, 37)), Some(3));

        // Neither the border cell beside it nor the pane body is the marker.
        assert_eq!(deck.marker_at(area, Position::new(102, 0)), None);
        assert_eq!(deck.marker_at(area, Position::new(110, 5)), None);
        // The master carries no marker of its own.
        assert_eq!(deck.marker_at(area, Position::new(3, 0)), None);
    }

    /// The marker cells sit inside the pane, so the gesture has to be consumed
    /// or the same click would also drag or promote.
    #[test]
    fn the_marker_overlaps_the_pane_it_belongs_to() {
        let projects = fixture::projects();
        let state = collapsed();
        let deck = deck_for(&projects, &state);
        let area = Rect::new(0, 0, 144, 42);

        assert_eq!(deck.marker_at(area, Position::new(103, 0)), Some(1));
        assert_eq!(deck.position_at(area, Position::new(103, 0)), Some(1));
    }

    /// Issues #32 and #39: the marker a fresh run draws is the marker a fresh
    /// run can click. Since #39 that is three strips, each of whose two cells
    /// expands its preview from frame one.
    #[test]
    fn the_markers_are_clickable_before_anything_is_expanded() {
        let projects = fixture::projects();
        let mut state = DeckState::new(4);
        let area = Rect::new(0, 0, 144, 42);
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // Three strips at the head of the column, each with its own marker.
        for (position, row) in [(1usize, 0u16), (2, 2), (3, 4)] {
            assert_eq!(buffer[(102u16, row)].symbol(), "▸", "row {row}");
            assert_eq!(
                deck_for(&projects, &state).marker_at(area, Position::new(102, row)),
                Some(position)
            );
            assert_eq!(
                deck_for(&projects, &state).marker_at(area, Position::new(103, row)),
                Some(position)
            );
            // The cell beside the marker's two is not the marker.
            assert_eq!(
                deck_for(&projects, &state).marker_at(area, Position::new(104, row)),
                None
            );
        }

        // Clicking one expands exactly that preview, from a stack with no
        // preview open.
        state.toggle_collapse(2);
        assert_eq!(state.collapsed_count(), 2);
        let (opened, _) = render(&fixture::frontend_active(), &state, (144, 42));
        assert!(text(&opened).contains("▾ 3 app"), "{}", text(&opened));
        assert!(
            text(&opened).contains("▸ 2 backend"),
            "the rest stay folded"
        );
    }

    /// Clicking a strip's marker expands that preview, and the rows it takes
    /// back come out of the pane that grew.
    #[test]
    fn toggling_a_marker_expands_just_that_preview() {
        let projects = fixture::projects();
        let mut state = collapsed();
        let area = Rect::new(0, 0, 144, 42);

        let marker = deck_for(&projects, &state).marker_at(area, Position::new(102, 35));
        assert_eq!(marker, Some(2));
        state.toggle_collapse(marker.unwrap());

        assert_eq!(state.collapsed_count(), 1, "worker stays folded");
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        // Two open previews now split the single fold's 11 freed rows 6/5.
        assert_eq!(buffer[(100u16, 17u16)].symbol(), "└");
        assert_eq!(buffer[(100u16, 19u16)].symbol(), "┌");
        assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
    }

    /// Zoom and the narrow fallback hide the stack, so there is no marker.
    #[test]
    fn a_hidden_stack_offers_no_marker() {
        let projects = fixture::projects();
        let mut state = collapsed();
        state.apply(&ActionCommand::ToggleZoom, &projects, fixture::NOW);
        let area = Rect::new(0, 0, 144, 42);

        assert_eq!(
            deck_for(&projects, &state).marker_at(area, Position::new(103, 0)),
            None
        );

        let folded = collapsed();
        assert_eq!(
            deck_for(&projects, &folded).marker_at(Rect::new(0, 0, 84, 24), Position::new(3, 0)),
            None
        );
    }

    /// The split at a given ratio, as the column the divider is drawn in.
    fn split(ratio: f64) -> DeckState {
        DeckState::new(4).with_master_ratio(ratio)
    }

    /// The column the divider's grip is drawn in, found by the grip rather
    /// than by the line so a pane border is never mistaken for it. (The
    /// scroll track's thumb is the same glyph, but the reference deck's list
    /// always fits, so it never draws one.)
    fn divider_column(buffer: &Buffer) -> Option<u16> {
        (0..buffer.area().width).find(|column| {
            let cell = &buffer[(*column, 20u16)];
            cell.symbol() == "┃" && matches!(cell.fg, HINT | ACCENT)
        })
    }

    /// Issue #41: the split has to be visible before it can be draggable. The
    /// divider takes the gutter column beside the master, so it costs neither
    /// pane a column, and it carries a grip at its middle.
    #[test]
    fn the_divider_is_drawn_in_the_gutter_with_a_grip_to_take_hold_of() {
        let (buffer, _) = reference();

        // The master's own border still ends at 97 and the stack's begins at
        // 100: the divider took the gutter, not a column of either pane.
        assert_eq!(buffer[(97u16, 0u16)].symbol(), "┐");
        assert_eq!(buffer[(100u16, 0u16)].symbol(), " ", "the folded stack");
        for row in [0u16, 10, 39] {
            assert_eq!(buffer[(98u16, row)].symbol(), "│", "row {row}");
            assert_eq!(buffer[(98u16, row)].fg, SEPARATOR);
        }
        // Three cells at the middle of the body say it can be taken hold of.
        for row in 19..=21u16 {
            assert_eq!(buffer[(98u16, row)].symbol(), "┃", "row {row}");
            assert_eq!(buffer[(98u16, row)].fg, HINT);
        }
        // It stops at the body: the blank row and the status row are not it.
        assert_ne!(buffer[(98u16, 40u16)].symbol(), "│");
    }

    /// The gutter belongs to no pane, so holding the divider can never be a
    /// pane drag, a promotion or a marker click.
    #[test]
    fn the_divider_column_is_the_divider_and_nothing_else() {
        let projects = fixture::projects();
        let state = DeckState::new(4);
        let view = deck_for(&projects, &state);

        assert!(view.divider_at(SCREEN, Position::new(98, 20)));
        assert!(view.divider_at(SCREEN, Position::new(98, 0)));
        assert_eq!(view.position_at(SCREEN, Position::new(98, 20)), None);
        assert_eq!(view.swap_position_at(SCREEN, Position::new(98, 20)), None);
        assert_eq!(view.marker_at(SCREEN, Position::new(98, 0)), None);
        // Its neighbours are not it: the master's last column and the scroll
        // track's column both answer for themselves.
        assert!(!view.divider_at(SCREEN, Position::new(97, 20)));
        assert!(!view.divider_at(SCREEN, Position::new(99, 20)));
        assert_eq!(view.position_at(SCREEN, Position::new(97, 20)), Some(0));
        // The wheel over the gutter still pages the list (#34b), because the
        // wheel and the drag are different gestures on the same chrome.
        assert!(view.stack_scroll_at(SCREEN, Position::new(98, 20)));
        // Below the body it is chrome, not the divider.
        assert!(!view.divider_at(SCREEN, Position::new(98, 41)));
    }

    /// Dragging leaves the divider under the pointer: the ratio a column maps
    /// to is the ratio that draws the divider back in that column.
    #[test]
    fn dragging_the_divider_puts_the_split_under_the_pointer() {
        let projects = fixture::projects();
        let mut state = DeckState::new(4);

        // Every column the range reaches, because the ratio a column maps to
        // is a division whose last bit must not move the split a column on.
        for column in 77..=120u16 {
            let ratio = deck_for(&projects, &state)
                .ratio_at(SCREEN, column)
                .expect("the split can move at this width");
            state.set_master_ratio(ratio);

            let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
            assert_eq!(divider_column(&buffer), Some(column), "dragged to {column}");
            // The master ends one column short of the divider, the stack one
            // column past the track: the panes follow the divider exactly.
            assert_eq!(buffer[(column - 1, 0u16)].symbol(), "┐");
        }
    }

    /// The range is the configuration's own, so a drag past either end stops
    /// at the split the configuration would have accepted.
    #[test]
    fn a_drag_past_the_ends_of_the_range_stops_at_them() {
        let projects = fixture::projects();
        let mut state = DeckState::new(4);

        let narrow = deck_for(&projects, &state).ratio_at(SCREEN, 20).unwrap();
        state.set_master_ratio(narrow);
        assert_eq!(state.master_ratio(), super::state::MIN_MASTER_RATIO);
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        assert_eq!(divider_column(&buffer), Some(77), "0.55 of 144");

        let wide = deck_for(&projects, &state).ratio_at(SCREEN, 140).unwrap();
        state.set_master_ratio(wide);
        assert_eq!(state.master_ratio(), super::state::MAX_MASTER_RATIO);
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        assert_eq!(divider_column(&buffer), Some(120), "0.85 of 144");
    }

    /// Keyboard parity (#41): `^g -` / `^g =` reach the same splits the
    /// pointer does, and the screen cannot tell which one moved it.
    #[test]
    fn a_nudged_split_and_a_dragged_split_are_the_same_screen() {
        let projects = fixture::projects();
        let mut nudged = DeckState::new(4);
        nudged.nudge_master_ratio(-1);
        assert_eq!(nudged.master_ratio(), 0.65);

        let mut dragged = DeckState::new(4);
        let ratio = deck_for(&projects, &dragged).ratio_at(SCREEN, 91).unwrap();
        dragged.set_master_ratio(ratio);

        let (by_key, _) = render(&fixture::frontend_active(), &nudged, (144, 42));
        let (by_pointer, _) = render(&fixture::frontend_active(), &dragged, (144, 42));
        assert_eq!(divider_column(&by_key), Some(91));
        assert_eq!(text(&by_key), text(&by_pointer));
    }

    /// While the divider is held it takes the accent, the way a dragged pane
    /// does: the gesture states itself for as long as it lasts.
    #[test]
    fn the_divider_takes_the_accent_while_it_is_held() {
        let mut state = DeckState::new(4);
        state.set_resizing(true);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_eq!(buffer[(98u16, 0u16)].fg, ACCENT);
        assert_eq!(buffer[(98u16, 20u16)].fg, ACCENT);
        assert_eq!(buffer[(98u16, 20u16)].symbol(), "┃");
        // Releasing it hands the divider back to its resting colours.
        state.set_resizing(false);
        let (released, _) = render(&fixture::frontend_active(), &state, (144, 42));
        assert_eq!(released[(98u16, 0u16)].fg, SEPARATOR);
    }

    /// A split that cannot move has no divider to offer: zoom hides the
    /// stack, the narrow fallback drops it, and below `WIDE_COLUMNS` the
    /// export fixes the stack width outright.
    #[test]
    fn a_stack_that_cannot_be_resized_offers_no_divider() {
        let projects = fixture::projects();
        let zoom = zoomed();
        assert!(!deck_for(&projects, &zoom).divider_at(SCREEN, Position::new(98, 20)));

        let state = DeckState::new(4);
        let narrow = Rect::new(0, 0, 84, 22);
        assert!(!deck_for(&projects, &state).divider_at(narrow, Position::new(50, 10)));

        // 110 columns still stacks, but at the export's fixed 34-column
        // stack: there is no ratio to move, so there is no divider.
        let fixed = Rect::new(0, 0, 110, 42);
        assert_eq!(deck_for(&projects, &state).ratio_at(fixed, 74), None);
        assert!(!deck_for(&projects, &state).divider_at(fixed, Position::new(74, 10)));
        let (buffer, _) = render(&fixture::frontend_active(), &state, (110, 42));
        assert_eq!(divider_column(&buffer), None, "no divider to mislead with");
    }

    #[test]
    fn split_dragged_matches_the_divider_affordance() {
        let mut state = split(0.55);
        state.set_resizing(true);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_snapshot("split-dragged", &buffer);
    }

    #[test]
    fn frontend_active_matches_the_reference_canvas() {
        let (buffer, _) = reference();

        assert_snapshot("frontend-active", &buffer);
    }

    #[test]
    fn reference_chrome_carries_the_accepted_palette() {
        let (buffer, cursor) = reference();

        // Master border is the accent.
        assert_eq!(buffer[(0u16, 0u16)].fg, ACCENT);
        // The status row owns the second background value.
        assert_eq!(buffer[(0u16, 41u16)].bg, STATUS_BG);
        // The master cursor sits after the last line of engine-owned output.
        assert_eq!(cursor, Some(Position::new(3, 29)));

        // The preview chrome the export states is a click away since #39, so
        // it is read from an opened stack rather than from the fresh run.
        let (open, _) = render(&fixture::frontend_active(), &expanded(4), (144, 42));
        assert_ne!(open[(100u16, 0u16)].fg, ACCENT, "no preview takes focus");
        // The exited preview's footer rule carries the error colour.
        assert_eq!(open[(103u16, 22u16)].fg, ERROR);
    }

    #[test]
    fn engine_cell_styles_reach_the_buffer() {
        let (buffer, _) = reference();

        let vite = &buffer[(5u16, 6u16)];
        assert_eq!(vite.symbol(), "V");
        assert_eq!(
            vite.fg,
            super::colour(crate::contracts::Rgb {
                red: 0xc8,
                green: 0x98,
                blue: 0xe0,
            })
        );
        assert!(vite.modifier.contains(ratatui::style::Modifier::BOLD));
    }

    #[test]
    fn backend_promoted_matches_the_reference_canvas() {
        let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

        assert_snapshot("backend-promoted", &buffer);
    }

    #[test]
    fn promotion_keeps_numbers_and_puts_the_old_master_in_the_vacated_slot() {
        let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));
        let screen = text(&buffer);

        // Backend keeps its configured number 2 while holding the master.
        assert!(screen.contains("> 2 backend"), "{screen}");
        // Frontend keeps number 1 and lands in the slot backend vacated.
        // Every pane number, in draw order. The disclosure marker is what
        // tells a stacked preview from the caret-marked master; since #39 the
        // demoted master is the open one and the untouched previews are
        // strips, so both markers are collected.
        let mut stack: Vec<_> = ["▾ ", "▸ "]
            .iter()
            .flat_map(|marker| screen.match_indices(marker))
            .filter_map(|(at, marker)| {
                screen[at + marker.len()..]
                    .split(' ')
                    .next()
                    .map(|n| (at, n))
            })
            .collect();
        stack.sort_unstable();
        let numbers: Vec<_> = stack.iter().map(|(_, number)| *number).collect();
        assert_eq!(numbers, ["1", "3", "4"], "{screen}");
        assert!(
            screen.contains("┌─ ▾ 1 frontend"),
            "the demoted master is open"
        );
        assert!(screen.contains("▸ 3 app"), "{screen}");
        assert!(screen.contains("promoted backend · ^g 1 back"), "{screen}");
    }

    #[test]
    fn the_demoted_pane_holds_its_highlight() {
        let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

        // The top preview is the pane frontend was demoted into.
        assert_eq!(buffer[(100u16, 0u16)].fg, DEMOTED_BORDER);
        assert_eq!(buffer[(103u16, 1u16)].bg, DEMOTED_BG);
        // The untouched previews keep the ordinary chrome. They are strips
        // since #39, so the row to read is the first of them.
        // (A strip's own background is the same #101317 the demotion tint
        // uses, so what separates them here is the border and the marker.)
        assert_eq!(buffer[(102u16, 35u16)].symbol(), "▸");
        assert_ne!(buffer[(102u16, 35u16)].fg, DEMOTED_BORDER);
        assert_eq!(
            buffer[(100u16, 35u16)].symbol(),
            " ",
            "a strip has no border"
        );
    }

    #[test]
    fn zoomed_matches_the_reference_canvas() {
        let (buffer, cursor) = render(&fixture::frontend_active(), &zoomed(), (144, 42));

        assert_snapshot("zoomed", &buffer);
        assert_eq!(cursor, Some(Position::new(3, 29)));
    }

    #[test]
    fn zoom_gives_the_master_the_full_width_and_hides_the_stack() {
        let (buffer, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
        let screen = text(&buffer);

        // One pane, spanning every column of the body.
        assert_eq!(screen.matches('┌').count(), 1, "{screen}");
        assert_eq!(buffer[(143u16, 0u16)].symbol(), "┐");
        assert!(screen.contains(" ZOOM "), "{screen}");
        // The hidden terminals stay accounted for in the status row.
        assert!(screen.contains("hidden: 2● 3✕ 4○"), "{screen}");
        assert!(screen.contains("^g z unzoom"), "{screen}");
    }

    /// The spec's zoom + collapse rule: zoom hides the stack, so the folds
    /// it hides say nothing in the status row. Since #39 that is every run.
    #[test]
    fn a_zoomed_deck_states_the_keys_its_hidden_folds_do_not_take() {
        let (buffer, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
        let screen = text(&buffer);

        assert_eq!(zoomed().collapsed_count(), 3, "the folds are still held");
        assert!(screen.contains("^g [ scroll"), "{screen}");
        assert!(!screen.contains("^g c"), "an inert key is not advertised");
        assert!(!screen.contains("collapsed"), "{screen}");
    }

    #[test]
    fn unzooming_restores_the_stack() {
        let mut state = zoomed();
        state.apply(
            &ActionCommand::ToggleZoom,
            &fixture::projects(),
            fixture::NOW,
        );

        let (zoomed, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
        let (restored, _) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_ne!(text(&zoomed), text(&restored));
        assert_eq!(text(&restored), text(&reference().0));
    }

    #[test]
    fn a_deck_with_nothing_exited_reports_all_running() {
        let mut engine = fixture::frontend_active();
        engine.set_status(&TerminalId::new("app"), TerminalStatus::Running);

        // The fold census replaces this one outright, so the stack is opened.
        let (buffer, _) = render(&engine, &expanded(4), (144, 42));

        assert!(text(&buffer).contains("3 stacked  ·  all running"));
    }

    fn scrolling() -> DeckState {
        let mut state = DeckState::new(4);
        state.apply(
            &ActionCommand::ToggleScrollback,
            &fixture::projects(),
            fixture::NOW,
        );
        state
    }

    #[test]
    fn scrollback_matches_the_supplement_canvas() {
        let (buffer, cursor) = render(&fixture::scrolled(), &scrolling(), (144, 42));

        assert_snapshot("scrollback", &buffer);
        // The viewport is detached from the live tail, so no cursor is drawn:
        // the backend keeps the origin it started at.
        assert_eq!(cursor, Some(Position::new(0, 0)));
    }

    #[test]
    fn scrollback_is_a_mode_of_the_master_pane_only() {
        let (buffer, _) = render(&fixture::scrolled(), &scrolling(), (144, 42));
        let screen = text(&buffer);

        // The stack stays visible and live: only zoom hides it. Since #39 it
        // is visible as the three strips a fresh run draws.
        assert_eq!(screen.matches('┌').count(), 1, "{screen}");
        assert_eq!(screen.matches('▸').count(), 3, "{screen}");
        assert!(screen.contains("▸ 2 backend"), "{screen}");
        assert!(screen.contains(" SCROLL "), "{screen}");
        assert!(
            screen.contains("j/k ↑↓ line · pgup/pgdn page · g/G ends · esc live"),
            "{screen}"
        );
        assert!(
            screen.contains("scrollback · esc returns to live"),
            "{screen}"
        );
        // Stated once: the mode tag in the title, the keys in the footer, and
        // the absolute position in the status row.
        assert!(screen.contains("SCROLLBACK  ·  line 2217/2431"), "{screen}");
        assert_eq!(screen.matches("2217/2431").count(), 1, "{screen}");
        // The mode tag is warning, not accent, so it does not read as focus.
        assert_eq!(buffer[(88u16, 0u16)].bg, WARNING);
    }

    #[test]
    fn leaving_scrollback_returns_to_live_output() {
        let mut state = scrolling();
        state.apply(
            &ActionCommand::ToggleScrollback,
            &fixture::projects(),
            fixture::NOW,
        );

        let (live, cursor) = render(&fixture::scrolled(), &state, (144, 42));

        assert!(!text(&live).contains(" SCROLL "));
        assert_eq!(cursor, Some(Position::new(3, 29)));
    }

    #[test]
    fn a_starting_terminal_renders_the_warning_ring_and_its_label() {
        let mut engine = fixture::frontend_active();
        engine.set_status(&TerminalId::new("frontend"), TerminalStatus::Starting);

        let (buffer, _) = render(&engine, &DeckState::new(4), (144, 42));
        let screen = text(&buffer);

        // A wide master names the state it is in.
        assert!(
            screen.contains("> 1 frontend  ·  ~/idp/frontend  ·  ○ starting"),
            "{screen}"
        );
        // The ring is warning, not the accent a running master takes, and the
        // label follows the glyph's colour.
        let ring = (0..144u16)
            .find(|column| buffer[(*column, 0u16)].symbol() == "○")
            .expect("the master title carries the starting ring");
        assert_eq!(buffer[(ring, 0u16)].fg, WARNING);
        assert_eq!(buffer[(ring + 2, 0u16)].fg, WARNING);
        assert_eq!(buffer[(ring + 2, 0u16)].symbol(), "s");

        // A preview shows the ring alone: the border resumes right after it.
        let mut engine = fixture::frontend_active();
        engine.set_status(&TerminalId::new("backend"), TerminalStatus::Starting);
        // An open preview's title, so the stack is opened for it: a strip
        // states its own status in the collapsed-strip tests.
        let screen = text(&render(&engine, &expanded(4), (144, 42)).0);

        assert!(screen.contains("▾ 2 backend · …/backend · ○ ─"), "{screen}");
    }

    #[test]
    fn a_preview_holding_history_says_how_far_back_it_is() {
        let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

        assert!(
            text(&buffer).contains("↑ 214 lines above · ^g ["),
            "{}",
            text(&buffer)
        );
    }

    #[test]
    fn the_status_bar_drops_labels_before_keys_then_collapses() {
        // The unfolded hint set, which is the one this ladder is written for:
        // a folded stack swaps `^g [` for `^g c` (§3.5 of the collapse spec).
        let state = expanded(4);

        let labelled = render(&fixture::frontend_active(), &state, (144, 42)).0;
        let keys_only = render(&fixture::frontend_active(), &state, (128, 42)).0;
        let collapsed = render(&fixture::frontend_active(), &state, (100, 42)).0;

        assert!(text(&labelled).contains("^g j/k switch  ^g 1-4 select"));
        let keys = text(&keys_only);
        assert!(
            keys.contains("^g j/k  ^g 1-4  ^g z  ^g [  ^g ?  ^g q"),
            "{keys}"
        );
        assert!(!keys.contains("switch"), "{keys}");
        assert!(
            text(&collapsed).contains("^g j/k · 1-4 · z · [ · ? · q"),
            "{}",
            text(&collapsed)
        );
    }

    fn opened(action: ActionCommand) -> DeckState {
        let mut state = DeckState::new(4);
        state.apply(&action, &fixture::projects(), fixture::NOW);
        state
    }

    #[test]
    fn help_matches_the_supplement_canvas() {
        let state = opened(ActionCommand::ShowHelp);

        let (buffer, cursor) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_snapshot("help", &buffer);
        // The modal holds focus, so the master draws no cursor.
        assert_eq!(cursor, Some(Position::new(0, 0)));
    }

    /// Issue #32: the pointer has the marker, and the keyboard has the help
    /// overlay. `^g c` is named there under VIEW, beside the zoom it sits with.
    #[test]
    fn the_help_overlay_names_the_collapse_key() {
        let state = opened(ActionCommand::ShowHelp);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        let rendered = text(&buffer);

        assert!(
            rendered.contains("^g c             collapse / expand previews"),
            "{rendered}"
        );
        let view = rendered.find("VIEW").expect("the VIEW section");
        let zoom = rendered.find("^g z  ").expect("the zoom binding");
        let collapse = rendered.find("^g c  ").expect("the collapse binding");
        assert!(view < zoom && zoom < collapse, "collapse follows zoom");
    }

    #[test]
    fn the_help_overlay_takes_the_focus_the_master_gives_up() {
        let state = opened(ActionCommand::ShowHelp);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // 60x24 centred on the canvas: columns 42..101, rows 9..32. The
        // overlay grew a row for the split divider's keys (#41).
        assert_eq!(buffer[(42u16, 9u16)].symbol(), "┌");
        assert_eq!(buffer[(101u16, 32u16)].symbol(), "┘");
        assert_eq!(buffer[(42u16, 9u16)].fg, ACCENT);
        // Focus is singular: the master border is no longer the accent, and
        // the underlay recedes by foreground alone.
        assert_eq!(buffer[(0u16, 0u16)].fg, IDLE_BORDER);
        assert_eq!(buffer[(5u16, 1u16)].fg, UNDER_FG);
        assert_eq!(buffer[(5u16, 1u16)].bg, super::CANVAS);
        // The stack hint row sits between the panes and dims one step further.
        assert_eq!(buffer[(103u16, 39u16)].fg, UNDER_HINT);
        // The status row keeps its colours and states the modal's keys.
        assert_eq!(buffer[(2u16, 41u16)].bg, ACCENT);
        assert!(
            text(&buffer).contains("^g ? help open  ·  esc close"),
            "{}",
            text(&buffer)
        );
    }

    #[test]
    fn quit_matches_the_supplement_canvas() {
        let state = opened(ActionCommand::RequestQuit);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_snapshot("quit", &buffer);
    }

    #[test]
    fn the_quit_confirmation_counts_terminals_and_warns_once() {
        let state = opened(ActionCommand::RequestQuit);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        let screen = text(&buffer);

        // 52x10 centred on the canvas: columns 46..97, rows 16..25.
        assert_eq!(buffer[(46u16, 16u16)].symbol(), "┌");
        assert_eq!(buffer[(97u16, 25u16)].symbol(), "┘");
        // The count, not the names.
        assert!(screen.contains("4 terminals will be closed."), "{screen}");
        assert!(!screen.contains("frontend, backend"), "{screen}");
        // A destructive modal keeps the accent border and carries the warning
        // colour on its consequence line alone.
        assert_eq!(buffer[(46u16, 16u16)].fg, ACCENT);
        // The warning colour is confined to that one line: the count above it
        // stays an ordinary key colour.
        assert_eq!(buffer[(49u16, 21u16)].fg, WARNING);
        assert_eq!(buffer[(49u16, 20u16)].fg, super::PREVIEW_FG);
        assert!(screen.contains("y  quit         n  cancel         esc  cancel"));
        assert!(
            screen.contains("confirm quit  ·  y quit  n cancel"),
            "{screen}"
        );
    }

    #[test]
    fn a_modal_does_not_disturb_the_interface_it_recedes() {
        let mut state = opened(ActionCommand::ShowHelp);
        assert!(state.close_modal());

        let (restored, _) = render(&fixture::frontend_active(), &state, (144, 42));

        assert_eq!(text(&restored), text(&reference().0));
    }

    #[test]
    fn a_modal_over_a_narrow_deck_still_fits_inside_the_canvas() {
        let state = opened(ActionCommand::RequestQuit);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (84, 22));

        // 52 columns fit in 84; the box is centred and the status row is clear.
        assert_eq!(buffer[(16u16, 6u16)].symbol(), "┌");
        assert!(text(&buffer).contains("confirm quit"));
    }

    #[test]
    fn a_canvas_smaller_than_the_overlay_clips_it_to_the_canvas() {
        let state = opened(ActionCommand::ShowHelp);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (20, 8));

        // Clipped to the canvas rather than drawn past its edge.
        assert_eq!(buffer[(0u16, 0u16)].symbol(), "┌");
        assert_eq!(buffer[(19u16, 7u16)].symbol(), "┘");
    }

    #[test]
    fn narrow_matches_the_reference_canvas() {
        let (buffer, _) = render(&fixture::frontend_active(), &DeckState::new(4), (84, 22));

        assert_snapshot("narrow", &buffer);
    }

    #[test]
    fn below_a_usable_preview_width_the_stack_becomes_a_pane_strip() {
        let (buffer, _) = render(&fixture::frontend_active(), &DeckState::new(4), (84, 22));
        let screen = text(&buffer);

        assert_eq!(screen.matches('┌').count(), 1, "{screen}");
        assert!(
            screen.starts_with("  1 frontend  2 backend · 3 app ✕1 · 4 worker ○"),
            "{screen}"
        );
        assert!(screen.contains("stack hidden"), "{screen}");
        assert!(screen.contains("^g j/k · 1-4 · z · [ · ? · q"), "{screen}");
        // The active chip and the warning both carry their accepted colours.
        assert_eq!(buffer[(1u16, 0u16)].bg, ACCENT);
        assert_eq!(buffer[(14u16, 0u16)].bg, CHIP_BG);
        assert_eq!(buffer[(19u16, 21u16)].fg, WARNING);
    }

    #[test]
    fn one_column_above_the_fallback_still_stacks() {
        // Counted in preview boxes, so the stack is opened; the threshold
        // itself never reads the folds.
        let state = expanded(4);

        let (narrow, _) = render(&fixture::frontend_active(), &state, (99, 30));
        let (stacked, _) = render(&fixture::frontend_active(), &state, (100, 30));

        assert_eq!(text(&narrow).matches('┌').count(), 1);
        // 34 stack columns hold previews, which the export fixes below 120.
        assert!(text(&stacked).matches('┌').count() > 1);
        assert_eq!(stacked[(63u16, 0u16)].symbol(), "┐");
    }

    /// A synthetic workspace of `count` terminals.
    ///
    /// The configuration still caps a workspace at four until #34a lands, so a
    /// stack longer than the reference deck is built here rather than loaded
    /// from a workspace file. The renderer has never known the cap.
    fn synthetic(count: usize) -> Vec<Project> {
        (1..=count)
            .map(|number| Project {
                terminal: TerminalId::new(format!("t{number}")),
                path: std::path::PathBuf::from(fixture::HOME).join(format!("idp/t{number}")),
                command: vec!["sh".to_owned()],
            })
            .collect()
    }

    /// Renders a synthetic deck of any length on the reference canvas.
    fn render_long(projects: &[Project], state: &DeckState, size: (u16, u16)) -> Buffer {
        let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
        let deck = deck_for(projects, state);
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| deck.render(&engine as &dyn TerminalEngine, frame))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The reference canvas, as a hit-testing area.
    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        width: 144,
        height: 42,
    };

    /// Eight terminals leave seven previews for a column that holds three: the
    /// window states where it starts and how much of the list it is showing.
    #[test]
    fn a_long_list_fills_the_column_with_whole_previews() {
        let projects = synthetic(8);
        // Whole previews, so the window arithmetic is the open-preview one;
        // `folding_inside_a_long_list_lets_more_previews_into_the_window`
        // covers what folds do to it.
        let state = expanded(8);

        let window = deck_for(&projects, &state).stack_window(SCREEN);

        assert_eq!(window.offset, 0);
        assert_eq!(window.visible, 3, "39 budget rows hold 3 x (12 + 1)");
        assert_eq!(window.total, 7);
        assert!(window.overflows());
    }

    /// The four previews that fit the reference deck are the whole list, so
    /// nothing scrolls and screens 01-04 are untouched.
    #[test]
    fn a_list_that_fits_does_not_scroll() {
        let projects = fixture::projects();
        // Four open previews are the tightest list that still fits.
        let mut state = expanded(4);

        let window = deck_for(&projects, &state).stack_window(SCREEN);
        assert!(!window.overflows());
        assert_eq!(window.scrolled(1), 0, "there is nothing below to reach");

        // A stored offset it cannot honour is still ignored by the renderer.
        state.set_stack_offset(2);
        let buffer = render_long(&projects, &state, (144, 42));
        assert_eq!(
            buffer[(99u16, 5u16)].symbol(),
            " ",
            "no track in the gutter"
        );
        assert!(!text(&buffer).contains("more"));
    }

    /// The window stops when its last preview is the list's last preview, so
    /// the column never scrolls into empty space.
    #[test]
    fn paging_stops_with_the_last_preview_in_view() {
        let projects = synthetic(8);
        let mut state = expanded(8);
        let window = deck_for(&projects, &state).stack_window(SCREEN);

        assert_eq!(window.paged(1), 3, "one page is one window of previews");
        assert_eq!(window.scrolled(9), 4, "7 previews less the 3 on screen");
        assert_eq!(window.scrolled(-1), 0);

        state.set_stack_offset(window.paged(1));
        let scrolled = deck_for(&projects, &state).stack_window(SCREEN);
        assert_eq!(scrolled.offset, 3);
        assert_eq!(scrolled.paged(-1), 0);
        assert_eq!(scrolled.paged(1), 4);
    }

    /// Promotion, drag and the disclosure markers all address a configured
    /// position, so they must read the window rather than the list.
    #[test]
    fn hit_testing_follows_the_scrolled_window() {
        let projects = synthetic(8);
        let mut state = expanded(8);
        state.toggle_collapse(6);
        state.set_stack_offset(2);
        let view = deck_for(&projects, &state);

        // The stack is [1..=7]; the window starts at its third preview.
        assert_eq!(view.position_at(SCREEN, Position::new(110, 5)), Some(3));
        assert_eq!(view.position_at(SCREEN, Position::new(110, 18)), Some(4));
        assert_eq!(view.position_at(SCREEN, Position::new(110, 31)), Some(5));
        assert_eq!(
            view.swap_position_at(SCREEN, Position::new(110, 5)),
            Some(3)
        );
        assert_eq!(
            view.terminal_at(SCREEN, Position::new(110, 5))
                .map(ToString::to_string)
                .as_deref(),
            Some("t4")
        );
        // The marker cells belong to whichever preview the window put there.
        assert_eq!(view.marker_at(SCREEN, Position::new(103, 0)), Some(3));
        assert_eq!(view.marker_at(SCREEN, Position::new(103, 13)), Some(4));
    }

    /// Issue #39: the fresh run is a column of strips, and every pointer
    /// gesture still resolves on it — a strip's body promotes, drags and
    /// names its terminal, its two marker cells expand it, and the empty
    /// column below the strips is still the list's own chrome.
    #[test]
    fn a_fresh_folded_stack_answers_every_pointer_gesture() {
        let projects = fixture::projects();
        let state = DeckState::new(4);
        let view = deck_for(&projects, &state);

        for (position, row, terminal) in
            [(1usize, 0u16, "backend"), (2, 2, "app"), (3, 4, "worker")]
        {
            assert_eq!(
                view.position_at(SCREEN, Position::new(120, row)),
                Some(position)
            );
            assert_eq!(
                view.swap_position_at(SCREEN, Position::new(120, row)),
                Some(position)
            );
            assert_eq!(
                view.terminal_at(SCREEN, Position::new(120, row))
                    .map(ToString::to_string)
                    .as_deref(),
                Some(terminal)
            );
            assert_eq!(
                view.marker_at(SCREEN, Position::new(102, row)),
                Some(position)
            );
            assert_eq!(
                view.marker_at(SCREEN, Position::new(103, row)),
                Some(position)
            );
        }
        // The blank column the folds leave below them belongs to the list.
        assert!(view.stack_scroll_at(SCREEN, Position::new(120, 20)));
        assert!(
            !view.stack_scroll_at(SCREEN, Position::new(120, 0)),
            "a strip"
        );
        // Three strips are the whole list, so there is nothing to page to.
        assert!(!view.stack_window(SCREEN).overflows());
    }

    /// Folds buy the window room, so it takes many more strips than previews
    /// before the list overflows — but it still pages when it does.
    #[test]
    fn a_folded_list_longer_than_the_column_still_pages() {
        let projects = synthetic(24);
        let mut state = DeckState::new(24);

        let window = deck_for(&projects, &state).stack_window(SCREEN);
        assert_eq!(window.visible, 19, "39 budget rows hold 19 x (1 + 1)");
        assert_eq!(window.total, 23);
        assert!(window.overflows());

        state.set_stack_offset(window.paged(1));
        let scrolled = deck_for(&projects, &state).stack_window(SCREEN);
        assert_eq!(scrolled.offset, 4, "the last window that ends on the list");
        let rendered = text(&render_long(&projects, &state, (144, 42)));
        assert!(rendered.contains("↑ 4 more"), "{rendered}");
        assert!(rendered.contains("▸ 24 t24"), "{rendered}");
    }

    /// The wheel over a preview is that preview's (#25), so the list is paged
    /// from the column's own chrome instead.
    #[test]
    fn the_stack_chrome_is_where_the_wheel_pages_the_list() {
        let projects = synthetic(8);
        let state = expanded(8);
        let view = deck_for(&projects, &state);

        assert!(view.stack_scroll_at(SCREEN, Position::new(99, 5)), "gutter");
        assert!(view.stack_scroll_at(SCREEN, Position::new(120, 12)), "gap");
        assert!(
            view.stack_scroll_at(SCREEN, Position::new(120, 39)),
            "footer"
        );
        assert!(
            !view.stack_scroll_at(SCREEN, Position::new(110, 5)),
            "preview"
        );
        assert!(
            !view.stack_scroll_at(SCREEN, Position::new(10, 10)),
            "master"
        );
        assert!(
            !view.stack_scroll_at(SCREEN, Position::new(120, 41)),
            "the status row is not the stack"
        );
    }

    /// A folded preview keeps its place in the list and costs one row, so the
    /// window reaches further down the list without the column growing.
    #[test]
    fn folding_inside_a_long_list_lets_more_previews_into_the_window() {
        let projects = synthetic(8);
        let mut state = expanded(8);
        assert!(state.toggle_collapse(1));
        assert!(state.toggle_collapse(2));

        let window = deck_for(&projects, &state).stack_window(SCREEN);
        assert_eq!(window.visible, 4, "two strips buy room for a fourth pane");

        let buffer = render_long(&projects, &state, (144, 42));
        let rendered = text(&buffer);
        assert!(rendered.contains("▸ 2 t2"), "the folds stay in place");
        assert!(rendered.contains("▸ 3 t3"));
        assert!(rendered.contains("▾ 4 t4"));
        // Two strips and two open previews, and the footer still on row 39.
        assert_eq!(buffer[(100u16, 0u16)].symbol(), " ");
        assert_eq!(buffer[(102u16, 0u16)].symbol(), "▸");
        assert_eq!(buffer[(102u16, 2u16)].symbol(), "▸");
        assert_eq!(buffer[(100u16, 4u16)].symbol(), "┌");
    }

    /// The column never overruns its footer row, whatever the mix of folds and
    /// whatever the window is showing.
    #[test]
    fn a_scrolled_column_never_overruns_its_footer() {
        let projects = synthetic(9);
        // Walked from every preview open to every preview folded, so both
        // ends of the #39 default are covered.
        let mut state = expanded(9);

        for step in 0..9 {
            for offset in 0..8 {
                state.set_stack_offset(offset);
                let buffer = render_long(&projects, &state, (144, 42));
                assert_eq!(
                    buffer[(102u16, 38u16)].symbol(),
                    " ",
                    "row 38 stays blank at offset {offset} with {step} folded"
                );
            }
            if step < 8 {
                state.toggle_collapse(step + 1);
            }
        }
    }

    /// The footer names what the window hides at each end, and the keys that
    /// move it while it has the columns for them.
    #[test]
    fn the_footer_states_what_the_window_hides() {
        let projects = synthetic(8);
        // Open previews, so the window hides four of the seven.
        let mut state = expanded(8);

        let head = text(&render_long(&projects, &state, (144, 42)));
        assert!(head.contains("↓ 4 more · ^g pgup/pgdn"), "{head}");

        state.set_stack_offset(2);
        let middle = text(&render_long(&projects, &state, (144, 42)));
        assert!(
            middle.contains("↑ 2 more · ↓ 2 more · ^g pgup/pgdn"),
            "{middle}"
        );

        state.set_stack_offset(4);
        let tail = text(&render_long(&projects, &state, (144, 42)));
        assert!(tail.contains("↑ 4 more"), "{tail}");
        assert!(!tail.contains("↓"), "nothing is left below: {tail}");

        // The narrower column drops the keys before the counts.
        state.set_stack_offset(2);
        let compact = text(&render_long(&projects, &state, (110, 42)));
        assert!(compact.contains("↑ 2 more · ↓ 2 more"), "{compact}");
        assert!(!compact.contains("pgup"), "{compact}");
    }

    /// A hidden preview is the one thing collapse never announces elsewhere,
    /// so it takes the footer while the fold census keeps the status row.
    #[test]
    fn hidden_previews_outrank_the_fold_census_in_the_footer() {
        let projects = synthetic(8);
        let mut state = expanded(8);
        state.toggle_collapse(1);

        let rendered = text(&render_long(&projects, &state, (144, 42)));

        assert!(rendered.contains("more"), "{rendered}");
        assert!(!rendered.contains("expand all"));
        assert!(
            rendered.contains("1 collapsed"),
            "the status row still says"
        );
    }

    /// The track sits in the gutter, so it takes no columns from the previews.
    /// Its thumb is the window's share of the list and reaches each end.
    #[test]
    fn the_gutter_carries_a_track_while_the_list_is_longer_than_the_column() {
        let projects = synthetic(8);
        // Three open previews of seven is the window the thumb is sized for.
        let mut state = expanded(8);

        let head = render_long(&projects, &state, (144, 42));
        let track = |buffer: &Buffer| {
            (0..39u16)
                .map(|row| buffer[(99u16, row)].symbol().to_owned())
                .collect::<String>()
        };
        let thumb = |buffer: &Buffer| {
            let rows: Vec<u16> = (0..39u16)
                .filter(|row| buffer[(99u16, *row)].symbol() == "┃")
                .collect();
            (rows[0], rows[rows.len() - 1])
        };
        assert_eq!(track(&head).matches('┃').count(), 17, "3 of 7 previews");
        assert_eq!(thumb(&head).0, 0, "the window is at the head of the list");
        assert_eq!(head[(99u16, 0u16)].fg, HINT);
        assert_eq!(head[(99u16, 38u16)].fg, IDLE_BORDER);
        // The pane beside it keeps every column it had.
        assert_eq!(head[(100u16, 0u16)].symbol(), "┌");

        state.set_stack_offset(4);
        let tail = render_long(&projects, &state, (144, 42));
        assert_eq!(thumb(&tail).1, 38, "the window is at the end of the list");
    }

    #[test]
    fn too_few_rows_for_a_preview_also_falls_back() {
        // Counted in preview boxes, so the stack is opened; the fallback is
        // decided by the row budget a whole preview needs, not by the folds.
        let state = expanded(4);

        let (short, _) = render(&fixture::frontend_active(), &state, (144, 15));
        let (tall, _) = render(&fixture::frontend_active(), &state, (144, 16));

        assert_eq!(text(&short).matches('┌').count(), 1);
        assert_eq!(text(&tall).matches('┌').count(), 2);
    }
}
