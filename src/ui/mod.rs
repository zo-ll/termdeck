//! Presentation layer for the master-and-preview-stack interface.
//!
//! This module renders one screen from owned application types plus the
//! interface's own [`DeckState`]. It has no event loop, no key decoding, and
//! no terminal backend of its own: the caller supplies a Ratatui [`Frame`], a
//! [`TerminalEngine`] to read from, and the state to render.
//!
//! The public surface is the renderer, its state, its key handling, and the
//! frozen contracts only. Reference fixtures and `FakeEngine` are test-only and
//! never reach a release build.

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
pub use state::{DeckState, Modal};

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
/// Help overlay size, from the supplement: 60 columns by 21 rows.
const HELP_SIZE: (u16, u16) = (60, 21);
/// Quit confirmation size, from the supplement: 52 columns by 10 rows.
const QUIT_SIZE: (u16, u16) = (52, 10);
/// Column the help overlay's descriptions start at.
const HELP_KEYS: usize = 17;

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
                let mut top = body.y;
                for position in self.state.stack().iter().copied() {
                    if top + preview > body.y + body.height.saturating_sub(2) {
                        break;
                    }
                    let pane = Rect {
                        x: master.width + GUTTER,
                        y: top,
                        width: stack,
                        height: preview,
                    };
                    if pane.contains(pointer) {
                        return self.projects.get(position).map(|_| position);
                    }
                    top += preview + 1;
                }
                None
            }
            _ => None,
        }
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
        let mut drawn = Vec::new();
        let mut top = area.y;
        for position in self.state.stack().iter().copied() {
            let Some(project) = self.projects.get(position) else {
                continue;
            };
            if top + preview > area.y + area.height.saturating_sub(2) {
                break;
            }
            let kind = if self.state.dragged() == Some(position) {
                Pane::Preview.dragging(false)
            } else if self.state.drag_target() == Some(position) {
                Pane::Preview.dragging(true)
            } else if demoted == Some(position) {
                Pane::Demoted
            } else {
                Pane::Preview
            };
            let rect = Rect {
                y: top,
                height: preview,
                ..area
            };
            self.draw_pane(engine, buffer, rect, project, position, kind);
            drawn.push(rect);
            top += preview + 1;
        }
        buffer.set_line(
            area.x + 1,
            area.y + area.height - 1,
            &Line::from(self.stack_hints(demoted)),
            area.width - 1,
        );
        drawn
    }

    /// The stack footer names the promotion that just happened and the key
    /// that undoes it; otherwise it states the promotion keys.
    fn stack_hints(&self, demoted: Option<usize>) -> Vec<Span<'static>> {
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
        vec![
            self.hints(&KEY_HINTS, layout, true),
            self.hints(&KEY_HINTS, layout, false),
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
            // Zoom names its own exit and takes the accent while it is on.
            let zoom = layout == Layout::Zoom && *name == "^g z";
            spans.push(Span::styled(
                (*name).to_owned(),
                if zoom {
                    Style::new().fg(ACCENT).bg(STATUS_BG)
                } else {
                    key
                },
            ));
            if labels {
                spans.push(Span::styled(
                    format!(" {}", if zoom { "unzoom" } else { *label }),
                    if zoom { key } else { hint },
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
const HELP: [(&str, &str); 13] = [
    ("NAVIGATE", ""),
    ("^g j  ^g k", "promote next / previous"),
    ("^g ↓  ^g ↑", "same, with arrow keys"),
    ("^g 1 … ^g 4", "promote by configured position"),
    ("VIEW", ""),
    ("^g z", "toggle zoom"),
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

/// At or above [`WIDE_COLUMNS`] the stack takes the ceiling of the non-master
/// share so the master never overruns: at 144 columns with the default 0.70
/// this is the design's 44. Below that the export fixes it instead.
fn stack_width(width: u16, master_ratio: f64) -> u16 {
    if width < WIDE_COLUMNS {
        return COMPACT_STACK;
    }
    let stack = (f64::from(width) * (1.0 - master_ratio)).ceil() as u16;
    stack.clamp(1, width.saturating_sub(GUTTER + 1))
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
        ACCENT, CHIP_BG, DEMOTED_BG, DEMOTED_BORDER, Deck, DeckState, ERROR, IDLE_BORDER,
        STATUS_BG, UNDER_FG, UNDER_HINT, WARNING, fixture,
    };
    use crate::{
        contracts::{ActionCommand, TerminalEngine, TerminalId, TerminalStatus},
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
            master_ratio: 0.70,
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

    #[test]
    fn pane_hit_testing_follows_the_rendered_layout() {
        let projects = fixture::projects();
        let deck = Deck {
            workspace: "idp",
            projects: &projects,
            state: &DeckState::new(4),
            home: Some(fixture::home()),
            master_ratio: 0.70,
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
        let mut state = DeckState::new(4);
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

    #[test]
    fn frontend_active_matches_the_reference_canvas() {
        let (buffer, _) = reference();

        assert_snapshot("frontend-active", &buffer);
    }

    #[test]
    fn reference_chrome_carries_the_accepted_palette() {
        let (buffer, cursor) = reference();

        // Master border is the accent; the preview border is not.
        assert_eq!(buffer[(0u16, 0u16)].fg, ACCENT);
        assert_ne!(buffer[(100u16, 0u16)].fg, ACCENT);
        // The status row owns the second background value.
        assert_eq!(buffer[(0u16, 41u16)].bg, STATUS_BG);
        // The exited preview's footer rule carries the error colour.
        assert_eq!(buffer[(103u16, 22u16)].fg, ERROR);
        // The master cursor sits after the last line of engine-owned output.
        assert_eq!(cursor, Some(Position::new(3, 29)));
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
        // Every pane number, in draw order, skipping the caret-marked master.
        let stack: Vec<_> = screen
            .match_indices("┌─ ")
            .filter_map(|(at, marker)| screen[at + marker.len()..].split(' ').next())
            .filter(|number| *number != ">")
            .collect();
        assert_eq!(stack, ["1", "3", "4"], "{screen}");
        assert!(screen.contains("promoted backend · ^g 1 back"), "{screen}");
    }

    #[test]
    fn the_demoted_pane_holds_its_highlight() {
        let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

        // The top preview is the pane frontend was demoted into.
        assert_eq!(buffer[(100u16, 0u16)].fg, DEMOTED_BORDER);
        assert_eq!(buffer[(103u16, 1u16)].bg, DEMOTED_BG);
        // The untouched previews keep the ordinary preview chrome.
        assert_ne!(buffer[(100u16, 13u16)].fg, DEMOTED_BORDER);
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

        let (buffer, _) = render(&engine, &DeckState::new(4), (144, 42));

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

        // The stack stays visible and live: only zoom hides it.
        assert_eq!(screen.matches('┌').count(), 4, "{screen}");
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
        let screen = text(&render(&engine, &DeckState::new(4), (144, 42)).0);

        assert!(screen.contains("2 backend · …/backend · ○ ───"), "{screen}");
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
        let state = DeckState::new(4);

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

    #[test]
    fn the_help_overlay_takes_the_focus_the_master_gives_up() {
        let state = opened(ActionCommand::ShowHelp);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

        // 60x21 centred on the canvas: columns 42..101, rows 10..30.
        assert_eq!(buffer[(42u16, 10u16)].symbol(), "┌");
        assert_eq!(buffer[(101u16, 30u16)].symbol(), "┘");
        assert_eq!(buffer[(42u16, 10u16)].fg, ACCENT);
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
        let state = DeckState::new(4);

        let (narrow, _) = render(&fixture::frontend_active(), &state, (99, 30));
        let (stacked, _) = render(&fixture::frontend_active(), &state, (100, 30));

        assert_eq!(text(&narrow).matches('┌').count(), 1);
        // 34 stack columns hold previews, which the export fixes below 120.
        assert!(text(&stacked).matches('┌').count() > 1);
        assert_eq!(stacked[(63u16, 0u16)].symbol(), "┐");
    }

    #[test]
    fn too_few_rows_for_a_preview_also_falls_back() {
        let state = DeckState::new(4);

        let (short, _) = render(&fixture::frontend_active(), &state, (144, 15));
        let (tall, _) = render(&fixture::frontend_active(), &state, (144, 16));

        assert_eq!(text(&short).matches('┌').count(), 1);
        assert_eq!(text(&tall).matches('┌').count(), 2);
    }
}
