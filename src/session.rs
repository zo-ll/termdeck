//! The interactive composition root: terminal mode, event loop, UI, and PTYs.

use std::{
    error::Error,
    io::{self, Read, Write},
    panic,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use ratatui::{
    Terminal,
    backend::{Backend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier},
};

use crate::{
    config::Workspace,
    contracts::{EngineCommand, ScreenSize, ScrollCommand, TerminalEngine, Timestamp, UserCommand},
    engine::NativeEngine,
    ui::{Deck, DeckState, Input, Key, Reaction},
};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// A wheel tick is intentionally smaller than a keyboard page movement.
const WHEEL_LINES: u16 = 3;
const DOUBLE_CLICK_WINDOW: u64 = 500;
static SIGNAL: AtomicI32 = AtomicI32::new(0);
static SAVED_TERMIOS: OnceLock<Mutex<Option<libc::termios>>> = OnceLock::new();
type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;
static SAVED_PANIC_HOOK: OnceLock<Mutex<Option<PanicHook>>> = OnceLock::new();

/// Runs an already validated workspace. Configuration is deliberately loaded
/// before this point, so no PTY exists when validation fails.
pub fn run(workspace: &Workspace) -> Result<(), Box<dyn Error>> {
    let _panic = PanicGuard::install();
    let _signals = SignalGuard::install()?;
    let _outer = OuterTerminal::enter()?;
    let mut size = screen_size()?;
    let mut engine = NativeEngine::spawn(&workspace.projects, size)
        .map_err(|error| format!("cannot start workspace '{}': {error}", workspace.name))?;
    let mut terminal = Terminal::new(AnsiBackend::new()?)?;
    let mut deck = DeckState::new(workspace.projects.len());
    let mut input = Input::new(size.rows.saturating_sub(4));
    let mut keys = KeyReader::default();
    let mut last_click = None;
    // The preview whose disclosure marker is being pressed, if any.
    let mut marker_press: Option<usize> = None;

    let mut dirty = true;
    'session: loop {
        if SIGNAL.swap(0, Ordering::SeqCst) != 0 {
            break;
        }
        let current_size = screen_size()?;
        if current_size != size {
            for project in &workspace.projects {
                engine.dispatch(EngineCommand::Resize {
                    terminal: project.terminal.clone(),
                    size: current_size,
                });
            }
            input.set_page(current_size.rows.saturating_sub(4));
            size = current_size;
            terminal.autoresize()?;
            dirty = true;
        }
        dirty |= !engine.drain_events().is_empty();

        for event in keys.read(POLL_INTERVAL)? {
            dirty = true;
            match event {
                InputEvent::Key(key) => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    let was_scrollback = deck.scrollback();
                    let reaction = input.press(key, &mut deck, &workspace.projects, now());
                    if was_scrollback
                        && key == Key::Escape
                        && !deck.scrollback()
                        && let Some(active) = deck.active()
                    {
                        engine.dispatch(EngineCommand::Scroll {
                            terminal: workspace.projects[active].terminal.clone(),
                            command: crate::contracts::ScrollCommand::Bottom,
                        });
                    }
                    match reaction {
                        Some(Reaction::Send(UserCommand::Input(command))) => {
                            if let Some(active) = deck.active() {
                                let bytes = match command {
                                    crate::contracts::InputCommand::Bytes(bytes) => bytes,
                                    crate::contracts::InputCommand::Paste(text) => {
                                        text.into_bytes()
                                    }
                                };
                                dispatch_live_input(
                                    &mut engine,
                                    workspace.projects[active].terminal.clone(),
                                    bytes,
                                );
                            }
                        }
                        Some(Reaction::Send(UserCommand::Action(_))) => {}
                        Some(Reaction::Scroll(command)) => {
                            if let Some(active) = deck.active() {
                                engine.dispatch(EngineCommand::Scroll {
                                    terminal: workspace.projects[active].terminal.clone(),
                                    command,
                                });
                            }
                        }
                        Some(Reaction::Respawn) => {
                            if let Some(active) = deck.active() {
                                engine.dispatch(EngineCommand::Respawn {
                                    terminal: workspace.projects[active].terminal.clone(),
                                });
                            }
                        }
                        Some(Reaction::Quit) => break 'session,
                        None => {}
                    }
                }
                InputEvent::Wheel { pointer, command } => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    if deck.modal().is_none() {
                        let terminal = Deck {
                            workspace: &workspace.name,
                            projects: &workspace.projects,
                            state: &deck,
                            home: None,
                            master_ratio: workspace.master_ratio.get(),
                            now: now(),
                        }
                        .position_at(
                            ratatui::layout::Rect::new(0, 0, size.columns, size.rows),
                            pointer,
                        )
                        // A collapsed preview has no viewport, so there is
                        // nothing under the pointer to scroll.
                        .filter(|position| !deck.collapsed(*position))
                        .and_then(|position| workspace.projects.get(position))
                        .map(|project| project.terminal.clone());
                        if let Some(terminal) = terminal {
                            engine.dispatch(EngineCommand::Scroll { terminal, command });
                        }
                    }
                }
                InputEvent::Mouse { pointer, action } => {
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let (marker, position) = {
                            let pane = Deck {
                                workspace: &workspace.name,
                                projects: &workspace.projects,
                                state: &deck,
                                home: None,
                                master_ratio: workspace.master_ratio.get(),
                                now: now(),
                            };
                            (
                                pane.marker_at(area, pointer),
                                match action {
                                    MouseAction::Down => pane.swap_position_at(area, pointer),
                                    MouseAction::Move | MouseAction::Up => {
                                        pane.position_at(area, pointer)
                                    }
                                },
                            )
                        };
                        // The disclosure marker owns its two cells: pressing
                        // there starts no drag and arms no promotion, and the
                        // release folds or unfolds that preview.
                        match (action, marker, marker_press) {
                            (MouseAction::Down, Some(pressed), _) => {
                                deck.cancel_drag();
                                last_click = None;
                                marker_press = Some(pressed);
                            }
                            (MouseAction::Move, _, Some(_)) => {}
                            (MouseAction::Up, _, Some(pressed)) => {
                                marker_press = None;
                                if marker == Some(pressed) {
                                    deck.toggle_collapse(pressed);
                                }
                            }
                            _ => {
                                marker_press = None;
                                if let Some(action) = mouse_action(
                                    &mut deck,
                                    position,
                                    action,
                                    now(),
                                    &mut last_click,
                                ) {
                                    deck.apply(&action, &workspace.projects, now());
                                }
                            }
                        }
                    }
                }
                InputEvent::Paste(text) => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    if let Some(active) = deck.active()
                        && deck.modal().is_none()
                        && !deck.scrollback()
                    {
                        dispatch_live_input(
                            &mut engine,
                            workspace.projects[active].terminal.clone(),
                            text.into_bytes(),
                        );
                    }
                }
            }
        }
        if dirty {
            terminal.draw(|frame| {
                Deck {
                    workspace: &workspace.name,
                    projects: &workspace.projects,
                    state: &deck,
                    home: std::env::var_os("HOME")
                        .as_deref()
                        .map(std::path::Path::new),
                    master_ratio: workspace.master_ratio.get(),
                    now: now(),
                }
                .render(&engine, frame);
            })?;
            dirty = false;
        }
    }
    engine.dispatch(EngineCommand::Shutdown);
    Ok(())
}

fn now() -> Timestamp {
    Timestamp {
        unix_millis: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    }
}

/// Live shell input always resumes the active terminal at its tail. Wheel
/// scrolling deliberately has no modal state, unlike keyboard scrollback.
fn dispatch_live_input(
    engine: &mut dyn TerminalEngine,
    terminal: crate::contracts::TerminalId,
    bytes: Vec<u8>,
) {
    engine.dispatch(EngineCommand::Scroll {
        terminal: terminal.clone(),
        command: ScrollCommand::Bottom,
    });
    engine.dispatch(EngineCommand::Input { terminal, bytes });
}

fn screen_size() -> io::Result<ScreenSize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: `size` points at valid writable storage for the ioctl result.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: TIOCGWINSZ initialized the value after the successful ioctl.
    let size = unsafe { size.assume_init() };
    if size.ws_col == 0 || size.ws_row == 0 {
        return Err(io::Error::other("terminal has no usable cell dimensions"));
    }
    Ok(ScreenSize::new(size.ws_col, size.ws_row))
}

struct OuterTerminal;

impl OuterTerminal {
    fn enter() -> io::Result<Self> {
        let mut previous = std::mem::MaybeUninit::<libc::termios>::zeroed();
        // SAFETY: stdin is a valid descriptor and `previous` is writable.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, previous.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let previous = unsafe { previous.assume_init() };
        let mut raw = previous;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: stdin is a valid descriptor and `raw` is a valid termios value.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        *saved_termios()
            .lock()
            .expect("terminal state lock poisoned") = Some(previous);
        let mut stdout = io::stdout();
        if let Err(error) =
            stdout.write_all(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h")
        {
            restore_outer_terminal();
            return Err(error);
        }
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for OuterTerminal {
    fn drop(&mut self) {
        restore_outer_terminal();
    }
}

fn saved_termios() -> &'static Mutex<Option<libc::termios>> {
    SAVED_TERMIOS.get_or_init(|| Mutex::new(None))
}

fn saved_panic_hook() -> &'static Mutex<Option<PanicHook>> {
    SAVED_PANIC_HOOK.get_or_init(|| Mutex::new(None))
}

fn restore_outer_terminal() {
    let _ =
        io::stdout().write_all(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?25h\x1b[?1049l");
    let _ = io::stdout().flush();
    if let Some(previous) = saved_termios()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take())
    {
        // SAFETY: `previous` came from tcgetattr for this process's stdin.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &previous) };
    }
}

struct PanicGuard;

impl PanicGuard {
    fn install() -> Self {
        let previous = panic::take_hook();
        *saved_panic_hook().lock().expect("panic hook lock poisoned") = Some(previous);
        panic::set_hook(Box::new(|info| {
            restore_outer_terminal();
            if let Ok(hooks) = saved_panic_hook().lock()
                && let Some(previous) = hooks.as_ref()
            {
                previous(info);
            }
        }));
        Self
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        let _ = panic::take_hook();
        if let Some(previous) = saved_panic_hook()
            .lock()
            .ok()
            .and_then(|mut hook| hook.take())
        {
            panic::set_hook(previous);
        }
    }
}

extern "C" fn caught_signal(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

struct SignalGuard {
    #[cfg(unix)]
    previous_int: libc::sigaction,
    #[cfg(unix)]
    previous_term: libc::sigaction,
}

impl SignalGuard {
    fn install() -> io::Result<Self> {
        // SAFETY: the handler only performs an atomic store, which is signal-safe.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = caught_signal as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous_int = std::mem::zeroed();
            let mut previous_term = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, &action, &mut previous_int) != 0
                || libc::sigaction(libc::SIGTERM, &action, &mut previous_term) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                previous_int,
                previous_term,
            })
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: these are the handlers captured by `install`.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous_int, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.previous_term, std::ptr::null_mut());
        }
    }
}

#[derive(Default)]
struct KeyReader {
    bytes: Vec<u8>,
}

enum InputEvent {
    Key(Key),
    Paste(String),
    Wheel {
        pointer: Position,
        command: ScrollCommand,
    },
    Mouse {
        pointer: Position,
        action: MouseAction,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouseAction {
    Down,
    Move,
    Up,
}

fn mouse_action(
    deck: &mut DeckState,
    position: Option<usize>,
    action: MouseAction,
    now: Timestamp,
    last_click: &mut Option<(usize, Timestamp)>,
) -> Option<crate::contracts::ActionCommand> {
    match action {
        MouseAction::Down => {
            deck.cancel_drag();
            deck.begin_drag(position?);
            None
        }
        MouseAction::Move => {
            deck.update_drag(position);
            None
        }
        MouseAction::Up => {
            deck.update_drag(position);
            let (source, target) = deck.finish_drag()?;
            if let Some(target) = target {
                *last_click = None;
                return Some(crate::contracts::ActionCommand::SelectPosition(
                    if deck.active() == Some(source) {
                        target
                    } else {
                        source
                    },
                ));
            }
            if position == Some(source) && deck.active() != Some(source) {
                if last_click.is_some_and(|(previous, at)| {
                    previous == source
                        && now.unix_millis.saturating_sub(at.unix_millis) <= DOUBLE_CLICK_WINDOW
                }) {
                    *last_click = None;
                    return Some(crate::contracts::ActionCommand::SelectPosition(source));
                }
                *last_click = Some((source, now));
            } else {
                *last_click = None;
            }
            None
        }
    }
}

impl KeyReader {
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<InputEvent>> {
        let mut poll = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `poll` points at one valid descriptor record.
        let result = unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as libc::c_int) };
        if result < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            return Ok(Vec::new());
        }
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if result > 0 {
            let mut read = [0; 4096];
            let count = io::stdin().read(&mut read)?;
            self.bytes.extend_from_slice(&read[..count]);
        }
        Ok(self.decode(result == 0))
    }

    fn decode(&mut self, flush_escape: bool) -> Vec<InputEvent> {
        let mut events = Vec::new();
        while !self.bytes.is_empty() {
            if self.bytes.starts_with(b"\x1b[200~") {
                let Some(end) = self.bytes.windows(6).position(|part| part == b"\x1b[201~") else {
                    break;
                };
                let text = String::from_utf8_lossy(&self.bytes[6..end]).into_owned();
                self.bytes.drain(..end + 6);
                events.push(InputEvent::Paste(text));
                continue;
            }
            if self.bytes.starts_with(b"\x1b[<") {
                let Some(end) = self.bytes[3..]
                    .iter()
                    .position(|byte| matches!(byte, b'M' | b'm'))
                else {
                    break;
                };
                let end = end + 3;
                let event = mouse_event(&self.bytes[3..end], self.bytes[end]);
                self.bytes.drain(..=end);
                if let Some(event) = event {
                    events.push(event);
                }
                continue;
            }
            let sequence = [
                (b"\x1b[A".as_slice(), Key::Up),
                (b"\x1b[B".as_slice(), Key::Down),
                (b"\x1b[C".as_slice(), Key::Right),
                (b"\x1b[D".as_slice(), Key::Left),
                (b"\x1b[5~".as_slice(), Key::PageUp),
                (b"\x1b[6~".as_slice(), Key::PageDown),
            ];
            if let Some((bytes, key)) = sequence
                .iter()
                .find(|(bytes, _)| self.bytes.starts_with(bytes))
            {
                self.bytes.drain(..bytes.len());
                events.push(InputEvent::Key(*key));
                continue;
            }
            if self.bytes[0] == 0x1b {
                if self.bytes.len() == 1 && !flush_escape {
                    break;
                }
                self.bytes.remove(0);
                events.push(InputEvent::Key(Key::Escape));
                continue;
            }
            if !self.bytes[0].is_ascii() {
                match std::str::from_utf8(&self.bytes) {
                    Ok(text) => {
                        let character = text.chars().next().expect("nonempty input");
                        self.bytes.drain(..character.len_utf8());
                        events.push(InputEvent::Key(Key::Char(character)));
                        continue;
                    }
                    Err(error) if error.error_len().is_none() => break,
                    Err(_) => {
                        self.bytes.remove(0);
                        events.push(InputEvent::Key(Key::Char('\u{fffd}')));
                        continue;
                    }
                }
            }
            let byte = self.bytes.remove(0);
            let key = match byte {
                b'\r' | b'\n' => Key::Enter,
                b'\t' => Key::Tab,
                0x7f => Key::Backspace,
                1..=26 => Key::Ctrl((b'a' + byte - 1) as char),
                byte => Key::Char(byte as char),
            };
            events.push(InputEvent::Key(key));
        }
        events
    }
}

/// Decodes an xterm SGR mouse report. Mouse reports stay in the outer UI and
/// never leak into the active shell.
fn mouse_event(bytes: &[u8], terminator: u8) -> Option<InputEvent> {
    let mut fields = std::str::from_utf8(bytes).ok()?.split(';');
    let code = fields.next()?.parse::<u16>().ok()?;
    let column = fields.next()?.parse::<u16>().ok()?.saturating_sub(1);
    let row = fields.next()?.parse::<u16>().ok()?.saturating_sub(1);
    let pointer = Position::new(column, row);
    if code & 0b11_000_000 == 64 {
        let command = match code & 0b11 {
            0 => ScrollCommand::Up(WHEEL_LINES),
            1 => ScrollCommand::Down(WHEEL_LINES),
            _ => return None,
        };
        return Some(InputEvent::Wheel { pointer, command });
    }
    let action = match (terminator, code & 0b11, code & 32) {
        (b'm', _, _) => MouseAction::Up,
        (b'M', 0, 0) => MouseAction::Down,
        (b'M', _, 32) => MouseAction::Move,
        _ => return None,
    };
    Some(InputEvent::Mouse { pointer, action })
}

struct AnsiBackend {
    output: io::Stdout,
    cursor: Position,
}

impl AnsiBackend {
    fn new() -> io::Result<Self> {
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

#[cfg(test)]
mod tests {
    use super::{InputEvent, KeyReader, MouseAction, dispatch_live_input, mouse_action};
    use crate::{
        contracts::{
            ActionCommand, ScrollCommand, ScrollbackPosition, TerminalEngine, TerminalId,
            TerminalMetadata, Timestamp,
        },
        engine::FakeEngine,
        ui::{DeckState, Key},
    };
    use ratatui::layout::Position;

    #[test]
    fn decoder_keeps_terminal_controls_and_mouse_out_of_the_shell_input_path() {
        let mut reader = KeyReader {
            bytes: "a\x03\x1b[A\x1b[200~paste\x1b[201~\x1b[<64;3;5M\x1b[<0;4;6M\x1b[<32;5;6M\x1b[<0;5;6m界"
                .as_bytes()
                .to_vec(),
        };

        let events = reader.decode(true);

        assert!(matches!(events[0], InputEvent::Key(Key::Char('a'))));
        assert!(matches!(events[1], InputEvent::Key(Key::Ctrl('c'))));
        assert!(matches!(events[2], InputEvent::Key(Key::Up)));
        assert!(matches!(events[3], InputEvent::Paste(ref text) if text == "paste"));
        assert!(matches!(
            events[4],
            InputEvent::Wheel {
                pointer: Position { x: 2, y: 4 },
                command: ScrollCommand::Up(super::WHEEL_LINES),
            }
        ));
        assert!(matches!(
            events[5],
            InputEvent::Mouse {
                pointer: Position { x: 3, y: 5 },
                action: MouseAction::Down,
            }
        ));
        assert!(matches!(
            events[6],
            InputEvent::Mouse {
                pointer: Position { x: 4, y: 5 },
                action: MouseAction::Move,
            }
        ));
        assert!(matches!(
            events[7],
            InputEvent::Mouse {
                pointer: Position { x: 4, y: 5 },
                action: MouseAction::Up,
            }
        ));
        assert!(matches!(events[8], InputEvent::Key(Key::Char('界'))));
    }

    #[test]
    fn live_input_returns_a_wheel_scrolled_terminal_to_its_tail() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_metadata(
            &terminal,
            TerminalMetadata {
                scrollback: ScrollbackPosition {
                    lines_above: 2_179,
                    lines_below: 214,
                },
                ..TerminalMetadata::default()
            },
        );

        dispatch_live_input(&mut engine, terminal.clone(), b"echo live\r".to_vec());

        assert_eq!(
            engine.metadata(&terminal).unwrap().scrollback.lines_below,
            0
        );
        assert_eq!(engine.input(), &[(terminal, b"echo live\r".to_vec())]);
    }

    #[test]
    fn drag_and_double_click_dispatch_the_existing_promotion_action() {
        let mut state = DeckState::new(4);
        let mut last_click = None;
        let at = |millis| Timestamp {
            unix_millis: millis,
        };

        assert_eq!(
            mouse_action(
                &mut state,
                Some(1),
                MouseAction::Down,
                at(0),
                &mut last_click
            ),
            None
        );
        assert_eq!(
            mouse_action(
                &mut state,
                Some(0),
                MouseAction::Move,
                at(1),
                &mut last_click
            ),
            None
        );
        let action = mouse_action(&mut state, Some(0), MouseAction::Up, at(2), &mut last_click);
        assert_eq!(action, Some(ActionCommand::SelectPosition(1)));
        state.apply(&action.unwrap(), &[], at(2));
        assert_eq!(state.active(), Some(1));

        for millis in [10, 20] {
            assert_eq!(
                mouse_action(
                    &mut state,
                    Some(2),
                    MouseAction::Down,
                    at(millis),
                    &mut last_click
                ),
                None
            );
            let action = mouse_action(
                &mut state,
                Some(2),
                MouseAction::Up,
                at(millis + 1),
                &mut last_click,
            );
            if millis == 20 {
                assert_eq!(action, Some(ActionCommand::SelectPosition(2)));
                state.apply(&action.unwrap(), &[], at(millis + 1));
            } else {
                assert_eq!(action, None);
            }
        }
        assert_eq!(state.active(), Some(2));
    }
}
