use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect},
};

use super::{
    ACCENT, CHIP_BG, DEMOTED_BG, DEMOTED_BORDER, Deck, DeckState, ERROR, HINT, IDLE_BORDER,
    Notifications, SEPARATOR, STATUS_BG, UNDER_FG, UNDER_HINT, WARNING, fixture,
};
use crate::{
    contracts::{
        ActionCommand, NotifyKind, Project, ScrollbackPosition, TerminalEngine, TerminalId,
        TerminalMetadata, TerminalStatus, Timestamp,
    },
    engine::FakeEngine,
};

/// A deck with nothing pending: what every canvas that predates #97 draws.
fn quiet() -> &'static Notifications {
    static QUIET: std::sync::OnceLock<Notifications> = std::sync::OnceLock::new();
    QUIET.get_or_init(Notifications::new)
}

/// Renders one reference canvas at the given size.
fn render(engine: &FakeEngine, state: &DeckState, size: (u16, u16)) -> (Buffer, Option<Position>) {
    render_at(engine, state, quiet(), fixture::NOW, size)
}

/// The same canvas with notifications pending, at a chosen frame of their
/// windows (#97). Time is an input here exactly as it is in the deck: every
/// keyframe below is one call with a different `now`.
fn render_at(
    engine: &FakeEngine,
    state: &DeckState,
    notifies: &Notifications,
    now: Timestamp,
    size: (u16, u16),
) -> (Buffer, Option<Position>) {
    let projects = fixture::projects();
    let deck = Deck {
        workspace: "idp",
        projects: &projects,
        state,
        notifies,
        master_ratio: state.master_ratio(),
        now,
    };
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| deck.render(engine as &dyn TerminalEngine, frame))
        .unwrap();
    let cursor = terminal.get_cursor_position().ok();
    (terminal.backend().buffer().clone(), cursor)
}

/// The accepted 144x42 reference canvas with the first terminal as master.
fn reference() -> (Buffer, Option<Position>) {
    render(&fixture::frontend_active(), &reference_deck(4), (144, 42))
}

/// Open previews own three bands of the column; the folded default is
/// covered by `a_folded_strip_is_hit_tested_but_has_nothing_to_scroll`.
#[test]
fn pane_hit_testing_follows_the_rendered_layout() {
    let projects = fixture::projects();
    let state = &expanded(4);
    let deck = Deck {
        workspace: "idp",
        projects: &projects,
        state,
        notifies: quiet(),
        master_ratio: state.master_ratio(),
        now: fixture::NOW,
    };
    let area = Rect::new(0, 0, 144, 42);

    let terminal = |pointer| deck.terminal_at(area, pointer).map(ToString::to_string);
    assert_eq!(terminal(Position::new(10, 12)).as_deref(), Some("frontend"));
    assert_eq!(terminal(Position::new(110, 7)).as_deref(), Some("backend"));
    assert_eq!(terminal(Position::new(110, 20)).as_deref(), Some("app"));
    assert_eq!(terminal(Position::new(110, 33)).as_deref(), Some("worker"));
    assert_eq!(
        terminal(Position::new(99, 12)),
        None,
        "the gutter is not a pane"
    );
    assert_eq!(
        terminal(Position::new(10, 1)),
        None,
        "the blank row is not a pane"
    );
    assert_eq!(deck.swap_position_at(area, Position::new(10, 12)), Some(0));
    assert_eq!(
        deck.swap_position_at(Rect::new(0, 0, 84, 22), Position::new(10, 12)),
        None,
        "a hidden stack has no swap target"
    );
}

/// #74: the cell math wheel forwarding stands on — zoomed, stacked and
/// folded panes, plus the chrome that is no pane at all.
#[test]
fn pane_cell_measures_from_the_viewport_origin() {
    let projects = fixture::projects();
    let area = Rect::new(0, 0, 144, 42);
    fn deck<'a>(projects: &'a [Project], state: &'a DeckState) -> Deck<'a> {
        Deck {
            workspace: "idp",
            projects,
            state,
            notifies: quiet(),
            master_ratio: state.master_ratio(),
            now: fixture::NOW,
        }
    }

    // Zoomed: the body is the pane, so the origin is border plus inset.
    let mut zoomed = reference_deck(4);
    zoomed.apply(&ActionCommand::ToggleZoom, &projects, fixture::NOW);
    assert_eq!(
        deck(&projects, &zoomed).pane_cell(area, Position::new(10, 12)),
        Some((0, 7, 9)),
        "column past border and title inset, row past the border"
    );

    // Stacked: the master agrees with hit testing, the gutter and the
    // blank row are nothing.
    let state = expanded(4);
    let pane = deck(&projects, &state);
    assert!(matches!(
        pane.pane_cell(area, Position::new(10, 12)),
        Some((0, _, _))
    ));
    assert_eq!(pane.pane_cell(area, Position::new(99, 12)), None);
    assert_eq!(pane.pane_cell(area, Position::new(10, 1)), None);

    // Folded strips keep hit testing but own no cells to forward into.
    let folded = DeckState::new(4);
    let pane = deck(&projects, &folded);
    let strip = (2..42)
        .flat_map(|y| (100..144).map(move |x| Position::new(x, y)))
        .find(|pointer| {
            pane.position_at(area, *pointer).is_some() && pane.pane_cell(area, *pointer).is_none()
        });
    assert!(
        strip.is_some(),
        "a folded strip is hit-tested but owns no cells"
    );
    assert!(
        matches!(pane.pane_cell(area, Position::new(10, 12)), Some((0, _, _))),
        "the master still does"
    );
}

/// Hotfix: #74's `pane_cell` predates `Layout::Single` and did not cover
/// it, which broke the build. An empty deck's master is the full body,
/// so its cells measure from the same origin as a zoomed master's.
#[test]
fn pane_cell_covers_an_empty_stack() {
    let projects = synthetic(1);
    let state = DeckState::new(1);
    let area = Rect::new(0, 0, 144, 42);
    let pane = deck_for(&projects, &state);

    assert_eq!(
        pane.pane_cell(area, Position::new(130, 12)),
        Some((0, 127, 9)),
        "far past where the stack would start, still the full-body master"
    );
    assert_eq!(
        pane.pane_cell(area, Position::new(10, 1)),
        None,
        "the blank row is still nothing"
    );
}

#[test]
fn dragging_marks_the_source_and_only_valid_drop_target() {
    // The drop target's border chrome is what this reads, so the previews
    // are open.
    let mut state = expanded(4);
    assert!(state.begin_drag(1));
    state.update_drag(Some(0));

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // The held preview is warning-coloured, while the master is lifted as
    // the valid drop target using the accepted accent palette.
    assert_eq!(buffer[(100u16, 2u16)].fg, WARNING);
    assert_eq!(buffer[(0u16, 2u16)].fg, ACCENT);
    assert_eq!(buffer[(5u16, 3u16)].bg, DEMOTED_BG);
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

/// Compares against a committed snapshot. Set `TERMDECK_BLESS` to rewrite
/// the snapshots after a reviewed visual change.
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

fn promote(position: usize) -> DeckState {
    let mut state = reference_deck(4);
    state.apply(
        &ActionCommand::SelectPosition(position),
        &fixture::projects(),
        fixture::NOW,
    );
    state
}

fn zoomed() -> DeckState {
    let mut state = reference_deck(4);
    state.apply(
        &ActionCommand::ToggleZoom,
        &fixture::projects(),
        fixture::NOW,
    );
    state
}

/// The split the design export draws every screen at: 98 columns of
/// master, the 2-column gutter and the 44-column stack, which is a
/// `master_ratio` of 0.70.
///
/// Since #44 a fresh deck starts at the top of the range instead, giving
/// the stack its minimum width, so the canvases the export fixes ask for
/// its split by name rather than inheriting it from the default. What the
/// default itself draws is held to account by `fresh_start_*` below.
const EXPORT_SPLIT: f64 = 0.70;

fn reference_deck(terminals: usize) -> DeckState {
    DeckState::new(terminals).with_master_ratio(EXPORT_SPLIT)
}

/// The export's screen 05: app and worker folded to their title rows.
/// Since #39 every preview starts folded, so the one open preview is what
/// this has to ask for; the rendered state is the same as before.
fn collapsed() -> DeckState {
    let mut state = reference_deck(4);
    assert!(state.toggle_collapse(1));
    state
}

/// A deck with every preview open. Since #39 that is no longer the state
/// a run starts in, so a test whose subject is open-preview chrome or
/// geometry asks for it rather than leaning on the default.
fn expanded(terminals: usize) -> DeckState {
    let mut state = reference_deck(terminals);
    assert!(state.toggle_collapse_all());
    assert_eq!(state.collapsed_count(), 0);
    state
}

#[test]
fn collapsed_stack_matches_the_reference_canvas() {
    let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));

    assert_snapshot("collapsed-stack", &buffer);
}

/// Screen 05 measured: the one open preview takes both folds' rows, so it
/// runs from row 0 to row 33 and the strips sit on rows 35 and 37.
#[test]
fn folded_previews_hand_their_rows_to_the_one_still_open() {
    let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));

    assert_eq!(buffer[(100u16, 2u16)].symbol(), "┌");
    assert_eq!(
        buffer[(100u16, 35u16)].symbol(),
        "└",
        "12 + 2 x 11 = 34 rows"
    );
    assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
    assert_eq!(buffer[(102u16, 39u16)].symbol(), "▸");
    // The strips sit on the export's #101317, and carry no border.
    assert_eq!(buffer[(100u16, 37u16)].bg, DEMOTED_BG);
}

/// One fold among three previews: 11 freed rows split 6/5, the remainder
/// going to the topmost open pane.
#[test]
fn freed_rows_split_evenly_with_the_remainder_going_to_the_top() {
    let mut state = reference_deck(4);
    state.toggle_collapse(1);
    state.toggle_collapse(2);
    assert_eq!(state.collapsed_count(), 1, "worker alone is left folded");

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // 18 rows, then 17, then the strip.
    assert_eq!(buffer[(100u16, 19u16)].symbol(), "└");
    assert_eq!(buffer[(100u16, 21u16)].symbol(), "┌");
    assert_eq!(buffer[(100u16, 37u16)].symbol(), "└");
    assert_eq!(buffer[(102u16, 39u16)].symbol(), "▸");
}

/// Every fold hands over exactly the rows it gave up, so the stack always
/// ends on the same row whatever the mix.
#[test]
fn folding_never_changes_the_height_the_stack_uses() {
    // Walked from the folded default outwards, one expansion at a time.
    let mut state = reference_deck(4);

    for opened in 0..4 {
        let folds = 3 - opened;
        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        // The stack's own columns, on the row the footer always owns.
        let footer = (100..144)
            .map(|x| buffer[(x, 41u16)].symbol())
            .collect::<String>();
        // A fold states itself in its strip and in the status row, so the
        // footer has nothing to add — but the row is still the footer's,
        // and still row 39.
        assert_eq!(
            footer.trim(),
            "",
            "the footer stays blank on row 39 with {folds} folded"
        );
        // The row above it stays blank, so nothing has overrun.
        assert_eq!(buffer[(102u16, 40u16)].symbol(), " ");
        if opened < 3 {
            state.toggle_collapse(opened + 1);
        }
    }
}

/// Issues #32 and #39: a fresh run has to show the affordance, and since
/// #39 the state it shows is folded. Every stack pane carries `▸` before
/// anything is expanded, and the master carries no marker at all.
#[test]
fn the_disclosure_markers_are_drawn_before_anything_is_expanded() {
    let (plain, _) = render(&fixture::frontend_active(), &reference_deck(4), (144, 42));
    let rendered = text(&plain);

    assert!(rendered.contains("▸ 2 backend"), "{rendered}");
    assert!(rendered.contains("▸ 3 app"), "{rendered}");
    assert!(rendered.contains("▸ 4 worker"), "{rendered}");
    assert_eq!(rendered.matches('▸').count(), 3, "one per stacked preview");
    assert!(!rendered.contains("▾"), "nothing is expanded yet");
    assert!(
        !rendered.contains("▸ > 1 frontend"),
        "the master never folds, so it never claims a marker"
    );

    // Expanding every preview is what turns them over.
    let (open, _) = render(&fixture::frontend_active(), &expanded(4), (144, 42));
    let opened = text(&open);
    assert_eq!(opened.matches('▾').count(), 3, "one per stacked preview");
    assert!(!opened.contains("▸"), "nothing is folded any more");
}

/// The markers track each preview's own state once folds are in play.
#[test]
fn each_marker_states_its_own_panes_fold() {
    let (folded, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
    let rendered = text(&folded);

    assert!(rendered.contains("▾ 2 backend"), "the open pane opens");
    assert!(rendered.contains("▸ 3 app"), "the folded panes close");
    assert!(rendered.contains("▸ 4 worker"), "{rendered}");
}

/// An exit outranks the live output, so a folded exited pane says so.
#[test]
fn a_folded_pane_states_its_exit_and_its_idle_age() {
    let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
    let rendered = text(&buffer);

    assert!(rendered.contains("▸ 3 app · ✕ · exit 1"), "{rendered}");
    assert!(rendered.contains("▸ 4 worker · ○ · idle 6m"), "{rendered}");
}

/// A pane that is neither exited nor idle falls back to its last output.
#[test]
fn a_folded_running_pane_shows_its_last_output_line() {
    // Backend is folded from the first frame since #39.
    let state = reference_deck(4);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // The tail takes what the 44-column strip has left — less the close
    // affordance's own three columns (#84) — and clips.
    assert!(
        text(&buffer).contains("▸ 2 backend · ● · 12:06:09 /api/term…"),
        "{}",
        text(&buffer)
    );
}

/// The census and the key hint both switch over, and the exited summary
/// gives up its place, exactly as screen 05 states it.
#[test]
fn the_status_bar_counts_the_folds_instead_of_the_stack() {
    let (buffer, _) = render(&fixture::frontend_active(), &collapsed(), (144, 42));
    let rendered = text(&buffer);

    assert!(
        rendered.contains("> 1 frontend  ·  + add  ·  1 open  ·  2 collapsed"),
        "{rendered}"
    );
    assert!(!rendered.contains("3 stacked"));
    assert!(
        !rendered.contains("1 exited"),
        "the fold census stands alone"
    );
    // The `+ add` affordance (#50) leaves the reference row without the
    // columns for labels; the key is what it promises.
    assert!(rendered.contains("^g c"));
    assert!(
        !rendered.contains("^g [ scroll"),
        "collapse takes scroll's slot"
    );
}

/// A folded strip still answers the hit test, so promoting or swapping it
/// by pointer keeps working; the caller decides it has nothing to scroll.
#[test]
fn a_folded_strip_is_hit_tested_but_has_nothing_to_scroll() {
    let projects = fixture::projects();
    let state = collapsed();
    let deck = Deck {
        workspace: "idp",
        projects: &projects,
        state: &state,
        notifies: quiet(),
        master_ratio: state.master_ratio(),
        now: fixture::NOW,
    };
    let area = Rect::new(0, 0, 144, 42);

    let position = deck.position_at(area, Position::new(110, 37));

    assert_eq!(position, Some(2), "the strip on row 35 is app");
    assert!(state.collapsed(2), "so the wheel skips it");
    assert_eq!(deck.position_at(area, Position::new(110, 12)), Some(1));
    assert!(!state.collapsed(1), "the open preview still scrolls");
}

fn deck_for<'a>(projects: &'a [Project], state: &'a DeckState) -> Deck<'a> {
    Deck {
        workspace: "idp",
        projects,
        state,
        notifies: quiet(),
        master_ratio: state.master_ratio(),
        now: fixture::NOW,
    }
}

/// Every cell the close affordance is drawn in, top to bottom (#84).
///
/// The body only: the status row keeps the glyph for the narrow fallback's
/// `84×22`, which is a size and not an affordance, and no pane is ever drawn
/// there.
fn close_marks(buffer: &Buffer) -> Vec<Position> {
    let area = buffer.area();
    (2..area.height)
        .flat_map(|row| (0..area.width).map(move |column| Position::new(column, row)))
        .filter(|at| buffer[(at.x, at.y)].symbol() == super::CLOSE_AFFORDANCE)
        .collect()
}

/// #84: the pointer's half of `^g x`. Every drawn pane carries a mark at the
/// inset the title keeps on the left, the master included, and the cells that
/// answer are the cells the mark is actually drawn in.
#[test]
fn every_open_pane_draws_a_close_mark_the_pointer_answers_for() {
    let projects = fixture::projects();
    let state = expanded(4);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let view = deck_for(&projects, &state);

    // The master at column 94, and the three open previews at 140: two
    // columns in from each pane's right edge, which is where the title starts
    // on the left.
    assert_eq!(
        close_marks(&buffer),
        [
            Position::new(94, 2),
            Position::new(140, 2),
            Position::new(140, 15),
            Position::new(140, 28),
        ]
    );
    for (mark, position) in close_marks(&buffer).into_iter().zip([0, 1, 2, 3]) {
        assert_eq!(view.close_at(SCREEN, mark), Some(position), "{mark:?}");
        // The blank beside it belongs to the affordance too, so the pointer
        // has the marker's own two cells to land in.
        assert_eq!(
            view.close_at(SCREEN, Position::new(mark.x + 1, mark.y)),
            Some(position)
        );
        assert_eq!(
            view.close_at(SCREEN, Position::new(mark.x - 1, mark.y)),
            None
        );
        assert_eq!(
            view.close_at(SCREEN, Position::new(mark.x, mark.y + 1)),
            None
        );
    }
    // And it never takes cells the other title affordances own.
    assert_eq!(view.close_at(SCREEN, Position::new(102, 2)), None);
    assert_eq!(view.marker_at(SCREEN, Position::new(140, 2)), None);
}

/// A folded preview closes like an open one, and its mark lines up with
/// theirs: one column down the stack, whatever each pane is doing.
#[test]
fn a_folded_strip_carries_the_same_close_mark_as_an_open_pane() {
    let projects = fixture::projects();
    let state = collapsed();
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let view = deck_for(&projects, &state);

    // The open preview at the top of the column, then the two strips.
    assert_eq!(
        close_marks(&buffer),
        [
            Position::new(94, 2),
            Position::new(140, 2),
            Position::new(140, 37),
            Position::new(140, 39),
        ]
    );
    assert_eq!(view.close_at(SCREEN, Position::new(140, 37)), Some(2));
    assert_eq!(view.close_at(SCREEN, Position::new(140, 39)), Some(3));
    // The strip's own marker still owns its head, so the two gestures never
    // contend for a cell.
    assert_eq!(view.marker_at(SCREEN, Position::new(102, 37)), Some(2));
}

/// With nothing stacked (#76) the one pane still closes; zoom hides the
/// stack, so only the master it shows carries a mark.
#[test]
fn the_single_and_zoomed_layouts_close_the_pane_they_show() {
    let single = synthetic(1);
    let state = DeckState::new(1);
    let buffer = render_long(&single, &state, (144, 42));
    assert_eq!(close_marks(&buffer), [Position::new(140, 2)]);
    assert_eq!(
        deck_for(&single, &state).close_at(SCREEN, Position::new(140, 2)),
        Some(0)
    );

    let projects = fixture::projects();
    let state = zoomed();
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert_eq!(close_marks(&buffer), [Position::new(140, 2)]);
    assert_eq!(
        deck_for(&projects, &state).close_at(SCREEN, Position::new(140, 2)),
        Some(0)
    );
}

/// The narrow fallback draws no mark and answers for none, exactly as the
/// status bar's `+` is absent there: its one pane is the session.
#[test]
fn the_narrow_fallback_offers_no_close_affordance() {
    let projects = fixture::projects();
    let state = reference_deck(4);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (84, 22));

    assert_eq!(close_marks(&buffer), []);
    let narrow = Rect {
        width: 84,
        height: 22,
        ..SCREEN
    };
    let view = deck_for(&projects, &state);
    for row in 0..22 {
        for column in 0..84 {
            assert_eq!(view.close_at(narrow, Position::new(column, row)), None);
        }
    }
}

/// The two cells the export draws the disclosure marker in, on an open
/// preview's top border and at the head of each strip.
#[test]
fn the_disclosure_marker_is_hit_tested_in_its_own_two_cells() {
    let projects = fixture::projects();
    let state = collapsed();
    let deck = deck_for(&projects, &state);
    let area = Rect::new(0, 0, 144, 42);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // An open preview insets its title past the border: columns 103-104.
    assert_eq!(buffer[(103u16, 2u16)].symbol(), "▾");
    assert_eq!(deck.marker_at(area, Position::new(103, 2)), Some(1));
    assert_eq!(deck.marker_at(area, Position::new(104, 2)), Some(1));
    // A strip has no border to inset past: columns 102-103.
    assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
    assert_eq!(deck.marker_at(area, Position::new(102, 37)), Some(2));
    assert_eq!(deck.marker_at(area, Position::new(103, 39)), Some(3));

    // Neither the border cell beside it nor the pane body is the marker.
    assert_eq!(deck.marker_at(area, Position::new(102, 2)), None);
    assert_eq!(deck.marker_at(area, Position::new(110, 7)), None);
    // The master carries no marker of its own.
    assert_eq!(deck.marker_at(area, Position::new(3, 2)), None);
}

/// The marker cells sit inside the pane, so the gesture has to be consumed
/// or the same click would also drag or promote.
#[test]
fn the_marker_overlaps_the_pane_it_belongs_to() {
    let projects = fixture::projects();
    let state = collapsed();
    let deck = deck_for(&projects, &state);
    let area = Rect::new(0, 0, 144, 42);

    assert_eq!(deck.marker_at(area, Position::new(103, 2)), Some(1));
    assert_eq!(deck.position_at(area, Position::new(103, 2)), Some(1));
}

/// Issues #32 and #39: the marker a fresh run draws is the marker a fresh
/// run can click. Since #39 that is three strips, each of whose two cells
/// expands its preview from frame one.
#[test]
fn the_markers_are_clickable_before_anything_is_expanded() {
    let projects = fixture::projects();
    let mut state = reference_deck(4);
    let area = Rect::new(0, 0, 144, 42);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // Three strips at the head of the column, each with its own marker.
    for (position, row) in [(1usize, 2u16), (2, 4), (3, 6)] {
        assert_eq!(buffer[(102u16, row)].symbol(), "▸", "row {row}");
        assert_eq!(
            deck_for(&projects, &state).marker_at(area, Position::new(102, row)),
            Some(position)
        );
        assert_eq!(
            deck_for(&projects, &state).marker_at(area, Position::new(103, row)),
            Some(position)
        );
        // The cell beside the marker's two is not the marker.
        assert_eq!(
            deck_for(&projects, &state).marker_at(area, Position::new(104, row)),
            None
        );
    }

    // Clicking one expands exactly that preview, from a stack with no
    // preview open.
    state.toggle_collapse(2);
    assert_eq!(state.collapsed_count(), 2);
    let (opened, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert!(text(&opened).contains("▾ 3 app"), "{}", text(&opened));
    assert!(
        text(&opened).contains("▸ 2 backend"),
        "the rest stay folded"
    );
}

/// Clicking a strip's marker expands that preview, and the rows it takes
/// back come out of the pane that grew.
#[test]
fn toggling_a_marker_expands_just_that_preview() {
    let projects = fixture::projects();
    let mut state = collapsed();
    let area = Rect::new(0, 0, 144, 42);

    let marker = deck_for(&projects, &state).marker_at(area, Position::new(102, 37));
    assert_eq!(marker, Some(2));
    state.toggle_collapse(marker.unwrap());

    assert_eq!(state.collapsed_count(), 1, "worker stays folded");
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    // Two open previews now split the single fold's 11 freed rows 6/5.
    assert_eq!(buffer[(100u16, 19u16)].symbol(), "└");
    assert_eq!(buffer[(100u16, 21u16)].symbol(), "┌");
    assert_eq!(buffer[(102u16, 39u16)].symbol(), "▸");
}

/// Zoom and the narrow fallback hide the stack, so there is no marker.
#[test]
fn a_hidden_stack_offers_no_marker() {
    let projects = fixture::projects();
    let mut state = collapsed();
    state.apply(&ActionCommand::ToggleZoom, &projects, fixture::NOW);
    let area = Rect::new(0, 0, 144, 42);

    assert_eq!(
        deck_for(&projects, &state).marker_at(area, Position::new(103, 2)),
        None
    );

    let folded = collapsed();
    assert_eq!(
        deck_for(&projects, &folded).marker_at(Rect::new(0, 0, 84, 24), Position::new(3, 2)),
        None
    );
}

/// The split at a given ratio, as the column the divider is drawn in.
fn split(ratio: f64) -> DeckState {
    DeckState::new(4).with_master_ratio(ratio)
}

/// The column the divider's grip is drawn in, found by the grip rather
/// than by the line so a pane border is never mistaken for it. (The
/// scroll track's thumb is the same glyph, but the reference deck's list
/// always fits, so it never draws one.)
fn divider_column(buffer: &Buffer) -> Option<u16> {
    (0..buffer.area().width).find(|column| {
        let cell = &buffer[(*column, 22u16)];
        cell.symbol() == "┃" && matches!(cell.fg, HINT | ACCENT)
    })
}

/// Issue #41: the split has to be visible before it can be draggable. The
/// divider takes the gutter column beside the master, so it costs neither
/// pane a column, and it carries a grip at its middle.
#[test]
fn the_divider_is_drawn_in_the_gutter_with_a_grip_to_take_hold_of() {
    let (buffer, _) = reference();

    // The master's own border still ends at 97 and the stack's begins at
    // 100: the divider took the gutter, not a column of either pane.
    assert_eq!(buffer[(97u16, 2u16)].symbol(), "┐");
    assert_eq!(buffer[(100u16, 2u16)].symbol(), " ", "the folded stack");
    for row in [2u16, 12, 41] {
        assert_eq!(buffer[(98u16, row)].symbol(), "│", "row {row}");
        assert_eq!(buffer[(98u16, row)].fg, SEPARATOR);
    }
    // Three cells at the middle of the body say it can be taken hold of.
    for row in 21..=23u16 {
        assert_eq!(buffer[(98u16, row)].symbol(), "┃", "row {row}");
        assert_eq!(buffer[(98u16, row)].fg, HINT);
    }
    // It stops at the body: the blank row above it is not the divider.
    assert_ne!(buffer[(98u16, 1u16)].symbol(), "│");
}

/// The gutter belongs to no pane, so holding the divider can never be a
/// pane drag, a promotion or a marker click.
#[test]
fn the_divider_column_is_the_divider_and_nothing_else() {
    let projects = fixture::projects();
    let state = reference_deck(4);
    let view = deck_for(&projects, &state);

    assert!(view.divider_at(SCREEN, Position::new(98, 22)));
    assert!(view.divider_at(SCREEN, Position::new(98, 2)));
    assert_eq!(view.position_at(SCREEN, Position::new(98, 22)), None);
    assert_eq!(view.swap_position_at(SCREEN, Position::new(98, 22)), None);
    assert_eq!(view.marker_at(SCREEN, Position::new(98, 2)), None);
    // Its neighbours are not it: the master's last column and the scroll
    // track's column both answer for themselves.
    assert!(!view.divider_at(SCREEN, Position::new(97, 22)));
    assert!(!view.divider_at(SCREEN, Position::new(99, 22)));
    assert_eq!(view.position_at(SCREEN, Position::new(97, 22)), Some(0));
    // The wheel over the gutter still pages the list (#34b), because the
    // wheel and the drag are different gestures on the same chrome.
    assert!(view.stack_scroll_at(SCREEN, Position::new(98, 22)));
    // Below the body it is chrome, not the divider.
    assert!(!view.divider_at(SCREEN, Position::new(98, 0)));
}

/// Dragging leaves the divider under the pointer: the ratio a column maps
/// to is the ratio that draws the divider back in that column.
#[test]
fn dragging_the_divider_puts_the_split_under_the_pointer() {
    let projects = fixture::projects();
    let mut state = reference_deck(4);

    // Every column the range reaches, because the ratio a column maps to
    // is a division whose last bit must not move the split a column on.
    for column in 77..=120u16 {
        let ratio = deck_for(&projects, &state)
            .ratio_at(SCREEN, column)
            .expect("the split can move at this width");
        state.set_master_ratio(ratio);

        let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
        assert_eq!(divider_column(&buffer), Some(column), "dragged to {column}");
        // The master ends one column short of the divider, the stack one
        // column past the track: the panes follow the divider exactly.
        assert_eq!(buffer[(column - 1, 2u16)].symbol(), "┐");
    }
}

/// The range is the configuration's own, so a drag past either end stops
/// at the split the configuration would have accepted.
#[test]
fn a_drag_past_the_ends_of_the_range_stops_at_them() {
    let projects = fixture::projects();
    let mut state = reference_deck(4);

    let narrow = deck_for(&projects, &state).ratio_at(SCREEN, 20).unwrap();
    state.set_master_ratio(narrow);
    assert_eq!(state.master_ratio(), super::state::MIN_MASTER_RATIO);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert_eq!(divider_column(&buffer), Some(77), "0.55 of 144");

    let wide = deck_for(&projects, &state).ratio_at(SCREEN, 140).unwrap();
    state.set_master_ratio(wide);
    assert_eq!(state.master_ratio(), super::state::MAX_MASTER_RATIO);
    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert_eq!(divider_column(&buffer), Some(120), "0.85 of 144");
}

/// Keyboard parity (#41): `^g -` / `^g =` reach the same splits the
/// pointer does, and the screen cannot tell which one moved it.
#[test]
fn a_nudged_split_and_a_dragged_split_are_the_same_screen() {
    let projects = fixture::projects();
    let mut nudged = reference_deck(4);
    nudged.nudge_master_ratio(-1);
    assert_eq!(nudged.master_ratio(), 0.65);

    let mut dragged = reference_deck(4);
    let ratio = deck_for(&projects, &dragged).ratio_at(SCREEN, 91).unwrap();
    dragged.set_master_ratio(ratio);

    let (by_key, _) = render(&fixture::frontend_active(), &nudged, (144, 42));
    let (by_pointer, _) = render(&fixture::frontend_active(), &dragged, (144, 42));
    assert_eq!(divider_column(&by_key), Some(91));
    assert_eq!(text(&by_key), text(&by_pointer));
}

/// While the divider is held it takes the accent, the way a dragged pane
/// does: the gesture states itself for as long as it lasts.
#[test]
fn the_divider_takes_the_accent_while_it_is_held() {
    let mut state = reference_deck(4);
    state.set_resizing(true);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_eq!(buffer[(98u16, 2u16)].fg, ACCENT);
    assert_eq!(buffer[(98u16, 22u16)].fg, ACCENT);
    assert_eq!(buffer[(98u16, 22u16)].symbol(), "┃");
    // Releasing it hands the divider back to its resting colours.
    state.set_resizing(false);
    let (released, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert_eq!(released[(98u16, 2u16)].fg, SEPARATOR);
}

/// A split that cannot move has no divider to offer: zoom hides the
/// stack, the narrow fallback drops it, and below `WIDE_COLUMNS` the
/// export fixes the stack width outright.
#[test]
fn a_stack_that_cannot_be_resized_offers_no_divider() {
    let projects = fixture::projects();
    let zoom = zoomed();
    assert!(!deck_for(&projects, &zoom).divider_at(SCREEN, Position::new(98, 22)));

    let state = reference_deck(4);
    let narrow = Rect::new(0, 0, 84, 22);
    assert!(!deck_for(&projects, &state).divider_at(narrow, Position::new(50, 12)));

    // 110 columns still stacks, but at the export's fixed 34-column
    // stack: there is no ratio to move, so there is no divider.
    let fixed = Rect::new(0, 0, 110, 42);
    assert_eq!(deck_for(&projects, &state).ratio_at(fixed, 74), None);
    assert!(!deck_for(&projects, &state).divider_at(fixed, Position::new(74, 12)));
    let (buffer, _) = render(&fixture::frontend_active(), &state, (110, 42));
    assert_eq!(divider_column(&buffer), None, "no divider to mislead with");
}

#[test]
fn split_dragged_matches_the_divider_affordance() {
    let mut state = split(0.55);
    state.set_resizing(true);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_snapshot("split-dragged", &buffer);
}

/// Issue #44: a fresh run gives the stack its minimum width. The previews
/// start folded (#39), so a strip is all the column has to hold, and the
/// master takes everything the divider's range allows.
#[test]
fn a_fresh_run_gives_the_stack_its_minimum_width() {
    let state = DeckState::new(4);
    assert_eq!(state.master_ratio(), super::state::MAX_MASTER_RATIO);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // 120 columns of master, the 2-column gutter, a 22-column stack.
    assert_eq!(divider_column(&buffer), Some(120));
    assert_eq!(buffer[(119u16, 2u16)].symbol(), "┐", "the master's border");
    assert_eq!(buffer[(122u16, 2u16)].symbol(), " ", "the stack's padding");
    // Which is the narrow end of the divider's travel: `^g -` and a drag
    // both open the stack from here, and nothing widens it further.
    let mut wider = state.clone();
    assert!(!wider.nudge_master_ratio(1), "already at the top");
    assert!(wider.nudge_master_ratio(-1));
    let (opened, _) = render(&fixture::frontend_active(), &wider, (144, 42));
    assert_eq!(divider_column(&opened), Some(113));
}

/// The minimum width is only worth having if a strip still reads there:
/// the marker, the configured number, the name and the status dot all
/// survive, and it is the tail that goes.
#[test]
fn a_strip_still_names_itself_at_the_minimum_stack_width() {
    let (buffer, _) = render(&fixture::frontend_active(), &DeckState::new(4), (144, 42));
    let rendered = text(&buffer);

    // The shortest name keeps a clipped tail; the longer two spend the
    // column on their own names, which is the right order to give things
    // up in. A strip with no room left for a tail drops the separator
    // with it, rather than ending on one that separates nothing.
    assert!(rendered.contains("▸ 3 app · ✕"), "{rendered}");
    // The close affordance keeps its column whatever the tail does, and
    // the tail gives way to it: at 22 columns there is none left (#84).
    for (row, strip) in [(2u16, "▸ 2 backend · ●"), (6, "▸ 4 worker · ○")] {
        let drawn: String = (122..144).map(|c| buffer[(c, row)].symbol()).collect();
        assert_eq!(drawn.trim_end(), format!("  {strip:<16}×"), "row {row}");
    }
    // Every marker is still in its own two cells, so the affordance the
    // fresh run depends on (#39) survives the narrower column.
    let projects = fixture::projects();
    let state = DeckState::new(4);
    let view = deck_for(&projects, &state);
    assert_eq!(view.marker_at(SCREEN, Position::new(124, 2)), Some(1));
    assert_eq!(view.marker_at(SCREEN, Position::new(125, 6)), Some(3));
}

/// The updated canvas (screen 05) leaves the stack footer empty while
/// previews are folded: the census belongs to the status row alone, at the
/// minimum stack width (#44) as much as at the export's own.
#[test]
fn the_fold_census_no_longer_takes_the_stack_footer() {
    let (narrow, _) = render(&fixture::frontend_active(), &DeckState::new(4), (144, 42));
    let footer: String = (122..144).map(|c| narrow[(c, 41u16)].symbol()).collect();
    assert_eq!(footer.trim(), "", "{footer}");

    let (wide, _) = render(&fixture::frontend_active(), &reference_deck(4), (144, 42));
    let footer: String = (100..144).map(|c| wide[(c, 41u16)].symbol()).collect();
    assert_eq!(footer.trim(), "", "{footer}");
    // The count itself is not lost: the status row still carries it.
    assert!(text(&wide).contains("3 collapsed"), "{}", text(&wide));
}

#[test]
fn fresh_start_matches_the_minimum_stack_width() {
    let (buffer, _) = render(&fixture::frontend_active(), &DeckState::new(4), (144, 42));

    assert_snapshot("fresh-start", &buffer);
}

#[test]
fn frontend_active_matches_the_reference_canvas() {
    let (buffer, _) = reference();

    assert_snapshot("frontend-active", &buffer);
}

/// #101: the canvas opens with the status bar. Every session screen in the
/// design puts the workspace chip, the census and the keys on row 1, keeps
/// row 2 blank, and starts the panes on row 3 — the pre-move grid the
/// export's specification panel still describes (`status row 42`) is what
/// the screens replaced.
#[test]
fn the_status_row_opens_the_canvas_two_rows_above_the_panes() {
    let (buffer, _) = reference();

    // Row 1 is the bar: it owns the second background value for its whole
    // width, and it opens with the workspace chip.
    let bar: String = (0..144u16).map(|x| buffer[(x, 0u16)].symbol()).collect();
    assert!(bar.trim_start().starts_with("idp "), "{bar}");
    assert!(bar.trim_end().ends_with("^g q quit"), "{bar}");
    // The chip is the one thing on the row that takes the accent instead.
    for column in 0..144u16 {
        let bg = buffer[(column, 0u16)].bg;
        assert!(bg == STATUS_BG || bg == ACCENT, "column {column}: {bg:?}");
    }
    assert_eq!(buffer[(143u16, 0u16)].bg, STATUS_BG);

    // Row 2 separates the bar from the panes, and carries neither.
    let blank: String = (0..144u16).map(|x| buffer[(x, 1u16)].symbol()).collect();
    assert_eq!(blank.trim(), "", "{blank}");
    assert_eq!(buffer[(0u16, 1u16)].bg, super::CANVAS);

    // The master takes every row from there to the foot of the canvas.
    assert_eq!(buffer[(0u16, 2u16)].symbol(), "┌");
    assert_eq!(buffer[(0u16, 41u16)].symbol(), "└");

    // The `+` the pointer opens the add sheet with moved up with the row.
    let projects = fixture::projects();
    let state = reference_deck(4);
    let view = deck_for(&projects, &state);
    let plus = (0..144u16)
        .find(|column| buffer[(*column, 0u16)].symbol() == "+")
        .expect("the add affordance");
    assert!(view.add_at(SCREEN, Position::new(plus, 0)));
    assert!(!view.add_at(SCREEN, Position::new(plus, 41)));
}

#[test]
fn reference_chrome_carries_the_accepted_palette() {
    let (buffer, cursor) = reference();

    // Master border is the accent.
    assert_eq!(buffer[(0u16, 2u16)].fg, ACCENT);
    // The status row owns the second background value.
    assert_eq!(buffer[(0u16, 0u16)].bg, STATUS_BG);
    // The master cursor sits after the last line of engine-owned output.
    assert_eq!(cursor, Some(Position::new(3, 31)));

    // The preview chrome the export states is a click away since #39, so
    // it is read from an opened stack rather than from the fresh run.
    let (open, _) = render(&fixture::frontend_active(), &expanded(4), (144, 42));
    assert_ne!(open[(100u16, 0u16)].fg, ACCENT, "no preview takes focus");
    // The exited preview's footer rule carries the error colour.
    assert_eq!(open[(103u16, 24u16)].fg, ERROR);
}

#[test]
fn engine_cell_styles_reach_the_buffer() {
    let (buffer, _) = reference();

    let vite = &buffer[(5u16, 8u16)];
    assert_eq!(vite.symbol(), "V");
    assert_eq!(
        vite.fg,
        super::colour(crate::contracts::Rgb {
            red: 0xc8,
            green: 0x98,
            blue: 0xe0,
        })
    );
    assert!(vite.modifier.contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn backend_promoted_matches_the_reference_canvas() {
    let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

    assert_snapshot("backend-promoted", &buffer);
}

#[test]
fn promotion_keeps_numbers_and_puts_the_old_master_in_the_vacated_slot() {
    let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));
    let screen = text(&buffer);

    // Backend keeps its configured number 2 while holding the master.
    assert!(screen.contains("> 2 backend"), "{screen}");
    // Frontend keeps number 1 and lands in the slot backend vacated.
    // Every pane number, in draw order. The disclosure marker is what
    // tells a stacked preview from the caret-marked master; since #39 the
    // demoted master is the open one and the untouched previews are
    // strips, so both markers are collected.
    let mut stack: Vec<_> = ["▾ ", "▸ "]
        .iter()
        .flat_map(|marker| screen.match_indices(marker))
        .filter_map(|(at, marker)| {
            screen[at + marker.len()..]
                .split(' ')
                .next()
                .map(|n| (at, n))
        })
        .collect();
    stack.sort_unstable();
    let numbers: Vec<_> = stack.iter().map(|(_, number)| *number).collect();
    assert_eq!(numbers, ["1", "3", "4"], "{screen}");
    assert!(
        screen.contains("┌─ ▾ 1 frontend"),
        "the demoted master is open"
    );
    assert!(screen.contains("▸ 3 app"), "{screen}");
    // The swap is reported by the demoted pane's own highlight, not by a
    // footer line: the declutter pass took that line out.
    assert!(!screen.contains("promoted backend"), "{screen}");
}

#[test]
fn the_demoted_pane_holds_its_highlight() {
    let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));

    // The top preview is the pane frontend was demoted into.
    assert_eq!(buffer[(100u16, 2u16)].fg, DEMOTED_BORDER);
    assert_eq!(buffer[(103u16, 3u16)].bg, DEMOTED_BG);
    // The untouched previews keep the ordinary chrome. They are strips
    // since #39, so the row to read is the first of them.
    // (A strip's own background is the same #101317 the demotion tint
    // uses, so what separates them here is the border and the marker.)
    assert_eq!(buffer[(102u16, 37u16)].symbol(), "▸");
    assert_ne!(buffer[(102u16, 37u16)].fg, DEMOTED_BORDER);
    assert_eq!(
        buffer[(100u16, 37u16)].symbol(),
        " ",
        "a strip has no border"
    );
}

#[test]
fn zoomed_matches_the_reference_canvas() {
    let (buffer, cursor) = render(&fixture::frontend_active(), &zoomed(), (144, 42));

    assert_snapshot("zoomed", &buffer);
    assert_eq!(cursor, Some(Position::new(3, 31)));
}

#[test]
fn zoom_gives_the_master_the_full_width_and_hides_the_stack() {
    let (buffer, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
    let screen = text(&buffer);

    // One pane, spanning every column of the body.
    assert_eq!(screen.matches('┌').count(), 1, "{screen}");
    assert_eq!(buffer[(143u16, 2u16)].symbol(), "┐");
    assert!(screen.contains(" ZOOM "), "{screen}");
    // The hidden terminals stay accounted for in the status row.
    assert!(screen.contains("hidden: 2● 3✕ 4○"), "{screen}");
    // The affordance took the room the labels had at this width; the
    // key itself is what the row promises.
    assert!(screen.contains("^g z"), "{screen}");
}

/// The spec's zoom + collapse rule: zoom hides the stack, so the folds
/// it hides say nothing in the status row. Since #39 that is every run.
#[test]
fn a_zoomed_deck_states_the_keys_its_hidden_folds_do_not_take() {
    let (buffer, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
    let screen = text(&buffer);

    assert_eq!(zoomed().collapsed_count(), 3, "the folds are still held");
    // The trimmed key set buys back the room the `+ add` affordance
    // (#50) took, so the labels are back at the reference width — and
    // they are the unfolded set, which is what this is about.
    assert!(
        screen.contains("^g j/k switch  ^g z unzoom  ^g ? help  ^g q quit"),
        "{screen}"
    );
    assert!(!screen.contains("^g c"), "an inert key is not advertised");
    assert!(!screen.contains("collapsed"), "{screen}");
}

#[test]
fn unzooming_restores_the_stack() {
    let mut state = zoomed();
    state.apply(
        &ActionCommand::ToggleZoom,
        &fixture::projects(),
        fixture::NOW,
    );

    let (zoomed, _) = render(&fixture::frontend_active(), &zoomed(), (144, 42));
    let (restored, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_ne!(text(&zoomed), text(&restored));
    assert_eq!(text(&restored), text(&reference().0));
}

#[test]
fn a_deck_with_nothing_exited_reports_all_running() {
    let mut engine = fixture::frontend_active();
    engine.set_status(&TerminalId::new("app"), TerminalStatus::Running);

    // The fold census replaces this one outright, so the stack is opened.
    let (buffer, _) = render(&engine, &expanded(4), (144, 42));

    let screen = text(&buffer);
    assert!(screen.contains("  ·  all running"), "{screen}");
    assert!(!screen.contains("stacked"), "the stack census is gone");
}

fn scrolling() -> DeckState {
    let mut state = reference_deck(4);
    state.apply(
        &ActionCommand::ToggleScrollback,
        &fixture::projects(),
        fixture::NOW,
    );
    state
}

#[test]
fn scrollback_matches_the_supplement_canvas() {
    let (buffer, cursor) = render(&fixture::scrolled(), &scrolling(), (144, 42));

    assert_snapshot("scrollback", &buffer);
    // The viewport is detached from the live tail, so no cursor is drawn:
    // the backend keeps the origin it started at.
    assert_eq!(cursor, Some(Position::new(0, 0)));
}

#[test]
fn scrollback_is_a_mode_of_the_master_pane_only() {
    let (buffer, _) = render(&fixture::scrolled(), &scrolling(), (144, 42));
    let screen = text(&buffer);

    // The stack stays visible and live: only zoom hides it. Since #39 it
    // is visible as the three strips a fresh run draws.
    assert_eq!(screen.matches('┌').count(), 1, "{screen}");
    assert_eq!(screen.matches('▸').count(), 3, "{screen}");
    assert!(screen.contains("▸ 2 backend"), "{screen}");
    assert!(screen.contains(" SCROLL "), "{screen}");
    assert!(
        screen.contains("j/k ↑↓ line · pgup/pgdn page · g/G ends · esc live"),
        "{screen}"
    );
    assert!(
        screen.contains("scrollback · esc returns to live"),
        "{screen}"
    );
    // Stated once: the mode tag in the title, the keys in the footer, and
    // the absolute position in the status row.
    assert!(screen.contains("SCROLLBACK  ·  line 2217/2431"), "{screen}");
    assert_eq!(screen.matches("2217/2431").count(), 1, "{screen}");
    // The mode tag is warning, not accent, so it does not read as focus.
    assert_eq!(buffer[(88u16, 2u16)].bg, WARNING);
}

#[test]
fn leaving_scrollback_returns_to_live_output() {
    let mut state = scrolling();
    state.apply(
        &ActionCommand::ToggleScrollback,
        &fixture::projects(),
        fixture::NOW,
    );

    let (live, cursor) = render(&fixture::scrolled(), &state, (144, 42));

    assert!(!text(&live).contains(" SCROLL "));
    assert_eq!(cursor, Some(Position::new(3, 31)));
}

/// The other half of the declutter pass: dropping `running` did not drop
/// `exit 1`. A live state is inferable from the glyph; an exit code is not.
#[test]
fn an_exited_preview_still_names_its_code_in_its_title() {
    let screen = text(&render(&fixture::frontend_active(), &expanded(4), (144, 42)).0);

    assert!(screen.contains("▾ 3 app · ✕ exit 1"), "{screen}");
    // A pane that is merely alive says so with the glyph and stops: the
    // border resumes right after it, with no state word in between.
    // (`running` is not searched for here — the backend's own output says
    // "Server running on", which is the terminal's text, not the chrome.)
    assert!(screen.contains("▾ 2 backend · ● ─"), "{screen}");
}

#[test]
fn a_starting_terminal_renders_the_warning_ring_alone() {
    let mut engine = fixture::frontend_active();
    engine.set_status(&TerminalId::new("frontend"), TerminalStatus::Starting);

    let (buffer, _) = render(&engine, &reference_deck(4), (144, 42));
    let screen = text(&buffer);

    // The glyph carries the state; the declutter pass dropped the word
    // that repeated it, and the path it used to sit behind.
    assert!(
        screen.contains("> 1 frontend  ·  ○  ·  pnpm dev"),
        "{screen}"
    );
    assert!(!screen.contains("starting"), "the glyph says it alone");
    // The ring is warning, not the accent a running master takes.
    let ring = (0..144u16)
        .find(|column| buffer[(*column, 2u16)].symbol() == "○")
        .expect("the master title carries the starting ring");
    assert_eq!(buffer[(ring, 2u16)].fg, WARNING);

    // A preview shows the ring alone: the border resumes right after it.
    let mut engine = fixture::frontend_active();
    engine.set_status(&TerminalId::new("backend"), TerminalStatus::Starting);
    // An open preview's title, so the stack is opened for it: a strip
    // states its own status in the collapsed-strip tests.
    let screen = text(&render(&engine, &expanded(4), (144, 42)).0);

    assert!(screen.contains("▾ 2 backend · ○ ─"), "{screen}");
}

#[test]
fn a_preview_holding_history_says_how_far_back_it_is() {
    let mut engine = fixture::backend_promoted();
    engine.set_metadata(
        &TerminalId::new("frontend"),
        TerminalMetadata {
            scrollback: ScrollbackPosition {
                lines_above: 214,
                lines_below: 1,
            },
            ..TerminalMetadata::default()
        },
    );
    let (buffer, _) = render(&engine, &promote(1), (144, 42));

    assert!(
        text(&buffer).contains("↑ 214 lines above · ^g ["),
        "{}",
        text(&buffer)
    );
}

#[test]
fn a_live_preview_keeps_its_last_terminal_row() {
    let (buffer, _) = render(&fixture::backend_promoted(), &promote(1), (144, 42));
    let screen = text(&buffer);

    assert!(!screen.contains("↑ 214 lines above · ^g ["), "{screen}");
    assert!(
        screen.contains("➜  press h + enter to show help"),
        "{screen}"
    );
}

#[test]
fn the_status_bar_drops_labels_before_keys_then_collapses() {
    // The unfolded hint set, which is the one this ladder is written for:
    // a folded stack swaps `^g [` for `^g c` (§3.5 of the collapse spec).
    let state = expanded(4);

    // The ladder is what this tests, not where each rung falls: labels
    // first, then keys, then — once the layout goes narrow — the
    // collapsed form, which is the only rung that still names every key.
    let labelled = render(&fixture::frontend_active(), &state, (144, 42)).0;
    let keys_only = render(&fixture::frontend_active(), &state, (100, 42)).0;
    let collapsed = render(&fixture::frontend_active(), &state, (84, 42)).0;

    assert!(text(&labelled).contains("^g j/k switch  ^g z zoom"));
    let keys = text(&keys_only);
    assert!(keys.contains("^g j/k  ^g z  ^g ?  ^g q"), "{keys}");
    assert!(!keys.contains("switch"), "{keys}");
    assert!(
        text(&collapsed).contains("^g j/k · N · z · [ · ? · q"),
        "{}",
        text(&collapsed)
    );
}

#[test]
fn a_rejected_terminal_number_is_visible_in_the_status_row() {
    let mut state = expanded(4);
    state.set_notice("terminal 19 unavailable".to_owned());

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert!(
        text(&buffer).contains("terminal 19 unavailable"),
        "{}",
        text(&buffer)
    );
}

/// A master title whose name eats the budget drops the command and its
/// separator together. Leaving the separator behind printed `·` with
/// nothing after it, which states nothing.
#[test]
fn a_title_with_no_room_for_the_command_drops_its_separator_too() {
    for len in 30..=45usize {
        let projects = vec![Project {
            terminal: TerminalId::new("a".repeat(len)),
            path: std::path::PathBuf::from("/tmp/x"),
            command: vec!["pnpm".into(), "dev".into()],
            shell_hook: false,
        }];
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|frame| {
            Deck {
                workspace: "w",
                projects: &projects,
                state: &DeckState::new(1),
                notifies: quiet(),
                master_ratio: super::DEFAULT_MASTER_RATIO,
                now: fixture::NOW,
            }
            .render(&fixture::frontend_active(), frame);
        })
        .unwrap();
        let title: String = (0..100u16)
            .map(|x| term.backend().buffer()[(x, 2u16)].symbol())
            .collect();
        // Names are elastic before structural title chrome: the glyph
        // and its separator remain whole rather than being cut mid-row.
        let head = title.split('[').next().unwrap_or("").trim_end();
        assert!(head.contains('○'), "name of {len} cuts the glyph: {head}");
        assert!(
            !head.ends_with('·'),
            "name of {len} leaves a dangling separator: {head}"
        );
    }
}

fn opened(action: ActionCommand) -> DeckState {
    let mut state = reference_deck(4);
    state.apply(&action, &fixture::projects(), fixture::NOW);
    state
}

#[test]
fn help_matches_the_supplement_canvas() {
    let state = opened(ActionCommand::ShowHelp);

    let (buffer, cursor) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_snapshot("help", &buffer);
    // The modal holds focus, so the master draws no cursor.
    assert_eq!(cursor, Some(Position::new(0, 0)));
}

/// Issue #32: the pointer has the marker, and the keyboard has the help
/// overlay. `^g c` is named there under VIEW, beside the zoom it sits with.
#[test]
fn the_help_overlay_names_the_collapse_key() {
    let state = opened(ActionCommand::ShowHelp);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let rendered = text(&buffer);

    assert!(
        rendered.contains("^g c             collapse / expand previews"),
        "{rendered}"
    );
    let view = rendered.find("VIEW").expect("the VIEW section");
    let zoom = rendered.find("^g z  ").expect("the zoom binding");
    let collapse = rendered.find("^g c  ").expect("the collapse binding");
    assert!(view < zoom && zoom < collapse, "collapse follows zoom");
}

#[test]
fn the_help_overlay_takes_the_focus_the_master_gives_up() {
    let state = opened(ActionCommand::ShowHelp);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    // 60x27 centred on the canvas: columns 42..101, rows 7..33. The
    // overlay grew a row for the split divider's keys (#41), another for
    // the runtime-add sheet (#50), another for `^g x` (#84) and another
    // for the pin (#113).
    assert_eq!(buffer[(42u16, 7u16)].symbol(), "┌");
    assert_eq!(buffer[(101u16, 33u16)].symbol(), "┘");
    assert_eq!(buffer[(42u16, 7u16)].fg, ACCENT);
    // Focus is singular: the master border is no longer the accent, and
    // the underlay recedes by foreground alone.
    assert_eq!(buffer[(0u16, 2u16)].fg, IDLE_BORDER);
    assert_eq!(buffer[(5u16, 3u16)].fg, UNDER_FG);
    assert_eq!(buffer[(5u16, 3u16)].bg, super::CANVAS);
    // The stack hint row sits between the panes and dims one step further.
    assert_eq!(buffer[(103u16, 41u16)].fg, UNDER_HINT);
    // The status row keeps its colours and states the modal's keys.
    assert_eq!(buffer[(2u16, 0u16)].bg, ACCENT);
    assert!(
        text(&buffer).contains("^g ? help open  ·  esc close"),
        "{}",
        text(&buffer)
    );
}

#[test]
fn quit_matches_the_supplement_canvas() {
    let state = opened(ActionCommand::RequestQuit);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_snapshot("quit", &buffer);
}

#[test]
fn the_quit_confirmation_counts_terminals_and_warns_once() {
    let state = opened(ActionCommand::RequestQuit);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let screen = text(&buffer);

    // 52x10 centred on the canvas: columns 46..97, rows 16..25.
    assert_eq!(buffer[(46u16, 16u16)].symbol(), "┌");
    assert_eq!(buffer[(97u16, 25u16)].symbol(), "┘");
    // The count, not the names.
    assert!(screen.contains("4 terminals will be closed."), "{screen}");
    assert!(!screen.contains("frontend, backend"), "{screen}");
    // A destructive modal keeps the accent border and carries the warning
    // colour on its consequence line alone.
    assert_eq!(buffer[(46u16, 16u16)].fg, ACCENT);
    // The warning colour is confined to that one line: the count above it
    // stays an ordinary key colour.
    assert_eq!(buffer[(49u16, 21u16)].fg, WARNING);
    assert_eq!(buffer[(49u16, 20u16)].fg, super::PREVIEW_FG);
    assert!(screen.contains("y  quit         n  cancel         esc  cancel"));
    assert!(
        screen.contains("confirm quit  ·  y quit  n cancel"),
        "{screen}"
    );
}

#[test]
fn a_modal_does_not_disturb_the_interface_it_recedes() {
    let mut state = opened(ActionCommand::ShowHelp);
    assert!(state.close_modal());

    let (restored, _) = render(&fixture::frontend_active(), &state, (144, 42));

    assert_eq!(text(&restored), text(&reference().0));
}

#[test]
fn a_modal_over_a_narrow_deck_still_fits_inside_the_canvas() {
    let state = opened(ActionCommand::RequestQuit);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (84, 22));

    // 52 columns fit in 84; the box is centred and the status row is clear.
    assert_eq!(buffer[(16u16, 6u16)].symbol(), "┌");
    assert!(text(&buffer).contains("confirm quit"));
}

#[test]
fn a_canvas_smaller_than_the_overlay_clips_it_to_the_canvas() {
    let state = opened(ActionCommand::ShowHelp);

    let (buffer, _) = render(&fixture::frontend_active(), &state, (20, 8));

    // Clipped to the canvas rather than drawn past its edge.
    assert_eq!(buffer[(0u16, 0u16)].symbol(), "┌");
    assert_eq!(buffer[(19u16, 7u16)].symbol(), "┘");
}

#[test]
fn narrow_matches_the_reference_canvas() {
    let (buffer, _) = render(&fixture::frontend_active(), &reference_deck(4), (84, 22));

    assert_snapshot("narrow", &buffer);
}

#[test]
fn below_a_usable_preview_width_the_stack_becomes_a_pane_strip() {
    let (buffer, _) = render(&fixture::frontend_active(), &reference_deck(4), (84, 22));
    let screen = text(&buffer);

    assert_eq!(screen.matches('┌').count(), 1, "{screen}");
    assert!(
        screen.lines().nth(2).is_some_and(
            |strip| strip.starts_with("  1 frontend  2 backend · 3 app ✕1 · 4 worker ○")
        ),
        "{screen}"
    );
    assert!(screen.contains("stack hidden"), "{screen}");
    assert!(screen.contains("^g j/k · N · z · [ · ? · q"), "{screen}");
    // The active chip and the warning both carry their accepted colours.
    assert_eq!(buffer[(1u16, 2u16)].bg, ACCENT);
    assert_eq!(buffer[(14u16, 2u16)].bg, CHIP_BG);
    assert_eq!(buffer[(19u16, 0u16)].fg, WARNING);
}

#[test]
fn one_column_above_the_fallback_still_stacks() {
    // Counted in preview boxes, so the stack is opened; the threshold
    // itself never reads the folds.
    let state = expanded(4);

    let (narrow, _) = render(&fixture::frontend_active(), &state, (99, 30));
    let (stacked, _) = render(&fixture::frontend_active(), &state, (100, 30));

    assert_eq!(text(&narrow).matches('┌').count(), 1);
    // 34 stack columns hold previews, which the export fixes below 120.
    assert!(text(&stacked).matches('┌').count() > 1);
    assert_eq!(stacked[(63u16, 2u16)].symbol(), "┐");
}

/// With zero stacked previews the stack column, its gutter and its
/// divider all go: the master takes the full body on the reference
/// canvas. The status row still reads as an ordinary deck — census,
/// `+ add`, full keys — never as a zoom.
#[test]
fn an_empty_stack_gives_the_master_the_full_width() {
    let projects = synthetic(1);
    let state = DeckState::new(1);
    let buffer = render_long(&projects, &state, (144, 42));
    let screen = text(&buffer);

    // The master's border runs the full canvas width.
    assert_eq!(buffer[(0u16, 2u16)].symbol(), "┌");
    assert_eq!(buffer[(143u16, 2u16)].symbol(), "┐");
    assert_eq!(buffer[(143u16, 41u16)].symbol(), "┘");
    assert_eq!(divider_column(&buffer), None, "no divider without a stack");
    assert!(!screen.contains('▸'), "no folded strips, {screen}");
    assert!(!screen.contains('▾'), "no disclosure markers, {screen}");
    assert!(screen.contains("1 terminal"), "{screen}");
    assert!(screen.contains("all running"), "{screen}");
    assert!(
        !screen.contains("hidden:"),
        "that census belongs to zoom, {screen}"
    );
    assert!(!screen.contains("ZOOM"), "{screen}");

    // And there is nothing of the stack left to hit.
    let deck = deck_for(&projects, &state);
    assert_eq!(deck.position_at(SCREEN, Position::new(10, 12)), Some(0));
    assert_eq!(deck.position_at(SCREEN, Position::new(130, 12)), Some(0));
    assert!(!deck.divider_at(SCREEN, Position::new(100, 22)));
    assert_eq!(deck.ratio_at(SCREEN, 100), None);
    assert_eq!(deck.marker_at(SCREEN, Position::new(100, 2)), None);
    assert_eq!(deck.stack_window(SCREEN), super::StackWindow::default());
    assert!(!deck.stack_scroll_at(SCREEN, Position::new(100, 22)));
    assert_eq!(deck.swap_position_at(SCREEN, Position::new(130, 12)), None);

    // The visible PTY size is the full body, not a split share of it.
    let sizes = deck.terminal_sizes(SCREEN);
    assert_eq!(sizes.len(), 1);
    let size = sizes[0].expect("the master keeps a viewport");
    assert_eq!(size.columns, 144 - 2 - 2 * 2);
    assert_eq!(size.rows, 40 - 2);
}

/// Adding a terminal at runtime brings the stack — and its divider —
/// back: the layout follows the live stack length, so nothing else has
/// to restore the split.
#[test]
fn runtime_add_brings_the_stack_and_its_divider_back() {
    let projects = synthetic(2);
    let mut state = DeckState::new(1);
    assert_eq!(state.push_terminal(), 1);

    let buffer = render_long(&projects, &state, (144, 42));

    // A fresh deck starts at the top of the range: a 22-column stack and
    // the divider in column 120. The new preview starts folded, so it
    // answers on its title row.
    assert_eq!(divider_column(&buffer), Some(120));
    assert_eq!(buffer[(119u16, 2u16)].symbol(), "┐");
    let deck = deck_for(&projects, &state);
    assert_eq!(deck.position_at(SCREEN, Position::new(10, 12)), Some(0));
    assert_eq!(deck.position_at(SCREEN, Position::new(130, 2)), Some(1));
    assert!(deck.divider_at(SCREEN, Position::new(120, 22)));
    assert!(deck.ratio_at(SCREEN, 120).is_some());
}

/// Zooming an empty deck changes nothing visible: the master is already
/// full, and unzooming cannot summon a stack that is not there.
#[test]
fn zoom_and_an_empty_stack_agree_on_the_master() {
    let projects = synthetic(1);
    let mut zoomed = DeckState::new(1);
    zoomed.apply(&ActionCommand::ToggleZoom, &projects, fixture::NOW);

    let plain = render_long(&projects, &DeckState::new(1), (144, 42));
    let zoomed_buffer = render_long(&projects, &zoomed, (144, 42));
    let screen = text(&zoomed_buffer);

    assert_eq!(
        screen,
        text(&plain),
        "the same master-full canvas either way"
    );
    assert!(!screen.contains("hidden:"), "{screen}");
    assert!(!screen.contains("ZOOM"), "{screen}");
    assert_eq!(divider_column(&zoomed_buffer), None);
}

/// A zoomed deck of `count` synthetic terminals.
fn zoomed_long(count: usize) -> DeckState {
    let mut state = reference_deck(count);
    state.apply(&ActionCommand::ToggleZoom, &synthetic(count), fixture::NOW);
    state
}

/// A synthetic workspace of `count` terminals.
///
/// The configuration still caps a workspace at four until #34a lands, so a
/// stack longer than the reference deck is built here rather than loaded
/// from a workspace file. The renderer has never known the cap.
fn synthetic(count: usize) -> Vec<Project> {
    (1..=count)
        .map(|number| Project {
            terminal: TerminalId::new(format!("t{number}")),
            path: std::path::PathBuf::from(fixture::HOME).join(format!("idp/t{number}")),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        })
        .collect()
}

/// Renders a synthetic deck of any length on the reference canvas.
fn render_long(projects: &[Project], state: &DeckState, size: (u16, u16)) -> Buffer {
    let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let deck = deck_for(projects, state);
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| deck.render(&engine as &dyn TerminalEngine, frame))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The reference canvas, as a hit-testing area.
const SCREEN: Rect = Rect {
    x: 0,
    y: 0,
    width: 144,
    height: 42,
};

/// Eight terminals leave seven previews for a column that holds three: the
/// window states where it starts and how much of the list it is showing.
#[test]
fn a_long_list_fills_the_column_with_whole_previews() {
    let projects = synthetic(8);
    // Whole previews, so the window arithmetic is the open-preview one;
    // `folding_inside_a_long_list_lets_more_previews_into_the_window`
    // covers what folds do to it.
    let state = expanded(8);

    let window = deck_for(&projects, &state).stack_window(SCREEN);

    assert_eq!(window.offset, 0);
    assert_eq!(window.visible, 3, "39 budget rows hold 3 x (12 + 1)");
    assert_eq!(window.total, 7);
    assert!(window.overflows());
}

/// The four previews that fit the reference deck are the whole list, so
/// nothing scrolls and screens 01-04 are untouched.
#[test]
fn a_list_that_fits_does_not_scroll() {
    let projects = fixture::projects();
    // Four open previews are the tightest list that still fits.
    let mut state = expanded(4);

    let window = deck_for(&projects, &state).stack_window(SCREEN);
    assert!(!window.overflows());
    assert_eq!(window.scrolled(1), 0, "there is nothing below to reach");

    // A stored offset it cannot honour is still ignored by the renderer.
    state.set_stack_offset(2);
    let buffer = render_long(&projects, &state, (144, 42));
    assert_eq!(
        buffer[(99u16, 7u16)].symbol(),
        " ",
        "no track in the gutter"
    );
    assert!(!text(&buffer).contains("more"));
}

/// The window stops when its last preview is the list's last preview, so
/// the column never scrolls into empty space.
#[test]
fn paging_stops_with_the_last_preview_in_view() {
    let projects = synthetic(8);
    let mut state = expanded(8);
    let window = deck_for(&projects, &state).stack_window(SCREEN);

    assert_eq!(window.paged(1), 3, "one page is one window of previews");
    assert_eq!(window.scrolled(9), 4, "7 previews less the 3 on screen");
    assert_eq!(window.scrolled(-1), 0);

    state.set_stack_offset(window.paged(1));
    let scrolled = deck_for(&projects, &state).stack_window(SCREEN);
    assert_eq!(scrolled.offset, 3);
    assert_eq!(scrolled.paged(-1), 0);
    assert_eq!(scrolled.paged(1), 4);
}

/// Promotion, drag and the disclosure markers all address a configured
/// position, so they must read the window rather than the list.
#[test]
fn hit_testing_follows_the_scrolled_window() {
    let projects = synthetic(8);
    let mut state = expanded(8);
    state.toggle_collapse(6);
    state.set_stack_offset(2);
    let view = deck_for(&projects, &state);

    // The stack is [1..=7]; the window starts at its third preview.
    assert_eq!(view.position_at(SCREEN, Position::new(110, 7)), Some(3));
    assert_eq!(view.position_at(SCREEN, Position::new(110, 20)), Some(4));
    assert_eq!(view.position_at(SCREEN, Position::new(110, 33)), Some(5));
    assert_eq!(
        view.swap_position_at(SCREEN, Position::new(110, 7)),
        Some(3)
    );
    assert_eq!(
        view.terminal_at(SCREEN, Position::new(110, 7))
            .map(ToString::to_string)
            .as_deref(),
        Some("t4")
    );
    // The marker cells belong to whichever preview the window put there.
    assert_eq!(view.marker_at(SCREEN, Position::new(103, 2)), Some(3));
    assert_eq!(view.marker_at(SCREEN, Position::new(103, 15)), Some(4));
}

/// Issue #39: the fresh run is a column of strips, and every pointer
/// gesture still resolves on it — a strip's body promotes, drags and
/// names its terminal, its two marker cells expand it, and the empty
/// column below the strips is still the list's own chrome.
#[test]
fn a_fresh_folded_stack_answers_every_pointer_gesture() {
    let projects = fixture::projects();
    let state = reference_deck(4);
    let view = deck_for(&projects, &state);

    for (position, row, terminal) in [(1usize, 2u16, "backend"), (2, 4, "app"), (3, 6, "worker")] {
        assert_eq!(
            view.position_at(SCREEN, Position::new(120, row)),
            Some(position)
        );
        assert_eq!(
            view.swap_position_at(SCREEN, Position::new(120, row)),
            Some(position)
        );
        assert_eq!(
            view.terminal_at(SCREEN, Position::new(120, row))
                .map(ToString::to_string)
                .as_deref(),
            Some(terminal)
        );
        assert_eq!(
            view.marker_at(SCREEN, Position::new(102, row)),
            Some(position)
        );
        assert_eq!(
            view.marker_at(SCREEN, Position::new(103, row)),
            Some(position)
        );
    }
    // The blank column the folds leave below them belongs to the list.
    assert!(view.stack_scroll_at(SCREEN, Position::new(120, 22)));
    assert!(
        !view.stack_scroll_at(SCREEN, Position::new(120, 2)),
        "a strip"
    );
    // Three strips are the whole list, so there is nothing to page to.
    assert!(!view.stack_window(SCREEN).overflows());
}

/// Folds buy the window room, so it takes many more strips than previews
/// before the list overflows — but it still pages when it does.
#[test]
fn a_folded_list_longer_than_the_column_still_pages() {
    let projects = synthetic(24);
    let mut state = reference_deck(24);

    let window = deck_for(&projects, &state).stack_window(SCREEN);
    assert_eq!(window.visible, 19, "39 budget rows hold 19 x (1 + 1)");
    assert_eq!(window.total, 23);
    assert!(window.overflows());

    state.set_stack_offset(window.paged(1));
    let scrolled = deck_for(&projects, &state).stack_window(SCREEN);
    assert_eq!(scrolled.offset, 4, "the last window that ends on the list");
    let rendered = text(&render_long(&projects, &state, (144, 42)));
    assert!(rendered.contains("↑ 4 more"), "{rendered}");
    assert!(rendered.contains("▸ 24 t24"), "{rendered}");
}

/// The wheel over a preview is that preview's (#25), so the list is paged
/// from the column's own chrome instead.
#[test]
fn the_stack_chrome_is_where_the_wheel_pages_the_list() {
    let projects = synthetic(8);
    let state = expanded(8);
    let view = deck_for(&projects, &state);

    assert!(view.stack_scroll_at(SCREEN, Position::new(99, 7)), "gutter");
    assert!(view.stack_scroll_at(SCREEN, Position::new(120, 14)), "gap");
    assert!(
        view.stack_scroll_at(SCREEN, Position::new(120, 41)),
        "footer"
    );
    assert!(
        !view.stack_scroll_at(SCREEN, Position::new(110, 7)),
        "preview"
    );
    assert!(
        !view.stack_scroll_at(SCREEN, Position::new(10, 12)),
        "master"
    );
    assert!(
        !view.stack_scroll_at(SCREEN, Position::new(120, 0)),
        "the status row is not the stack"
    );
}

/// A folded preview keeps its place in the list and costs one row, so the
/// window reaches further down the list without the column growing.
#[test]
fn folding_inside_a_long_list_lets_more_previews_into_the_window() {
    let projects = synthetic(8);
    let mut state = expanded(8);
    assert!(state.toggle_collapse(1));
    assert!(state.toggle_collapse(2));

    let window = deck_for(&projects, &state).stack_window(SCREEN);
    assert_eq!(window.visible, 4, "two strips buy room for a fourth pane");

    let buffer = render_long(&projects, &state, (144, 42));
    let rendered = text(&buffer);
    assert!(rendered.contains("▸ 2 t2"), "the folds stay in place");
    assert!(rendered.contains("▸ 3 t3"));
    assert!(rendered.contains("▾ 4 t4"));
    // Two strips and two open previews, and the footer still on row 39.
    assert_eq!(buffer[(100u16, 2u16)].symbol(), " ");
    assert_eq!(buffer[(102u16, 2u16)].symbol(), "▸");
    assert_eq!(buffer[(102u16, 4u16)].symbol(), "▸");
    assert_eq!(buffer[(100u16, 6u16)].symbol(), "┌");
}

/// The column never overruns its footer row, whatever the mix of folds and
/// whatever the window is showing.
#[test]
fn a_scrolled_column_never_overruns_its_footer() {
    let projects = synthetic(9);
    // Walked from every preview open to every preview folded, so both
    // ends of the #39 default are covered.
    let mut state = expanded(9);

    for step in 0..9 {
        for offset in 0..8 {
            state.set_stack_offset(offset);
            let buffer = render_long(&projects, &state, (144, 42));
            assert_eq!(
                buffer[(102u16, 40u16)].symbol(),
                " ",
                "row 40 stays blank at offset {offset} with {step} folded"
            );
        }
        if step < 8 {
            state.toggle_collapse(step + 1);
        }
    }
}

/// The footer names what the window hides at each end, and the keys that
/// move it while it has the columns for them.
#[test]
fn the_footer_states_what_the_window_hides() {
    let projects = synthetic(8);
    // Open previews, so the window hides four of the seven.
    let mut state = expanded(8);

    let head = text(&render_long(&projects, &state, (144, 42)));
    assert!(head.contains("↓ 4 more · ^g pgup/pgdn"), "{head}");

    state.set_stack_offset(2);
    let middle = text(&render_long(&projects, &state, (144, 42)));
    assert!(
        middle.contains("↑ 2 more · ↓ 2 more · ^g pgup/pgdn"),
        "{middle}"
    );

    state.set_stack_offset(4);
    let tail = text(&render_long(&projects, &state, (144, 42)));
    assert!(tail.contains("↑ 4 more"), "{tail}");
    assert!(!tail.contains("↓"), "nothing is left below: {tail}");

    // The narrower column drops the keys before the counts.
    state.set_stack_offset(2);
    let compact = text(&render_long(&projects, &state, (110, 42)));
    assert!(compact.contains("↑ 2 more · ↓ 2 more"), "{compact}");
    assert!(!compact.contains("pgup"), "{compact}");
}

/// A hidden preview is the one thing collapse never announces elsewhere,
/// so it takes the footer while the fold census keeps the status row.
#[test]
fn the_footer_states_hidden_previews_while_the_status_row_keeps_the_census() {
    let projects = synthetic(8);
    let mut state = expanded(8);
    state.toggle_collapse(1);

    let rendered = text(&render_long(&projects, &state, (144, 42)));

    assert!(rendered.contains("more"), "{rendered}");
    assert!(
        rendered.contains("1 collapsed"),
        "the status row still says"
    );
}

/// The track sits in the gutter, so it takes no columns from the previews.
/// Its thumb is the window's share of the list and reaches each end.
#[test]
fn the_gutter_carries_a_track_while_the_list_is_longer_than_the_column() {
    let projects = synthetic(8);
    // Three open previews of seven is the window the thumb is sized for.
    let mut state = expanded(8);

    let head = render_long(&projects, &state, (144, 42));
    let track = |buffer: &Buffer| {
        (2..41u16)
            .map(|row| buffer[(99u16, row)].symbol().to_owned())
            .collect::<String>()
    };
    let thumb = |buffer: &Buffer| {
        let rows: Vec<u16> = (2..41u16)
            .filter(|row| buffer[(99u16, *row)].symbol() == "┃")
            .collect();
        (rows[0], rows[rows.len() - 1])
    };
    assert_eq!(track(&head).matches('┃').count(), 17, "3 of 7 previews");
    assert_eq!(thumb(&head).0, 2, "the window is at the head of the list");
    assert_eq!(head[(99u16, 2u16)].fg, HINT);
    assert_eq!(head[(99u16, 40u16)].fg, IDLE_BORDER);
    // The pane beside it keeps every column it had.
    assert_eq!(head[(100u16, 2u16)].symbol(), "┌");

    state.set_stack_offset(4);
    let tail = render_long(&projects, &state, (144, 42));
    assert_eq!(thumb(&tail).1, 40, "the window is at the end of the list");
}

#[test]
fn too_few_rows_for_a_preview_also_falls_back() {
    // Counted in preview boxes, so the stack is opened; the fallback is
    // decided by the row budget a whole preview needs, not by the folds.
    let state = expanded(4);

    let (short, _) = render(&fixture::frontend_active(), &state, (144, 15));
    let (tall, _) = render(&fixture::frontend_active(), &state, (144, 16));

    assert_eq!(text(&short).matches('┌').count(), 1);
    assert_eq!(text(&tall).matches('┌').count(), 2);
}

// #97 — terminal notifications.
//
// Every frame below is one render at a chosen `now`, so the flash is pinned
// by keyframes rather than by waiting: nothing here reads a clock.

/// Milliseconds after the reference canvas's own wall clock.
fn at(millis: u64) -> Timestamp {
    Timestamp {
        unix_millis: fixture::NOW.unix_millis + millis,
    }
}

fn message(body: &str) -> NotifyKind {
    NotifyKind::Message {
        title: String::new(),
        body: body.to_owned(),
    }
}

fn rich_message(title: &str, body: &str) -> NotifyKind {
    NotifyKind::Message {
        title: title.to_owned(),
        body: body.to_owned(),
    }
}

/// The bell rings in app's folded strip at t=0; backend, the one open
/// preview, is sent an explicit message two seconds later. Nothing is the
/// master, so nothing is dropped.
fn notified() -> Notifications {
    let mut notifies = Notifications::new();
    assert!(notifies.record(&TerminalId::new("app"), None, NotifyKind::Attention, at(0)));
    assert!(notifies.record(
        &TerminalId::new("backend"),
        None,
        message("tests passed"),
        at(2_000)
    ));
    notifies
}

#[test]
fn notify_keyframes_match_the_reference_canvases() {
    let notifies = notified();
    let engine = fixture::frontend_active();

    // t=0: the bell alone, on the folded strip it rang in.
    let (zero, _) = render_at(&engine, &collapsed(), &notifies, at(0), (144, 42));
    assert_snapshot("notify-flash-t0", &zero);

    // t=2s: the strip is still flashing and the message has just armed the
    // open preview beside it.
    let (two, _) = render_at(&engine, &collapsed(), &notifies, at(2_000), (144, 42));
    assert_snapshot("notify-flash-t2", &two);

    // t=4s: the bell's window has passed and the strip has settled; the
    // message, two seconds younger, is still asking.
    let (four, _) = render_at(&engine, &collapsed(), &notifies, at(4_000), (144, 42));
    assert_snapshot("notify-flash-t4", &four);
}

/// Rich shell-hook completions use the same keyframes as a bare bell, but
/// keep the command and outcome visible wherever the layout has room.
#[test]
fn rich_notify_keyframes_match_the_reference_canvases() {
    let mut notifies = Notifications::new();
    assert!(notifies.record(
        &TerminalId::new("backend"),
        None,
        rich_message("cargo build", "exit 1 · 84s"),
        at(0)
    ));
    let engine = fixture::frontend_active();

    for (name, now) in [
        ("notify-rich-flash-t0", at(0)),
        ("notify-rich-flash-t2", at(2_000)),
        ("notify-rich-flash-t4", at(4_000)),
    ] {
        let (buffer, _) = render_at(&engine, &collapsed(), &notifies, now, (144, 42));
        assert_snapshot(name, &buffer);
    }
}

#[test]
fn a_rich_hidden_completion_uses_the_toast() {
    let mut notifies = Notifications::new();
    assert!(notifies.record(
        &TerminalId::new("worker"),
        None,
        rich_message("cargo test", "done · 6m12s"),
        at(0)
    ));
    let (buffer, _) = render_at(
        &fixture::frontend_active(),
        &zoomed(),
        &notifies,
        at(1_000),
        (144, 42),
    );

    assert_snapshot("notify-rich-toast", &buffer);
}

/// The chrome the keyframes above draw, read as colours rather than glyphs.
#[test]
fn a_notified_pane_flashes_in_the_demotion_idiom_and_then_settles() {
    let notifies = notified();
    let engine = fixture::frontend_active();
    let quiet_frame = render(&engine, &collapsed(), (144, 42)).0;

    let (two, _) = render_at(&engine, &collapsed(), &notifies, at(2_000), (144, 42));
    // The open preview takes the warning border on the demotion's lifted
    // background: the same idiom, saying "answer me" rather than "settled".
    assert_eq!(two[(100u16, 2u16)].fg, WARNING);
    assert_eq!(two[(103u16, 3u16)].bg, DEMOTED_BG);
    // A strip cannot lift a background it already sits on, so it inverts.
    let strip = text(&two)
        .lines()
        .position(|line| line.contains("3 app"))
        .expect("app's strip") as u16;
    assert_eq!(two[(102u16, strip)].bg, WARNING);
    assert_eq!(two[(102u16, strip)].fg, super::CANVAS);
    assert!(
        text(&two).contains("▸ 3 app · ! · attention"),
        "{}",
        text(&two)
    );

    // t=4s: the bell has settled back into an ordinary strip, and only the
    // younger message is still flashing.
    let (four, _) = render_at(&engine, &collapsed(), &notifies, at(4_000), (144, 42));
    assert_eq!(four[(102u16, strip)].bg, DEMOTED_BG);
    assert_eq!(four[(100u16, 2u16)].fg, WARNING, "the message is younger");

    // t=6s: everything has settled, and the canvas is the one the deck drew
    // before any of it — a flash leaves nothing behind on a drawn pane.
    let (six, _) = render_at(&engine, &collapsed(), &notifies, at(6_000), (144, 42));
    assert_eq!(text(&six), text(&quiet_frame));
}

/// #101: the canvas spends a flashing pane's right slot on what it is
/// asking about, and leaves the status dot alone — the slot says in words
/// what the border says in colour, so the mark the censuses use is only
/// needed where there is no slot to say it in.
#[test]
fn a_flashing_pane_names_its_notification_where_its_meter_was() {
    let notifies = notified();
    let engine = fixture::frontend_active();

    let (two, _) = render_at(&engine, &collapsed(), &notifies, at(2_000), (144, 42));
    let title: String = (100..144u16).map(|x| two[(x, 2u16)].symbol()).collect();
    assert!(title.contains("▾ 2 backend · ●"), "{title}");
    assert!(title.contains("tests passed"), "{title}");
    assert!(
        !title.contains("[##····]"),
        "the meter gave up the slot: {title}"
    );

    // Once the flash settles the meter has its slot back.
    let (six, _) = render_at(&engine, &collapsed(), &notifies, at(6_000), (144, 42));
    let settled: String = (100..144u16).map(|x| six[(x, 2u16)].symbol()).collect();
    assert!(settled.contains("[##····]"), "{settled}");
    assert!(!settled.contains("tests passed"), "{settled}");
}

/// A notification for a pane the layout does not draw has nowhere to flash,
/// so it goes to the toast: the quit confirmation's shape without its
/// authority. The master keeps its cursor and the deck is not dimmed.
#[test]
fn a_hidden_pane_notifies_through_a_toast_that_takes_no_focus() {
    let mut notifies = Notifications::new();
    notifies.record(
        &TerminalId::new("worker"),
        None,
        NotifyKind::Attention,
        at(0),
    );
    notifies.record(
        &TerminalId::new("backend"),
        None,
        message("build failed"),
        at(2_000),
    );
    let engine = fixture::frontend_active();

    let (buffer, cursor) = render_at(&engine, &zoomed(), &notifies, at(3_000), (144, 42));

    assert_snapshot("notify-toast", &buffer);
    let screen = text(&buffer);
    // The quit confirmation's 52 columns, centred like it, and as tall as
    // the batch it is listing: two notifications, so rows 18..23.
    assert_eq!(buffer[(46u16, 18u16)].symbol(), "┌");
    assert_eq!(buffer[(97u16, 23u16)].symbol(), "┘");
    assert_eq!(buffer[(46u16, 18u16)].fg, WARNING, "it is not the focus");
    assert!(screen.contains("2 notifications"), "{screen}");
    // Newest first, each with its own age off the injected clock.
    let backend = screen
        .find("2 backend · build failed · 1s")
        .expect("the message");
    let worker = screen.find("4 worker · attention · 3s").expect("the bell");
    assert!(backend < worker, "{screen}");
    // The master pane behind it keeps its cursor and its colours: a toast
    // never takes the focus a modal does.
    assert_eq!(cursor, Some(Position::new(3, 31)));
    assert_eq!(buffer[(0u16, 2u16)].fg, ACCENT);
    assert_eq!(buffer[(5u16, 3u16)].fg, super::MASTER_FG);
}

/// The batch is a list, not a queue: four newest and a count of the rest.
#[test]
fn the_toast_lists_four_and_counts_the_rest() {
    let projects = synthetic(8);
    let mut notifies = Notifications::new();
    for (index, project) in projects.iter().enumerate().skip(1) {
        notifies.record(
            &project.terminal,
            None,
            message("done"),
            at(index as u64 * 100),
        );
    }
    let state = zoomed_long(8);
    let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let deck = Deck {
        workspace: "idp",
        projects: &projects,
        state: &state,
        notifies: &notifies,
        master_ratio: state.master_ratio(),
        now: at(1_000),
    };
    let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
    terminal
        .draw(|frame| deck.render(&engine as &dyn TerminalEngine, frame))
        .unwrap();
    let screen = text(terminal.backend().buffer());

    assert!(screen.contains("7 notifications"), "{screen}");
    // Newest first: t8 down to t5, then the count.
    for listed in ["8 t8 · done", "7 t7 · done", "6 t6 · done", "5 t5 · done"] {
        assert!(screen.contains(listed), "{listed} missing from {screen}");
    }
    assert!(!screen.contains("4 t4 · done"), "{screen}");
    assert!(screen.contains("+3 more"), "{screen}");
}

/// The toast is dismissible and the flash settles, so the censuses are what
/// keeps a hidden pane accounted for afterwards: `3!` among the zoom dots,
/// and a marked chip in the narrow fallback's pane strip.
#[test]
fn the_censuses_keep_a_dismissed_notification_honest() {
    let mut notifies = Notifications::new();
    notifies.record(
        &TerminalId::new("backend"),
        None,
        message("tests passed"),
        at(0),
    );
    let engine = fixture::frontend_active();

    let (zoom, _) = render_at(&engine, &zoomed(), &notifies, at(0), (144, 42));
    assert!(text(&zoom).contains("hidden: 2! 3✕ 4○"), "{}", text(&zoom));

    // Dismissed: the toast goes, the mark stays, and the deck behind it is
    // otherwise the canvas it always was.
    notifies.dismiss(at(1_000));
    let (dismissed, _) = render_at(&engine, &zoomed(), &notifies, at(2_000), (144, 42));
    let screen = text(&dismissed);
    assert!(!screen.contains("notification"), "{screen}");
    assert!(screen.contains("hidden: 2! 3✕ 4○"), "{screen}");

    // The narrow fallback hides every pane but the master, so its chips are
    // the only census it has.
    let (narrow, _) = render_at(&engine, &reference_deck(4), &notifies, at(2_000), (84, 22));
    let strip: String = (0..84)
        .map(|column| narrow[(column, 2u16)].symbol())
        .collect();
    assert!(strip.contains("2 backend !"), "{strip}");
    assert_eq!(narrow[(14u16, 2u16)].fg, WARNING);

    // Being seen is what clears it: promoting the pane takes the mark.
    notifies.clear(&TerminalId::new("backend"));
    let (seen, _) = render_at(&engine, &zoomed(), &notifies, at(2_000), (144, 42));
    assert!(text(&seen).contains("hidden: 2● 3✕ 4○"), "{}", text(&seen));
}

/// A preview the stack column has scrolled past is as hidden as a zoomed one.
#[test]
fn a_preview_outside_the_stack_window_notifies_through_the_toast() {
    let projects = synthetic(8);
    let state = expanded(8);
    let mut notifies = Notifications::new();
    // The column holds three open previews, so t8 is well past its end.
    notifies.record(&projects[7].terminal, None, message("done"), at(0));
    let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
    let deck = Deck {
        workspace: "idp",
        projects: &projects,
        state: &state,
        notifies: &notifies,
        master_ratio: state.master_ratio(),
        now: at(0),
    };
    assert_eq!(deck.stack_window(SCREEN).visible, 3);
    let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
    terminal
        .draw(|frame| deck.render(&engine as &dyn TerminalEngine, frame))
        .unwrap();

    assert!(
        text(terminal.backend().buffer()).contains("8 t8 · done"),
        "{}",
        text(terminal.backend().buffer())
    );
}

/// The column a run of cells starts at on one row, for the status bar's own
/// assertions: every cell it draws is one character wide, so a character
/// offset into the joined row is a column.
fn column_of(buffer: &Buffer, row: u16, needle: &str) -> Option<u16> {
    let joined: String = (0..buffer.area().width)
        .map(|column| buffer[(column, row)].symbol())
        .collect();
    joined
        .find(needle)
        .map(|byte| joined[..byte].chars().count() as u16)
}

/// #113: worker pinned, then frontend promoted. The pinned pane is the top
/// of the stack rather than the slot frontend vacated, and it wears the mark.
fn pinned() -> DeckState {
    let mut state = expanded(4);
    let projects = fixture::projects();
    state.apply(&ActionCommand::SelectPosition(3), &projects, fixture::NOW);
    assert!(state.toggle_pin());
    state.apply(&ActionCommand::SelectPosition(0), &projects, fixture::NOW);
    assert_eq!(state.stack(), [3, 1, 2]);
    state
}

#[test]
fn pinned_stack_matches_the_canvas() {
    let (buffer, _) = render(&fixture::frontend_active(), &pinned(), (144, 42));

    assert_snapshot("pinned-stack", &buffer);
}

/// The mark leads the title row behind the disclosure marker, in accent, and
/// no other pane wears one.
#[test]
fn the_pinned_preview_wears_its_mark_in_the_title() {
    let (buffer, _) = render(&fixture::frontend_active(), &pinned(), (144, 42));
    let rendered = text(&buffer);

    assert!(rendered.contains("▾ ↑ 4 worker"), "{rendered}");
    assert!(rendered.contains("▾ 2 backend"), "one pin, one mark");
    assert!(rendered.contains("▾ 3 app"), "{rendered}");
    // The marker keeps its own two cells at the head of the row, so the fold
    // affordance and its hit test are where they were.
    assert_eq!(buffer[(103u16, 2u16)].symbol(), "▾");
    assert_eq!(buffer[(105u16, 2u16)].symbol(), "↑");
    assert_eq!(buffer[(105u16, 2u16)].fg, ACCENT);
    let projects = fixture::projects();
    let state = pinned();
    assert_eq!(
        deck_for(&projects, &state).marker_at(SCREEN, Position::new(103, 2)),
        Some(3),
        "the pinned pane still folds from its marker"
    );
}

/// A folded pinned pane says the same thing on its strip: the pin is order,
/// not disclosure, so it keeps its slot folded.
#[test]
fn a_folded_pinned_preview_wears_the_mark_on_its_strip() {
    let mut state = pinned();
    assert!(state.toggle_collapse(3));

    let (buffer, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let rendered = text(&buffer);

    assert!(
        rendered.contains("▸ ↑ 4 worker · ○ · idle 6m"),
        "{rendered}"
    );
    assert_eq!(state.stack(), [3, 1, 2], "and it is still the top of it");
}

/// Zoom hides the stack and the narrow fallback has none, so both keep the
/// pin stated in the census they already draw.
#[test]
fn zoom_and_the_narrow_fallback_keep_the_pin_stated() {
    let mut state = pinned();
    state.apply(
        &ActionCommand::ToggleZoom,
        &fixture::projects(),
        fixture::NOW,
    );

    let (zoom, _) = render(&fixture::frontend_active(), &state, (144, 42));
    assert!(text(&zoom).contains("hidden: ↑4○ 2● 3✕"), "{}", text(&zoom));

    let (narrow, _) = render(&fixture::frontend_active(), &pinned(), (84, 22));
    assert!(text(&narrow).contains("↑ 4 worker"), "{}", text(&narrow));
}

/// The explicit unpin #113 asks for: while the master is the pinned pane the
/// status row advertises the key that undoes the state, accented, exactly as
/// it advertises `^g c` while a fold is in play. Elsewhere the row keeps its
/// columns and the pinned pane's own mark carries the state.
#[test]
fn the_status_row_advertises_the_unpin_key_while_the_master_is_pinned() {
    let (stacked, _) = render(&fixture::frontend_active(), &pinned(), (144, 42));
    assert!(
        !text(&stacked).contains("^g p"),
        "the pinned pane is in the stack, so the key would not unpin it"
    );

    let mut state = pinned();
    state.apply(
        &ActionCommand::SelectPosition(3),
        &fixture::projects(),
        fixture::NOW,
    );
    let (master, _) = render(&fixture::frontend_active(), &state, (144, 42));
    let rendered = text(&master);

    assert!(rendered.contains("^g p"), "{rendered}");
    let column = column_of(&master, 0, "^g p").expect("the unpin key");
    assert_eq!(master[(column, 0u16)].fg, ACCENT, "a state in play");
    // And the master says it is the pinned pane, in its own title.
    assert!(rendered.contains("↑ > 4 worker"), "{rendered}");
}

/// Nothing is pinned on any canvas that predates #113, so nothing about them
/// moves: the mark and the key are both conditional.
#[test]
fn an_unpinned_deck_draws_exactly_what_it_drew_before() {
    let (buffer, _) = render(&fixture::frontend_active(), &reference_deck(4), (144, 42));
    let rendered = text(&buffer);

    assert!(!rendered.contains("↑"), "{rendered}");
    assert!(!rendered.contains("^g p"), "{rendered}");
}
