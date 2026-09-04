use std::path::{Path, PathBuf};

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Position};

use super::super::Key;
use super::sheet::SHEET_INSTANCE;
use super::*;

/// The export's screen 06 folder, frozen so the snapshots do not depend on
/// anyone's disk.
struct Fixture;

fn code() -> PathBuf {
    PathBuf::from("/home/dev/code")
}

impl Browse for Fixture {
    fn list(&self, path: &Path) -> Listing {
        if path == Path::new("/home/dev/code/horizon-frontend") {
            return Listing::of(vec![
                Entry::parent("/home/dev/code"),
                Entry::folder("projects", path.join("projects")),
            ]);
        }
        // A folder that is genuinely empty, and one that cannot be read:
        // the two the picker must never confuse.
        if path == Path::new("/home/dev/code/vendor/tmp") {
            return Listing::of(vec![Entry::parent("/home/dev/code/vendor")]);
        }
        if path == Path::new("/home/dev/code/secret") {
            return Listing {
                entries: vec![Entry::parent("/home/dev/code")],
                error: Some("Permission denied (os error 13)".to_owned()),
                elsewhere: 0,
            };
        }
        let entries = vec![
            Entry::parent("/home/dev"),
            Entry::folder("archive", code().join("archive")).holding(3, 0),
            Entry::repository("horizon-frontend", code().join("horizon-frontend"))
                .git("main", 0, "2h ago"),
            Entry::repository("horizon-backend", code().join("horizon-backend"))
                .git("main", 3, "18m ago"),
            Entry::repository("horizon-app", code().join("horizon-app")).git(
                "feat/rn-0.75",
                0,
                "4d ago",
            ),
            Entry::repository("horizon-infra", code().join("horizon-infra"))
                .git("main", 0, "3w ago"),
            Entry::folder("notes", code().join("notes")).holding(14, 0),
            Entry::repository("termdeck", code().join("termdeck")).git("main", 0, "just now"),
            Entry::folder("vendor", code().join("vendor")).holding(6, 0),
            Entry::file("README.md", code().join("README.md")),
        ];
        Listing::of(entries)
    }

    fn search(&self, path: &Path) -> Vec<Entry> {
        let mut found: Vec<Entry> = self
            .list(path)
            .entries
            .into_iter()
            .filter(Entry::selectable)
            .collect();
        found.push(
            Entry::repository("horizon-docs", "/home/dev/work/archive/horizon-docs")
                .git("main", 0, "1y ago"),
        );
        found
    }

    fn roots(&self) -> Vec<Entry> {
        vec![
            Entry::folder("~/code", code()).holding(9, 5),
            Entry::folder("~/work", "/home/dev/work").holding(12, 12),
            Entry::folder("/srv", "/srv").holding(2, 2),
        ]
    }
}

fn render(state: &PickerState) -> Buffer {
    let listing = state.listing(&Fixture);
    let roots = Fixture.roots();
    let view = Picker {
        state,
        listing: &listing,
        roots: &roots,
        home: Some(Path::new("/home/dev")),
    };
    let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
    terminal.draw(|frame| view.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    let area = buffer.area();
    (0..area.height)
        .map(|row| {
            (0..area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_snapshot(name: &str, buffer: &Buffer) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/ui/testdata")
        .join(format!("{name}.txt"));
    let rendered = text(buffer);
    if std::env::var_os("TERMDECK_BLESS").is_some() {
        std::fs::write(&path, format!("{rendered}\n")).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(rendered, expected.trim_end_matches('\n'), "{name}");
}

fn browsing() -> PickerState {
    PickerState::at(code())
}

fn rows_of(state: &PickerState) -> Vec<Entry> {
    state.rows(&Fixture)
}

fn select(state: &mut PickerState, name: &str) {
    let rows = rows_of(state);
    let entry = rows
        .iter()
        .find(|entry| entry.name == name)
        .expect("the fixture lists it");
    assert!(state.toggle(entry));
}

/// A directory of this test's own, for the cases that need a real one.
fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "termdeck-picker-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn entry(state: &PickerState, name: &str) -> Entry {
    rows_of(state)
        .into_iter()
        .find(|entry| entry.name == name)
        .expect("the fixture lists it")
}

/// The note: "the ordinal is the pane number", and the first selected is
/// the master.
#[test]
fn selection_order_is_pane_order_and_the_first_is_master() {
    let mut state = browsing();

    select(&mut state, "horizon-frontend");
    select(&mut state, "horizon-backend");
    select(&mut state, "horizon-app");

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["horizon-frontend", "horizon-backend", "horizon-app"]
    );
    assert_eq!(state.launch()[0].0, "horizon-frontend", "pane 1 is master");

    // `m` promotes, and everything above it shifts down.
    assert!(state.set_master(&entry(&state, "horizon-app")));
    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["horizon-app", "horizon-frontend", "horizon-backend"]
    );
}

/// §3.1: a path may be selected repeatedly, each instance its own pane
/// with its own name.
#[test]
fn the_same_path_can_open_more_than_one_terminal() {
    let mut state = browsing();
    let frontend = entry(&state, "horizon-frontend");

    select(&mut state, "horizon-frontend");
    assert!(state.add(&frontend));
    assert!(state.add(&frontend));

    assert_eq!(state.instances(&frontend.path), 3);
    let launch = state.launch();
    assert_eq!(
        launch
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "horizon-frontend",
            "horizon-frontend-2",
            "horizon-frontend-3"
        ]
    );
    assert!(
        launch.iter().all(|(_, path)| *path == frontend.path),
        "every instance runs in the same directory"
    );

    // `-` sheds the most recent; `space`/`x` take the whole path.
    assert!(state.drop_one(&frontend));
    assert_eq!(state.instances(&frontend.path), 2);
    assert!(state.toggle(&frontend));
    assert_eq!(state.instances(&frontend.path), 0);
}

/// A suffix another selection already holds is skipped, because names are
/// what the engine keys a terminal by.
#[test]
fn instance_names_skip_the_ones_already_taken() {
    let mut state = browsing();
    let app = Entry::repository("horizon-app", code().join("horizon-app"));
    let decoy = Entry::repository("horizon-app-2", code().join("horizon-app-2"));

    assert!(state.add(&app));
    assert!(state.add(&decoy));
    assert!(state.add(&app));

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(names, ["horizon-app", "horizon-app-2", "horizon-app-3"]);
}

/// A bulk key must not multiply what a deliberate one built.
#[test]
fn selecting_every_target_leaves_existing_instances_alone() {
    let mut state = browsing();
    let frontend = entry(&state, "horizon-frontend");
    state.add(&frontend);
    state.add(&frontend);

    assert!(state.select_all(&rows_of(&state)));

    assert_eq!(state.instances(&frontend.path), 2, "not multiplied");
    assert_eq!(state.selection().len(), 9, "8 targets, one of them twice");
}

/// Any directory can be a terminal, so a folder joins a workspace on the
/// same terms as a repository. A plain file cannot, and neither can `..`.
#[test]
fn any_directory_can_be_selected_but_a_file_cannot() {
    let mut state = browsing();

    assert!(!state.toggle(&entry(&state, "README.md")), "a file");
    assert!(!state.toggle(&entry(&state, "..")), "a way out, not a pane");
    assert!(!state.launchable());

    assert!(state.toggle(&entry(&state, "archive")), "a plain folder");
    assert!(state.toggle(&entry(&state, "termdeck")), "a repository");
    assert_eq!(state.selection().len(), 2);
    assert!(state.launchable());
}

/// `⏎` selects the row under the cursor, and pressing it again lets go.
#[test]
fn enter_selects_the_row_and_a_second_press_deselects_it() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    state.point_at(index, rows.len());

    assert_eq!(press(&mut state, &rows, &roots, Key::Enter), None);
    assert_eq!(state.selection().len(), 1);
    assert_eq!(state.selection()[0].name, "horizon-frontend");

    assert_eq!(press(&mut state, &rows, &roots, Key::Enter), None);
    assert!(state.selection().is_empty(), "the second press lets go");
    assert_eq!(
        state.cwd(),
        Some(code().as_path()),
        "and neither press moved anywhere"
    );
}

/// `ctrl+j` used to decode as `⏎` (#95), so it punched through the listing
/// as a stray select. It is the control twin of `j`, and moves the cursor.
#[test]
fn ctrl_j_moves_the_picker_cursor_down_instead_of_selecting() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    state.point_at(0, rows.len());

    assert_eq!(press(&mut state, &rows, &roots, Key::Ctrl('j')), None);

    assert_eq!(state.cursor(), 1, "the same step `j` takes");
    assert!(
        state.selection().is_empty(),
        "and nothing was selected on the way"
    );
}

/// `+` is deliberately distinct from `⏎`: it appends another named
/// instance of the cursor path instead of merging it into the first.
#[test]
fn plus_adds_a_second_named_instance_to_the_picker_selection() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    state.point_at(index, rows.len());

    press(&mut state, &rows, &roots, Key::Enter);
    press(&mut state, &rows, &roots, Key::Char('+'));

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(names, ["horizon-frontend", "horizon-frontend-2"]);
    let rendered = text(&render(&state));
    assert!(rendered.contains("horizon-frontend-2"), "{rendered}");
    assert!(rendered.contains("o  Open 2 as terminals"), "{rendered}");
}

/// `→` goes inside whatever the cursor is on, repository or not — the
/// case a repository holding `projects/` used to make impossible.
#[test]
fn the_right_arrow_goes_inside_a_repository_as_well_as_a_folder() {
    let roots = Fixture.roots();
    for (name, expected) in [
        ("termdeck", code().join("termdeck")),
        ("archive", code().join("archive")),
    ] {
        let mut state = browsing();
        let rows = rows_of(&state);
        let index = rows.iter().position(|entry| entry.name == name).unwrap();
        state.point_at(index, rows.len());

        press(&mut state, &rows, &roots, Key::Right);

        assert_eq!(state.cwd(), Some(expected.as_path()), "inside {name}");
        assert!(state.selection().is_empty(), "going inside selects nothing");
    }
}

/// `←` comes back out again.
#[test]
fn the_left_arrow_goes_back_one_level() {
    let mut state = PickerState::at(code().join("termdeck"));
    let roots = Fixture.roots();
    let rows = rows_of(&state);

    press(&mut state, &rows, &roots, Key::Left);

    assert_eq!(state.cwd(), Some(code().as_path()));
}

/// `o` is what opens the selection now that `⏎` selects.
#[test]
fn o_opens_the_selection_and_only_once_there_is_one() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);

    assert_eq!(
        press(&mut state, &rows, &roots, Key::Char('o')),
        None,
        "nothing selected, nothing to open"
    );

    select(&mut state, "termdeck");
    assert_eq!(
        press(&mut state, &rows, &roots, Key::Char('o')),
        Some(PickerReaction::Launch)
    );
}

/// Navigation: `l` enters, `h` climbs, `~` returns to the roots, and none
/// of it disturbs the selection.
#[test]
fn navigation_keeps_the_selection_and_the_filter_does_not_survive_it() {
    let mut state = browsing();
    select(&mut state, "termdeck");
    let vendor = entry(&state, "vendor");
    state.begin_filter();
    state.push_filter('h');

    assert!(state.enter(&vendor));

    assert_eq!(state.cwd(), Some(code().join("vendor").as_path()));
    assert!(!state.filtering(), "a move clears the query");
    assert_eq!(state.selection().len(), 1, "selection is workspace-wide");

    assert!(state.up(&Fixture.roots()));
    assert_eq!(state.cwd(), Some(code().as_path()));
    assert!(state.go_home());
    assert_eq!(state.cwd(), None, "~ lands on the root list");
    assert_eq!(state.selection().len(), 1);
}

/// `h` from a configured root lands on the root list rather than walking
/// out into `/`.
#[test]
fn climbing_out_of_a_root_lands_on_the_root_list() {
    let mut state = browsing();

    assert!(state.up(&Fixture.roots()));

    assert_eq!(state.cwd(), None);
}

/// `g` is root and `/` is filter — the contradiction the note flagged,
/// resolved by the key map board.
#[test]
fn g_goes_to_the_root_and_slash_opens_the_filter() {
    let mut state = PickerState::at(code().join("vendor/tmp"));

    assert!(state.go_root(&Fixture.roots()));
    assert_eq!(state.cwd(), Some(code().as_path()));

    assert!(state.begin_filter());
    assert_eq!(state.filter(), Some(""));
    let rows = rows_of(&state);
    press(&mut state, &rows, &Fixture.roots(), Key::Char('g'));
    assert_eq!(
        state.filter(),
        Some("g"),
        "typing narrows, it does not move"
    );
}

/// The first `esc` clears the filter; only the second one quits.
#[test]
fn escape_clears_the_filter_before_it_quits() {
    let mut state = browsing();
    let roots = Fixture.roots();
    state.begin_filter();
    state.push_filter('h');

    let rows = rows_of(&state);
    assert_eq!(press(&mut state, &rows, &roots, Key::Escape), None);
    assert!(!state.filtering());

    assert_eq!(
        press(&mut state, &rows, &roots, Key::Escape),
        Some(PickerReaction::Quit)
    );
}

/// The filter reaches every configured root, not just this folder.
#[test]
fn the_filter_searches_beyond_the_current_folder() {
    let mut state = browsing();
    state.begin_filter();
    for character in "hor".chars() {
        state.push_filter(character);
    }

    let rows = rows_of(&state);
    let names: Vec<_> = rows.iter().map(|entry| entry.name.as_str()).collect();
    assert!(names.contains(&"horizon-docs"), "{names:?}");
    assert!(!names.contains(&"termdeck"), "{names:?}");
}

/// `⇧↓` takes everything selectable from the cursor to the end of the
/// listing — folders as much as repositories, since any directory can be
/// a terminal — and leaves files and `..` alone.
#[test]
fn shift_down_selects_everything_below_the_cursor() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-infra")
        .unwrap();
    state.point_at(index, rows.len());

    press(&mut state, &rows, &roots, Key::ShiftDown);

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    // From the cursor down: the repo it is on, the folder under it, the
    // next repo, the last folder. README.md is a file, so it is not here.
    assert_eq!(
        names,
        ["horizon-infra", "notes", "termdeck", "vendor"],
        "{names:?}"
    );
    assert!(
        !names.contains(&"README.md") && !names.contains(&".."),
        "a file and the way out are not terminals"
    );
    assert!(
        names.contains(&"notes") && names.contains(&"vendor"),
        "folders are in the range"
    );

    // Each press decides from the first selectable row afresh: the second
    // one removes the span, and the third selects it again.
    press(&mut state, &rows, &roots, Key::ShiftDown);
    assert!(state.selection().is_empty());
    press(&mut state, &rows, &roots, Key::ShiftDown);
    assert_eq!(state.selection().len(), 4);
}

/// `⇧↑` is the same thing upward, and takes its rows in listing order so
/// the pane numbers read the way the screen does.
#[test]
fn shift_up_selects_everything_above_the_cursor() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-backend")
        .unwrap();
    state.point_at(index, rows.len());

    press(&mut state, &rows, &roots, Key::ShiftUp);

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["archive", "horizon-frontend", "horizon-backend"],
        "{names:?}"
    );
    assert_eq!(
        state.selection()[0].name,
        "archive",
        "the topmost of the range is pane 1, so the panes read downwards"
    );
}

/// A selected first row makes a range remove every path in its span,
/// including every deliberate instance of those paths.
#[test]
fn a_range_uses_its_first_selectable_row_to_choose_removal() {
    let mut state = browsing();
    let roots = Fixture.roots();
    let rows = rows_of(&state);
    let app = entry(&state, "horizon-app");
    state.add(&app);
    state.add(&app);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-app")
        .unwrap();
    state.point_at(index, rows.len());

    press(&mut state, &rows, &roots, Key::ShiftDown);

    assert!(state.selection().is_empty(), "both app instances are gone");
}

/// The pointer selects a folder exactly as `⏎` does, and a second click
/// on it descends — the row semantics do not care what kind it is.
#[test]
fn a_click_selects_a_folder_and_a_second_click_goes_inside() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "archive")
        .unwrap();

    click(&mut state, &rows, Hit::Row(index));

    assert_eq!(state.selection().len(), 1, "a folder is selectable");
    assert_eq!(state.selection()[0].name, "archive");

    assert!(descend(&mut state, &rows, Hit::Row(index)));
    assert_eq!(state.cwd(), Some(code().join("archive").as_path()));
    assert!(state.selection().is_empty(), "the click was navigation");
}

/// The checkbox is the only separate hit region; the row body still owns
/// selection and second-click descent.
#[test]
fn a_folder_checkbox_is_separate_from_its_row_body() {
    let state = browsing();
    let listing = state.listing(&Fixture);
    let roots = Fixture.roots();
    let picker = Picker {
        state: &state,
        listing: &listing,
        roots: &roots,
        home: Some(Path::new("/home/dev")),
    };
    let area = ratatui::layout::Rect::new(0, 0, 144, 42);

    // `archive/` is the second row drawn, under the header and its rule.
    for column in [1u16, 8, 20, 40] {
        assert_eq!(
            picker.hit(area, Position::new(column, 6)),
            Some(if column <= 2 {
                Hit::Checkbox(1)
            } else {
                Hit::Row(1)
            }),
            "column {column}"
        );
    }
    assert_eq!(
        picker.hit(area, Position::new(1, 5)),
        Some(Hit::Row(0)),
        ".. has no checkbox"
    );
}

/// Shift-click is the range's pointer twin: everything between the
/// highlight and the clicked row, and the highlight follows the click.
#[test]
fn shift_click_selects_from_the_highlight_to_the_clicked_row() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let from = rows
        .iter()
        .position(|entry| entry.name == "horizon-backend")
        .unwrap();
    let to = rows
        .iter()
        .position(|entry| entry.name == "vendor")
        .unwrap();
    state.point_at(from, rows.len());

    assert!(click_range(&mut state, &rows, Hit::Row(to)));

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "horizon-backend",
            "horizon-app",
            "horizon-infra",
            "notes",
            "termdeck",
            "vendor"
        ],
        "the whole span, in listing order: {names:?}"
    );
    assert!(names.contains(&"notes"), "a folder inside the span");
    assert!(!names.contains(&"README.md"), "but not a file");
    assert_eq!(state.cursor(), to, "the highlight follows the click");

    state.point_at(from, rows.len());
    assert!(click_range(&mut state, &rows, Hit::Row(to)));
    assert!(state.selection().is_empty(), "the same span toggles off");
}

/// Clicking *above* the highlight is the same span read the other way,
/// and the panes still read downwards.
#[test]
fn shift_click_above_the_highlight_selects_the_same_span() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let from = rows
        .iter()
        .position(|entry| entry.name == "horizon-app")
        .unwrap();
    let to = rows
        .iter()
        .position(|entry| entry.name == "archive")
        .unwrap();
    state.point_at(from, rows.len());

    click_range(&mut state, &rows, Hit::Row(to));

    let names: Vec<_> = state
        .selection()
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "archive",
            "horizon-frontend",
            "horizon-backend",
            "horizon-app"
        ],
        "{names:?}"
    );
    assert_eq!(
        state.selection()[0].name,
        "archive",
        "the topmost of the span is pane 1"
    );
    assert_eq!(state.cursor(), to);
}

/// Shift-click applies the same fresh first-row rule as the keyboard.
#[test]
fn shift_click_uses_the_first_selectable_row_to_toggle_the_span() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let frontend = entry(&state, "horizon-frontend");
    state.add(&frontend);
    state.add(&frontend);
    let frontend_index = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    state.point_at(frontend_index, rows.len());

    let app = rows
        .iter()
        .position(|entry| entry.name == "horizon-app")
        .unwrap();
    assert!(click_range(&mut state, &rows, Hit::Row(app)));
    assert!(state.selection().is_empty(), "all frontend instances go");
}

/// The pointer follows the same two keys: one click selects, a second on
/// the same row goes inside it.
#[test]
fn a_second_click_on_a_row_goes_inside_it() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let index = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();

    click(&mut state, &rows, Hit::Row(index));
    assert_eq!(state.selection().len(), 1, "one click selects");

    assert!(descend(&mut state, &rows, Hit::Row(index)));
    assert_eq!(state.cwd(), Some(code().join("termdeck").as_path()));
    assert!(
        state.selection().is_empty(),
        "the click that opened it was not a selection after all"
    );
}

/// Mouse parity (§7): the pointer reaches what the keys reach.
#[test]
fn the_pointer_toggles_enters_and_adds_an_instance() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let roots = Fixture.roots();

    // Row 2 of the listing is horizon-frontend; its body toggles it.
    let hit = {
        let listing = state.listing(&Fixture);
        let picker = Picker {
            state: &state,
            listing: &listing,
            roots: &roots,
            home: Some(Path::new("/home/dev")),
        };
        // The listing starts under its header and rule: row 0 is `..`,
        // so horizon-frontend is the third row drawn.
        picker.hit(
            ratatui::layout::Rect::new(0, 0, 144, 42),
            Position::new(60, 7),
        )
    };
    assert_eq!(hit, Some(Hit::Row(2)));
    click(&mut state, &rows, hit.unwrap());
    assert_eq!(state.selection().len(), 1);
    assert_eq!(state.selection()[0].name, "horizon-frontend");

    // The badge cells add another instance of the same path.
    click(&mut state, &rows, Hit::Badge(2));
    assert_eq!(state.selection().len(), 2);
    assert_eq!(state.selection()[1].name, "horizon-frontend-2");

    // The checkbox removes the path outright, however many instances it
    // has, without using the row's descend behaviour.
    click(&mut state, &rows, Hit::Checkbox(2));
    assert!(state.selection().is_empty());
}

/// The checkbox and `⇥` share `space`/`x`'s whole-path toggle rule, while
/// only the row body can take the second-click navigation path.
#[test]
fn checkbox_and_tab_toggle_a_path_without_conflicting_with_row_descent() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let roots = Fixture.roots();
    let index = rows
        .iter()
        .position(|entry| entry.name == "archive")
        .unwrap();
    let archive = rows[index].clone();
    state.add(&archive);
    state.add(&archive);

    click(&mut state, &rows, Hit::Checkbox(index));
    assert!(
        state.selection().is_empty(),
        "checkbox removes all instances"
    );
    assert_eq!(
        state.cwd(),
        Some(code().as_path()),
        "checkbox never descends"
    );

    press(&mut state, &rows, &roots, Key::Tab);
    assert_eq!(
        state.instances(&archive.path),
        1,
        "tab toggles the cursor row"
    );
    press(&mut state, &rows, &roots, Key::Tab);
    assert!(state.selection().is_empty());

    click(&mut state, &rows, Hit::Row(index));
    assert!(descend(&mut state, &rows, Hit::Row(index)));
    assert_eq!(state.cwd(), Some(code().join("archive").as_path()));
}

#[test]
fn the_launch_button_only_answers_once_it_is_enabled() {
    let mut state = browsing();
    let rows = rows_of(&state);

    assert_eq!(click(&mut state, &rows, Hit::Launch), None);
    select(&mut state, "termdeck");
    assert_eq!(
        click(&mut state, &rows, Hit::Launch),
        Some(PickerReaction::Launch)
    );
}

/// The blocking case: a folder the picker cannot read must say so. An
/// error that draws as an empty listing tells the user their folder holds
/// nothing, which is a lie about the filesystem.
#[test]
fn an_unreadable_folder_says_why_instead_of_looking_empty() {
    let state = PickerState::at("/home/dev/code/secret");

    let listing = state.listing(&Fixture);
    assert!(listing.error.is_some(), "the browser reported the failure");
    assert!(
        !listing.is_empty_folder(),
        "a folder that could not be read is not an empty one"
    );

    let rendered = text(&render(&state));
    assert!(rendered.contains("cannot read ~/code/secret"), "{rendered}");
    assert!(rendered.contains("Permission denied"), "{rendered}");
    assert!(rendered.contains("← back · ~ home"), "the way out");
    assert!(!rendered.contains("empty folder"), "{rendered}");
    // And the row that leads out of it is still drawn.
    assert!(rendered.contains("▴  .."), "{rendered}");
}

/// The other half of the same distinction: a folder that really is empty
/// gets the designed empty state, which the error case must not take.
#[test]
fn a_genuinely_empty_folder_shows_the_empty_state() {
    let state = PickerState::at("/home/dev/code/vendor/tmp");

    let listing = state.listing(&Fixture);
    assert!(listing.error.is_none());
    assert!(listing.is_empty_folder());

    let rendered = text(&render(&state));
    assert!(rendered.contains("empty folder"), "{rendered}");
    assert!(rendered.contains("← back · ~ home"), "{rendered}");
    assert!(!rendered.contains("cannot read"), "{rendered}");
    assert!(rendered.contains("▴  .."), "{rendered}");
}

/// The same distinction against the real filesystem, so the branch the
/// fixture describes is the branch `read_dir` actually produces.
#[test]
fn the_filesystem_browser_separates_empty_from_unreadable() {
    let root = temp_root("states");
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let browser = FsBrowse::new(vec![root.clone()]);

    let listing = browser.list(&empty);
    assert!(listing.error.is_none(), "an empty folder reads fine");
    assert!(
        listing.is_empty_folder(),
        "and holds nothing but its parent"
    );

    let missing = root.join("does-not-exist");
    let listing = browser.list(&missing);
    assert!(
        listing.error.is_some(),
        "a folder that cannot be read reports why"
    );
    assert!(!listing.is_empty_folder(), "and is not called empty");
    assert!(
        listing
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Parent),
        "the way out is still there"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

/// Overlapping roots — the default pair is the working directory and the
/// home above it — must not list the same repository twice. The nearer
/// root claims it; a repository no root contains still shows up under the
/// filter's own rule.
#[test]
fn a_repository_inside_two_roots_is_listed_once() {
    let root = temp_root("overlap");
    let outer = root.join("outer");
    let inner = outer.join("inner");
    let shared = inner.join("repo-shared");
    let only_outer = outer.join("repo-outer");
    for repository in [&shared, &only_outer] {
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::write(repository.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    // Both roots contain `repo-shared`; only the outer contains
    // `repo-outer`.
    let browser = FsBrowse::new(vec![outer.clone(), inner.clone()]);

    let found = browser.search(&inner);

    let names: Vec<_> = found.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names.iter().filter(|name| **name == "repo-shared").count(),
        1,
        "the nearer root claims it: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|name| **name == "repo-outer").count(),
        1,
        "{names:?}"
    );

    // And through the filter, the shared repository sits in the browsed
    // folder's own partition while the outer one is ruled off as
    // elsewhere — each exactly once.
    let mut state = PickerState::at(&inner);
    state.begin_filter();
    for character in "repo".chars() {
        state.push_filter(character);
    }
    let listing = state.listing(&browser);

    let names: Vec<_> = listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["repo-shared", "repo-outer"], "{names:?}");
    assert_eq!(listing.elsewhere, 1, "only the unclaimed one is elsewhere");

    std::fs::remove_dir_all(&root).unwrap();
}

/// Mouse parity's other half (§7): the secondary button is the `-`.
#[test]
fn the_secondary_button_sheds_an_instance() {
    let mut state = browsing();
    let rows = rows_of(&state);
    let frontend = entry(&state, "horizon-frontend");
    state.add(&frontend);
    state.add(&frontend);
    let index = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();

    assert!(click_secondary(&mut state, &rows, Hit::Badge(index)));

    assert_eq!(state.instances(&frontend.path), 1, "one shed, one kept");
    assert!(click_secondary(&mut state, &rows, Hit::Badge(index)));
    assert!(state.selection().is_empty());
    assert!(
        !click_secondary(&mut state, &rows, Hit::Badge(index)),
        "nothing left to shed"
    );
}

/// The filter's own rule: matches from other roots are counted, and the
/// view draws them under their own heading.
#[test]
fn matches_from_other_roots_are_counted_and_ruled_off() {
    let mut state = browsing();
    state.begin_filter();
    for character in "hor".chars() {
        state.push_filter(character);
    }

    let listing = state.listing(&Fixture);

    assert_eq!(listing.elsewhere, 1, "horizon-docs lives under ~/work");
    assert_eq!(
        listing.entries.last().map(|entry| entry.name.as_str()),
        Some("horizon-docs"),
        "and it sorts after everything from this root"
    );
    let rendered = text(&render(&state));
    assert!(rendered.contains("also in other roots"), "{rendered}");
    // The rule introduces the match; it does not replace it.
    assert!(
        rendered.contains("horizon-docs"),
        "the elsewhere match is drawn under its rule: {rendered}"
    );
    let rule = rendered.find("also in other roots").unwrap();
    let docs = rendered.find("horizon-docs").unwrap();
    assert!(rule < docs, "the rule comes first");
}

// ---- the runtime-add sheet (#50 A3) -------------------------------

fn open_two() -> Vec<Open> {
    vec![
        Open {
            path: code().join("horizon-frontend"),
            pane: 1,
        },
        Open {
            path: code().join("horizon-backend"),
            pane: 2,
        },
    ]
}

fn sheet_state() -> SheetState {
    SheetState::new(&Fixture.roots())
}

fn sheet_rows(sheet: &SheetState) -> Vec<Entry> {
    sheet.rows(&Fixture)
}

fn render_sheet(sheet: &SheetState, open: &[Open]) -> Buffer {
    let rows = sheet_rows(sheet);
    let roots = Fixture.roots();
    let view = Sheet {
        state: sheet,
        rows: &rows,
        roots: &roots,
        open,
        home: Some(Path::new("/home/dev")),
        next_pane: open.len() + 1,
    };
    let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
    terminal.draw(|frame| view.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The sheet lists the current folder's targets, including folders and
/// already-open repositories, without locking either.
#[test]
fn the_sheet_lists_unrestricted_terminal_targets() {
    let sheet = sheet_state();
    let open = open_two();

    let rendered = text(&render_sheet(&sheet, &open));

    assert!(rendered.contains("> add terminal"), "{rendered}");
    assert!(rendered.contains("ROOT ~/code"), "{rendered}");
    assert!(
        rendered.contains("[ ] ◆  horizon-frontend    +  · already open · pane 1"),
        "{rendered}"
    );
    assert!(
        rendered.contains("[ ] ▸  archive"),
        "a plain folder is a terminal target: {rendered}"
    );
    assert!(rendered.contains("[ ] ◆  termdeck"), "an unopened one");
    assert!(
        rendered.contains("esc cancel · master unchanged"),
        "{rendered}"
    );
}

/// `⏎` marks an open row as another instance rather than refusing it.
#[test]
fn enter_marks_another_instance_of_an_open_repository() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let open = open_two();
    let frontend = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    sheet.state_mut().point_at(frontend, rows.len());

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert_eq!(sheet.marked().len(), 1);
    assert_eq!(sheet.marked()[0].name, "horizon-frontend");
    let rendered = text(&render_sheet(&sheet, &open));
    assert!(rendered.contains("[+] ◆  horizon-frontend"), "{rendered}");
    assert!(rendered.contains("another instance · pane 1"), "{rendered}");
    assert!(rendered.contains("o  Add 1 terminal"), "{rendered}");
}

/// The same regression inside the runtime-add sheet: `ctrl+j` moves the
/// cursor rather than marking the row it started on (#95).
#[test]
fn ctrl_j_moves_the_sheet_cursor_down_instead_of_marking() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let open = open_two();
    sheet.state_mut().point_at(0, rows.len());

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Ctrl('j'));

    assert_eq!(sheet.state().cursor(), 1, "the same step `j` takes");
    assert!(
        sheet.marked().is_empty(),
        "and nothing was marked on the way"
    );
}

/// `+` still appends an explicit additional instance of an open path.
#[test]
fn plus_appends_another_instance_of_an_open_repository() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let open = open_two();
    let frontend = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    sheet.state_mut().point_at(frontend, rows.len());

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('+'));

    assert_eq!(sheet.marked().len(), 1, "an open path is still targetable");
    assert_eq!(sheet.marked()[0].path, code().join("horizon-frontend"));

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('+'));
    assert_eq!(sheet.marked().len(), 2, "and again for a third pane");
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('-'));
    assert_eq!(sheet.marked().len(), 1, "`-` sheds one");
}

/// The add sheet's advertised keys move, mark, add instances, switch
/// roots, filter, and commit those marks — without a range-select path.
#[test]
fn the_add_sheet_keys_drive_its_selection_state() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let open = open_two();

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Down);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Up);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Down);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Down);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Down);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Down);
    assert_eq!(rows[sheet.state().cursor()].name, "horizon-app");

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert_eq!(sheet.marked().len(), 1, "enter marks the row");
    assert_eq!(sheet.marked()[0].name, "horizon-app");
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert!(sheet.marked().is_empty(), "enter unmarks it again");

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('+'));
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('+'));
    assert_eq!(sheet.marked().len(), 2, "plus adds instances");
    assert_eq!(sheet.marked()[1].name, "horizon-app-2");
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert!(sheet.marked().is_empty(), "enter unmarks every instance");

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    sheet_press(&mut sheet, &rows, &roots, &open, Key::ShiftTab);
    assert_eq!(sheet.root(), 1, "shift-tab switches roots");
    assert_eq!(sheet.marked().len(), 1, "marks survive the switch");
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('/'));
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('t'));
    assert_eq!(sheet.state().filter(), Some("t"));
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Escape);
    assert_eq!(
        sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('o')),
        Some(PickerReaction::Launch),
        "o commits the marked rows"
    );
}

/// `→` and `←` use the picker's folder navigation, while `⏎` keeps a
/// folder a terminal target in its own right.
#[test]
fn the_sheet_navigates_into_repositories_and_marks_folders() {
    let mut sheet = sheet_state();
    let roots = Fixture.roots();
    let open = open_two();
    let rows = sheet_rows(&sheet);
    let frontend = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();
    sheet.state_mut().point_at(frontend, rows.len());

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Right);
    assert_eq!(
        sheet.state().cwd(),
        Some(code().join("horizon-frontend").as_path())
    );
    let inside = sheet_rows(&sheet);
    assert_eq!(inside[0].name, "..", "the repository's contents are shown");
    assert_eq!(inside[1].name, "projects");

    sheet_press(&mut sheet, &inside, &roots, &open, Key::Left);
    let rows = sheet_rows(&sheet);
    let archive = rows
        .iter()
        .position(|entry| entry.name == "archive")
        .unwrap();
    sheet.state_mut().point_at(archive, rows.len());
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert_eq!(sheet.marked()[0].path, code().join("archive"));
}

/// `a` is a bulk terminal-target gesture, so folders join repositories
/// while files and the parent row stay out.
#[test]
fn the_sheet_marks_every_selectable_target() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();

    sheet_press(&mut sheet, &rows, &roots, &open_two(), Key::Char('a'));

    assert_eq!(
        sheet.marked().len(),
        8,
        "five repositories and three folders"
    );
    assert!(
        sheet
            .marked()
            .iter()
            .any(|target| target.path == code().join("archive"))
    );
    assert!(
        !sheet
            .marked()
            .iter()
            .any(|target| target.name == "README.md")
    );
    assert!(
        !sheet.marked().iter().any(|target| target.name == ".."),
        "the parent row is navigation, not a terminal"
    );
}

/// `⇧⇥` cycles the configured roots in place, and the marks survive it —
/// the sheet's selection is as workspace-wide as the picker's.
#[test]
fn the_sheet_cycles_roots_and_keeps_what_is_marked() {
    let mut sheet = sheet_state();
    let roots = Fixture.roots();
    let rows = sheet_rows(&sheet);
    let open = open_two();
    let termdeck = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();
    sheet.state_mut().point_at(termdeck, rows.len());
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert_eq!(sheet.root(), 0);

    sheet_press(&mut sheet, &rows, &roots, &open, Key::Tab);

    assert_eq!(sheet.root(), 1, "the next configured root");
    assert_eq!(
        sheet.marked().len(),
        1,
        "and the mark made under the first one is still there"
    );
}

/// `esc` closes without adding; `o` commits what is marked.
#[test]
fn escape_cancels_the_sheet_and_o_commits_it() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let open = open_two();

    assert_eq!(
        sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('o')),
        None,
        "nothing marked, nothing to add"
    );
    assert_eq!(
        sheet_press(&mut sheet, &rows, &roots, &open, Key::Escape),
        Some(PickerReaction::Quit)
    );

    let termdeck = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();
    sheet.state_mut().point_at(termdeck, rows.len());
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);
    assert_eq!(
        sheet_press(&mut sheet, &rows, &roots, &open, Key::Char('o')),
        Some(PickerReaction::Launch)
    );
}

/// The naming rule the session uses when it commits: a second instance of
/// an open path takes `-2`, skipping what is already running.
#[test]
fn an_instance_of_an_open_path_is_named_around_the_running_ones() {
    let running = ["horizon-frontend", "horizon-backend"];
    let taken: Vec<&str> = running.to_vec();

    assert_eq!(unique_name(&taken, "termdeck"), "termdeck");
    assert_eq!(
        unique_name(&taken, "horizon-frontend"),
        "horizon-frontend-2"
    );

    let taken = ["horizon-frontend", "horizon-frontend-2"];
    assert_eq!(
        unique_name(taken.as_ref(), "horizon-frontend"),
        "horizon-frontend-3"
    );
}

/// Mouse parity in the sheet (#50 NB): every gesture drives the state
/// its key drives, and the keys are untouched by any of it.
#[test]
fn the_pointer_marks_a_row_exactly_as_enter_does() {
    let roots = Fixture.roots();
    let open = open_two();
    let rows = sheet_rows(&sheet_state());
    let termdeck = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();
    let frontend = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();

    let mut by_key = sheet_state();
    by_key.state_mut().point_at(termdeck, rows.len());
    sheet_press(&mut by_key, &rows, &roots, &open, Key::Enter);

    let mut by_pointer = sheet_state();
    sheet_click(
        &mut by_pointer,
        &rows,
        &roots,
        &open,
        SheetHit::Row(termdeck),
    );

    assert_eq!(by_pointer.marked(), by_key.marked());
    assert_eq!(by_pointer.state().cursor(), termdeck, "and it points there");

    // An open path is just another target for the pointer too.
    sheet_click(
        &mut by_pointer,
        &rows,
        &roots,
        &open,
        SheetHit::Row(frontend),
    );
    assert_eq!(by_pointer.marked().len(), 2, "an open repo is re-marked");
}

/// The instance slot is `+`, and `-` on the secondary button, including
/// for a path the session already holds.
#[test]
fn the_instance_slot_is_the_pointers_plus_and_minus() {
    let roots = Fixture.roots();
    let open = open_two();
    let rows = sheet_rows(&sheet_state());
    let frontend = rows
        .iter()
        .position(|entry| entry.name == "horizon-frontend")
        .unwrap();

    let mut by_key = sheet_state();
    by_key.state_mut().point_at(frontend, rows.len());
    sheet_press(&mut by_key, &rows, &roots, &open, Key::Char('+'));
    sheet_press(&mut by_key, &rows, &roots, &open, Key::Char('+'));

    let mut by_pointer = sheet_state();
    sheet_click(
        &mut by_pointer,
        &rows,
        &roots,
        &open,
        SheetHit::Instance(frontend),
    );
    sheet_click(
        &mut by_pointer,
        &rows,
        &roots,
        &open,
        SheetHit::Instance(frontend),
    );

    assert_eq!(by_pointer.marked().len(), 2, "an open row still appends");
    assert_eq!(by_pointer.marked(), by_key.marked());

    // And the secondary button sheds one, as `-` does.
    sheet_press(&mut by_key, &rows, &roots, &open, Key::Char('-'));
    assert!(sheet_click_secondary(
        &mut by_pointer,
        &rows,
        SheetHit::Instance(frontend)
    ));
    assert_eq!(by_pointer.marked(), by_key.marked());
    assert_eq!(by_pointer.marked().len(), 1);
}

/// The header's switch label is `⇧⇥`, and the query line is `/`.
#[test]
fn the_header_and_query_line_answer_the_pointer() {
    let roots = Fixture.roots();
    let open = open_two();
    let rows = sheet_rows(&sheet_state());

    let mut by_key = sheet_state();
    sheet_press(&mut by_key, &rows, &roots, &open, Key::ShiftTab);
    let mut by_pointer = sheet_state();
    sheet_click(&mut by_pointer, &rows, &roots, &open, SheetHit::Root);
    assert_eq!(by_pointer.root(), by_key.root());
    assert_eq!(by_pointer.root(), 1);
    // And it keeps going round, as the key does.
    for expected in [2, 0, 1] {
        sheet_click(&mut by_pointer, &rows, &roots, &open, SheetHit::Root);
        assert_eq!(by_pointer.root(), expected, "three roots, cycled");
    }

    let mut by_key = sheet_state();
    sheet_press(&mut by_key, &rows, &roots, &open, Key::Char('/'));
    let mut by_pointer = sheet_state();
    sheet_click(&mut by_pointer, &rows, &roots, &open, SheetHit::Filter);
    assert_eq!(by_pointer.state().filter(), by_key.state().filter());
    assert_eq!(by_pointer.state().filter(), Some(""));
}

/// The button is `o`, and it refuses the same press the key refuses.
#[test]
fn the_button_commits_exactly_as_o_does() {
    let roots = Fixture.roots();
    let open = open_two();
    let rows = sheet_rows(&sheet_state());
    let mut sheet = sheet_state();

    assert_eq!(
        sheet_click(&mut sheet, &rows, &roots, &open, SheetHit::Add),
        None,
        "nothing marked, nothing to add"
    );

    let termdeck = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();
    sheet_click(&mut sheet, &rows, &roots, &open, SheetHit::Row(termdeck));

    assert_eq!(
        sheet_click(&mut sheet, &rows, &roots, &open, SheetHit::Add),
        Some(PickerReaction::Launch)
    );
}

/// The targets are where the sheet draws them: found in the rendered
/// buffer rather than assumed, so the hit test cannot drift from the
/// glyph it belongs to.
#[test]
fn every_sheet_target_sits_on_what_it_is_drawn_as() {
    let sheet = sheet_state();
    let open = open_two();
    let buffer = render_sheet(&sheet, &open);
    let rows = sheet_rows(&sheet);
    let roots = Fixture.roots();
    let view = Sheet {
        state: &sheet,
        rows: &rows,
        roots: &roots,
        open: &open,
        home: Some(Path::new("/home/dev")),
        next_pane: 3,
    };
    let area = ratatui::layout::Rect::new(0, 0, 144, 42);
    let find = |needle: &str, row: u16| -> u16 {
        (0..144u16)
            .find(|column| buffer[(*column, row)].symbol() == needle)
            .unwrap_or_else(|| panic!("{needle} is drawn on row {row}"))
    };

    // `..` is the first listed row: it is navigation-only, so even the
    // instance-slot columns remain its row.
    let first = view.rect(area).y + 3;
    let slot = view.rect(area).x + 2 + SHEET_INSTANCE;
    assert_eq!(
        view.hit(area, Position::new(slot, first)),
        Some(SheetHit::Row(0))
    );

    // The first terminal target, `archive/`, owns the same two-cell
    // instance slot as `+`.
    let archive = first + 1;
    let plus = find("+", archive);
    assert_eq!(
        view.hit(area, Position::new(plus, archive)),
        Some(SheetHit::Instance(1))
    );
    assert_eq!(
        view.hit(area, Position::new(plus + 1, archive)),
        Some(SheetHit::Instance(1)),
        "two columns wide, so the click needs no precision"
    );
    assert_eq!(
        view.hit(area, Position::new(plus - 4, archive)),
        Some(SheetHit::Row(1)),
        "the name is still the row"
    );

    // The header's switch label, the query line and the button.
    let header = view.rect(area).y + 1;
    let switch = find("⇧", header);
    assert_eq!(
        view.hit(area, Position::new(switch, header)),
        Some(SheetHit::Root)
    );
    assert_eq!(
        view.hit(area, Position::new(switch - 20, header)),
        None,
        "the census beside it is not a control"
    );
    let bottom = view.rect(area).y + view.rect(area).height;
    assert_eq!(
        view.hit(area, Position::new(switch, bottom - 4)),
        Some(SheetHit::Filter)
    );
    let button = find("o", bottom - 3);
    assert_eq!(
        view.hit(area, Position::new(button, bottom - 3)),
        Some(SheetHit::Add)
    );
}

#[test]
fn add_sheet_matches_the_runtime_add_canvas() {
    let mut sheet = sheet_state();
    let rows = sheet_rows(&sheet);
    let open = open_two();
    let roots = Fixture.roots();
    let termdeck = rows
        .iter()
        .position(|entry| entry.name == "termdeck")
        .unwrap();
    sheet.state_mut().point_at(termdeck, rows.len());
    sheet_press(&mut sheet, &rows, &roots, &open, Key::Enter);

    assert_snapshot("add-sheet", &render_sheet(&sheet, &open));
}

#[test]
fn browse_matches_the_picker_canvas() {
    assert_snapshot("picker-browse", &render(&browsing()));
}

#[test]
fn a_doubled_selection_shows_its_instance_badge() {
    let mut state = browsing();
    select(&mut state, "horizon-frontend");
    select(&mut state, "horizon-backend");
    let frontend = entry(&state, "horizon-frontend");
    state.add(&frontend);

    let buffer = render(&state);
    let rendered = text(&buffer);

    assert!(rendered.contains("×2"), "{rendered}");
    assert!(
        rendered.contains("horizon-frontend-2"),
        "the panel lists it"
    );
    assert!(rendered.contains("o  Open 3 as terminals"), "{rendered}");
    assert_snapshot("picker-selected", &buffer);
}

#[test]
fn a_filter_with_no_match_says_so_and_keeps_the_selection() {
    let mut state = browsing();
    select(&mut state, "termdeck");
    state.begin_filter();
    for character in "zzq".chars() {
        state.push_filter(character);
    }

    let buffer = render(&state);
    let rendered = text(&buffer);

    assert!(rendered.contains("no match for zzq"), "{rendered}");
    assert!(rendered.contains("selection kept (1)"), "{rendered}");
    assert_snapshot("picker-no-match", &buffer);
}

#[test]
fn the_root_list_is_the_pickers_own_top_level() {
    let state = PickerState::new();

    let buffer = render(&state);
    let rendered = text(&buffer);

    assert!(rendered.contains("ROOTS"), "{rendered}");
    assert!(
        rendered.contains("nothing selected · o disabled"),
        "{rendered}"
    );
    assert_snapshot("picker-roots", &buffer);
}
