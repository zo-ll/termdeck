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

/// The horizontal span occupied by a glyph in a terminal grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellWidth {
    One,
    Two,
}

/// Contents of one screen cell.
///
/// `text` preserves every Unicode scalar stored in the terminal cell, including
/// combining marks. A two-cell glyph is represented by
/// `Glyph { width: CellWidth::Two }` in its first cell and `Continuation` in
/// the cell immediately to its right. Continuations do not draw text.
/// Producers must not place a wide glyph in the final column and must never
/// emit a standalone continuation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CellContent {
    Empty,
    Glyph { text: String, width: CellWidth },
    Continuation,
}

/// One screen cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCell {
    pub content: CellContent,
    pub style: CellStyle,
}

impl Default for ScreenCell {
    fn default() -> Self {
        Self {
            content: CellContent::Empty,
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
///
/// `cells` is a row-major, rectangular grid: the cell at `(column, row)` is
/// at `row * size.columns + column`, and its length is always
/// `size.columns * size.rows`. Use [`Self::cell`] when coordinates are more
/// convenient. The UI renders a `Glyph` at its cell and skips a
/// `Continuation`; it does not calculate Unicode display width itself.
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

    pub fn cell_index(&self, column: u16, row: u16) -> Option<usize> {
        (column < self.size.columns && row < self.size.rows)
            .then_some(row as usize * self.size.columns as usize + column as usize)
    }

    pub fn cell(&self, column: u16, row: u16) -> Option<&ScreenCell> {
        self.cell_index(column, row)
            .and_then(|index| self.cells.get(index))
    }
}

#[cfg(test)]
mod tests {
    use super::{CellContent, CellWidth, ScreenCell, ScreenSize, TerminalFrame};
    use crate::contracts::TerminalId;

    #[test]
    fn frames_are_row_major_and_wide_cells_are_explicit() {
        let mut frame = TerminalFrame::blank(TerminalId::new("test"), ScreenSize::new(3, 2), 0);
        frame.cells[1].content = CellContent::Glyph {
            text: "界".to_owned(),
            width: CellWidth::Two,
        };
        frame.cells[2] = ScreenCell {
            content: CellContent::Continuation,
            style: frame.cells[1].style,
        };

        assert_eq!(frame.cell_index(1, 0), Some(1));
        assert_eq!(frame.cell_index(0, 1), Some(3));
        assert!(matches!(
            frame.cell(2, 0).map(|cell| &cell.content),
            Some(CellContent::Continuation)
        ));
        assert_eq!(frame.cell(3, 0), None);
    }

    #[test]
    fn glyphs_preserve_combining_sequences() {
        let cell = ScreenCell {
            content: CellContent::Glyph {
                text: "e\u{301}".to_owned(),
                width: CellWidth::One,
            },
            style: Default::default(),
        };

        assert!(matches!(
            cell.content,
            CellContent::Glyph { ref text, .. } if text == "e\u{301}"
        ));
    }
}
