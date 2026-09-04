use super::{
    InputEvent, KeyReader, MouseAction, WheelRoute, add_terminal, app_wheel, chosen,
    dispatch_live_input, mouse_action, open_terminals, resize_terminals, route_wheel,
    spawn_terminals, terminal_sizes,
};
use crate::{
    contracts::{
        ActionCommand, EngineCommand, Project, ScreenSize, ScrollCommand, ScrollbackPosition,
        TerminalEngine, TerminalId, TerminalMetadata, Timestamp,
    },
    engine::FakeEngine,
    ui::{DeckState, Key, SheetState},
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
        },
        Project {
            terminal: TerminalId::new("web"),
            path: PathBuf::from("/code/web"),
            command: vec!["sh".to_owned()],
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
        },
        Project {
            terminal: TerminalId::new("web"),
            path: PathBuf::from("/code/web"),
            command: vec!["sh".to_owned()],
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

#[cfg(target_os = "linux")]
#[test]
fn runtime_add_uses_the_usable_terminal_height() {
    let size = ScreenSize::new(100, 30);
    let first = Project {
        terminal: TerminalId::new("first"),
        path: PathBuf::from("/"),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
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
        },
        Project {
            terminal: TerminalId::new("second"),
            path: PathBuf::from("/"),
            command: vec!["sh".to_owned()],
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
