use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::Line,
    term::{Config, Term, TermMode, cell::Flags, color::Colors},
    vte::ansi::{Color, NamedColor, Processor},
};
use unicode_width::UnicodeWidthChar;

use crate::contracts::{
    CellContent, CellStyle, CellWidth, Cursor, MouseProtocol, Rgb, ScreenCell, ScreenSize,
    ScrollCommand, ScrollbackPosition, TerminalFrame, TerminalId,
};

/// Current-thread adapter from recorded VT output to an owned frame.
pub struct VtFrameAdapter {
    terminal: TerminalId,
    term: Term<PtyReplyListener>,
    parser: Processor,
    input_guard: VtInputGuard,
    replies: Rc<RefCell<Vec<u8>>>,
    bells: Rc<Cell<u32>>,
    revision: u64,
}

/// Alacritty asks its listener to return answers to terminal queries such as
/// DSR (`CSI 6 n`). Those bytes belong on this terminal's PTY, not the outer
/// UI terminal.
///
/// It also counts BEL, which the emulator already decodes for us: a bell is
/// the one notification an untaught tool sends for free (#97), and counting
/// it here needs no parser of our own.
#[derive(Clone)]
struct PtyReplyListener {
    replies: Rc<RefCell<Vec<u8>>>,
    bells: Rc<Cell<u32>>,
}

impl EventListener for PtyReplyListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(reply) => self.replies.borrow_mut().extend(reply.bytes()),
            Event::Bell => self.bells.set(self.bells.get().saturating_add(1)),
            _ => {}
        }
    }
}

impl VtFrameAdapter {
    /// Builds a terminal with a bounded primary-screen history. The limit is
    /// the number of off-screen lines; callers that inspect the whole grid
    /// may additionally receive the visible viewport rows.
    pub fn new(terminal: TerminalId, size: ScreenSize, scrollback: usize) -> Self {
        let dimensions = VtSize::from(size);
        let replies = Rc::new(RefCell::new(Vec::new()));
        let bells = Rc::new(Cell::new(0));
        Self {
            terminal,
            term: Term::new(
                Config {
                    scrolling_history: scrollback,
                    ..Config::default()
                },
                &dimensions,
                PtyReplyListener {
                    replies: Rc::clone(&replies),
                    bells: Rc::clone(&bells),
                },
            ),
            parser: Processor::new(),
            input_guard: VtInputGuard::default(),
            replies,
            bells,
            revision: 0,
        }
    }

    /// Advances the parser on this thread and returns the resulting owned frame.
    pub fn feed(&mut self, bytes: &[u8]) -> TerminalFrame {
        let mut accepted = Vec::with_capacity(bytes.len().min(MAX_CONTROL_STRING_BYTES));
        for &byte in bytes {
            let reset_parser = self.input_guard.advance(byte, &mut accepted);
            if reset_parser || accepted.len() >= MAX_CONTROL_STRING_BYTES {
                self.parser.advance(&mut self.term, &accepted);
                accepted.clear();
            }
            if reset_parser {
                // `vte` keeps an OSC string in a Vec while it is unfinished.
                // Starting over here discards that incomplete sequence and its
                // allocation; the guard then ignores its remainder through the
                // control-string terminator before normal parsing resumes.
                self.parser = Processor::new();
            }
        }
        self.parser.advance(&mut self.term, &accepted);
        self.revision = self.revision.saturating_add(1);
        self.frame()
    }

    #[cfg(test)]
    fn retained_control_bytes(&self) -> usize {
        self.input_guard.retained_control_bytes()
    }

    /// Takes bytes the terminal emulator generated in response to child
    /// queries, ready to be written to the same child PTY.
    pub fn take_pty_replies(&self) -> Vec<u8> {
        std::mem::take(&mut *self.replies.borrow_mut())
    }

    /// Takes the bells the child rang since the last call (#97). The count is
    /// taken rather than read so one drain raises one notification, however
    /// many bells a single burst of output carried.
    pub fn take_bells(&self) -> u32 {
        self.bells.replace(0)
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

    /// Whether the terminal shows its alternate screen: a full-screen app
    /// (vim, less, claude) owns the grid, so termdeck scrollback is inert
    /// there and the wheel belongs to the app (#74).
    pub fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Whether the child enabled DECCKM application-cursor mode.
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether the app enabled mouse reporting.
    pub fn mouse_reporting(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE)
    }

    /// The wire encoding selected by a mouse-reporting application.
    pub fn mouse_protocol(&self) -> MouseProtocol {
        let mode = self.term.mode();
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseProtocol::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseProtocol::Utf8
        } else {
            MouseProtocol::X10
        }
    }

    /// Whether the child enabled bracketed paste (DEC 2004): the session
    /// re-emits pastes as delimited regions while this holds (#120).
    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// The retained primary-screen lines, oldest to newest.
    pub fn history_lines(&self, max: usize) -> Vec<String> {
        if self.alt_screen() {
            return Vec::new();
        }
        self.active_screen_lines(max)
    }

    /// Lines from the grid currently displayed by the terminal, whether that
    /// is the primary screen or an application-owned alternate screen.
    pub fn active_screen_lines(&self, max: usize) -> Vec<String> {
        if max == 0 {
            return Vec::new();
        }
        let grid = self.term.grid();
        let history = grid.history_size();
        let start = grid.total_lines().saturating_sub(max);
        (start..grid.total_lines())
            .map(|line| {
                let mut text = String::new();
                for cell in &grid[Line(line as i32 - history as i32)] {
                    if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        text.push(cell.c);
                        text.extend(cell.zerowidth().unwrap_or_default());
                    }
                }
                text.trim_end().to_owned()
            })
            .collect()
    }
}

/// Maximum payload kept by `vte` while it waits for a control-string terminator.
///
/// This comfortably covers ordinary titles, hyperlinks, clipboard requests, and
/// image protocol headers, while making a child that dies mid-sequence bounded.
const MAX_CONTROL_STRING_BYTES: usize = 64 * 1024;

/// A normal grapheme cluster is much shorter than this (including emoji ZWJ
/// clusters). Keeping the cap generous preserves ordinary Unicode while
/// preventing one grid cell from retaining an unbounded `Vec<char>`.
const MAX_COMBINING_MARKS_PER_CELL: usize = 64;

/// A deliberately small mirror of the parser states relevant to data that can
/// be retained by the emulator. It is not a second VT parser: `vte` remains
/// authoritative for terminal semantics. The guard merely identifies strings
/// whose unfinished payload must be discarded before `vte` can grow its OSC
/// buffer without bound, and filters excess zero-width scalars before the
/// terminal grid stores them in a cell.
#[derive(Default)]
struct VtInputGuard {
    state: InputState,
    pending_utf8: Vec<u8>,
    combining_marks: usize,
}

#[derive(Default)]
enum InputState {
    #[default]
    Ground,
    Escape,
    Csi,
    String {
        kind: ControlString,
        retained: usize,
    },
    StringEscape,
    DiscardingString {
        kind: ControlString,
    },
    DiscardingStringEscape,
}

#[derive(Clone, Copy)]
enum ControlString {
    Osc,
    Dcs,
    SosPmApc,
}

impl VtInputGuard {
    /// Appends a safe byte to `accepted`. Returns true exactly when the caller
    /// must reset the real parser to drop an overlong unfinished string.
    fn advance(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        match self.state {
            InputState::Ground => self.advance_ground(byte, accepted),
            InputState::Escape => self.advance_escape(byte, accepted),
            InputState::Csi => self.advance_csi(byte, accepted),
            InputState::String { kind, retained } => {
                self.advance_string(byte, kind, retained, accepted)
            }
            InputState::StringEscape => self.advance_string_escape(byte, accepted),
            InputState::DiscardingString { kind } => self.advance_discarding_string(byte, kind),
            InputState::DiscardingStringEscape => self.advance_discarding_escape(byte, accepted),
        }
    }

    fn advance_ground(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        if byte == 0x1b {
            self.flush_pending_utf8(accepted);
            accepted.push(byte);
            self.state = InputState::Escape;
        } else {
            self.advance_text_byte(byte, accepted);
        }
        false
    }

    fn advance_escape(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        accepted.push(byte);
        self.state = match byte {
            b']' => InputState::String {
                kind: ControlString::Osc,
                retained: 0,
            },
            b'P' => InputState::String {
                kind: ControlString::Dcs,
                retained: 0,
            },
            b'X' | b'^' | b'_' => InputState::String {
                kind: ControlString::SosPmApc,
                retained: 0,
            },
            b'[' => InputState::Csi,
            0x1b => InputState::Escape,
            _ => InputState::Ground,
        };
        false
    }

    fn advance_csi(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        accepted.push(byte);
        self.state = match byte {
            0x1b => InputState::Escape,
            0x40..=0x7e => InputState::Ground,
            _ => InputState::Csi,
        };
        false
    }

    fn advance_string(
        &mut self,
        byte: u8,
        kind: ControlString,
        retained: usize,
        accepted: &mut Vec<u8>,
    ) -> bool {
        match byte {
            0x18 | 0x1a => {
                accepted.push(byte);
                self.state = InputState::Ground;
                false
            }
            0x1b => {
                accepted.push(byte);
                self.state = InputState::StringEscape;
                false
            }
            0x07 if matches!(kind, ControlString::Osc) => {
                accepted.push(byte);
                self.state = InputState::Ground;
                false
            }
            0x9c if matches!(kind, ControlString::Dcs) => {
                accepted.push(byte);
                self.state = InputState::Ground;
                false
            }
            _ if retained == MAX_CONTROL_STRING_BYTES => {
                self.state = InputState::DiscardingString { kind };
                true
            }
            _ => {
                accepted.push(byte);
                self.state = InputState::String {
                    kind,
                    retained: retained + 1,
                };
                false
            }
        }
    }

    fn advance_string_escape(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        accepted.push(byte);
        self.state = match byte {
            b']' => InputState::String {
                kind: ControlString::Osc,
                retained: 0,
            },
            b'P' => InputState::String {
                kind: ControlString::Dcs,
                retained: 0,
            },
            b'X' | b'^' | b'_' => InputState::String {
                kind: ControlString::SosPmApc,
                retained: 0,
            },
            b'[' => InputState::Csi,
            0x1b => InputState::Escape,
            _ => InputState::Ground,
        };
        false
    }

    fn advance_discarding_string(&mut self, byte: u8, kind: ControlString) -> bool {
        self.state = match byte {
            0x18 | 0x1a => InputState::Ground,
            0x07 if matches!(kind, ControlString::Osc) => InputState::Ground,
            0x9c if matches!(kind, ControlString::Dcs) => InputState::Ground,
            0x1b => InputState::DiscardingStringEscape,
            _ => InputState::DiscardingString { kind },
        };
        false
    }

    fn advance_discarding_escape(&mut self, byte: u8, accepted: &mut Vec<u8>) -> bool {
        if byte == b'\\' {
            self.state = InputState::Ground;
            return false;
        }

        // ESC ends a string even when it does not form ST. Preserve the new
        // escape sequence by replaying both bytes from a fresh parser state.
        self.state = InputState::Ground;
        self.advance(0x1b, accepted) || self.advance(byte, accepted)
    }

    fn advance_text_byte(&mut self, byte: u8, accepted: &mut Vec<u8>) {
        self.pending_utf8.push(byte);
        loop {
            match std::str::from_utf8(&self.pending_utf8) {
                Ok(text) => {
                    debug_assert_eq!(text.chars().count(), 1);
                    let character = text.chars().next().expect("pending UTF-8 is nonempty");
                    if self.accept_text_character(character) {
                        accepted.extend_from_slice(&self.pending_utf8);
                    }
                    self.pending_utf8.clear();
                    return;
                }
                Err(error) if error.error_len().is_none() && self.pending_utf8.len() < 4 => return,
                Err(error) => {
                    let valid = error.valid_up_to();
                    let invalid_len = error.error_len().unwrap_or(self.pending_utf8.len() - valid);
                    accepted.extend_from_slice(&self.pending_utf8[..valid + invalid_len]);
                    self.pending_utf8.drain(..valid + invalid_len);
                    if self.pending_utf8.is_empty() {
                        return;
                    }
                }
            }
        }
    }

    fn flush_pending_utf8(&mut self, accepted: &mut Vec<u8>) {
        accepted.append(&mut self.pending_utf8);
    }

    fn accept_text_character(&mut self, character: char) -> bool {
        match character.width() {
            Some(0) => {
                if self.combining_marks == MAX_COMBINING_MARKS_PER_CELL {
                    false
                } else {
                    self.combining_marks += 1;
                    true
                }
            }
            Some(_) => {
                self.combining_marks = 0;
                true
            }
            None => true,
        }
    }

    #[cfg(test)]
    fn retained_control_bytes(&self) -> usize {
        match self.state {
            InputState::String { retained, .. } => retained,
            _ => 0,
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
    use crate::contracts::{
        CellContent, CellWidth, DEFAULT_SCROLLBACK, MouseProtocol, Rgb, ScreenSize, ScrollCommand,
        TerminalId,
    };

    use super::{MAX_COMBINING_MARKS_PER_CELL, MAX_CONTROL_STRING_BYTES, VtFrameAdapter};

    fn adapter(size: ScreenSize) -> VtFrameAdapter {
        VtFrameAdapter::new(TerminalId::new("recording"), size, DEFAULT_SCROLLBACK)
    }

    /// #97: BEL is the one notification an untaught tool sends for free, and
    /// the emulator already decodes it. The count is taken, so one drain
    /// raises one notification however many bells the burst carried, and the
    /// bell itself leaves no cell behind.
    #[test]
    fn bells_are_counted_and_taken_rather_than_drawn() {
        let mut adapter = adapter(ScreenSize::new(8, 2));

        let frame = adapter.feed(b"hi\x07\x07");

        assert_eq!(adapter.take_bells(), 2);
        assert_eq!(adapter.take_bells(), 0, "taken, not read");
        assert!(matches!(
            frame.cell(2, 0).unwrap().content,
            CellContent::Empty
        ));
        adapter.feed(b"quiet");
        assert_eq!(adapter.take_bells(), 0, "output alone rings nothing");
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
    fn ordinary_multi_mark_graphemes_remain_intact() {
        let grapheme = "a\u{301}\u{327}\u{20dd}";
        let frame = adapter(ScreenSize::new(4, 1)).feed(grapheme.as_bytes());

        assert!(matches!(
            frame.cell(0, 0).map(|cell| &cell.content),
            Some(CellContent::Glyph { text, width: CellWidth::One }) if text == grapheme
        ));
    }

    /// #142: `vte` retains an unfinished OSC in a `Vec`. Once the guard has
    /// discarded one overlong sequence, further payload must not make that
    /// retention grow again, and a later terminator must restore normal output.
    #[test]
    fn unfinished_control_strings_recover_with_flat_retention() {
        const CHUNK: usize = 4096;
        let mut terminal = adapter(ScreenSize::new(16, 1));
        terminal.feed(b"\x1b]0;");

        let payload = vec![b'x'; CHUNK];
        let mut recovered = false;
        for _ in 0..=(MAX_CONTROL_STRING_BYTES / CHUNK) {
            terminal.feed(&payload);
            if terminal.retained_control_bytes() == 0 {
                recovered = true;
                break;
            }
            assert!(
                terminal.retained_control_bytes() <= MAX_CONTROL_STRING_BYTES,
                "unfinished control string exceeded its retention cap"
            );
        }
        assert!(recovered, "overlong control string did not recover");

        for _ in 0..32 {
            terminal.feed(&payload);
            assert_eq!(
                terminal.retained_control_bytes(),
                0,
                "recovered parser retained more unfinished control data"
            );
        }

        let frame = terminal.feed(b"\x07recovered");
        assert!(
            frame_text(&frame).contains("recovered"),
            "ordinary output after the discarded string was not parsed"
        );
    }

    /// Repeated zero-width scalars used to grow Alacritty's `CellExtra` Vec
    /// indefinitely and then get copied into each owned frame.
    #[test]
    fn combining_marks_per_cell_are_bounded() {
        let mut input = String::from("a");
        input.push_str(&"\u{301}".repeat(MAX_COMBINING_MARKS_PER_CELL + 512));

        let frame = adapter(ScreenSize::new(4, 1)).feed(input.as_bytes());
        let Some(CellContent::Glyph { text, .. }) = frame.cell(0, 0).map(|cell| &cell.content)
        else {
            panic!("base cell disappeared");
        };
        assert_eq!(
            text.chars().count(),
            1 + MAX_COMBINING_MARKS_PER_CELL,
            "a cell retained more combining scalars than the configured cap"
        );
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

    /// `scrollback` is the capacity of off-screen history, not the capacity
    /// of a whole peek. A complete primary-screen peek can include that
    /// history plus the visible viewport rows.
    #[test]
    fn configured_scrollback_bounds_history_but_keeps_the_viewport() {
        const HISTORY: usize = 1;
        let size = ScreenSize::new(16, 2);
        let mut adapter = VtFrameAdapter::new(TerminalId::new("recording"), size, HISTORY);
        adapter.feed(b"line-0\r\nline-1\r\nline-2\r\nline-3\r\nline-4\r\n");

        let lines = adapter.history_lines(usize::MAX);
        assert!(
            lines.len() <= HISTORY + usize::from(size.rows),
            "history plus viewport exceeded the configured capacity: {lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("line-4")));
        assert!(
            !lines.iter().any(|line| line.contains("line-0")),
            "evicted history remained visible: {lines:?}"
        );
        assert_eq!(adapter.scrollback_position().lines_above, HISTORY as u32);
    }

    #[test]
    fn history_lines_include_retained_output_beyond_the_viewport() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        adapter.feed(b"zero\r\none\r\ntwo\r\nthree\r\n");

        let history = adapter.history_lines(16);
        assert!(history.len() > 2, "history: {history:?}");
        assert!(
            history.iter().any(|line| line.contains("zero")),
            "history: {history:?}"
        );
        assert_eq!(
            adapter.history_lines(1),
            vec![history.last().unwrap().clone()]
        );
    }

    #[test]
    fn active_screen_lines_follow_the_main_and_alternate_grids() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        adapter.feed(b"main");
        assert!(
            adapter
                .active_screen_lines(2)
                .iter()
                .any(|line| line.contains("main"))
        );

        adapter.feed(b"\x1b[?1049h");
        adapter.feed(b"alt");
        assert!(adapter.alt_screen());
        assert!(adapter.history_lines(2).is_empty());
        assert!(
            adapter
                .active_screen_lines(2)
                .iter()
                .any(|line| line.contains("alt"))
        );

        adapter.feed(b"\x1b[?1049l");
        assert!(
            adapter
                .active_screen_lines(2)
                .iter()
                .any(|line| line.contains("main"))
        );
    }

    /// REPRO (#74): a full-screen app owns the grid, so termdeck's own
    /// scrollback has nothing to display while one runs — scrolling the
    /// alternate screen moves nothing, and the app never sees the wheel.
    #[test]
    fn scrollback_scroll_is_inert_on_the_alternate_screen() {
        let mut app = adapter(ScreenSize::new(8, 2));
        app.feed(b"\x1b[?1049h");
        app.feed(b"one\r\ntwo\r\nthree\r\n");
        app.scroll(ScrollCommand::Up(3));

        assert_eq!(
            app.scrollback_position().lines_below,
            0,
            "termdeck scrollback cannot move an app-owned grid"
        );
    }

    #[test]
    fn alt_screen_tracks_the_alternate_buffer() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        assert!(!adapter.alt_screen());
        adapter.feed(b"\x1b[?1049h");
        assert!(adapter.alt_screen());
        adapter.feed(b"\x1b[?1049l");
        assert!(!adapter.alt_screen());
    }

    /// `fzf`, which Horizon uses, asks for the cursor position before it
    /// draws. The emulator's reply must survive the frame adapter so its PTY
    /// owner can write it back to the application.
    #[test]
    fn device_status_query_produces_a_pty_reply() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        adapter.feed(b"\x1b[2;3H\x1b[6n");

        assert_eq!(adapter.take_pty_replies(), b"\x1b[2;3R");
    }

    #[test]
    fn mouse_reporting_tracks_the_app_mouse_mode() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        assert!(!adapter.mouse_reporting());
        adapter.feed(b"\x1b[?1000h\x1b[?1006h");
        assert!(adapter.mouse_reporting());
        adapter.feed(b"\x1b[?1000l");
        assert!(!adapter.mouse_reporting());
    }

    #[test]
    fn application_cursor_and_mouse_protocol_follow_the_childs_modes() {
        let mut adapter = adapter(ScreenSize::new(8, 2));
        adapter.feed(b"\x1b[?1h\x1b[?1000h");
        assert!(adapter.application_cursor());
        assert!(adapter.mouse_reporting());
        assert_eq!(adapter.mouse_protocol(), MouseProtocol::X10);

        adapter.feed(b"\x1b[?1006h");
        assert_eq!(adapter.mouse_protocol(), MouseProtocol::Sgr);
        adapter.feed(b"\x1b[?1006l\x1b[?1005h");
        assert_eq!(adapter.mouse_protocol(), MouseProtocol::Utf8);

        adapter.feed(b"\x1b[?1l\x1b[?1000l");
        assert!(!adapter.application_cursor());
        assert!(!adapter.mouse_reporting());
    }

    /// Output advances a live viewport, but never steals a deliberately
    /// detached one. Both are properties of Alacritty's display offset.
    #[test]
    fn output_follows_the_tail_without_pinning_a_scrolled_viewport() {
        let mut live = adapter(ScreenSize::new(8, 2));
        live.feed(b"one\r\ntwo\r\nthree\r\n");
        assert_eq!(live.scrollback_position().lines_below, 0);
        let frame = live.feed(b"LIVE\r\n");
        assert!(
            frame_text(&frame).contains("LIVE"),
            "live tail did not advance"
        );
        assert_eq!(live.scrollback_position().lines_below, 0);

        let mut scrolled = adapter(ScreenSize::new(8, 2));
        scrolled.feed(b"one\r\ntwo\r\nthree\r\nfour\r\n");
        scrolled.scroll(ScrollCommand::Up(1));
        assert!(
            scrolled.scrollback_position().lines_below > 0,
            "fixture must detach the viewport"
        );
        scrolled.feed(b"PINNED\r\n");
        assert!(
            scrolled.scrollback_position().lines_below > 0,
            "new output must not pin a detached viewport to the tail"
        );
    }

    fn frame_text(frame: &crate::contracts::TerminalFrame) -> String {
        frame
            .cells
            .iter()
            .map(|cell| match &cell.content {
                CellContent::Glyph { text, .. } => text.as_str(),
                CellContent::Empty => " ",
                CellContent::Continuation => "",
            })
            .collect()
    }
}
