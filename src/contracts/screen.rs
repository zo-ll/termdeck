use super::TerminalId;

/// Character-grid dimensions in terminal cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenSize {
    pub columns: u16,
    pub rows: u16,
}

impl ScreenSize {
    pub const fn new(columns: u16, rows: u16) -> Self {
        Self { columns, rows }
    }

    pub const fn cell_count(self) -> usize {
        self.columns as usize * self.rows as usize
    }
}

/// An RGB terminal colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// Visual attributes required to render a terminal cell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellStyle {
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// One screen cell. `character` is a Unicode scalar value; wide-cell handling
/// is owned by the engine that produces a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCell {
    pub character: char,
    pub style: CellStyle,
}

impl Default for ScreenCell {
    fn default() -> Self {
        Self {
            character: ' ',
            style: CellStyle::default(),
        }
    }
}

/// The cursor location in a terminal frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub column: u16,
    pub row: u16,
    pub visible: bool,
}

/// A complete, revisioned terminal screen snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalFrame {
    pub terminal: TerminalId,
    pub size: ScreenSize,
    pub cells: Vec<ScreenCell>,
    pub cursor: Cursor,
    pub revision: u64,
}

impl TerminalFrame {
    pub fn blank(terminal: TerminalId, size: ScreenSize, revision: u64) -> Self {
        Self {
            terminal,
            size,
            cells: vec![ScreenCell::default(); size.cell_count()],
            cursor: Cursor {
                column: 0,
                row: 0,
                visible: true,
            },
            revision,
        }
    }
}
