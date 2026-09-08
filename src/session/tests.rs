#[cfg(unix)]
use super::dispatch_control;
use super::input::{CAPTURE_PAYLOAD, MOUSE_SEQUENCE_CAP, PASTE_CLOSE, PASTE_OPEN};
use super::{
    InputEvent, KeyReader, MouseAction, SelectStep, Selecting, WheelRoute, add_failure,
    add_terminal, app_wheel, chosen, clipboard_sequence, close_terminal, dispatch_live_input,
    encode_paste, master_terminal, mouse_action, now, open_terminals, request_close,
    resize_terminals, resized, route_wheel, schedule_expiry_repaint, selection_text,
    spawn_terminals, spawn_terminals_with_socket, terminal_sizes, workspace_of,
};
use crate::{
    contracts::{
        ActionCommand, EngineCommand, EngineEvent, MouseProtocol, NotifyKind, Project, ScreenSize,
        ScrollCommand, ScrollbackPosition, TerminalEngine, TerminalId, TerminalMetadata, Timestamp,
    },
    engine::FakeEngine,
    ui::{
        DeckState, Input, Key, Modal, Notifications, PickerState, Reaction, Selection, SheetState,
    },
};
use ratatui::layout::Position;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// #143: restoring the process panic hook is forbidden while unwinding. Run
/// this in a child so an accidental second panic is observable as SIGABRT.
#[test]
fn panic_guard_unwinds_without_aborting() {
    const CHILD: &str = "TERMDECK_PANIC_GUARD_UNWIND_CHILD";

    if std::env::var_os(CHILD).is_some() {
        let _guard = super::outer::PanicGuard::install();
        panic!("panic through PanicGuard");
    }

    let status = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([
            "--exact",
            "session::tests::panic_guard_unwinds_without_aborting",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .status()
        .expect("run panic guard child test");

    assert_eq!(
        status.code(),
        Some(101),
        "child must exit from one panic rather than aborting: {status:?}"
    );
}

#[test]
fn decoder_keeps_terminal_controls_and_mouse_out_of_the_shell_input_path() {
    let mut reader = reader_of(
        "a\x03\x1b[A\x1b[200~paste\x1b[201~\x1b[<64;3;5M\x1b[<0;4;6M\x1b[<32;5;6M\x1b[<0;5;6m界"
            .as_bytes(),
    );

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

/// #120: every fragmentation boundary of a bracketed paste decodes as one
/// paste once complete — and a partial opener emits nothing, not an escape
/// plus literal keys (the audit's split after `ESC[2`).
#[test]
fn bracketed_paste_survives_every_fragmentation_boundary() {
    let full = b"\x1b[200~hi\nbye\x1b[201~";
    // Single-shot baseline.
    let mut reader = reader_of(full);
    let events = reader.decode(false);
    assert!(
        matches!(events.as_slice(), [InputEvent::Paste(text)] if text == "hi\nbye"),
        "single-shot paste must decode"
    );
    assert!(reader.bytes.is_empty());
    // Every split point: the prefix waits silently, the rest completes.
    for split in 1..full.len() {
        let mut reader = reader_of(&full[..split]);
        let events = reader.decode(false);
        assert!(
            events.is_empty(),
            "split at {split}: an opener fragment must wait, not emit keys"
        );
        reader.bytes.extend_from_slice(&full[split..]);
        let events = reader.decode(false);
        assert!(
            matches!(events.as_slice(), [InputEvent::Paste(text)] if text == "hi\nbye"),
            "split at {split}: the completed paste must decode"
        );
        assert!(
            reader.bytes.is_empty(),
            "split at {split}: nothing may linger"
        );
    }
}

/// #126: a valid UTF-8 scalar must be consumed before a later malformed byte
/// is replaced. Decoding the whole unread suffix turned `é` plus `0xff` into
/// three replacement characters.
#[test]
fn utf8_consumes_a_valid_prefix_before_a_later_malformed_byte() {
    let events = reader_of(&[0xc3, 0xa9, 0xff]).decode(true);

    assert!(matches!(
        events.as_slice(),
        [
            InputEvent::Key(Key::Char('é')),
            InputEvent::Key(Key::Char('\u{fffd}')),
        ]
    ));
}

/// #126: no unterminated sequence is allowed to grow the session reader
/// without bound. Overflow is deliberately discarded through its terminator
/// so hostile escape traffic cannot become shell input after recovery.
#[test]
fn unterminated_paste_and_mouse_reports_are_bounded_and_recover() {
    let mut paste = KeyReader::default();
    paste.bytes.extend_from_slice(PASTE_OPEN);
    paste
        .bytes
        .extend(std::iter::repeat_n(b'x', CAPTURE_PAYLOAD + 1));
    assert!(paste.decode(false).is_empty());
    assert!(paste.retained_len() <= CAPTURE_PAYLOAD + PASTE_CLOSE.len());
    paste.bytes.extend_from_slice(PASTE_CLOSE);
    paste.bytes.extend_from_slice(b"ok");
    assert!(matches!(
        paste.decode(false).as_slice(),
        [
            InputEvent::Key(Key::Char('o')),
            InputEvent::Key(Key::Char('k'))
        ]
    ));

    let mut mouse = KeyReader::default();
    mouse.bytes.extend_from_slice(b"\x1b[<");
    mouse
        .bytes
        .extend(std::iter::repeat_n(b'9', MOUSE_SEQUENCE_CAP + 1));
    assert!(mouse.decode(false).is_empty());
    assert!(mouse.retained_len() <= MOUSE_SEQUENCE_CAP);
    mouse.bytes.extend_from_slice(b"Mx");
    let events = mouse.decode(false);
    assert_eq!(event_signature(&events), ["key:Char('x')"]);
}

/// #126: arbitrary read boundaries must not change a valid input stream's
/// meaning. The deterministic pseudo-fuzz partitions include boundaries in
/// every multibyte scalar and every owned terminal sequence.
#[test]
fn parser_preserves_valid_streams_across_fuzzed_chunk_partitions() {
    let stream = b"a\xc3\xa9\x1b[A\x1b[200~hi\nthere\x1b[201~\x1b[<64;3;5Mz";
    let baseline = event_signature(&reader_of(stream).decode(true));

    // Every possible partition of this compact stream: the scalar and arrow
    // both cross arbitrary read boundaries, including the ESC prefix alone.
    let compact = b"\xc3\xa9\x1b[A";
    let compact_baseline = event_signature(&reader_of(compact).decode(true));
    for boundaries in 0..(1 << (compact.len() - 1)) {
        let mut reader = KeyReader::default();
        let mut events = Vec::new();
        let mut start = 0;
        for end in 1..compact.len() {
            if boundaries & (1 << (end - 1)) != 0 {
                reader.bytes.extend_from_slice(&compact[start..end]);
                events.extend(reader.decode(false));
                start = end;
            }
        }
        reader.bytes.extend_from_slice(&compact[start..]);
        events.extend(reader.decode(true));
        assert_eq!(event_signature(&events), compact_baseline, "{boundaries:b}");
    }

    for seed in 0..256_u64 {
        let mut reader = KeyReader::default();
        let mut events = Vec::new();
        let mut cursor = 0;
        let mut state = seed;
        while cursor < stream.len() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let width = 1 + (state as usize % 9);
            let end = (cursor + width).min(stream.len());
            reader.bytes.extend_from_slice(&stream[cursor..end]);
            events.extend(reader.decode(false));
            cursor = end;
        }
        events.extend(reader.decode(true));
        assert_eq!(event_signature(&events), baseline, "seed {seed}");
        assert_eq!(reader.retained_len(), 0, "seed {seed} left input behind");
    }
}

/// #120: paste encoding follows the child's DEC 2004 mode — bracketed
/// while the child holds it, raw bytes otherwise (and for unknown
/// terminals, the safe default).
#[test]
fn paste_encoding_follows_the_child_bracketed_paste_mode() {
    let terminal = TerminalId::new("pane");
    let mut engine = FakeEngine::new([terminal.clone()]);
    assert_eq!(
        encode_paste(&engine, &terminal, "a\nb".to_owned()).unwrap(),
        b"a\nb".to_vec(),
        "no mode means raw bytes, as before"
    );
    assert_eq!(
        encode_paste(&engine, &TerminalId::new("ghost"), "a\nb".to_owned()).unwrap(),
        b"a\nb".to_vec(),
        "unknown terminals stay raw"
    );
    engine.set_metadata(
        &terminal,
        TerminalMetadata {
            bracketed_paste: true,
            ..Default::default()
        },
    );
    assert_eq!(
        encode_paste(&engine, &terminal, "SAFE\nTEXT".to_owned()).unwrap(),
        b"\x1b[200~SAFE\nTEXT\x1b[201~".to_vec(),
        "paste mode means a delimited region"
    );
}

/// The paste encoder is the shared trust boundary: ctl and copied text do
/// not have the outer parser's guarantee that a close marker was stripped.
#[test]
fn paste_encoding_refuses_an_embedded_bracketed_paste_closer() {
    let terminal = TerminalId::new("pane");
    let mut engine = FakeEngine::new([terminal.clone()]);
    let payload = format!(
        "safe{close}unsafe",
        close = String::from_utf8_lossy(PASTE_CLOSE)
    );

    assert_eq!(
        encode_paste(&engine, &terminal, payload.clone()),
        Err(super::input::PasteEncodeError::EmbeddedCloser),
        "the check applies even before the child advertises paste mode"
    );
    engine.set_metadata(
        &terminal,
        TerminalMetadata {
            bracketed_paste: true,
            ..Default::default()
        },
    );
    assert_eq!(
        encode_paste(&engine, &terminal, payload),
        Err(super::input::PasteEncodeError::EmbeddedCloser),
        "a wrapped region must never contain its own closer"
    );
}

/// A refused ctl paste has not entered the engine at all. In particular it
/// must not send an opener or a prefix before reporting the error to ctl.
#[cfg(target_os = "linux")]
#[test]
fn ctl_paste_with_an_embedded_closer_is_refused_atomically() {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    let size = ScreenSize::new(80, 24);
    let terminal = TerminalId::new("pane");
    let probe = std::env::temp_dir().join(format!(
        "termdeck-paste-atomic-{}-{}",
        std::process::id(),
        now().unix_millis
    ));
    let _ = std::fs::remove_file(&probe);
    let mut projects = vec![Project {
        terminal: terminal.clone(),
        path: PathBuf::from("/"),
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "stty raw -echo; exec dd bs=1 of={} 2>/dev/null",
                probe.display()
            ),
        ],
        shell_hook: false,
    }];
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used = BTreeSet::from([terminal.to_string()]);
    let mut quit = false;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !probe.exists() {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(probe.exists(), "the recording child never started");

    let response = dispatch_control(
        crate::ctl::Request {
            schema: crate::ctl::SCHEMA.to_owned(),
            verb: "input".to_owned(),
            id: Some(terminal.to_string()),
            paste: Some(format!(
                "safe{close}touch should-not-run\\n",
                close = String::from_utf8_lossy(PASTE_CLOSE)
            )),
            ..Default::default()
        },
        None,
        &mut workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        false,
        std::path::Path::new("/tmp/termdeck-ctl-test.sock"),
        true,
        &mut closed,
        &mut used,
        &mut quit,
    );

    let error = response.error.expect("ctl must report the rejected paste");
    assert_eq!(error.code, 3);
    assert!(error.message.contains("bracketed-paste closer"));
    thread::sleep(Duration::from_millis(100));
    assert!(
        std::fs::read(&probe).unwrap().is_empty(),
        "rejection must not dispatch an opener or a payload prefix"
    );
    engine.dispatch(EngineCommand::Shutdown);
    let _ = std::fs::remove_file(&probe);
}

/// #120 audit repro at the byte level: a child holding DEC 2004 receives
/// the paste as a delimited REGION — markers intact around the content, so
/// its newlines arrive as data rather than submitted commands. `dd bs=1`
/// records every stdin byte unbuffered, so even a SIGKILLed child leaves
/// exactly what reached it; the file must equal the region byte for byte.
#[cfg(target_os = "linux")]
#[test]
fn a_child_with_paste_mode_receives_the_bracketed_region() {
    use std::time::{Duration, Instant};

    let probe = std::env::temp_dir().join(format!(
        "termdeck-paste-probe-{}-{}",
        std::process::id(),
        now().unix_millis
    ));
    let size = ScreenSize::new(80, 24);
    let projects = vec![Project {
        terminal: TerminalId::new("prober".to_owned()),
        path: PathBuf::from("/"),
        command: vec![
            "/usr/bin/bash".to_owned(),
            "-c".to_owned(),
            format!(
                "stty raw -echo; printf '\\033[?2004h'; exec dd bs=1 of={} 2>/dev/null",
                probe.display()
            ),
        ],
        shell_hook: false,
    }];
    let deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let terminal = projects[0].terminal.clone();
    // The child's mode set surfaces through real output first.
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline
        && !engine
            .metadata(&terminal)
            .is_some_and(|metadata| metadata.bracketed_paste)
    {
        engine.drain_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        engine
            .metadata(&terminal)
            .is_some_and(|metadata| metadata.bracketed_paste),
        "the child never enabled paste mode"
    );

    let expected = b"\x1b[200~SAFE\nTEXT\x1b[201~".to_vec();
    let bytes = encode_paste(&engine, &terminal, "SAFE\nTEXT".to_owned()).unwrap();
    assert_eq!(bytes, expected, "the dispatch must carry the region");
    engine.dispatch(EngineCommand::Input {
        terminal: terminal.clone(),
        bytes,
    });
    // Every byte acknowledged in the file before shutdown may reap.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && std::fs::read(&probe)
            .map(|contents| contents.len())
            .unwrap_or(0)
            < expected.len()
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.dispatch(EngineCommand::Shutdown);

    let recorded = std::fs::read(&probe).unwrap();
    let _ = std::fs::remove_file(&probe);
    assert_eq!(
        recorded, expected,
        "the child must receive the region, not stripped bytes"
    );
}

/// A validated workspace carries its scrollback setting through the session
/// startup boundary. The capacity is off-screen history, so a whole-grid
/// peek may include it plus the visible rows.
#[cfg(target_os = "linux")]
#[test]
fn workspace_scrollback_reaches_the_initial_engine() {
    use std::time::{Duration, Instant};

    const HISTORY: usize = 1;
    let size = ScreenSize::new(80, 24);
    let terminal = TerminalId::new("limited");
    let project = Project {
        terminal: terminal.clone(),
        path: PathBuf::from("/"),
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "i=0; while [ $i -lt 32 ]; do printf 'line-%s\\n' \"$i\"; i=$((i + 1)); done"
                .to_owned(),
        ],
        shell_hook: false,
    };
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), vec![project]);
    workspace.scrollback = HISTORY;
    let deck = DeckState::new(workspace.projects.len());
    let mut engine = spawn_terminals_with_socket(
        &workspace.projects,
        &deck,
        size,
        workspace.scrollback,
        std::path::Path::new("/tmp/termdeck-scrollback-test.sock"),
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline
        && !matches!(
            engine.status(&terminal),
            Some(crate::contracts::TerminalStatus::Exited { code: Some(0) })
        )
    {
        engine.drain_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        engine.status(&terminal),
        Some(crate::contracts::TerminalStatus::Exited { code: Some(0) })
    ));
    let lines = engine.history_lines(&terminal, usize::MAX).unwrap();
    let viewport_rows = usize::from(engine.frame(&terminal).unwrap().size.rows);
    assert!(
        lines.len() <= HISTORY + viewport_rows,
        "peek exceeded history plus viewport: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("line-31")),
        "latest output was lost: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("line-0")),
        "evicted history remained visible: {lines:?}"
    );
    engine.dispatch(EngineCommand::Shutdown);
}

/// The raw terminal sends CR for `⏎` and LF for `ctrl+j`. Merging them made
/// `ctrl+j` transmit a Carriage Return, so LF never reached the shell (#95).
#[test]
fn the_decoder_keeps_line_feed_apart_from_carriage_return() {
    let mut reader = reader_of(b"\n\r");

    let events = reader.decode(true);

    assert!(matches!(events[0], InputEvent::Key(Key::Ctrl('j'))));
    assert!(matches!(events[1], InputEvent::Key(Key::Enter)));
    assert_eq!(events.len(), 2);
}

fn reader_of(bytes: &[u8]) -> KeyReader {
    let mut reader = KeyReader::default();
    reader.bytes.extend_from_slice(bytes);
    reader
}

fn event_signature(events: &[InputEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            InputEvent::Key(key) => format!("key:{key:?}"),
            InputEvent::Paste(text) => format!("paste:{text:?}"),
            InputEvent::Wheel { pointer, command } => {
                format!("wheel:{}:{}:{command:?}", pointer.x, pointer.y)
            }
            InputEvent::Mouse { pointer, action } => {
                format!("mouse:{}:{}:{action:?}", pointer.x, pointer.y)
            }
        })
        .collect()
}

/// The picker's range keys arrive as modified arrows, which nothing read
/// before #56 — `\x1b[1;2A` would have been torn into an escape and the
/// characters `1;2A`.
#[test]
fn the_decoder_reads_shift_arrows() {
    let mut reader = reader_of(b"\x1b[1;2A\x1b[1;2B\x1b[A\x1b[B");

    let events = reader.decode(true);

    assert!(matches!(events[0], InputEvent::Key(Key::ShiftUp)));
    assert!(matches!(events[1], InputEvent::Key(Key::ShiftDown)));
    assert!(
        matches!(
            reader_of(b"\x1b[Z").decode(true)[0],
            InputEvent::Key(Key::ShiftTab)
        ),
        "and `⇧⇥`, which the runtime-add sheet cycles roots with"
    );
    assert!(
        matches!(events[2], InputEvent::Key(Key::Up)),
        "and the plain arrows still read as themselves"
    );
    assert!(matches!(events[3], InputEvent::Key(Key::Down)));
    assert_eq!(events.len(), 4);
}

/// Half of a sequence is not an escape key. A terminal can deliver one in
/// two reads, and the half that arrived must wait for its other half
/// rather than becoming `esc` followed by stray characters.
#[test]
fn a_half_arrived_sequence_waits_for_the_rest() {
    let mut reader = reader_of(b"\x1b[1;");

    assert!(
        reader.decode(false).is_empty(),
        "nothing is decided from half a sequence"
    );
    assert_eq!(
        reader.retained_len(),
        b"\x1b[1;".len(),
        "the partial sequence is retained by the parser state"
    );

    reader.bytes.extend_from_slice(b"2B");
    let events = reader.decode(false);

    assert!(matches!(events[0], InputEvent::Key(Key::ShiftDown)));
    assert!(reader.bytes.is_empty());
}

/// The same guard must not swallow a real escape: once the poll has timed
/// out with nothing following it, `esc` is `esc`.
#[test]
fn a_lone_escape_is_still_the_escape_key() {
    let mut reader = reader_of(b"\x1b");
    assert!(
        reader.decode(false).is_empty(),
        "it might still be a prefix"
    );

    let events = reader.decode(true);

    assert!(matches!(events[0], InputEvent::Key(Key::Escape)));
}

/// Shift held on a click is the range gesture, and it is distinguishable
/// from the ordinary release and from the secondary button.
#[test]
fn the_decoder_reads_a_shifted_click() {
    let mut reader = reader_of(b"\x1b[<4;7;9m\x1b[<0;7;9m\x1b[<2;7;9m");

    let events = reader.decode(true);

    assert!(matches!(
        events[0],
        InputEvent::Mouse {
            pointer: Position { x: 6, y: 8 },
            action: MouseAction::RangeUp,
        }
    ));
    assert!(matches!(
        events[1],
        InputEvent::Mouse {
            action: MouseAction::Up,
            ..
        }
    ));
    assert!(matches!(
        events[2],
        InputEvent::Mouse {
            action: MouseAction::SecondaryUp,
            ..
        }
    ));
}

/// A shift-click's press is an ordinary press as far as the deck can
/// tell, so it arms a drag. Its release is the picker's range gesture,
/// which the deck does not act on — but it must still let go, or the
/// session would carry a phantom drag until some later event cleared it.
#[test]
fn a_shifted_release_lets_go_of_whatever_its_press_armed() {
    let mut state = DeckState::new(4);
    let mut last_click = None;
    let at = |millis| Timestamp {
        unix_millis: millis,
    };

    mouse_action(
        &mut state,
        Some(1),
        MouseAction::Down,
        at(0),
        &mut last_click,
    );
    assert_eq!(state.dragged(), Some(1), "the press armed a drag");

    let action = mouse_action(
        &mut state,
        None,
        MouseAction::RangeUp,
        at(1),
        &mut last_click,
    );

    assert_eq!(action, None, "the deck has no gesture for it");
    assert_eq!(state.dragged(), None, "and it let go of the one it had");
    assert_eq!(last_click, None, "including the armed double-click");

    // The secondary button lets go the same way.
    mouse_action(
        &mut state,
        Some(2),
        MouseAction::Down,
        at(2),
        &mut last_click,
    );
    assert_eq!(state.dragged(), Some(2));
    mouse_action(
        &mut state,
        None,
        MouseAction::SecondaryUp,
        at(3),
        &mut last_click,
    );
    assert_eq!(state.dragged(), None);
}

/// What a committed sheet adds (#50 A3): names are made against what is
/// already running, so a second instance of an open path becomes `-2`
/// rather than an identity the engine would refuse.
#[test]
fn a_committed_sheet_names_its_additions_around_the_running_ones() {
    let running = [
        Project {
            terminal: TerminalId::new("api"),
            path: PathBuf::from("/code/api"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
        Project {
            terminal: TerminalId::new("web"),
            path: PathBuf::from("/code/web"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
    ];
    let roots = [crate::ui::Entry::folder("code", "/code")];
    let mut sheet = SheetState::new(&roots);
    let api = crate::ui::Entry::repository("api", "/code/api");
    let docs = crate::ui::Entry::repository("docs", "/code/docs");
    // One repository the session already holds, asked for again, and one
    // it does not.
    sheet.state_mut().add(&api);
    sheet.state_mut().add(&docs);
    sheet.state_mut().add(&api);

    let mut used: BTreeSet<String> = running
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let added = chosen(&sheet, &mut used);

    let names: Vec<String> = added
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    assert_eq!(names, ["api-2", "docs", "api-3"], "{names:?}");
    assert_eq!(added[0].path, PathBuf::from("/code/api"));
    assert_eq!(
        added[2].path, added[0].path,
        "both instances run in the same directory"
    );
}

#[test]
fn a_committed_sheet_opens_a_plain_folder() {
    let roots = [crate::ui::Entry::folder("code", "/code")];
    let mut sheet = SheetState::new(&roots);
    let folder = crate::ui::Entry::folder("archive", "/code/archive");
    sheet.state_mut().add(&folder);

    let added = chosen(&sheet, &mut BTreeSet::new());

    assert_eq!(added.len(), 1);
    assert_eq!(added[0].terminal, TerminalId::new("archive"));
    assert_eq!(added[0].path, PathBuf::from("/code/archive"));
    assert_eq!(added[0].command, ["bash", "-l"]);
    assert!(added[0].shell_hook);
}

#[test]
fn a_picker_launch_uses_the_login_shell_hook() {
    let mut state = PickerState::new();
    state.add(&crate::ui::Entry::folder("archive", "/code/archive"));

    let workspace = workspace_of(&state);

    assert_eq!(workspace.projects.len(), 1);
    assert_eq!(workspace.projects[0].command, ["bash", "-l"]);
    assert!(workspace.projects[0].shell_hook);
}

/// The sheet names the running panes in order beside paths they already
/// hold, without preventing another instance.
#[test]
fn the_open_terminals_are_listed_with_their_pane_numbers() {
    let running = [
        Project {
            terminal: TerminalId::new("api"),
            path: PathBuf::from("/code/api"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
        Project {
            terminal: TerminalId::new("web"),
            path: PathBuf::from("/code/web"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
    ];

    let open = open_terminals(&running);

    assert_eq!(open[0].pane, 1);
    assert_eq!(open[1].pane, 2);
    assert_eq!(open[1].path, PathBuf::from("/code/web"));
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

/// #74 revert-fail: the wheel over a line terminal keeps termdeck
/// scrollback, byte for byte the #25 behavior — only the alternate
/// screen leaves this path.
#[test]
fn wheel_over_a_line_terminal_keeps_termdeck_scrollback() {
    assert_eq!(
        route_wheel(None, ScrollCommand::Up(3)),
        WheelRoute::Scrollback(ScrollCommand::Up(3))
    );
    assert_eq!(
        route_wheel(Some(&TerminalMetadata::default()), ScrollCommand::Down(3)),
        WheelRoute::Scrollback(ScrollCommand::Down(3))
    );
}

/// #74 revert-fail: the wheel over a full-screen app reaches the app —
/// SGR reports when it enabled the mouse, cursor keys otherwise — and
/// never becomes termdeck scrollback.
#[test]
fn wheel_over_an_alt_screen_app_reaches_the_app() {
    let mouse = TerminalMetadata {
        alt_screen: true,
        mouse_reporting: true,
        mouse_protocol: MouseProtocol::Sgr,
        ..TerminalMetadata::default()
    };
    assert_eq!(
        route_wheel(Some(&mouse), ScrollCommand::Up(3)),
        WheelRoute::App {
            up: true,
            mouse: Some(MouseProtocol::Sgr),
            application_cursor: false,
        }
    );
    assert_eq!(
        route_wheel(Some(&mouse), ScrollCommand::Down(3)),
        WheelRoute::App {
            up: false,
            mouse: Some(MouseProtocol::Sgr),
            application_cursor: false,
        }
    );
    let keys = TerminalMetadata {
        alt_screen: true,
        application_cursor: true,
        ..TerminalMetadata::default()
    };
    assert_eq!(
        route_wheel(Some(&keys), ScrollCommand::Up(3)),
        WheelRoute::App {
            up: true,
            mouse: None,
            application_cursor: true,
        }
    );

    assert_eq!(
        app_wheel(true, 7, 4, Some(MouseProtocol::Sgr), false),
        b"\x1b[<64;7;4M"
    );
    assert_eq!(
        app_wheel(false, 7, 4, Some(MouseProtocol::Sgr), false),
        b"\x1b[<65;7;4M"
    );
    assert_eq!(
        app_wheel(true, 7, 4, Some(MouseProtocol::X10), false),
        b"\x1b[M`'$",
        "ordinary DECSET 1000 uses the original X10 report, not SGR"
    );
    assert_eq!(
        app_wheel(false, 7, 4, Some(MouseProtocol::Utf8), false),
        b"\x1b[Ma'$"
    );
    assert_eq!(app_wheel(true, 1, 1, None, true), b"\x1bOA\x1bOA\x1bOA");
    assert_eq!(app_wheel(false, 1, 1, None, false), b"\x1b[B\x1b[B\x1b[B");
}

/// A workspace of shells for the close tests: each one sleeps, so it is alive
/// until something ends it.
#[cfg(target_os = "linux")]
fn sleepers(count: usize) -> Vec<Project> {
    (1..=count)
        .map(|number| Project {
            terminal: TerminalId::new(format!("t{number}")),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            shell_hook: false,
        })
        .collect()
}

/// #84: closing a stacked pane drops it from the engine and the project list
/// together, and the deck reflows around what is left. The two lists renumber
/// as one, so pane 3 is still the third project afterwards.
#[cfg(target_os = "linux")]
#[test]
fn closing_a_stacked_pane_drops_it_from_the_engine_and_the_list() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(3);
    let closed = projects[1].terminal.clone();
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        1,
        &mut tombstones
    ));

    assert_eq!(engine.frame(&closed), None, "the engine dropped it");
    assert_eq!(
        projects
            .iter()
            .map(|project| project.terminal.to_string())
            .collect::<Vec<_>>(),
        ["t1", "t3"]
    );
    assert_eq!(deck.active(), Some(0), "the master kept the frame");
    assert_eq!(deck.stack(), [1]);
    // The deck's positions still index the project list they came from.
    assert_eq!(projects[deck.stack()[0]].terminal.to_string(), "t3");
    engine.dispatch(EngineCommand::Shutdown);
}

/// Closing the master promotes the pane behind it, and the promoted one is
/// resized to the master viewport it has just taken.
#[cfg(target_os = "linux")]
#[test]
fn closing_the_master_promotes_and_resizes_what_takes_its_place() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let promoted = projects[1].terminal.clone();
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();
    let preview = engine.frame(&promoted).unwrap().size;

    assert!(close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));

    assert_eq!(deck.active(), Some(0));
    assert_eq!(projects[0].terminal, promoted);
    assert!(resize_terminals(&mut engine, &projects, &deck, size));
    let master = engine.frame(&promoted).unwrap().size;
    assert_ne!(master, preview, "it holds the master viewport now");
    // Nothing stacked, so the master is the whole body (#76).
    assert_eq!(master, terminal_sizes(&projects, &deck, size)[0].unwrap());
    engine.dispatch(EngineCommand::Shutdown);
}

/// The close itself keeps no last pane back: it ends the only shell and
/// empties both lists. What asks first is `request_close`, below.
#[cfg(target_os = "linux")]
#[test]
fn closing_the_last_pane_leaves_nothing_for_the_session_to_run() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let only = projects[0].terminal.clone();
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));

    assert!(projects.is_empty(), "the session has nothing left to run");
    assert_eq!(deck.active(), None);
    assert_eq!(engine.frame(&only), None);
    // Out of range afterwards, and refused rather than panicking.
    assert!(!close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));
}

/// #84: the last pane is the session, so the close gesture asks the same
/// confirmation `^g q` asks instead of ending the run outright. Nothing is
/// closed while the question stands — the shell is still there to go back to.
#[cfg(target_os = "linux")]
#[test]
fn closing_the_last_pane_asks_the_quit_confirmation() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let only = projects[0].terminal.clone();
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(
        !request_close(&mut engine, &mut projects, &mut deck, 0, &mut tombstones),
        "it asked instead of closing"
    );

    assert_eq!(deck.modal(), Some(Modal::Quit));
    assert_eq!(projects.len(), 1);
    assert_eq!(deck.active(), Some(0));
    assert!(engine.frame(&only).is_some(), "its shell is still running");
    engine.dispatch(EngineCommand::Shutdown);
}

/// The confirmation is the ordinary one, keys and all: `n` and `esc` leave
/// the pane and its shell exactly as they were, and only `y` ends the
/// session — by the quit path, which is the loop's own break.
#[cfg(target_os = "linux")]
#[test]
fn the_last_pane_survives_a_cancelled_confirmation_and_leaves_on_a_confirmed_one() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let only = projects[0].terminal.clone();
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();
    let mut input = Input::new(size.rows);

    for key in [Key::Char('n'), Key::Escape] {
        request_close(&mut engine, &mut projects, &mut deck, 0, &mut tombstones);
        assert_eq!(deck.modal(), Some(Modal::Quit));

        assert_eq!(input.press(key, &mut deck, &projects, now()), None);

        assert_eq!(deck.modal(), None, "the question is gone");
        assert_eq!(projects.len(), 1, "and the pane is not");
        assert_eq!(deck.active(), Some(0));
        assert!(engine.frame(&only).is_some(), "its shell kept running");
    }

    request_close(&mut engine, &mut projects, &mut deck, 0, &mut tombstones);
    assert_eq!(
        input.press(Key::Char('y'), &mut deck, &projects, now()),
        Some(Reaction::Quit),
        "the exit is the quit path's"
    );
    assert_eq!(deck.modal(), None);
    engine.dispatch(EngineCommand::Shutdown);
}

/// Every other close is unchanged: the deck it leaves behind still holds a
/// terminal, so the shell ends there and then, with nothing to confirm.
#[cfg(target_os = "linux")]
#[test]
fn closing_a_pane_that_is_not_the_last_asks_nothing() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let closed = projects[1].terminal.clone();
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(request_close(
        &mut engine,
        &mut projects,
        &mut deck,
        1,
        &mut tombstones
    ));

    assert_eq!(deck.modal(), None, "nothing to confirm");
    assert_eq!(engine.frame(&closed), None, "the engine ended it");
    assert_eq!(projects.len(), 1);
    // The one left is the last, so its own close asks.
    assert!(!request_close(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));
    assert_eq!(deck.modal(), Some(Modal::Quit));
    engine.dispatch(EngineCommand::Shutdown);
}

#[cfg(target_os = "linux")]
#[test]
fn ctl_controls_share_the_live_paths_and_enforce_their_gates() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    let mut notifies = crate::ui::Notifications::new();
    let request = |verb: &str| crate::ctl::Request {
        schema: crate::ctl::SCHEMA.to_owned(),
        verb: verb.to_owned(),
        ..Default::default()
    };
    macro_rules! call {
        ($request:expr, $caller:expr, $allow:expr) => {
            dispatch_control(
                $request,
                $caller,
                &mut workspace,
                &mut projects,
                &mut deck,
                &mut engine,
                &mut notifies,
                size,
                false,
                socket,
                $allow,
                &mut closed,
                &mut used,
                &mut quit,
            )
        };
    }

    let mut sheet_open = request("open");
    sheet_open.path = Some("/tmp".to_owned());
    let response = dispatch_control(
        sheet_open,
        None,
        &mut workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        true,
        socket,
        false,
        &mut closed,
        &mut used,
        &mut quit,
    );
    assert_eq!(response.error.unwrap().code, 3);

    let mut open = request("open");
    open.path = Some("/tmp".to_owned());
    let response = call!(open, None, false);
    assert!(response.ok);
    let opened = response.data.unwrap()["id"].as_str().unwrap().to_owned();
    assert_eq!(projects.len(), 3);

    let mut promote = request("promote");
    promote.id = Some("t2".to_owned());
    assert!(call!(promote, None, false).ok);
    assert_eq!(deck.active(), Some(1));
    assert!(deck.toggle_collapse(0));
    let mut zoom = request("zoom");
    zoom.on = Some(true);
    assert_eq!(call!(zoom, None, false).data.unwrap()["zoom"], true);
    assert!(deck.collapsed(0), "zoom changes the model, not the fold");

    let mut input = request("input");
    input.id = Some("t2".to_owned());
    input.text = Some("echo ctl\n".to_owned());
    assert_eq!(call!(input.clone(), None, false).error.unwrap().code, 3);
    let response = call!(input, None, true);
    assert!(response.ok, "session input permission opens the gate");
    assert_eq!(response.data.unwrap()["queued"], false);
    let mut forced = request("input");
    forced.id = Some("t2".to_owned());
    forced.keys = Some("\u{3}".to_owned());
    forced.force = true;
    assert!(
        call!(forced, None, false).ok,
        "per-call force opens the gate"
    );

    let mut self_close = request("close");
    self_close.id = Some("t2".to_owned());
    assert_eq!(call!(self_close, Some("t2"), false).error.unwrap().code, 3);
    let mut close = request("close");
    close.id = Some(opened.clone());
    assert!(call!(close, None, false).ok);
    let mut again = request("close");
    again.id = Some(opened);
    assert_eq!(call!(again, None, false).data.unwrap()["already"], true);

    let mut close_t2 = request("close");
    close_t2.id = Some("t2".to_owned());
    assert!(call!(close_t2, None, false).ok);
    let mut last = request("close");
    last.id = Some("t1".to_owned());
    assert_eq!(call!(last.clone(), None, false).error.unwrap().code, 3);
    last.force = true;
    assert_eq!(call!(last, None, false).data.unwrap()["last"], true);
    assert!(quit, "forced last close takes the quit path");
    engine.dispatch(EngineCommand::Shutdown);
}

#[cfg(target_os = "linux")]
#[test]
fn ctl_input_to_an_exited_pane_is_refused_truthfully() {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    let size = ScreenSize::new(144, 42);
    let mut projects = vec![Project {
        terminal: TerminalId::new("exited"),
        path: PathBuf::from("/"),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
        shell_hook: false,
    }];
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");
    let terminal = projects[0].terminal.clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    while matches!(
        engine.status(&terminal),
        Some(
            crate::contracts::TerminalStatus::Starting | crate::contracts::TerminalStatus::Running
        )
    ) && Instant::now() < deadline
    {
        engine.drain_events();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(
            engine.status(&terminal),
            Some(crate::contracts::TerminalStatus::Exited { .. })
        ),
        "the fixture terminal must exit before ctl writes to it"
    );

    let response = dispatch_control(
        crate::ctl::Request {
            schema: crate::ctl::SCHEMA.to_owned(),
            verb: "input".to_owned(),
            id: Some(terminal.to_string()),
            text: Some("echo should-not-run\\n".to_owned()),
            ..Default::default()
        },
        None,
        &mut workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        false,
        socket,
        true,
        &mut closed,
        &mut used,
        &mut quit,
    );
    let error = response.error.expect("an exited pane refuses input");
    assert_eq!(error.code, 3);
    assert!(error.message.contains("not live"));
    engine.dispatch(EngineCommand::Shutdown);
}

/// #118/#122: an agent `input` verb tells the truth about a slow or wedged
/// pane. A queued request says so, and a saturated queue refuses with code 3
/// instead of hanging the loop; the pane stays live behind the refusal.
#[cfg(target_os = "linux")]
#[test]
fn ctl_input_to_a_wedged_pane_is_refused_truthfully() {
    use std::time::{Duration, Instant};

    let size = ScreenSize::new(144, 42);
    // A raw-mode sleeper: it never reads stdin, and raw mode keeps the
    // kernel from absorbing input into the line discipline, so the queue
    // saturates instead of draining into the kernel (canonical mode would
    // swallow megabytes without backpressure).
    let mut projects = vec![Project {
        terminal: TerminalId::new("t1".to_owned()),
        path: PathBuf::from("/"),
        command: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "stty raw -echo; exec sleep 30".to_owned(),
        ],
        shell_hook: false,
    }];
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");

    // A slow reader accepts the request, but it cannot flush this much input
    // immediately. ctl reports that it is queued instead of claiming an
    // immediate delivery.
    let queued = dispatch_control(
        crate::ctl::Request {
            schema: crate::ctl::SCHEMA.to_owned(),
            verb: "input".to_owned(),
            id: Some("t1".to_owned()),
            text: Some("x".repeat(512 * 1024)),
            ..Default::default()
        },
        None,
        &mut workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        false,
        socket,
        true,
        &mut closed,
        &mut used,
        &mut quit,
    );
    assert!(queued.ok, "a queue with room accepts input");
    assert_eq!(queued.data.unwrap()["queued"], true);

    let chunk = vec![b'x'; 64 * 1024];
    let started = Instant::now();
    let mut dropped = false;
    for _ in 0..64 {
        let events = engine.dispatch(EngineCommand::Input {
            terminal: projects[0].terminal.clone(),
            bytes: chunk.clone(),
        });
        if events
            .iter()
            .any(|event| matches!(event, EngineEvent::InputDropped { .. }))
        {
            dropped = true;
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "saturating dispatches must return, not block"
        );
    }
    assert!(dropped, "the sleeper must saturate its input queue");
    // Top up to the last byte: chunk saturation leaves up to one chunk of
    // slack, and the ctl probe below is only twelve bytes. Nothing drains
    // in between — the sleeper never reads — so this converges exactly.
    let mut filled = false;
    for _ in 0..128 * 1024 {
        let events = engine.dispatch(EngineCommand::Input {
            terminal: projects[0].terminal.clone(),
            bytes: b"x".to_vec(),
        });
        if events
            .iter()
            .any(|event| matches!(event, EngineEvent::InputDropped { .. }))
        {
            filled = true;
            break;
        }
    }
    assert!(filled, "the queue must fill to its last byte");

    let request = crate::ctl::Request {
        schema: crate::ctl::SCHEMA.to_owned(),
        verb: "input".to_owned(),
        id: Some("t1".to_owned()),
        text: Some("echo wedged\n".to_owned()),
        ..Default::default()
    };
    let response = dispatch_control(
        request,
        None,
        &mut workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        false,
        socket,
        true,
        &mut closed,
        &mut used,
        &mut quit,
    );
    let error = response.error.expect("a wedged pane refuses input");
    assert_eq!(error.code, 3);
    assert!(
        error.message.contains("queue"),
        "the refusal names the queue: {}",
        error.message
    );
    assert_eq!(
        engine.status(&projects[0].terminal),
        Some(&crate::contracts::TerminalStatus::Running),
        "a refused input is not a failed terminal"
    );
    engine.dispatch(EngineCommand::Shutdown);
}

/// #121 audit repro: open → close → reopen the same directory hands out a
/// FRESH identity, so a delayed close on the tombstoned id reports
/// `already` and cannot touch the replacement pane. Pre-fix the reopen
/// reused the name and the stale close killed the new pane.
#[cfg(target_os = "linux")]
#[test]
fn a_reopened_directory_never_reuses_its_tombstoned_identity() {
    use std::collections::BTreeSet;

    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");
    let request = |verb: &str| crate::ctl::Request {
        schema: crate::ctl::SCHEMA.to_owned(),
        verb: verb.to_owned(),
        ..Default::default()
    };
    macro_rules! control {
        ($request:expr, $caller:expr, $allow:expr) => {
            dispatch_control(
                $request,
                $caller,
                &mut workspace,
                &mut projects,
                &mut deck,
                &mut engine,
                &mut notifies,
                size,
                false,
                socket,
                $allow,
                &mut closed,
                &mut used,
                &mut quit,
            )
        };
    }

    let mut open = request("open");
    open.path = Some("/tmp".to_owned());
    let response = control!(open, None, false);
    assert!(response.ok);
    let first = response.data.unwrap()["id"].as_str().unwrap().to_owned();

    let mut close = request("close");
    close.id = Some(first.clone());
    let response = control!(close, None, false);
    assert!(response.ok, "closing the open pane succeeds");
    assert!(closed.contains(&first), "the close is tombstoned");

    let mut reopen = request("open");
    reopen.path = Some("/tmp".to_owned());
    let response = control!(reopen, None, false);
    assert!(response.ok);
    let second = response.data.unwrap()["id"].as_str().unwrap().to_owned();
    assert_ne!(
        first, second,
        "a reopened directory must not reuse the tombstoned id"
    );

    // The delayed stale close: already gone, and the replacement lives.
    let mut stale = request("close");
    stale.id = Some(first.clone());
    let response = control!(stale, None, false);
    assert!(response.ok);
    assert_eq!(
        response.data.unwrap()["already"],
        true,
        "a tombstoned close is idempotent, not a kill"
    );
    assert_eq!(
        engine.status(&TerminalId::new(&second)),
        Some(&crate::contracts::TerminalStatus::Running),
        "the replacement pane survives the stale close"
    );
    assert!(
        projects
            .iter()
            .any(|project| project.terminal.to_string() == second),
        "the replacement stays listed"
    );

    // Agent retries on the stale id fail unknown — never against the new
    // pane — while the live id keeps working.
    let mut peek = request("peek");
    peek.id = Some(first.clone());
    assert_eq!(control!(peek, None, false).error.unwrap().code, 2);
    let mut input = request("input");
    input.id = Some(first.clone());
    input.text = Some("echo stale\n".to_owned());
    let stale = control!(input, None, true);
    let error = stale.error.expect("a tombstoned pane refuses input");
    assert_eq!(error.code, 3);
    assert!(error.message.contains("closed"));
    let mut live = request("input");
    live.id = Some(second.clone());
    live.text = Some("echo live\n".to_owned());
    assert!(control!(live, None, true).ok, "the live id keeps working");
    engine.dispatch(EngineCommand::Shutdown);
}

/// #121: repeated open/close cycles on one directory hand out a distinct
/// identity each time, and every closed one stays tombstoned.
#[cfg(target_os = "linux")]
#[test]
fn identities_stay_unique_across_close_reopen_cycles() {
    use std::collections::BTreeSet;

    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");
    macro_rules! control {
        ($request:expr) => {
            dispatch_control(
                $request,
                None,
                &mut workspace,
                &mut projects,
                &mut deck,
                &mut engine,
                &mut notifies,
                size,
                false,
                socket,
                false,
                &mut closed,
                &mut used,
                &mut quit,
            )
        };
    }
    let mut ids = Vec::new();
    for _ in 0..3 {
        let open = crate::ctl::Request {
            schema: crate::ctl::SCHEMA.to_owned(),
            verb: "open".to_owned(),
            path: Some("/tmp".to_owned()),
            ..Default::default()
        };
        let response = control!(open);
        assert!(response.ok);
        let id = response.data.unwrap()["id"].as_str().unwrap().to_owned();
        let close = crate::ctl::Request {
            schema: crate::ctl::SCHEMA.to_owned(),
            verb: "close".to_owned(),
            id: Some(id.clone()),
            ..Default::default()
        };
        assert!(control!(close).ok);
        ids.push(id);
    }
    assert_eq!(ids.len(), 3);
    assert!(
        ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2],
        "no identity is ever handed out twice: {ids:?}"
    );
    assert!(
        ids.iter().all(|id| closed.contains(id)),
        "every closed identity stays tombstoned"
    );
    assert_eq!(projects.len(), 1, "only the sleeper remains");
    engine.dispatch(EngineCommand::Shutdown);
}

/// #121: keyboard, mouse, and API closes tombstone through one primitive.
/// `request_close` (what `^g x` and the `×` affordance both call) and the
/// API arm above it record every removal; the last-pane confirmation and
/// out-of-range positions remove nothing and tombstone nothing.
#[cfg(target_os = "linux")]
#[test]
fn every_close_path_tombstones_through_one_primitive() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let first = projects[0].terminal.to_string();
    let second = projects[1].terminal.to_string();
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(request_close(
        &mut engine,
        &mut projects,
        &mut deck,
        1,
        &mut tombstones
    ));
    assert!(
        tombstones.contains(&second),
        "the gesture primitive records its removal"
    );
    assert!(close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));
    assert!(
        tombstones.contains(&first),
        "the underlying remover records too"
    );
    assert!(projects.is_empty());
    engine.dispatch(EngineCommand::Shutdown);

    // Last-pane confirmation and out-of-range positions remove nothing.
    let mut projects = sleepers(1);
    let only = projects[0].terminal.to_string();
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();
    assert!(!request_close(
        &mut engine,
        &mut projects,
        &mut deck,
        0,
        &mut tombstones
    ));
    assert!(!close_terminal(
        &mut engine,
        &mut projects,
        &mut deck,
        4,
        &mut tombstones
    ));
    assert!(
        !tombstones.contains(&only),
        "asking (not closing) tombstones nothing"
    );
    assert!(tombstones.is_empty());
    engine.dispatch(EngineCommand::Shutdown);
}

/// A position no pane holds closes nothing and asks nothing, whatever is
/// left in the list.
#[cfg(target_os = "linux")]
#[test]
fn a_close_out_of_range_neither_closes_nor_asks() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut tombstones = BTreeSet::new();

    assert!(!request_close(
        &mut engine,
        &mut projects,
        &mut deck,
        4,
        &mut tombstones
    ));

    assert_eq!(deck.modal(), None);
    assert_eq!(projects.len(), 1);
    engine.dispatch(EngineCommand::Shutdown);
}

#[cfg(target_os = "linux")]
#[test]
fn runtime_add_uses_the_usable_terminal_height() {
    let size = ScreenSize::new(100, 30);
    let first = Project {
        terminal: TerminalId::new("first"),
        path: PathBuf::from("/"),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        shell_hook: false,
    };
    let added = Project {
        terminal: TerminalId::new("added"),
        ..first.clone()
    };
    let deck = DeckState::new(1);
    let expected = terminal_sizes(std::slice::from_ref(&first), &deck, size)[0].unwrap();
    let mut engine = spawn_terminals(std::slice::from_ref(&first), &deck, size).unwrap();

    add_terminal(
        &mut engine,
        added.clone(),
        std::slice::from_ref(&first),
        &deck,
        size,
    )
    .unwrap();

    assert_eq!(engine.frame(&added.terminal).unwrap().size, expected);
    assert_eq!(
        engine.frame(&first.terminal).unwrap().size,
        expected,
        "initial terminals use the same usable size"
    );
    engine.dispatch(EngineCommand::Shutdown);
}

#[test]
fn resize_uses_narrow_chrome_for_a_short_wide_window() {
    let terminal = TerminalId::new("frontend");
    let projects = [Project {
        terminal: terminal.clone(),
        path: PathBuf::from("/"),
        command: vec!["sh".to_owned()],
        shell_hook: false,
    }];
    let mut engine = FakeEngine::new([terminal.clone()]);
    let deck = DeckState::new(1);

    resize_terminals(&mut engine, &projects, &deck, ScreenSize::new(120, 13));

    assert_eq!(
        engine.frame(&terminal).unwrap().size,
        ScreenSize::new(114, 7),
        "the narrow pane strip leaves the prompt's two extra chrome rows"
    );
}

#[test]
fn pty_size_is_the_visible_pane_interior() {
    let project = Project {
        terminal: TerminalId::new("frontend"),
        path: PathBuf::from("/"),
        command: vec!["sh".to_owned()],
        shell_hook: false,
    };
    let deck = DeckState::new(1);
    assert_eq!(
        terminal_sizes(
            std::slice::from_ref(&project),
            &deck,
            ScreenSize::new(100, 30)
        ),
        // No stacked previews, so no stack column: the master is the
        // full body, not a split share of it.
        vec![Some(ScreenSize::new(94, 26))]
    );
    assert_eq!(
        terminal_sizes(
            std::slice::from_ref(&project),
            &deck,
            ScreenSize::new(99, 30)
        ),
        vec![Some(ScreenSize::new(93, 24))]
    );
    assert_eq!(
        terminal_sizes(
            std::slice::from_ref(&project),
            &deck,
            ScreenSize::new(120, 13)
        ),
        vec![Some(ScreenSize::new(114, 7))],
        "a wide window still falls back to narrow when too short"
    );
}

#[test]
fn visible_panes_resize_after_layout_changes() {
    let projects = [
        Project {
            terminal: TerminalId::new("first"),
            path: PathBuf::from("/"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
        Project {
            terminal: TerminalId::new("second"),
            path: PathBuf::from("/"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
    ];
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let mut deck = DeckState::new(projects.len());
    let area = ScreenSize::new(144, 42);

    assert!(resize_terminals(&mut engine, &projects, &deck, area));
    let initial = terminal_sizes(&projects, &deck, area);
    assert_eq!(
        engine.frame(&projects[0].terminal).unwrap().size,
        initial[0].unwrap()
    );
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        ScreenSize::new(80, 24)
    );

    assert!(deck.toggle_collapse(1));
    assert!(resize_terminals(&mut engine, &projects, &deck, area));
    let expanded = terminal_sizes(&projects, &deck, area);
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        expanded[1].unwrap()
    );

    assert!(deck.set_master_ratio(0.55));
    assert!(resize_terminals(&mut engine, &projects, &deck, area));
    let dragged = terminal_sizes(&projects, &deck, area);
    assert_eq!(
        engine.frame(&projects[0].terminal).unwrap().size,
        dragged[0].unwrap()
    );
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        dragged[1].unwrap()
    );

    assert!(deck.promote(1, Timestamp::default()));
    assert!(resize_terminals(&mut engine, &projects, &deck, area));
    let promoted = terminal_sizes(&projects, &deck, area);
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        promoted[1].unwrap()
    );

    assert!(deck.apply(&ActionCommand::ToggleZoom, &projects, Timestamp::default()));
    assert!(resize_terminals(&mut engine, &projects, &deck, area));
    let zoomed = terminal_sizes(&projects, &deck, area);
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        zoomed[1].unwrap()
    );

    let resized = ScreenSize::new(120, 30);
    assert!(resize_terminals(&mut engine, &projects, &deck, resized));
    assert_eq!(
        engine.frame(&projects[1].terminal).unwrap().size,
        terminal_sizes(&projects, &deck, resized)[1].unwrap()
    );
}

#[test]
fn timing_visibility_matches_the_panes_the_deck_renders() {
    let projects = [
        Project {
            terminal: TerminalId::new("first"),
            path: PathBuf::from("/"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
        Project {
            terminal: TerminalId::new("second"),
            path: PathBuf::from("/"),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        },
    ];
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let mut deck = DeckState::new(projects.len());
    let area = ScreenSize::new(144, 42);

    resize_terminals(&mut engine, &projects, &deck, area);
    assert_eq!(
        engine.timing_visible(),
        &BTreeSet::from([projects[0].terminal.clone(), projects[1].terminal.clone()]),
        "the folded strip renders timing metadata"
    );

    assert!(deck.toggle_collapse(1));
    resize_terminals(&mut engine, &projects, &deck, area);
    assert_eq!(
        engine.timing_visible(),
        &BTreeSet::from([projects[0].terminal.clone(), projects[1].terminal.clone()]),
        "an expanded preview renders timing metadata"
    );

    assert!(deck.apply(&ActionCommand::ToggleZoom, &projects, Timestamp::default()));
    resize_terminals(&mut engine, &projects, &deck, area);
    assert_eq!(
        engine.timing_visible(),
        &BTreeSet::from([projects[0].terminal.clone()]),
        "zoom hides the preview and stops its timing tick"
    );
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

/// #97: the loop's ingest, in the lines it is made of. A bell names the
/// terminal it rang in; the master frame's own bell is dropped, because that
/// pane is on screen in every layout; and taking the master frame is what
/// answers a notification, with no clock involved.
#[test]
fn a_bell_marks_its_pane_and_the_master_ignores_its_own() {
    let projects = sleepers(3);
    let mut deck = DeckState::new(projects.len());
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let mut notifies = Notifications::new();
    let at = Timestamp { unix_millis: 1_000 };
    let ring = |engine: &mut FakeEngine,
                notifies: &mut Notifications,
                deck: &DeckState,
                terminal: &TerminalId| {
        for event in engine.bell(terminal) {
            let EngineEvent::Notify { terminal, kind } = event else {
                continue;
            };
            notifies.record(&terminal, master_terminal(deck, &projects), kind, at);
        }
    };

    ring(&mut engine, &mut notifies, &deck, &TerminalId::new("t3"));
    ring(&mut engine, &mut notifies, &deck, &TerminalId::new("t1"));

    assert!(notifies.pending(&TerminalId::new("t3")).is_some());
    assert!(
        notifies.pending(&TerminalId::new("t1")).is_none(),
        "t1 holds the master frame"
    );

    // Promotion is what the session calls `clear` for, once a frame, for
    // whichever terminal holds the master frame.
    deck.apply(&ActionCommand::SelectPosition(2), &sleepers(3), now());
    let master = master_terminal(&deck, &projects).cloned().unwrap();
    assert_eq!(master, TerminalId::new("t3"));
    assert!(notifies.clear(&master));
    assert!(notifies.is_empty());

    // An unknown terminal rings nothing at all.
    assert!(engine.bell(&TerminalId::new("gone")).is_empty());
}

/// The picker read the terminal's size every pass and never compared it, so
/// a resized window kept its old layout until the next keypress (#127). A
/// resize is a redraw by itself — and only for the pass it happens on.
#[test]
fn a_resize_alone_is_a_reason_to_redraw() {
    let mut size = ScreenSize::new(80, 24);

    assert!(
        !resized(&mut size, ScreenSize::new(80, 24)),
        "nothing moved"
    );
    assert!(
        resized(&mut size, ScreenSize::new(100, 40)),
        "the window did"
    );
    assert_eq!(size, ScreenSize::new(100, 40), "and is remembered");
    assert!(
        !resized(&mut size, ScreenSize::new(100, 40)),
        "one redraw, not one per pass"
    );
}

/// #115's scrollbar hides itself on exactly the terms a demotion does, and
/// carried the same gap #125 closed for the other two transients: the frame
/// that takes the bar off the screen was never asked for, so it sat there
/// until something unrelated redrew (#132). Its window is 4s, and the pass
/// that crosses the deadline is the one that has to draw.
#[test]
fn the_scrollbar_disappearance_schedules_its_final_repaint() {
    let mut deck = DeckState::new(2);
    deck.mark_scrolled(0, Timestamp::default());
    let notifies = Notifications::new();
    let mut was_active = false;

    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp::default(),
    ));
    assert!(deck.scrolling(Timestamp { unix_millis: 3_999 }).is_some());
    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 3_999 },
    ));

    assert!(
        deck.scrolling(Timestamp { unix_millis: 4_000 }).is_none(),
        "the window is over on the millisecond, not after it"
    );
    assert!(
        schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 4_000 },
        ),
        "the bar's disappearance receives a final frame"
    );
    assert!(
        !schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 4_001 },
        ),
        "and exactly one: the loop goes idle again behind it"
    );
}

/// The copy receipt is the third transient on that scheduler (#150): the
/// highlight goes on the clock rather than on an event, so the pass that
/// crosses its deadline is the one that has to draw — otherwise the inverted
/// cells sit there until something unrelated redraws, which is the whole of
/// what #150 is about.
#[test]
fn the_copy_receipt_schedules_its_final_repaint() {
    let mut deck = DeckState::new(2);
    deck.set_selection(Selection::new(0, (0, 0)).to((3, 0)));
    let notifies = Notifications::new();
    let mut was_active = false;

    assert!(
        !schedule_expiry_repaint(&mut was_active, &deck, &notifies, Timestamp::default()),
        "a selection still being dragged repaints on its own pointer moves"
    );

    deck.mark_copied(Timestamp::default());
    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp::default(),
    ));
    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 1_999 },
    ));
    assert!(
        schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 2_000 },
        ),
        "the highlight's disappearance receives a final frame"
    );
    assert!(
        !schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 2_001 },
        ),
        "and exactly one: the loop goes idle again behind it"
    );
}

#[test]
fn expiry_transitions_schedule_their_final_repaint() {
    let mut deck = DeckState::new(2);
    deck.apply(
        &ActionCommand::SelectPosition(1),
        &sleepers(2),
        Timestamp::default(),
    );
    let notifies = Notifications::new();
    let mut was_active = false;

    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp::default(),
    ));
    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 1_499 },
    ));
    assert!(
        schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 1_500 },
        ),
        "the demotion disappearance receives a final frame"
    );
    assert!(!schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 1_501 },
    ));

    let terminal = TerminalId::new("worker");
    let mut notifies = Notifications::new();
    assert!(notifies.record(&terminal, None, NotifyKind::Attention, Timestamp::default(),));
    let deck = DeckState::new(2);
    let mut was_active = false;
    assert!(schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 7_999 },
    ));
    assert!(!notifies.toasting(&terminal, Timestamp { unix_millis: 8_000 }));
    assert!(
        schedule_expiry_repaint(
            &mut was_active,
            &deck,
            &notifies,
            Timestamp { unix_millis: 8_000 },
        ),
        "the toast removal receives a final frame"
    );
    assert!(!schedule_expiry_repaint(
        &mut was_active,
        &deck,
        &notifies,
        Timestamp { unix_millis: 8_001 },
    ));
}

/// A terminal that would not start was dropped in silence, which made an
/// ignored selection and a failed process look exactly alike (#128). The
/// batch says so in one row: the first failure named in full, the rest
/// counted, and a paragraph of an error cut to the line that fits.
#[test]
fn a_failed_add_says_so_in_one_bounded_row() {
    assert!(add_failure(&[]).is_none(), "nothing failed, nothing to say");

    let (head, one) = add_failure(&[(
        TerminalId::new("frontend-2"),
        "No such file or directory (os error 2)\nwhile running bash".to_owned(),
    )])
    .expect("a failure is worth saying");
    assert_eq!(head, "frontend-2", "the row is about the terminal");
    assert_eq!(
        one,
        NotifyKind::Message {
            title: "did not start".to_owned(),
            body: "No such file or directory (os error 2)".to_owned(),
        },
        "the first line of the error, and only the first"
    );

    let (head, three) = add_failure(&[
        (TerminalId::new("api"), "permission denied".to_owned()),
        (TerminalId::new("web"), "permission denied".to_owned()),
        (TerminalId::new("db"), "permission denied".to_owned()),
    ])
    .expect("three failures are worth saying once");
    assert_eq!(head, "api");
    assert_eq!(
        three,
        NotifyKind::Message {
            title: "did not start".to_owned(),
            body: "permission denied · +2 more".to_owned(),
        },
        "three rows would be three times the same news"
    );
}

/// The socket and the dispatcher are each well covered alone — the listener
/// against real clients that stall, abandon their reply, or never read
/// (`src/ctl/mod.rs`), the dispatcher against requests built in memory
/// (above) — and nothing joined them. This is the seam the session actually
/// runs: bytes from a real client, over a real socket, into the live deck,
/// and an envelope back out (#130).
#[cfg(target_os = "linux")]
#[test]
fn a_request_over_the_real_socket_reaches_the_live_deck() {
    use std::io::{Read, Write};

    let _lock = crate::ctl::LISTENER_TEST_LOCK.lock().unwrap();
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut quit = false;
    assert_eq!(deck.active(), Some(0), "t1 opens as master");

    let mut listener = crate::ctl::Listener::bind().unwrap();
    let socket = listener.path().to_path_buf();
    let mut client = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    client
        .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"promote\",\"id\":\"t2\"}\n")
        .unwrap();

    let served = listener
        .poll_with(|request, caller| {
            dispatch_control(
                request,
                caller,
                &mut workspace,
                &mut projects,
                &mut deck,
                &mut engine,
                &mut notifies,
                size,
                false,
                &socket,
                false,
                &mut closed,
                &mut used,
                &mut quit,
            )
        })
        .unwrap();

    assert!(
        served,
        "the request was complete, so the poll dispatched it"
    );
    // The reply arrives because the listener closes the connection after
    // writing it. A change that stops closing it would leave this read
    // blocking forever, so it fails fast instead: CI's job ceiling would
    // catch the hang eventually, but a local run should not have to be
    // noticed and killed by hand.
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("the listener closes the connection after its reply");
    assert!(
        response.contains("\"schema\":\"ctl.v1\"") && response.contains("\"ok\":true"),
        "{response}"
    );
    assert!(response.contains("\"master\":true"), "{response}");
    assert_eq!(deck.active(), Some(1), "t2 holds the master frame now");
    assert!(!quit, "a promotion is not a reason to leave");

    engine.dispatch(EngineCommand::Shutdown);
}

/// #139 regression: the control socket receives text that never went through
/// the outer paste parser. A closer in that text used to end the region early,
/// leaving `touch` as ordinary readline input. This is deliberately a real
/// bash, PTY, and socket path: a byte-only assertion would miss execution.
#[cfg(target_os = "linux")]
#[test]
fn a_socket_paste_cannot_escape_bracketed_paste_and_execute() {
    use std::{
        io::{Read, Write},
        thread,
        time::{Duration, Instant},
    };

    let _lock = crate::ctl::LISTENER_TEST_LOCK.lock().unwrap();
    let marker = std::env::temp_dir().join(format!(
        "termdeck-paste-escape-{}-{}",
        std::process::id(),
        now().unix_millis
    ));
    let _ = std::fs::remove_file(&marker);
    let size = ScreenSize::new(80, 24);
    let terminal = TerminalId::new("bash");
    let mut projects = vec![Project {
        terminal: terminal.clone(),
        path: PathBuf::from("/"),
        command: vec![
            "/bin/bash".to_owned(),
            "--noprofile".to_owned(),
            "--norc".to_owned(),
            "-i".to_owned(),
        ],
        shell_hook: false,
    }];
    let mut workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    engine.dispatch(EngineCommand::Input {
        terminal: terminal.clone(),
        bytes: b"bind 'set enable-bracketed-paste on'\n".to_vec(),
    });
    let mode_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < mode_deadline
        && !engine
            .metadata(&terminal)
            .is_some_and(|metadata| metadata.bracketed_paste)
    {
        engine.drain_events();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        engine
            .metadata(&terminal)
            .is_some_and(|metadata| metadata.bracketed_paste),
        "bash never enabled bracketed paste"
    );

    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut used = BTreeSet::from([terminal.to_string()]);
    let mut quit = false;
    let mut listener = crate::ctl::Listener::bind().unwrap();
    let socket = listener.path().to_path_buf();
    let payload = format!(
        "{close}touch {marker}\n",
        close = String::from_utf8_lossy(PASTE_CLOSE),
        marker = marker.display(),
    );
    let request = crate::ctl::Request {
        schema: crate::ctl::SCHEMA.to_owned(),
        verb: "input".to_owned(),
        id: Some(terminal.to_string()),
        force: true,
        paste: Some(payload),
        ..Default::default()
    };
    let mut client = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    let mut wire = serde_json::to_vec(&request).unwrap();
    wire.push(b'\n');
    client.write_all(&wire).unwrap();
    assert!(
        listener
            .poll_with(|request, caller| {
                dispatch_control(
                    request,
                    caller,
                    &mut workspace,
                    &mut projects,
                    &mut deck,
                    &mut engine,
                    &mut notifies,
                    size,
                    false,
                    &socket,
                    true,
                    &mut closed,
                    &mut used,
                    &mut quit,
                )
            })
            .unwrap(),
        "the complete socket request must be dispatched"
    );
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reply = String::new();
    client.read_to_string(&mut reply).unwrap();
    let reply: crate::ctl::Response = serde_json::from_str(reply.trim()).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !marker.exists() {
        engine.drain_events();
        thread::sleep(Duration::from_millis(10));
    }
    let executed = marker.exists();
    engine.dispatch(EngineCommand::Shutdown);
    let _ = std::fs::remove_file(&marker);

    let error = reply
        .error
        .expect("the socket caller must learn the refusal");
    assert_eq!(error.code, 3);
    assert!(error.message.contains("bracketed-paste closer"));
    assert!(
        !executed,
        "a closer must never let the rest of a socket paste execute in bash"
    );
}

// ---------------------------------------------------------------------------
// #148: the pointer's text-selection gesture.

/// A terminal frame carrying `lines`, wide enough to hold them: the smallest
/// thing a selection can be read out of.
fn frame_of(terminal: &TerminalId, lines: &[&str]) -> crate::contracts::TerminalFrame {
    let columns = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1);
    let size = ScreenSize::new(columns.max(1) as u16, lines.len().max(1) as u16);
    let mut frame = crate::contracts::TerminalFrame::blank(terminal.clone(), size, 1);
    for (row, line) in lines.iter().enumerate() {
        for (column, character) in line.chars().enumerate() {
            let index = frame.cell_index(column as u16, row as u16).unwrap();
            frame.cells[index].content = crate::contracts::CellContent::Glyph {
                text: character.to_string(),
                width: crate::contracts::CellWidth::One,
            };
        }
    }
    frame
}

/// The whole of the coexistence rule: where the press lands decides what the
/// drag is, and a press that never moves off its cell is still a click.
#[test]
fn the_press_decides_whether_a_drag_selects_or_reorders() {
    let mut gesture = Selecting::default();

    // Pressed on chrome — a title row or a border — the gesture owns nothing,
    // whatever the pointer does next, so the reorder drag keeps the pointer.
    assert_eq!(
        gesture.step(MouseAction::Down, None, None),
        SelectStep::Pass
    );
    assert_eq!(gesture.pane(), None);
    assert_eq!(
        gesture.step(MouseAction::Move, Some((1, 4, 4)), Some((4, 4))),
        SelectStep::Pass
    );
    assert_eq!(gesture.step(MouseAction::Up, None, None), SelectStep::Pass);

    // Pressed on content and released on the same cell: still nothing, which
    // is what leaves the double-click promotion alone.
    assert_eq!(
        gesture.step(MouseAction::Down, Some((0, 7, 9)), Some((7, 9))),
        SelectStep::Pass
    );
    assert_eq!(gesture.pane(), Some(0), "the press holds the pane");
    assert_eq!(
        gesture.step(MouseAction::Move, Some((0, 7, 9)), Some((7, 9))),
        SelectStep::Pass,
        "a move that stays on the pressed cell is not a drag"
    );
    assert_eq!(gesture.step(MouseAction::Up, None, None), SelectStep::Pass);

    // Pressed on content and dragged: a selection, anchored where the press
    // landed and extended to wherever the pointer is now, even once that is
    // outside the pane and the caller has clamped it to the edge.
    assert_eq!(
        gesture.step(MouseAction::Down, Some((0, 2, 1)), Some((2, 1))),
        SelectStep::Pass
    );
    assert_eq!(
        gesture.step(MouseAction::Move, None, Some((9, 1))),
        SelectStep::Extend(Selection::new(0, (2, 1)).to((9, 1)))
    );
    assert_eq!(
        gesture.step(MouseAction::Move, None, Some((91, 6))),
        SelectStep::Extend(Selection::new(0, (2, 1)).to((91, 6))),
        "the gesture stays in the pressed pane wherever the pointer goes"
    );
    assert_eq!(gesture.step(MouseAction::Up, None, None), SelectStep::Copy);
    assert_eq!(gesture.pane(), None, "the release lets go");

    // A secondary or shift release lets go of an armed press too, so a
    // stray chord can never leave a selection half-made.
    gesture.step(MouseAction::Down, Some((0, 2, 1)), Some((2, 1)));
    assert_eq!(
        gesture.step(MouseAction::SecondaryUp, None, None),
        SelectStep::Pass
    );
    assert_eq!(gesture.pane(), None);
    assert_eq!(
        gesture.step(MouseAction::Move, None, Some((9, 1))),
        SelectStep::Pass
    );
}

/// A drag that selects text and one that reorders panes are the same three
/// events: this pins that the reorder half still reaches `mouse_action`
/// untouched when the press was on chrome.
#[test]
fn a_chrome_drag_still_reorders_while_a_content_drag_selects() {
    let mut gesture = Selecting::default();
    let mut state = DeckState::new(4);
    let mut last_click = None;
    let at = |millis| Timestamp {
        unix_millis: millis,
    };

    // Pressed on preview 1's title row: no content cell, so the gesture
    // passes and the existing promotion drag runs exactly as it did.
    assert_eq!(
        gesture.step(MouseAction::Down, None, None),
        SelectStep::Pass
    );
    mouse_action(
        &mut state,
        Some(1),
        MouseAction::Down,
        at(0),
        &mut last_click,
    );
    assert_eq!(
        gesture.step(MouseAction::Move, Some((0, 3, 3)), Some((3, 3))),
        SelectStep::Pass
    );
    mouse_action(
        &mut state,
        Some(0),
        MouseAction::Move,
        at(1),
        &mut last_click,
    );
    assert_eq!(gesture.step(MouseAction::Up, None, None), SelectStep::Pass);
    let action = mouse_action(&mut state, Some(0), MouseAction::Up, at(2), &mut last_click);

    assert_eq!(action, Some(ActionCommand::SelectPosition(1)));
}

/// The copy reads the frame the engine holds, and a drag over blank cells
/// copies nothing rather than clearing the clipboard.
#[test]
fn a_copy_reads_the_pane_under_the_selection() {
    let projects = sleepers(2);
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    engine.set_frame(frame_of(&projects[0].terminal, &["$ pnpm dev", ""]));

    // `$ pnpm dev` is the frame's first row.
    let selection = Selection::new(0, (0, 0)).to((5, 0));
    assert_eq!(
        selection_text(&engine as &dyn TerminalEngine, &projects, &selection),
        Some("$ pnpm".to_owned())
    );
    // Two rows, the second of which is blank: the blank one is an empty line.
    let selection = Selection::new(0, (7, 0)).to((3, 1));
    assert_eq!(
        selection_text(&engine as &dyn TerminalEngine, &projects, &selection),
        Some("dev\n".to_owned())
    );
    // Nothing but blanks copies nothing at all.
    let selection = Selection::new(0, (0, 1)).to((40, 1));
    assert_eq!(
        selection_text(&engine as &dyn TerminalEngine, &projects, &selection),
        None
    );
    // A pane the deck has no project for answers nothing rather than panicking.
    let selection = Selection::new(9, (0, 0)).to((5, 0));
    assert_eq!(
        selection_text(&engine as &dyn TerminalEngine, &projects, &selection),
        None
    );
}

/// The copy leaves as OSC 52, base64 with padding, terminated with BEL: the
/// one target that reaches the user's own clipboard over SSH, and the only
/// one that costs no dependency (`mouse-selection.md` §4.2).
#[test]
fn a_copy_leaves_as_an_osc_52_clipboard_request() {
    assert_eq!(clipboard_sequence("man"), b"\x1b]52;c;bWFu\x07".to_vec());
    // The two padding cases, and a byte sequence that exercises the top of
    // the alphabet rather than only its letters.
    assert_eq!(clipboard_sequence("ma"), b"\x1b]52;c;bWE=\x07".to_vec());
    assert_eq!(clipboard_sequence("m"), b"\x1b]52;c;bQ==\x07".to_vec());
    assert_eq!(clipboard_sequence(""), b"\x1b]52;c;\x07".to_vec());
    assert_eq!(
        clipboard_sequence("\u{fb}\u{ff}\u{fe}"),
        b"\x1b]52;c;w7vDv8O+\x07".to_vec()
    );
    // Multi-line copies travel as their own bytes; nothing is escaped away.
    assert_eq!(clipboard_sequence("a\nb"), b"\x1b]52;c;YQpi\x07".to_vec());
}

/// `^g v` is the way back out of a copy on a host that refuses OSC 52 and
/// never says so. It is a paste operation like every other (#120).
#[test]
fn the_pointer_copy_pastes_back_as_a_paste_operation() {
    let mut input = Input::new(38);
    let mut deck = DeckState::new(2);
    let projects = sleepers(2);
    let at = Timestamp { unix_millis: 0 };

    assert_eq!(input.press(Key::Ctrl('g'), &mut deck, &projects, at), None);
    assert_eq!(
        input.press(Key::Char('v'), &mut deck, &projects, at),
        Some(Reaction::PasteCopy)
    );

    // The session's half: bracketed for a child that holds DEC 2004, raw for
    // one that does not.
    let mut engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let frontend = projects[0].terminal.clone();
    assert_eq!(
        encode_paste(&engine, &frontend, "ls -l".to_owned()).unwrap(),
        b"ls -l".to_vec()
    );
    engine.set_metadata(
        &frontend,
        TerminalMetadata {
            bracketed_paste: true,
            ..TerminalMetadata::default()
        },
    );
    let mut expected = PASTE_OPEN.to_vec();
    expected.extend_from_slice(b"ls -l");
    expected.extend_from_slice(PASTE_CLOSE);
    assert_eq!(
        encode_paste(&engine, &frontend, "ls -l".to_owned()).unwrap(),
        expected
    );
}
