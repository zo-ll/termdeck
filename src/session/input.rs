use super::*;

/// Live shell input always resumes the active terminal at its tail. Wheel
/// scrolling deliberately has no modal state, unlike keyboard scrollback.
///
/// Returns what the engine answered, so the ctl `input` verb can tell an
/// accepted input from a refused one (#118). Live keystroke and paste paths
/// ignore it: dropping a keystroke against a wedged child is correct as
/// long as the loop never blocks for it.
pub(super) fn dispatch_live_input(
    engine: &mut dyn TerminalEngine,
    terminal: crate::contracts::TerminalId,
    bytes: Vec<u8>,
) -> Vec<crate::contracts::EngineEvent> {
    dispatch_scroll(engine, terminal.clone(), ScrollCommand::Bottom);
    engine.dispatch(EngineCommand::Input { terminal, bytes })
}

pub(super) fn dispatch_scroll(
    engine: &mut dyn TerminalEngine,
    terminal: TerminalId,
    command: ScrollCommand,
) {
    engine.dispatch(EngineCommand::Scroll { terminal, command });
}

/// Bracketed-paste delimiters (DEC 2004): the outer terminal wraps pastes
/// in these, and paste-aware children want them back (#120).
pub(super) const PASTE_OPEN: &[u8] = b"\x1b[200~";
pub(super) const PASTE_CLOSE: &[u8] = b"\x1b[201~";
const MOUSE_OPEN: &[u8] = b"\x1b[<";
/// The largest explicit paste capture the outer terminal accepts. An
/// unterminated paste is discarded after this point rather than retaining an
/// unbounded amount of input in the session loop.
pub(super) const CAPTURE_PAYLOAD: usize = 64 * 1024;
pub(super) const MOUSE_SEQUENCE_CAP: usize = 256;
const ESC_SEQUENCE_CAP: usize = PASTE_OPEN.len();

/// Encodes a paste for one child (#120): bracketed while the child holds
/// DEC 2004, raw bytes otherwise. Typed input (`Bytes`) and agent `text`
/// stay raw — only a paste operation wraps, so a child without paste mode
/// keeps today's submit-on-newline behavior while a paste-aware child gets
/// its region with newlines non-executable.
///
/// The content cannot hold the closer: the outer parse ended the paste at
/// the first one, so the same bytes re-emit unambiguously. The wrapped
/// unit rides the bounded input path like any other write (#118).
pub(super) fn encode_paste(
    engine: &dyn TerminalEngine,
    terminal: &TerminalId,
    text: String,
) -> Vec<u8> {
    if !engine
        .metadata(terminal)
        .is_some_and(|metadata| metadata.bracketed_paste)
    {
        return text.into_bytes();
    }
    let mut bytes = Vec::with_capacity(text.len() + PASTE_OPEN.len() + PASTE_CLOSE.len());
    bytes.extend_from_slice(PASTE_OPEN);
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(PASTE_CLOSE);
    bytes
}

/// What a wheel tick over a pane becomes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WheelRoute {
    /// A line terminal: the engine-owned #25 scrollback viewport.
    Scrollback(ScrollCommand),
    /// An alternate-screen app: bytes for its PTY. `up` is the tick
    /// direction, followed by its selected mouse protocol and DECCKM mode.
    App {
        up: bool,
        mouse: Option<crate::contracts::MouseProtocol>,
        application_cursor: bool,
    },
}

/// Routes a wheel tick: alternate-screen apps scroll natively, everything
/// else keeps termdeck scrollback. Unknown terminals keep the old behavior.
pub(super) fn route_wheel(
    metadata: Option<&TerminalMetadata>,
    command: ScrollCommand,
) -> WheelRoute {
    let app = metadata.is_some_and(|metadata| metadata.alt_screen);
    if !app {
        return WheelRoute::Scrollback(command);
    }
    WheelRoute::App {
        up: matches!(command, ScrollCommand::Up(_)),
        mouse: metadata
            .filter(|metadata| metadata.mouse_reporting)
            .map(|metadata| metadata.mouse_protocol),
        application_cursor: metadata.is_some_and(|metadata| metadata.application_cursor),
    }
}

/// Encodes one app-bound wheel tick in the child's selected mouse protocol
/// (1-based `column`/`row`, already clamped to its grid), or cursor keys when
/// the application did not enable mouse reporting.
/// One tick sends [`WHEEL_LINES`] arrows, the same distance as scrollback.
pub(super) fn app_wheel(
    up: bool,
    column: u16,
    row: u16,
    mouse: Option<crate::contracts::MouseProtocol>,
    application_cursor: bool,
) -> Vec<u8> {
    use crate::contracts::MouseProtocol;

    match mouse {
        Some(MouseProtocol::Sgr) => {
            format!("\x1b[{};{column};{row}M", if up { "<64" } else { "<65" }).into_bytes()
        }
        Some(MouseProtocol::X10) => x10_mouse(up, column, row, false),
        Some(MouseProtocol::Utf8) => x10_mouse(up, column, row, true),
        None => {
            let arrow = match (up, application_cursor) {
                (true, true) => b"\x1bOA",
                (false, true) => b"\x1bOB",
                (true, false) => b"\x1b[A",
                (false, false) => b"\x1b[B",
            };
            arrow.repeat(usize::from(WHEEL_LINES))
        }
    }
}

fn x10_mouse(up: bool, column: u16, row: u16, utf8: bool) -> Vec<u8> {
    let component = |value: u16| {
        let value = value.saturating_add(32);
        if utf8 {
            char::from_u32(u32::from(value))
                .unwrap_or(' ')
                .to_string()
                .into_bytes()
        } else {
            vec![value.min(255) as u8]
        }
    };
    let mut bytes = b"\x1b[M".to_vec();
    bytes.extend(component(if up { 64 } else { 65 }));
    bytes.extend(component(column));
    bytes.extend(component(row));
    bytes
}

#[derive(Default)]
pub(super) struct KeyReader {
    /// Bytes not yet assigned to a partial sequence.
    pub(super) bytes: Vec<u8>,
    state: ParseState,
}

/// The small set of terminal sequences the outer UI owns. Each state owns a
/// bounded capture; once that capture fills, the matching discard state keeps
/// consuming through the terminator without retaining or replaying hostile
/// input as shell keys.
#[derive(Default)]
enum ParseState {
    #[default]
    Ground,
    Escape(Vec<u8>),
    Paste {
        payload: Vec<u8>,
        close_match: usize,
    },
    Mouse(Vec<u8>),
    DiscardPaste {
        close_match: usize,
    },
    DiscardMouse,
}

pub(super) enum InputEvent {
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
pub(super) enum MouseAction {
    Down,
    Move,
    Up,
    /// A secondary-button release. The deck has no gesture for it; the picker
    /// uses it for the minus its parity table gives the badge (#42 A2).
    SecondaryUp,
    /// A primary release with shift held: the pointer twin of `⇧↓` / `⇧↑`.
    /// The deck has no gesture for this one either.
    RangeUp,
}

pub(super) fn mouse_action(
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
        // The deck owns neither of these; the picker does. What they do here
        // is let go: whatever the press before them armed ends with them, so
        // a shift-click can never leave a drag hanging behind it.
        MouseAction::SecondaryUp | MouseAction::RangeUp => {
            deck.cancel_drag();
            *last_click = None;
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
    pub(super) fn read(&mut self, timeout: Duration) -> io::Result<Vec<InputEvent>> {
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

    pub(super) fn decode(&mut self, flush_escape: bool) -> Vec<InputEvent> {
        let mut events = Vec::new();
        loop {
            let progressed = match self.state {
                ParseState::Ground => self.decode_ground(flush_escape, &mut events),
                ParseState::Escape(_) => self.decode_escape(flush_escape, &mut events),
                ParseState::Paste { .. } => self.decode_paste(&mut events),
                ParseState::Mouse(_) => self.decode_mouse(&mut events),
                ParseState::DiscardPaste { .. } => self.discard_paste(),
                ParseState::DiscardMouse => self.discard_mouse(),
            };
            if !progressed {
                break;
            }
        }
        events
    }

    fn decode_ground(&mut self, flush_escape: bool, events: &mut Vec<InputEvent>) -> bool {
        if self.bytes.starts_with(PASTE_OPEN) {
            self.bytes.drain(..PASTE_OPEN.len());
            self.state = ParseState::Paste {
                payload: Vec::new(),
                close_match: 0,
            };
            return true;
        }
        if self.bytes.starts_with(MOUSE_OPEN) {
            self.bytes.drain(..MOUSE_OPEN.len());
            self.state = ParseState::Mouse(Vec::new());
            return true;
        }
        if let Some((bytes, key)) = key_sequence()
            .iter()
            .find(|(bytes, _)| self.bytes.starts_with(bytes))
        {
            self.bytes.drain(..bytes.len());
            events.push(InputEvent::Key(*key));
            return true;
        }
        let Some(byte) = self.bytes.first().copied() else {
            return false;
        };
        if byte == 0x1b {
            self.bytes.remove(0);
            self.state = ParseState::Escape(vec![byte]);
            return true;
        }
        if !byte.is_ascii() {
            return self.decode_utf8(flush_escape, events);
        }
        self.bytes.remove(0);
        let key = match byte {
            // `⏎` is CR; LF is what `ctrl+j` sends, and it falls through
            // to the control range below so it stays that key (#95).
            b'\r' => Key::Enter,
            b'\t' => Key::Tab,
            0x7f => Key::Backspace,
            1..=26 => Key::Ctrl((b'a' + byte - 1) as char),
            byte => Key::Char(byte as char),
        };
        events.push(InputEvent::Key(key));
        true
    }

    fn decode_escape(&mut self, flush_escape: bool, events: &mut Vec<InputEvent>) -> bool {
        let ParseState::Escape(mut partial) = std::mem::take(&mut self.state) else {
            unreachable!("escape decoder only runs in the escape state");
        };
        if partial == PASTE_OPEN {
            self.state = ParseState::Paste {
                payload: Vec::new(),
                close_match: 0,
            };
            return true;
        }
        if partial == MOUSE_OPEN {
            self.state = ParseState::Mouse(Vec::new());
            return true;
        }
        if let Some((_, key)) = key_sequence().iter().find(|(bytes, _)| *bytes == partial) {
            events.push(InputEvent::Key(*key));
            return true;
        }
        if is_escape_prefix(&partial) && partial.len() < ESC_SEQUENCE_CAP {
            if let Some(byte) = self.bytes.first().copied() {
                self.bytes.remove(0);
                partial.push(byte);
                self.state = ParseState::Escape(partial);
                return true;
            }
            if !flush_escape {
                self.state = ParseState::Escape(partial);
                return false;
            }
        }
        // A timeout or a byte outside our owned sequence language turns the
        // leading ESC into its ordinary key and gives the remaining bytes a
        // fresh ground-state parse.
        let rest = partial.split_off(1);
        self.bytes.splice(..0, rest);
        events.push(InputEvent::Key(Key::Escape));
        true
    }

    fn decode_paste(&mut self, events: &mut Vec<InputEvent>) -> bool {
        let Some(byte) = self.bytes.first().copied() else {
            return false;
        };
        self.bytes.remove(0);
        let ParseState::Paste {
            mut payload,
            mut close_match,
        } = std::mem::take(&mut self.state)
        else {
            unreachable!("paste decoder only runs in the paste state");
        };
        let mut overflow = false;
        if byte == PASTE_CLOSE[close_match] {
            close_match += 1;
        } else {
            if close_match > 0 {
                overflow |= !append_capture(&mut payload, &PASTE_CLOSE[..close_match]);
                close_match = 0;
            }
            if byte == PASTE_CLOSE[0] {
                close_match = 1;
            } else {
                overflow |= !append_capture(&mut payload, &[byte]);
            }
        }
        if close_match == PASTE_CLOSE.len() {
            events.push(InputEvent::Paste(
                String::from_utf8_lossy(&payload).into_owned(),
            ));
        } else if overflow {
            self.state = ParseState::DiscardPaste { close_match };
        } else {
            self.state = ParseState::Paste {
                payload,
                close_match,
            };
        }
        true
    }

    fn decode_mouse(&mut self, events: &mut Vec<InputEvent>) -> bool {
        let Some(byte) = self.bytes.first().copied() else {
            return false;
        };
        self.bytes.remove(0);
        let ParseState::Mouse(mut payload) = std::mem::take(&mut self.state) else {
            unreachable!("mouse decoder only runs in the mouse state");
        };
        if matches!(byte, b'M' | b'm') {
            if let Some(event) = mouse_event(&payload, byte) {
                events.push(event);
            }
        } else if payload.len() == MOUSE_SEQUENCE_CAP {
            self.state = ParseState::DiscardMouse;
        } else {
            payload.push(byte);
            self.state = ParseState::Mouse(payload);
        }
        true
    }

    fn discard_paste(&mut self) -> bool {
        let Some(byte) = self.bytes.first().copied() else {
            return false;
        };
        self.bytes.remove(0);
        let ParseState::DiscardPaste { close_match } = std::mem::take(&mut self.state) else {
            unreachable!("discard parser only runs in the discard-paste state");
        };
        let close_match = next_marker_match(close_match, byte);
        if close_match < PASTE_CLOSE.len() {
            self.state = ParseState::DiscardPaste { close_match };
        }
        true
    }

    fn discard_mouse(&mut self) -> bool {
        let Some(byte) = self.bytes.first().copied() else {
            return false;
        };
        self.bytes.remove(0);
        if matches!(byte, b'M' | b'm') {
            self.state = ParseState::Ground;
        } else {
            self.state = ParseState::DiscardMouse;
        }
        true
    }

    fn decode_utf8(&mut self, flush_escape: bool, events: &mut Vec<InputEvent>) -> bool {
        let byte = self.bytes[0];
        let width = match byte {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        if self.bytes.len() < width && !flush_escape {
            return false;
        }
        if width > 1
            && self.bytes.len() >= width
            && let Ok(text) = std::str::from_utf8(&self.bytes[..width])
        {
            let character = text.chars().next().expect("complete UTF-8 scalar");
            self.bytes.drain(..width);
            events.push(InputEvent::Key(Key::Char(character)));
            return true;
        }
        // Invalid or timed-out incomplete UTF-8 consumes one byte only. That
        // leaves a valid scalar before a later malformed byte intact.
        self.bytes.remove(0);
        events.push(InputEvent::Key(Key::Char('\u{fffd}')));
        true
    }

    #[cfg(test)]
    pub(super) fn retained_len(&self) -> usize {
        self.bytes.len()
            + match &self.state {
                ParseState::Ground | ParseState::DiscardMouse => 0,
                ParseState::Escape(bytes) | ParseState::Mouse(bytes) => bytes.len(),
                ParseState::Paste {
                    payload,
                    close_match,
                } => payload.len() + close_match,
                ParseState::DiscardPaste { close_match } => *close_match,
            }
    }
}

fn key_sequence() -> [(&'static [u8], Key); 9] {
    [
        // The modified arrows come first: `\x1b[1;2A` must not be read as an
        // escape followed by junk.
        (b"\x1b[1;2A", Key::ShiftUp),
        (b"\x1b[1;2B", Key::ShiftDown),
        (b"\x1b[Z", Key::ShiftTab),
        (b"\x1b[A", Key::Up),
        (b"\x1b[B", Key::Down),
        (b"\x1b[C", Key::Right),
        (b"\x1b[D", Key::Left),
        (b"\x1b[5~", Key::PageUp),
        (b"\x1b[6~", Key::PageDown),
    ]
}

fn is_escape_prefix(bytes: &[u8]) -> bool {
    PASTE_OPEN.starts_with(bytes)
        || MOUSE_OPEN.starts_with(bytes)
        || key_sequence()
            .iter()
            .any(|(sequence, _)| sequence.starts_with(bytes))
}

fn append_capture(payload: &mut Vec<u8>, bytes: &[u8]) -> bool {
    if payload.len().saturating_add(bytes.len()) > CAPTURE_PAYLOAD {
        return false;
    }
    payload.extend_from_slice(bytes);
    true
}

fn next_marker_match(close_match: usize, byte: u8) -> usize {
    if byte == PASTE_CLOSE[close_match] {
        close_match + 1
    } else {
        usize::from(byte == PASTE_CLOSE[0])
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
    // Bit 2 of the code is shift, and a release carries it like any other
    // report, so the range gesture is still legible at the point it arrives.
    let shifted = code & 0b100 != 0;
    let action = match (terminator, code & 0b11, code & 32) {
        // A release reports the button it releases, so the secondary one is
        // still distinguishable at the point it arrives.
        (b'm', 2, _) => MouseAction::SecondaryUp,
        (b'm', _, _) if shifted => MouseAction::RangeUp,
        (b'm', _, _) => MouseAction::Up,
        (b'M', 0, 0) => MouseAction::Down,
        (b'M', _, 32) => MouseAction::Move,
        _ => return None,
    };
    Some(InputEvent::Mouse { pointer, action })
}
