use super::*;

/// Live shell input always resumes the active terminal at its tail. Wheel
/// scrolling deliberately has no modal state, unlike keyboard scrollback.
pub(super) fn dispatch_live_input(
    engine: &mut dyn TerminalEngine,
    terminal: crate::contracts::TerminalId,
    bytes: Vec<u8>,
) {
    dispatch_scroll(engine, terminal.clone(), ScrollCommand::Bottom);
    engine.dispatch(EngineCommand::Input { terminal, bytes });
}

pub(super) fn dispatch_scroll(
    engine: &mut dyn TerminalEngine,
    terminal: TerminalId,
    command: ScrollCommand,
) {
    engine.dispatch(EngineCommand::Scroll { terminal, command });
}

/// What a wheel tick over a pane becomes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WheelRoute {
    /// A line terminal: the engine-owned #25 scrollback viewport.
    Scrollback(ScrollCommand),
    /// An alternate-screen app: bytes for its PTY. `up` is the tick
    /// direction and `mouse` whether the app enabled mouse reporting.
    App { up: bool, mouse: bool },
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
        mouse: metadata.is_some_and(|metadata| metadata.mouse_reporting),
    }
}

/// Encodes one app-bound wheel tick: SGR mouse reports when the app enabled
/// the mouse (1-based `column`/`row`, already clamped to its grid), cursor
/// keys otherwise — the xterm alternate-scroll fallback `less` scrolls on.
/// One tick sends [`WHEEL_LINES`] arrows, the same distance as scrollback.
pub(super) fn app_wheel(up: bool, column: u16, row: u16, mouse: bool) -> Vec<u8> {
    if mouse {
        format!("\x1b[{};{column};{row}M", if up { "<64" } else { "<65" }).into_bytes()
    } else {
        let arrow = if up { b"\x1b[A" } else { b"\x1b[B" };
        arrow.repeat(usize::from(WHEEL_LINES))
    }
}

#[derive(Default)]
pub(super) struct KeyReader {
    pub(super) bytes: Vec<u8>,
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
                // The modified arrows come first: `\x1b[1;2A` must not be
                // read as an escape followed by junk.
                (b"\x1b[1;2A".as_slice(), Key::ShiftUp),
                (b"\x1b[1;2B".as_slice(), Key::ShiftDown),
                (b"\x1b[Z".as_slice(), Key::ShiftTab),
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
            // Half of a sequence is not an escape key: wait for the rest
            // rather than tearing `\x1b[1;2B` into an escape and `1;2B`.
            if !flush_escape
                && sequence
                    .iter()
                    .any(|(bytes, _)| bytes.starts_with(self.bytes.as_slice()))
            {
                break;
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
                // `⏎` is CR; LF is what `ctrl+j` sends, and it falls through
                // to the control range below so it stays that key (#95).
                b'\r' => Key::Enter,
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
