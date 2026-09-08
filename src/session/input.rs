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

/// Encodes the token values accepted by `termctl input --keys` into the bytes
/// a terminal receives. Named keys deliberately share the sequences used by
/// [`KeyReader`]: arrows, paging, tab, enter, backspace, escape, and the
/// two shift-arrow forms therefore cannot drift from the outer input path.
///
/// Bare words that are not key-shaped stay literal text, which makes mixed
/// calls such as `C-c Enter Up text` useful. A malformed modifier expression
/// (for example `C-not-a-key`) and an unknown function key are errors instead
/// of silent text. `Raw:` forces the rest of one argument to stay literal,
/// including spaces, so `Raw:Up` sends the three characters `Up` and existing
/// shell-escaped control values such as `$'\x03'` continue to pass through.
pub fn encode_keys<T: AsRef<str>>(values: &[T]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for value in values {
        let value = value.as_ref();
        if let Some(raw) = value.strip_prefix("Raw:") {
            bytes.extend_from_slice(raw.as_bytes());
            continue;
        }
        for token in value.split_whitespace() {
            encode_key_token(token, &mut bytes)?;
        }
    }
    Ok(bytes)
}

fn encode_key_token(token: &str, bytes: &mut Vec<u8>) -> Result<(), String> {
    if let Some(raw) = token.strip_prefix("Raw:") {
        bytes.extend_from_slice(raw.as_bytes());
        return Ok(());
    }

    let (control, meta, shift, name) = modifiers(token)?;
    if let Some(named) = named_key(name, control, meta, shift) {
        bytes.extend_from_slice(&named?);
        return Ok(());
    }

    if control || meta || shift || looks_like_function_key(name) {
        return Err(unknown_key(token));
    }
    bytes.extend_from_slice(token.as_bytes());
    Ok(())
}

fn modifiers(token: &str) -> Result<(bool, bool, bool, &str), String> {
    if !token.contains('-') {
        return Ok((false, false, false, token));
    }
    let mut pieces = token.split('-').peekable();
    let mut control = false;
    let mut meta = false;
    let mut shift = false;
    while let Some(piece) = pieces.peek().copied() {
        // The final component is always the key name, even when it happens
        // to be the literal character C, M, or S.
        if pieces.clone().nth(1).is_none() {
            break;
        }
        let modifier = match piece.to_ascii_uppercase().as_str() {
            "C" => &mut control,
            "M" => &mut meta,
            "S" => &mut shift,
            _ => break,
        };
        if *modifier {
            return Err(unknown_key(token));
        }
        *modifier = true;
        pieces.next();
    }
    let name = pieces.next().ok_or_else(|| unknown_key(token))?;
    if pieces.next().is_some() || name.is_empty() {
        return Err(unknown_key(token));
    }
    Ok((control, meta, shift, name))
}

fn named_key(
    name: &str,
    control: bool,
    meta: bool,
    shift: bool,
) -> Option<Result<Vec<u8>, String>> {
    let upper = name.to_ascii_uppercase();
    let modifier = modifier_number(control, meta, shift);
    let simple = |normal: &[u8]| {
        if control || shift {
            return Err(unknown_key(name));
        }
        let mut encoded = normal.to_vec();
        if meta {
            encoded.insert(0, 0x1b);
        }
        Ok(encoded)
    };
    let csi = |letter: u8| {
        if modifier == 1 {
            Ok([b"\x1b[".as_slice(), &[letter]].concat())
        } else {
            Ok(format!("\x1b[1;{modifier}{}", letter as char).into_bytes())
        }
    };
    let tilde = |number: u8| {
        if modifier == 1 {
            Ok(format!("\x1b[{number}~").into_bytes())
        } else {
            Ok(format!("\x1b[{number};{modifier}~").into_bytes())
        }
    };
    let result = match upper.as_str() {
        "ENTER" => simple(b"\r"),
        "TAB" if shift && !control && !meta => Ok(b"\x1b[Z".to_vec()),
        "TAB" => simple(b"\t"),
        "BACKSPACE" => simple(&[0x7f]),
        "ESCAPE" => simple(&[0x1b]),
        "UP" => csi(b'A'),
        "DOWN" => csi(b'B'),
        "RIGHT" => csi(b'C'),
        "LEFT" => csi(b'D'),
        "PAGEUP" => tilde(5),
        "PAGEDOWN" => tilde(6),
        "HOME" => csi(b'H'),
        "END" => csi(b'F'),
        "F1" | "F2" | "F3" | "F4" => {
            let letter = b'P' + upper.as_bytes()[1] - b'1';
            if modifier == 1 {
                Ok([b"\x1bO".as_slice(), &[letter]].concat())
            } else {
                Ok(format!("\x1b[1;{modifier}{}", letter as char).into_bytes())
            }
        }
        "F5" => tilde(15),
        "F6" => tilde(17),
        "F7" => tilde(18),
        "F8" => tilde(19),
        "F9" => tilde(20),
        "F10" => tilde(21),
        "F11" => tilde(23),
        "F12" => tilde(24),
        _ if control || meta || shift => return character_key(name, control, meta, shift),
        _ => return None,
    };
    Some(result)
}

fn character_key(
    name: &str,
    control: bool,
    meta: bool,
    shift: bool,
) -> Option<Result<Vec<u8>, String>> {
    if name.chars().count() != 1 {
        return None;
    }
    let mut character = name.as_bytes()[0];
    if !character.is_ascii() {
        return Some(Err(unknown_key(name)));
    }
    if shift {
        character.make_ascii_uppercase();
    }
    if control {
        if !matches!(character, b'@'..=b'_') && !character.is_ascii_alphabetic() {
            return Some(Err(unknown_key(name)));
        }
        character = character.to_ascii_uppercase() & 0x1f;
    }
    let mut encoded = vec![character];
    if meta {
        encoded.insert(0, 0x1b);
    }
    Some(Ok(encoded))
}

fn modifier_number(control: bool, meta: bool, shift: bool) -> u8 {
    1 + u8::from(shift) + 2 * u8::from(meta) + 4 * u8::from(control)
}

fn looks_like_function_key(name: &str) -> bool {
    name.strip_prefix(['F', 'f']).is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn unknown_key(token: &str) -> String {
    format!("unknown key name `{token}`; use Raw:{token} for literal text")
}

#[cfg(test)]
mod key_encoding_tests {
    use super::encode_keys;

    #[test]
    fn named_keys_match_the_outer_reader_sequences() {
        assert_eq!(
            encode_keys(&["C-c Enter Up Down Right Left PageUp PageDown Tab Backspace Escape"])
                .unwrap(),
            b"\x03\r\x1b[A\x1b[B\x1b[C\x1b[D\x1b[5~\x1b[6~\t\x7f\x1b"
        );
        assert_eq!(
            encode_keys(&["S-Tab S-Up S-Down"]).unwrap(),
            b"\x1b[Z\x1b[1;2A\x1b[1;2B"
        );
    }

    #[test]
    fn named_keys_cover_function_navigation_and_modifiers() {
        assert_eq!(
            encode_keys(&["Home End F1 F4 F5 F12 M-x C-a"]).unwrap(),
            b"\x1b[H\x1b[F\x1bOP\x1bOS\x1b[15~\x1b[24~\x1bx\x01"
        );
    }

    #[test]
    fn key_values_allow_multiple_arguments_and_literal_text() {
        assert_eq!(
            encode_keys(&["C-c Enter Up", "text"]).unwrap(),
            b"\x03\r\x1b[Atext"
        );
    }

    #[test]
    fn malformed_key_names_name_the_offending_token() {
        let error = encode_keys(&["C-not-a-key"]).unwrap_err();
        assert!(error.contains("C-not-a-key"));
        assert!(error.contains("Raw:C-not-a-key"));
        assert!(encode_keys(&["F13"]).is_err());
    }

    #[test]
    fn raw_control_bytes_and_raw_prefix_remain_literal() {
        assert_eq!(encode_keys(&["\u{3}"]).unwrap(), b"\x03");
        assert_eq!(encode_keys(&["\x1b[A"]).unwrap(), b"\x1b[A");
        assert_eq!(encode_keys(&["Raw:Up"]).unwrap(), b"Up");
        assert_eq!(encode_keys(&["Raw:git status"]).unwrap(), b"git status");
    }
}

/// Encodes a paste for one child (#120): bracketed while the child holds
/// DEC 2004, raw bytes otherwise. Typed input (`Bytes`) and agent `text`
/// stay raw — only a paste operation wraps, so a child without paste mode
/// keeps today's submit-on-newline behavior while a paste-aware child gets
/// its region with newlines non-executable.
///
/// Every caller is checked for a closer before bytes are made ready for the
/// bounded input path (#118). Keyboard paste has already passed through the
/// outer parser, but ctl and copied text have not; accepting a closer from
/// either of those sources would let the payload escape the wrapped region.
pub(super) fn encode_paste(
    engine: &dyn TerminalEngine,
    terminal: &TerminalId,
    text: String,
) -> Result<Vec<u8>, PasteEncodeError> {
    if text
        .as_bytes()
        .windows(PASTE_CLOSE.len())
        .any(|window| window == PASTE_CLOSE)
    {
        return Err(PasteEncodeError::EmbeddedCloser);
    }
    if !engine
        .metadata(terminal)
        .is_some_and(|metadata| metadata.bracketed_paste)
    {
        return Ok(text.into_bytes());
    }
    let mut bytes = Vec::with_capacity(text.len() + PASTE_OPEN.len() + PASTE_CLOSE.len());
    bytes.extend_from_slice(PASTE_OPEN);
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(PASTE_CLOSE);
    Ok(bytes)
}

/// A paste payload cannot contain the delimiter that ends the region we add.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PasteEncodeError {
    EmbeddedCloser,
}

/// The pointer's text-selection gesture (#148).
///
/// The press decides what a drag is: on pane content it arms a selection, on
/// the title row or the border it arms nothing here and the reorder drag
/// keeps the pointer as it always did. A press that is released without
/// moving to another cell selects nothing at all, so the double-click that
/// promotes a pane is untouched (`mouse-selection.md` §1).
#[derive(Default)]
pub(super) struct Selecting {
    /// The content cell the press landed on, while it may still become a
    /// selection.
    press: Option<(usize, u16, u16)>,
    /// The pane a selection is running in, once one has started.
    active: Option<usize>,
}

/// What one pointer event means to the selection gesture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SelectStep {
    /// Not this gesture's event: the deck's other pointer gestures own it.
    Pass,
    /// The selection now runs from its anchor to here.
    Extend(Selection),
    /// The gesture ended. Whatever is selected is what gets copied.
    Copy,
}

impl Selecting {
    /// The pane a press is being held in, if any. The gesture belongs to that
    /// pane until the release, wherever the pointer has travelled to, so this
    /// is the pane the caller clamps the pointer into.
    pub(super) fn pane(&self) -> Option<usize> {
        self.press.map(|(position, _, _)| position)
    }

    /// `content` is the cell under the pointer, `None` off a pane's viewport;
    /// `head` is the same pointer clamped into the pane the press landed in,
    /// so a drag that leaves the pane runs to its edge.
    pub(super) fn step(
        &mut self,
        action: MouseAction,
        content: Option<(usize, u16, u16)>,
        head: Option<(u16, u16)>,
    ) -> SelectStep {
        match action {
            MouseAction::Down => {
                self.press = content;
                self.active = None;
                SelectStep::Pass
            }
            MouseAction::Move => {
                let Some((position, column, row)) = self.press else {
                    return SelectStep::Pass;
                };
                let Some(head) = head else {
                    return SelectStep::Pass;
                };
                // The first move off the pressed cell is what tells a
                // selection from a click; after that every move extends.
                if self.active.is_none() && head == (column, row) {
                    return SelectStep::Pass;
                }
                self.active = Some(position);
                SelectStep::Extend(Selection::new(position, (column, row)).to(head))
            }
            MouseAction::Up => {
                self.press = None;
                match self.active.take() {
                    Some(_) => SelectStep::Copy,
                    None => SelectStep::Pass,
                }
            }
            // Neither button has a selection gesture, and both let go of
            // whatever the press before them armed.
            MouseAction::SecondaryUp | MouseAction::RangeUp => {
                self.press = None;
                self.active = None;
                SelectStep::Pass
            }
        }
    }
}

/// Asks the outer terminal to put `text` on the system clipboard: OSC 52,
/// base64-encoded, terminated with BEL (#148).
///
/// This is the one copy target that reaches the machine the user is sitting
/// at when termdeck is running over SSH, and it costs no dependency — see
/// `mouse-selection.md` §4.2 for the providers it was chosen over. It is
/// fire-and-forget: the host may refuse it and there is no reply to wait
/// for, which is what `^g v` is the answer to.
pub(super) fn clipboard_sequence(text: &str) -> Vec<u8> {
    let mut bytes = b"\x1b]52;c;".to_vec();
    bytes.extend_from_slice(base64(text.as_bytes()).as_bytes());
    bytes.push(0x07);
    bytes
}

/// Standard base64 with padding. Twenty lines against a dependency.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut group = [0_u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let packed = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for index in 0..4 {
            encoded.push(if index <= chunk.len() {
                ALPHABET[(packed >> (18 - 6 * index)) as usize & 0x3f] as char
            } else {
                '='
            });
        }
    }
    encoded
}

/// The text a standing selection stands for, read from the frame the engine
/// holds right now. Empty selections — a drag over blank cells — copy
/// nothing rather than clearing the clipboard (`mouse-selection.md` §4.1).
pub(super) fn selection_text(
    engine: &dyn TerminalEngine,
    projects: &[Project],
    selection: &Selection,
) -> Option<String> {
    let frame = engine.frame(&projects.get(selection.position)?.terminal)?;
    let text = selection.text(frame);
    (!text.trim().is_empty()).then_some(text)
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
