#[cfg(unix)]
use super::dispatch_control;
use super::{
    InputEvent, KeyReader, MouseAction, WheelRoute, add_failure, add_terminal, app_wheel, chosen,
    close_terminal, dispatch_live_input, encode_paste, master_terminal, mouse_action, now,
    open_terminals, request_close, resize_terminals, resized, route_wheel, spawn_terminals,
    spawn_terminals_with_socket, terminal_sizes,
};
use crate::{
    contracts::{
        ActionCommand, EngineCommand, EngineEvent, NotifyKind, Project, ScreenSize, ScrollCommand,
        ScrollbackPosition, TerminalEngine, TerminalId, TerminalMetadata, Timestamp,
    },
    engine::FakeEngine,
    ui::{DeckState, Input, Key, Modal, Notifications, Reaction, SheetState},
};
use ratatui::layout::Position;
use std::path::PathBuf;

#[test]
fn decoder_keeps_terminal_controls_and_mouse_out_of_the_shell_input_path() {
    let mut reader = KeyReader {
        bytes:
            "a\x03\x1b[A\x1b[200~paste\x1b[201~\x1b[<64;3;5M\x1b[<0;4;6M\x1b[<32;5;6M\x1b[<0;5;6m界"
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

/// #120: every fragmentation boundary of a bracketed paste decodes as one
/// paste once complete — and a partial opener emits nothing, not an escape
/// plus literal keys (the audit's split after `ESC[2`).
#[test]
fn bracketed_paste_survives_every_fragmentation_boundary() {
    let full = b"\x1b[200~hi\nbye\x1b[201~";
    // Single-shot baseline.
    let mut reader = KeyReader {
        bytes: full.to_vec(),
    };
    let events = reader.decode(false);
    assert!(
        matches!(events.as_slice(), [InputEvent::Paste(text)] if text == "hi\nbye"),
        "single-shot paste must decode"
    );
    assert!(reader.bytes.is_empty());
    // Every split point: the prefix waits silently, the rest completes.
    for split in 1..full.len() {
        let mut reader = KeyReader {
            bytes: full[..split].to_vec(),
        };
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

/// #120: paste encoding follows the child's DEC 2004 mode — bracketed
/// while the child holds it, raw bytes otherwise (and for unknown
/// terminals, the safe default).
#[test]
fn paste_encoding_follows_the_child_bracketed_paste_mode() {
    let terminal = TerminalId::new("pane");
    let mut engine = FakeEngine::new([terminal.clone()]);
    assert_eq!(
        encode_paste(&engine, &terminal, "a\nb".to_owned()),
        b"a\nb".to_vec(),
        "no mode means raw bytes, as before"
    );
    assert_eq!(
        encode_paste(&engine, &TerminalId::new("ghost"), "a\nb".to_owned()),
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
        encode_paste(&engine, &terminal, "SAFE\nTEXT".to_owned()),
        b"\x1b[200~SAFE\nTEXT\x1b[201~".to_vec(),
        "paste mode means a delimited region"
    );
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
    let bytes = encode_paste(&engine, &terminal, "SAFE\nTEXT".to_owned());
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
    KeyReader {
        bytes: bytes.to_vec(),
    }
}

/// The picker's range keys arrive as modified arrows, which nothing read
/// before #56 — `\x1b[1;2A` would have been torn into an escape and the
/// characters `1;2A`.
#[test]
fn the_decoder_reads_shift_arrows() {
    let mut reader = KeyReader {
        bytes: b"\x1b[1;2A\x1b[1;2B\x1b[A\x1b[B".to_vec(),
    };

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
    let mut reader = KeyReader {
        bytes: b"\x1b[1;".to_vec(),
    };

    assert!(
        reader.decode(false).is_empty(),
        "nothing is decided from half a sequence"
    );
    assert_eq!(reader.bytes, b"\x1b[1;", "and none of it is thrown away");

    reader.bytes.extend_from_slice(b"2B");
    let events = reader.decode(false);

    assert!(matches!(events[0], InputEvent::Key(Key::ShiftDown)));
    assert!(reader.bytes.is_empty());
}

/// The same guard must not swallow a real escape: once the poll has timed
/// out with nothing following it, `esc` is `esc`.
#[test]
fn a_lone_escape_is_still_the_escape_key() {
    let mut reader = KeyReader {
        bytes: b"\x1b".to_vec(),
    };
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
    let mut reader = KeyReader {
        bytes: b"\x1b[<4;7;9m\x1b[<0;7;9m\x1b[<2;7;9m".to_vec(),
    };

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

    let added = chosen(&sheet, &running);

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

    let added = chosen(&sheet, &[]);

    assert_eq!(added.len(), 1);
    assert_eq!(added[0].terminal, TerminalId::new("archive"));
    assert_eq!(added[0].path, PathBuf::from("/code/archive"));
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
        ..TerminalMetadata::default()
    };
    assert_eq!(
        route_wheel(Some(&mouse), ScrollCommand::Up(3)),
        WheelRoute::App {
            up: true,
            mouse: true
        }
    );
    assert_eq!(
        route_wheel(Some(&mouse), ScrollCommand::Down(3)),
        WheelRoute::App {
            up: false,
            mouse: true
        }
    );
    let keys = TerminalMetadata {
        alt_screen: true,
        ..TerminalMetadata::default()
    };
    assert_eq!(
        route_wheel(Some(&keys), ScrollCommand::Up(3)),
        WheelRoute::App {
            up: true,
            mouse: false
        }
    );

    assert_eq!(app_wheel(true, 7, 4, true), b"\x1b[<64;7;4M");
    assert_eq!(app_wheel(false, 7, 4, true), b"\x1b[<65;7;4M");
    assert_eq!(app_wheel(true, 1, 1, false), b"\x1b[A\x1b[A\x1b[A");
    assert_eq!(app_wheel(false, 1, 1, false), b"\x1b[B\x1b[B\x1b[B");
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

    assert!(close_terminal(&mut engine, &mut projects, &mut deck, 1));

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
    let preview = engine.frame(&promoted).unwrap().size;

    assert!(close_terminal(&mut engine, &mut projects, &mut deck, 0));

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

    assert!(close_terminal(&mut engine, &mut projects, &mut deck, 0));

    assert!(projects.is_empty(), "the session has nothing left to run");
    assert_eq!(deck.active(), None);
    assert_eq!(engine.frame(&only), None);
    // Out of range afterwards, and refused rather than panicking.
    assert!(!close_terminal(&mut engine, &mut projects, &mut deck, 0));
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

    assert!(
        !request_close(&mut engine, &mut projects, &mut deck, 0),
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
    let mut input = Input::new(size.rows);

    for key in [Key::Char('n'), Key::Escape] {
        request_close(&mut engine, &mut projects, &mut deck, 0);
        assert_eq!(deck.modal(), Some(Modal::Quit));

        assert_eq!(input.press(key, &mut deck, &projects, now()), None);

        assert_eq!(deck.modal(), None, "the question is gone");
        assert_eq!(projects.len(), 1, "and the pane is not");
        assert_eq!(deck.active(), Some(0));
        assert!(engine.frame(&only).is_some(), "its shell kept running");
    }

    request_close(&mut engine, &mut projects, &mut deck, 0);
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

    assert!(request_close(&mut engine, &mut projects, &mut deck, 1));

    assert_eq!(deck.modal(), None, "nothing to confirm");
    assert_eq!(engine.frame(&closed), None, "the engine ended it");
    assert_eq!(projects.len(), 1);
    // The one left is the last, so its own close asks.
    assert!(!request_close(&mut engine, &mut projects, &mut deck, 0));
    assert_eq!(deck.modal(), Some(Modal::Quit));
    engine.dispatch(EngineCommand::Shutdown);
}

#[cfg(target_os = "linux")]
#[test]
fn ctl_controls_share_the_live_paths_and_enforce_their_gates() {
    use std::collections::BTreeSet;

    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(2);
    let workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");
    let mut closed = BTreeSet::new();
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
                &workspace,
                &mut projects,
                &mut deck,
                &mut engine,
                &mut notifies,
                size,
                false,
                socket,
                $allow,
                &mut closed,
                &mut quit,
            )
        };
    }

    let mut sheet_open = request("open");
    sheet_open.path = Some("/tmp".to_owned());
    let response = dispatch_control(
        sheet_open,
        None,
        &workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        true,
        socket,
        false,
        &mut closed,
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
    assert!(
        call!(input, None, true).ok,
        "session input permission opens the gate"
    );
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

/// #118: the agent `input` verb tells the truth about a wedged pane. Once
/// its bounded queue is saturated the request is refused with code 3 — the
/// same `refused` family as the input gate — instead of hanging the loop,
/// and the pane stays live behind the refusal.
#[cfg(target_os = "linux")]
#[test]
fn ctl_input_to_a_wedged_pane_is_refused_truthfully() {
    use std::collections::BTreeSet;
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
    let workspace = crate::config::Workspace::discovered(PathBuf::from("/"), projects.clone());
    let mut deck = DeckState::new(projects.len());
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();
    let mut notifies = crate::ui::Notifications::new();
    let mut closed = BTreeSet::new();
    let mut quit = false;
    let socket = std::path::Path::new("/tmp/termdeck-ctl-test.sock");

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
        &workspace,
        &mut projects,
        &mut deck,
        &mut engine,
        &mut notifies,
        size,
        false,
        socket,
        true,
        &mut closed,
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

/// A position no pane holds closes nothing and asks nothing, whatever is
/// left in the list.
#[cfg(target_os = "linux")]
#[test]
fn a_close_out_of_range_neither_closes_nor_asks() {
    let size = ScreenSize::new(144, 42);
    let mut projects = sleepers(1);
    let mut deck = DeckState::new(1);
    let mut engine = spawn_terminals(&projects, &deck, size).unwrap();

    assert!(!request_close(&mut engine, &mut projects, &mut deck, 4));

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
