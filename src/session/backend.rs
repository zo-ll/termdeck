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
        write!(self.output, "\x1b[{};{}H", y + 1, x + 1)
    }
}

impl Backend for AnsiBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            self.cursor_to(x, y)?;
            write!(
                self.output,
                "\x1b[0m{}{}{}",
                colour(cell.fg, true),
                colour(cell.bg, false),
                modifiers(cell.modifier)
            )?;
            self.output.write_all(cell.symbol().as_bytes())?;
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

fn modifiers(modifier: Modifier) -> &'static str {
    if modifier.contains(Modifier::BOLD) {
        "\x1b[1m"
    } else if modifier.contains(Modifier::DIM) {
        "\x1b[2m"
    } else if modifier.contains(Modifier::ITALIC) {
        "\x1b[3m"
    } else if modifier.contains(Modifier::UNDERLINED) {
        "\x1b[4m"
    } else if modifier.contains(Modifier::REVERSED) {
        "\x1b[7m"
    } else {
        ""
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
