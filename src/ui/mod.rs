//! Presentation layer for the master-and-preview-stack interface.
//!
//! This module renders one static screen from owned application types. It has
//! no event loop, no input handling, and no terminal backend of its own: the
//! caller supplies a Ratatui [`Frame`] and a [`TerminalEngine`] to read from.
//!
//! The public surface is the renderer and the frozen contracts only. Reference
//! fixtures and `FakeEngine` are test-only and never reach a release build.

#[cfg(test)]
mod fixture;

use std::path::Path;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};

use crate::contracts::{
    CellContent, CellStyle, Cursor, Elapsed, Project, Rgb, TerminalEngine, TerminalFrame,
    TerminalMetadata, TerminalStatus, Timestamp,
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
}

use palette::*;

/// Columns between the master and the preview stack.
const GUTTER: u16 = 2;
/// Rows in one preview, including its borders.
const PREVIEW_HEIGHT: u16 = 12;
/// Horizontal pane padding, inside the border.
const PADDING: u16 = 2;
/// Output within this window counts as recent activity.
const ACTIVE_WINDOW: Elapsed = Elapsed { millis: 30_000 };
/// Cells in the activity meter.
const METER_CELLS: u64 = 6;

/// Everything the renderer needs that does not belong to the engine.
pub struct Deck<'a> {
    pub workspace: &'a str,
    pub projects: &'a [Project],
    /// Index into `projects` of the terminal currently holding the master pane.
    pub active: usize,
    /// Home directory used to abbreviate project paths.
    pub home: Option<&'a Path>,
    /// Share of the width given to the master pane.
    pub master_ratio: f64,
    /// Wall clock used to age exit timestamps.
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
        let stack_width = stack_width(area.width, self.master_ratio);
        let master = Rect {
            width: area.width - GUTTER - stack_width,
            ..body
        };
        let stack = Rect {
            x: master.width + GUTTER,
            width: stack_width,
            ..body
        };

        if let Some(project) = self.projects.get(self.active) {
            self.draw_pane(
                engine,
                frame.buffer_mut(),
                master,
                project,
                self.active,
                true,
            );
        }
        self.stack(engine, frame.buffer_mut(), stack);
        self.status_row(
            engine,
            frame.buffer_mut(),
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
        );
        self.place_cursor(engine, frame, master);
    }

    fn stack(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, area: Rect) {
        let mut top = area.y;
        for (position, project) in self.projects.iter().enumerate() {
            if position == self.active {
                continue;
            }
            if top + PREVIEW_HEIGHT > area.y + area.height.saturating_sub(2) {
                break;
            }
            let pane = Rect {
                y: top,
                height: PREVIEW_HEIGHT,
                ..area
            };
            self.draw_pane(engine, buffer, pane, project, position, false);
            top += PREVIEW_HEIGHT + 1;
        }
        let hints = Line::from(vec![
            Span::styled("ctrl+g ", Style::new().fg(HINT)),
            Span::styled("1-4", Style::new().fg(PREVIEW_FG)),
            Span::styled(" promote · ", Style::new().fg(HINT)),
            Span::styled("j/k", Style::new().fg(PREVIEW_FG)),
            Span::styled(" cycle", Style::new().fg(HINT)),
        ]);
        buffer.set_line(area.x + 1, area.y + area.height - 1, &hints, area.width - 1);
    }

    /// Draws one bordered pane: border, title chrome, terminal cells, footer.
    fn draw_pane(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        project: &Project,
        position: usize,
        master: bool,
    ) {
        let id = &project.terminal;
        let status = engine
            .status(id)
            .cloned()
            .unwrap_or(TerminalStatus::Starting);
        let metadata = engine.metadata(id).cloned().unwrap_or_default();
        let exited = matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        );

        let border = if master { ACCENT } else { IDLE_BORDER };
        Block::bordered()
            .border_style(Style::new().fg(border).bg(CANVAS))
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
        let border_style = Style::new().fg(border).bg(CANVAS);
        let slot = self.right_slot(&status, &metadata, master);
        let slot_width: u16 = slot.iter().map(|span| span.width() as u16).sum();
        let title = self.title(
            project,
            position,
            &status,
            &metadata,
            master,
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

        let default_fg = if master {
            MASTER_FG
        } else if exited {
            MUTED
        } else {
            PREVIEW_FG
        };
        let mut viewport = content;
        if exited {
            viewport.height = content.height.saturating_sub(2);
            self.exit_footer(buffer, content, &status, &metadata);
        }
        if let Some(terminal) = engine.frame(id) {
            draw_terminal(buffer, viewport, terminal, default_fg);
        }
    }

    /// `> {n} {name} · {cwd} · {dot} {state} · {cmd}` for the master, and
    /// `{n} {name} · {cwd} · {dot}` for a preview.
    fn title(
        &self,
        project: &Project,
        position: usize,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        master: bool,
        budget: u16,
    ) -> Vec<Span<'static>> {
        let number = position + 1;
        let name = project.terminal.to_string();
        let separator = if master { "  ·  " } else { " · " };
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
                Style::new().fg(PREVIEW_FG),
            ));
        }
        spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));

        let fixed: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let tail = separator.chars().count() + glyph.chars().count();
        let path_budget = (budget as usize).saturating_sub(fixed + tail);
        let path = self.path(&project.path, master, path_budget);
        spans.push(Span::styled(
            path,
            Style::new().fg(if master { PREVIEW_FG } else { MUTED }),
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
        if let Some(label) = status_label(status, master) {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::new().fg(glyph_colour),
            ));
        }
        if master {
            spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));
            spans.push(Span::styled(
                project.command.join(" "),
                Style::new().fg(MUTED),
            ));
        }
        spans
    }

    /// Master shows the complete path; previews keep the repository name only.
    fn path(&self, path: &Path, master: bool, budget: usize) -> String {
        let full = abbreviate(path, self.home);
        if master && full.chars().count() <= budget {
            return full;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(full);
        clip(&format!("…/{name}"), budget)
    }

    /// The right-aligned slot: activity meter, idle age, exit age, MASTER tag.
    fn right_slot(
        &self,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        master: bool,
    ) -> Vec<Span<'static>> {
        let colour = if master { ACCENT } else { HINT };
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
        let mut spans = vec![Span::styled(text, Style::new().fg(colour))];
        if master {
            spans.push(Span::styled(" MASTER", Style::new().fg(ACCENT)));
        }
        spans
    }

    /// Rule plus `exited · code 1 · {time} · r restart` at the pane foot.
    fn exit_footer(
        &self,
        buffer: &mut Buffer,
        content: Rect,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
    ) {
        if content.height < 2 {
            return;
        }
        let rule = "─".repeat(content.width as usize);
        buffer.set_line(
            content.x,
            content.y + content.height - 2,
            &Line::styled(rule, Style::new().fg(ERROR)),
            content.width,
        );
        let reason = match status {
            TerminalStatus::Exited { code: Some(code) } => format!("exited · code {code}"),
            TerminalStatus::Exited { code: None } => "exited".to_owned(),
            _ => "failed".to_owned(),
        };
        let mut spans = vec![Span::styled(reason, Style::new().fg(ERROR))];
        if let Some(at) = metadata.last_exit_at {
            spans.push(Span::styled(
                format!(" · {} · ", clock(at)),
                Style::new().fg(HINT),
            ));
        } else {
            spans.push(Span::styled(" · ", Style::new().fg(HINT)));
        }
        spans.push(Span::styled("r restart", Style::new().fg(PREVIEW_FG)));
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::from(spans),
            content.width,
        );
    }

    fn status_row(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, area: Rect) {
        Block::new()
            .style(Style::new().bg(STATUS_BG))
            .render(area, buffer);
        let total = self.projects.len();
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
        let active = self
            .projects
            .get(self.active)
            .map(|project| format!("> {} {}", self.active + 1, project.terminal))
            .unwrap_or_default();

        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let mut left = vec![
            Span::styled(
                format!(" {} ", self.workspace),
                Style::new()
                    .fg(CANVAS)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {total} terminal{}   ", plural(total)), hint),
            Span::styled(active, Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
            Span::styled(format!("  ·  {} stacked", total.saturating_sub(1)), hint),
        ];
        if exited > 0 {
            left.push(Span::styled("  ·  ", hint));
            left.push(Span::styled(
                format!("{exited} exited"),
                Style::new().fg(ERROR).bg(STATUS_BG),
            ));
        }
        let left = Line::from(left);
        buffer.set_line(area.x + PADDING, area.y, &left, area.width);

        let mut right = Vec::new();
        for (index, (key, label)) in KEY_HINTS.iter().enumerate() {
            if index > 0 {
                right.push(Span::styled("  ", hint));
            }
            right.push(Span::styled(
                *key,
                Style::new().fg(PREVIEW_FG).bg(STATUS_BG),
            ));
            right.push(Span::styled(format!(" {label}"), hint));
        }
        let right = Line::from(right);
        let width = right.width() as u16;
        if width + PADDING <= area.width {
            buffer.set_line(area.x + area.width - PADDING - width, area.y, &right, width);
        }
    }

    fn place_cursor(&self, engine: &dyn TerminalEngine, frame: &mut Frame, master: Rect) {
        let Some(project) = self.projects.get(self.active) else {
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
}

/// Surrounds title chrome with one blank border cell on each side.
fn clear_around(spans: Vec<Span<'static>>, style: Style) -> Vec<Span<'static>> {
    let mut padded = vec![Span::styled(" ", style)];
    padded.extend(spans);
    padded.push(Span::styled(" ", style));
    padded
}

const KEY_HINTS: [(&str, &str); 6] = [
    ("^g j/k", "switch"),
    ("^g 1-4", "select"),
    ("^g z", "zoom"),
    ("^g [", "scroll"),
    ("^g ?", "help"),
    ("^g q", "quit"),
];

/// The stack takes the ceiling of the non-master share so the master never
/// overruns. At 144 columns with the default 0.70 this is the design's 44.
fn stack_width(width: u16, master_ratio: f64) -> u16 {
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

/// The master always names its state; a preview only names an exit.
fn status_label(status: &TerminalStatus, master: bool) -> Option<String> {
    match status {
        TerminalStatus::Starting => master.then(|| "starting".to_owned()),
        TerminalStatus::Running => master.then(|| "running".to_owned()),
        TerminalStatus::Exited { code: Some(code) } => Some(format!("exit {code}")),
        TerminalStatus::Exited { code: None } => Some("exited".to_owned()),
        TerminalStatus::Failed { .. } => Some("failed".to_owned()),
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
fn draw_terminal(buffer: &mut Buffer, area: Rect, terminal: &TerminalFrame, default_fg: Color) {
    let clipped = terminal.size.columns > area.width;
    for row in 0..area.height.min(terminal.size.rows) {
        for column in 0..area.width.min(terminal.size.columns) {
            let Some(cell) = terminal.cell(column, row) else {
                continue;
            };
            let Some(target) = buffer.cell_mut((area.x + column, area.y + row)) else {
                continue;
            };
            let style = cell_style(&cell.style, default_fg);
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
                .set_style(Style::new().fg(default_fg).bg(CANVAS));
        }
    }
}

fn cell_style(style: &CellStyle, default_fg: Color) -> Style {
    let mut result = Style::new()
        .fg(style.foreground.map_or(default_fg, colour))
        .bg(style.background.map_or(CANVAS, colour));
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
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Position};

    use super::{ACCENT, Deck, ERROR, STATUS_BG, fixture};
    use crate::contracts::TerminalEngine;

    /// Renders the accepted 144x42 reference canvas.
    fn reference() -> (Buffer, Option<Position>) {
        let engine = fixture::frontend_active();
        let projects = fixture::projects();
        let deck = Deck {
            workspace: "idp",
            projects: &projects,
            active: 0,
            home: Some(fixture::home()),
            master_ratio: 0.70,
            now: fixture::NOW,
        };
        let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
        terminal
            .draw(|frame| deck.render(&engine as &dyn TerminalEngine, frame))
            .unwrap();
        let cursor = terminal.get_cursor_position().ok();
        (terminal.backend().buffer().clone(), cursor)
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

    #[test]
    fn frontend_active_matches_the_reference_canvas() {
        let (buffer, _) = reference();

        assert_eq!(
            text(&buffer),
            include_str!("testdata/frontend-active.txt").trim_end_matches('\n')
        );
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
}
