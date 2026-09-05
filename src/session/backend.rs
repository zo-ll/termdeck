use super::*;

pub(super) struct AnsiBackend {
    output: io::Stdout,
    cursor: Position,
}

impl AnsiBackend {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            output: io::stdout(),
            cursor: Position::ORIGIN,
        })
    }
    fn cursor_to(&mut self, x: u16, y: u16) -> io::Result<()> {
        cursor_to(&mut self.output, x, y)
    }
}

impl Backend for AnsiBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            write_cell(&mut self.output, x, y, cell)?;
            self.cursor = Position { x, y };
        }
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[?25l")
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[?25h")
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.cursor_to(position.x, position.y)?;
        self.cursor = position;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[2J\x1b[H")
    }
    fn size(&self) -> io::Result<Size> {
        let size = screen_size()?;
        Ok(Size::new(size.columns, size.rows))
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size()?,
            pixels: Size::ZERO,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

fn cursor_to(output: &mut impl Write, x: u16, y: u16) -> io::Result<()> {
    write!(output, "\x1b[{};{}H", y + 1, x + 1)
}

fn write_cell(output: &mut impl Write, x: u16, y: u16, cell: &Cell) -> io::Result<()> {
    cursor_to(output, x, y)?;
    write!(
        output,
        "\x1b[0m{}{}{}",
        colour(cell.fg, true),
        colour(cell.bg, false),
        modifiers(cell.modifier)
    )?;
    output.write_all(cell.symbol().as_bytes())
}

fn modifiers(modifier: Modifier) -> String {
    let mut codes = Vec::new();
    for (flag, code) in [
        (Modifier::BOLD, 1),
        (Modifier::DIM, 2),
        (Modifier::ITALIC, 3),
        (Modifier::UNDERLINED, 4),
        (Modifier::SLOW_BLINK, 5),
        (Modifier::RAPID_BLINK, 6),
        (Modifier::REVERSED, 7),
        (Modifier::HIDDEN, 8),
        (Modifier::CROSSED_OUT, 9),
    ] {
        if modifier.contains(flag) {
            codes.push(code.to_string());
        }
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

fn colour(colour: Color, foreground: bool) -> String {
    let base = if foreground { 30 } else { 40 };
    match colour {
        Color::Reset => if foreground { "\x1b[39m" } else { "\x1b[49m" }.to_owned(),
        Color::Black => format!("\x1b[{base}m"),
        Color::Red => format!("\x1b[{}m", base + 1),
        Color::Green => format!("\x1b[{}m", base + 2),
        Color::Yellow => format!("\x1b[{}m", base + 3),
        Color::Blue => format!("\x1b[{}m", base + 4),
        Color::Magenta => format!("\x1b[{}m", base + 5),
        Color::Cyan => format!("\x1b[{}m", base + 6),
        Color::Gray => format!("\x1b[{}m", base + 7),
        Color::DarkGray => format!("\x1b[{}m", base + 60),
        Color::LightRed => format!("\x1b[{}m", base + 61),
        Color::LightGreen => format!("\x1b[{}m", base + 62),
        Color::LightYellow => format!("\x1b[{}m", base + 63),
        Color::LightBlue => format!("\x1b[{}m", base + 64),
        Color::LightMagenta => format!("\x1b[{}m", base + 65),
        Color::LightCyan => format!("\x1b[{}m", base + 66),
        Color::White => format!("\x1b[{}m", base + 67),
        Color::Rgb(red, green, blue) => format!(
            "\x1b[{};2;{red};{green};{blue}m",
            if foreground { 38 } else { 48 }
        ),
        Color::Indexed(index) => format!("\x1b[{};5;{index}m", if foreground { 38 } else { 48 }),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{
        buffer::Cell,
        style::{Modifier, Style},
    };

    use super::write_cell;

    /// Assert the bytes written to a real `Write`, rather than the Ratatui
    /// buffer state that precedes this backend conversion.
    #[test]
    fn a_cell_emits_every_combined_text_attribute() {
        let attributes = Modifier::BOLD
            | Modifier::DIM
            | Modifier::ITALIC
            | Modifier::UNDERLINED
            | Modifier::SLOW_BLINK
            | Modifier::RAPID_BLINK
            | Modifier::REVERSED
            | Modifier::HIDDEN
            | Modifier::CROSSED_OUT;
        let mut cell = Cell::new("X");
        cell.set_style(Style::new().add_modifier(attributes));
        let mut output = Vec::new();

        write_cell(&mut output, 2, 3, &cell).unwrap();

        assert_eq!(
            output,
            b"\x1b[4;3H\x1b[0m\x1b[39m\x1b[49m\x1b[1;2;3;4;5;6;7;8;9mX"
        );
    }
}
