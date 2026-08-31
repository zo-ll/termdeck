use alacritty_terminal::{
    event::VoidListener,
    grid::{Dimensions, Scroll},
    term::{Config, Term, TermMode, cell::Flags, color::Colors},
    vte::ansi::{Color, NamedColor, Processor},
};

use crate::contracts::{
    CellContent, CellStyle, CellWidth, Cursor, Rgb, ScreenCell, ScreenSize, ScrollCommand,
    ScrollbackPosition, TerminalFrame, TerminalId,
};

const SCROLLBACK_LINES: usize = 10_000;

/// Current-thread adapter from recorded VT output to an owned frame.
pub struct VtFrameAdapter {
    terminal: TerminalId,
    term: Term<VoidListener>,
    parser: Processor,
    revision: u64,
}

impl VtFrameAdapter {
    pub fn new(terminal: TerminalId, size: ScreenSize) -> Self {
        let dimensions = VtSize::from(size);
        Self {
            terminal,
            term: Term::new(
                Config {
                    scrolling_history: SCROLLBACK_LINES,
                    ..Config::default()
                },
                &dimensions,
                VoidListener,
            ),
            parser: Processor::new(),
            revision: 0,
        }
    }

    /// Advances the parser on this thread and returns the resulting owned frame.
    pub fn feed(&mut self, bytes: &[u8]) -> TerminalFrame {
        self.parser.advance(&mut self.term, bytes);
        self.revision = self.revision.saturating_add(1);
        self.frame()
    }

    pub fn resize(&mut self, size: ScreenSize) -> TerminalFrame {
        self.term.resize(VtSize::from(size));
        self.revision = self.revision.saturating_add(1);
        self.frame()
    }

    pub fn scroll(&mut self, command: ScrollCommand) -> TerminalFrame {
        let scroll = match command {
            ScrollCommand::Up(lines) => Scroll::Delta(i32::from(lines)),
            ScrollCommand::Down(lines) => Scroll::Delta(-i32::from(lines)),
            ScrollCommand::Bottom => Scroll::Bottom,
        };
        self.term.scroll_display(scroll);
        self.revision = self.revision.saturating_add(1);
        self.frame()
    }

    pub fn frame(&self) -> TerminalFrame {
        let size = ScreenSize::new(self.term.columns() as u16, self.term.screen_lines() as u16);
        let content = self.term.renderable_content();
        let mut frame = TerminalFrame::blank(self.terminal.clone(), size, self.revision);

        for indexed in content.display_iter {
            let row = indexed.point.line.0 + content.display_offset as i32;
            let column = indexed.point.column.0;
            let Some(index) =
                (row >= 0)
                    .then_some((column, row as usize))
                    .and_then(|(column, row)| {
                        (column < usize::from(size.columns) && row < usize::from(size.rows))
                            .then_some(row * usize::from(size.columns) + column)
                    })
            else {
                continue;
            };

            let cell = indexed.cell;
            let style = cell_style(cell.fg, cell.bg, cell.flags, content.colors);
            frame.cells[index] = ScreenCell {
                content: cell_content(cell.c, cell.zerowidth(), cell.flags),
                style,
            };
        }

        let cursor_row = content.cursor.point.line.0 + content.display_offset as i32;
        frame.cursor = Cursor {
            column: content.cursor.point.column.0 as u16,
            row: cursor_row.max(0) as u16,
            visible: content.mode.contains(TermMode::SHOW_CURSOR)
                && cursor_row >= 0
                && cursor_row < i32::from(size.rows)
                && content.cursor.point.column.0 < usize::from(size.columns),
        };
        frame
    }

    pub fn scrollback_position(&self) -> ScrollbackPosition {
        let grid = self.term.grid();
        let lines_below = grid.display_offset();
        ScrollbackPosition {
            lines_above: grid.history_size().saturating_sub(lines_below) as u32,
            lines_below: lines_below as u32,
        }
    }
}

#[derive(Clone, Copy)]
struct VtSize {
    columns: usize,
    rows: usize,
}

impl From<ScreenSize> for VtSize {
    fn from(size: ScreenSize) -> Self {
        Self {
            columns: usize::from(size.columns),
            rows: usize::from(size.rows),
        }
    }
}

impl Dimensions for VtSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

fn cell_content(character: char, zerowidth: Option<&[char]>, flags: Flags) -> CellContent {
    if flags.contains(Flags::WIDE_CHAR_SPACER) {
        return CellContent::Continuation;
    }
    if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        return CellContent::Empty;
    }
    if character == ' ' && zerowidth.is_none() {
        return CellContent::Empty;
    }

    let mut text = character.to_string();
    if let Some(zerowidth) = zerowidth {
        text.extend(zerowidth);
    }
    CellContent::Glyph {
        text,
        width: if flags.contains(Flags::WIDE_CHAR) {
            CellWidth::Two
        } else {
            CellWidth::One
        },
    }
}

fn cell_style(foreground: Color, background: Color, flags: Flags, colors: &Colors) -> CellStyle {
    CellStyle {
        foreground: resolve_color(foreground, colors),
        background: resolve_color(background, colors),
        bold: flags.contains(Flags::BOLD),
        dim: flags.contains(Flags::DIM),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        inverse: flags.contains(Flags::INVERSE),
    }
}

fn resolve_color(color: Color, colors: &Colors) -> Option<Rgb> {
    let color = match color {
        Color::Spec(color) => Some(color),
        Color::Named(color) => colors[color].or_else(|| named_color(color)),
        Color::Indexed(index) => colors[usize::from(index)].or_else(|| indexed_color(index)),
    }?;
    Some(Rgb {
        red: color.r,
        green: color.g,
        blue: color.b,
    })
}

fn named_color(color: NamedColor) -> Option<alacritty_terminal::vte::ansi::Rgb> {
    let index = match color {
        NamedColor::Black => 0,
        NamedColor::Red => 1,
        NamedColor::Green => 2,
        NamedColor::Yellow => 3,
        NamedColor::Blue => 4,
        NamedColor::Magenta => 5,
        NamedColor::Cyan => 6,
        NamedColor::White => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        _ => return None,
    };
    indexed_color(index)
}

fn indexed_color(index: u8) -> Option<alacritty_terminal::vte::ansi::Rgb> {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];

    let (red, green, blue) = match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            let component = |value| if value == 0 { 0 } else { 55 + 40 * value };
            (
                component(value / 36),
                component(value % 36 / 6),
                component(value % 6),
            )
        }
        232..=255 => {
            let value = 8 + 10 * (index - 232);
            (value, value, value)
        }
    };
    Some(alacritty_terminal::vte::ansi::Rgb {
        r: red,
        g: green,
        b: blue,
    })
}

#[cfg(test)]
mod tests {
    use crate::contracts::{CellContent, CellWidth, Rgb, ScreenSize, ScrollCommand, TerminalId};

    use super::VtFrameAdapter;

    fn adapter(size: ScreenSize) -> VtFrameAdapter {
        VtFrameAdapter::new(TerminalId::new("recording"), size)
    }

    #[test]
    fn recorded_sgr_and_cursor_become_owned_cell_style() {
        let frame =
            adapter(ScreenSize::new(8, 2)).feed(b"\x1b[1;3H\x1b[1;2;3;4;7;38;2;1;2;3;48;2;4;5;6mX");

        let cell = frame.cell(2, 0).unwrap();
        assert!(matches!(cell.content, CellContent::Glyph { ref text, .. } if text == "X"));
        assert_eq!(
            cell.style.foreground,
            Some(Rgb {
                red: 1,
                green: 2,
                blue: 3
            })
        );
        assert_eq!(
            cell.style.background,
            Some(Rgb {
                red: 4,
                green: 5,
                blue: 6
            })
        );
        assert!(
            cell.style.bold
                && cell.style.dim
                && cell.style.italic
                && cell.style.underline
                && cell.style.inverse
        );
        assert_eq!(frame.cell(0, 0).unwrap().style.foreground, None);
        assert_eq!(frame.cell(0, 0).unwrap().style.background, None);
        assert_eq!(frame.cursor.column, 3);
        assert_eq!(frame.cursor.row, 0);
    }

    #[test]
    fn combining_unicode_stays_with_its_base_cell() {
        let frame = adapter(ScreenSize::new(4, 1)).feed("e\u{301}".as_bytes());

        assert!(matches!(
            frame.cell(0, 0).map(|cell| &cell.content),
            Some(CellContent::Glyph { text, width: CellWidth::One }) if text == "e\u{301}"
        ));
    }

    #[test]
    fn wide_glyphs_have_an_explicit_continuation_cell() {
        let frame = adapter(ScreenSize::new(4, 1)).feed("界".as_bytes());

        assert!(matches!(
            frame.cell(0, 0).map(|cell| &cell.content),
            Some(CellContent::Glyph { text, width: CellWidth::Two }) if text == "界"
        ));
        assert!(matches!(
            frame.cell(1, 0).map(|cell| &cell.content),
            Some(CellContent::Continuation)
        ));
    }

    #[test]
    fn resize_revisions_the_owned_frame() {
        let mut adapter = adapter(ScreenSize::new(4, 1));
        let first = adapter.feed(b"x");
        let resized = adapter.resize(ScreenSize::new(6, 3));

        assert_eq!(resized.size, ScreenSize::new(6, 3));
        assert_eq!(resized.cells.len(), 18);
        assert_eq!(resized.revision, first.revision + 1);
    }

    #[test]
    fn scrollback_position_tracks_the_visible_viewport() {
        let mut adapter = adapter(ScreenSize::new(4, 2));
        adapter.feed(b"0\r\n1\r\n2\r\n3\r\n");

        assert!(adapter.scrollback_position().lines_above > 0);
        assert_eq!(adapter.scrollback_position().lines_below, 0);
        adapter.scroll(ScrollCommand::Up(1));
        assert_eq!(adapter.scrollback_position().lines_below, 1);
    }
}
