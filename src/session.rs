//! The interactive composition root: terminal mode, event loop, UI, and PTYs.

mod backend;
mod input;
mod lifecycle;
mod outer;

#[cfg(test)]
mod tests;

use backend::AnsiBackend;
use input::*;
use lifecycle::*;
use outer::{OuterTerminal, PanicGuard, SignalGuard};

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
    contracts::{
        EngineCommand, Project, ScreenSize, ScrollCommand, TerminalEngine, TerminalId,
        TerminalMetadata, Timestamp, UserCommand,
    },
    engine::NativeEngine,
    ui::{
        Browse, Deck, DeckState, FsBrowse, Input, Key, Picker, PickerReaction, PickerState,
        Reaction, Sheet, SheetState, picker,
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// A wheel tick is intentionally smaller than a keyboard page movement.
const WHEEL_LINES: u16 = 3;
const DOUBLE_CLICK_WINDOW: u64 = 500;
static SIGNAL: AtomicI32 = AtomicI32::new(0);
static SAVED_TERMIOS: OnceLock<Mutex<Option<libc::termios>>> = OnceLock::new();
type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;
static SAVED_PANIC_HOOK: OnceLock<Mutex<Option<PanicHook>>> = OnceLock::new();

/// Runs the repository picker: what `termdeck` opens when it is given no
/// path (#42 A2). Returns the workspace the user chose, or `None` if they
/// left without choosing one.
///
/// No engine is spawned here — nothing has been picked yet, so there is
/// nothing to run. The picker only reads the filesystem through its browser
/// and hands back an ordered list of terminals.
pub fn pick(roots: Vec<std::path::PathBuf>) -> Result<Option<Workspace>, Box<dyn Error>> {
    let _panic = PanicGuard::install();
    let _signals = SignalGuard::install()?;
    let outer = OuterTerminal::enter()?;
    let mut terminal = Terminal::new(AnsiBackend::new()?)?;
    let mut keys = KeyReader::default();
    let browser = FsBrowse::new(roots);
    let mut state = PickerState::new();
    // The pointer's `→`: a second click on the row it is already on.
    let mut last_click: Option<(usize, Timestamp)> = None;
    let mut dirty = true;
    let chosen = 'picker: loop {
        if SIGNAL.swap(0, Ordering::SeqCst) != 0 {
            break None;
        }
        let roots = Browse::roots(&browser);
        let listing = state.listing(&browser);
        let rows = listing.entries.clone();
        let size = screen_size()?;
        if dirty {
            let height = usize::from(size.rows.saturating_sub(12));
            state.follow_cursor(height);
            terminal.autoresize()?;
            terminal.draw(|frame| {
                Picker {
                    state: &state,
                    listing: &listing,
                    roots: &roots,
                    home: browser.home(),
                }
                .render(frame);
            })?;
            dirty = false;
        }
        for event in keys.read(POLL_INTERVAL)? {
            dirty = true;
            let reaction = match event {
                InputEvent::Key(key) => picker::press(&mut state, &rows, &roots, key),
                InputEvent::Mouse {
                    pointer,
                    action:
                        action @ (MouseAction::Up | MouseAction::SecondaryUp | MouseAction::RangeUp),
                } => {
                    let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                    let view = Picker {
                        state: &state,
                        listing: &listing,
                        roots: &roots,
                        home: browser.home(),
                    };
                    let hit = view.hit(area, pointer);
                    match (hit, action) {
                        (Some(hit), MouseAction::Up) => {
                            // One click selects; a second on the same row
                            // goes inside it, which is what `→` does.
                            let now = now();
                            let again = matches!((hit, last_click), (
                                crate::ui::Hit::Row(row),
                                Some((previous, at)),
                            ) if row == previous
                                && now.unix_millis.saturating_sub(at.unix_millis)
                                    < DOUBLE_CLICK_WINDOW);
                            last_click = match hit {
                                crate::ui::Hit::Row(row) if !again => Some((row, now)),
                                _ => None,
                            };
                            if again {
                                picker::descend(&mut state, &rows, hit);
                                None
                            } else {
                                picker::click(&mut state, &rows, hit)
                            }
                        }
                        // Shift held: the range gesture, which is the same
                        // selection `⇧↓` makes, bounded by where it landed.
                        (Some(hit), MouseAction::RangeUp) => {
                            last_click = None;
                            picker::click_range(&mut state, &rows, hit);
                            None
                        }
                        (Some(hit), _) => {
                            picker::click_secondary(&mut state, &rows, hit);
                            None
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            match reaction {
                Some(PickerReaction::Launch) => break 'picker Some(workspace_of(&state)),
                Some(PickerReaction::Quit) => break 'picker None,
                None => {}
            }
        }
    };
    drop(outer);
    Ok(chosen)
}

/// What the session is already running, so the sheet can identify another
/// instance of an open path.
fn open_terminals(projects: &[Project]) -> Vec<crate::ui::Open> {
    projects
        .iter()
        .enumerate()
        .map(|(index, project)| crate::ui::Open {
            path: project.path.clone(),
            pane: index + 1,
        })
        .collect()
}

/// The projects a committed sheet adds, named against what is already
/// running so a second instance of an open path becomes `-2` rather than a
/// duplicate identity the engine would refuse.
fn chosen(sheet: &SheetState, projects: &[Project]) -> Vec<Project> {
    let mut taken: Vec<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let mut added = Vec::new();
    for instance in sheet.marked() {
        let base = instance
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| instance.name.clone());
        let names: Vec<&str> = taken.iter().map(String::as_str).collect();
        let name = picker::unique_name(&names, &base);
        taken.push(name.clone());
        added.push(Project {
            terminal: TerminalId::new(name),
            path: instance.path.clone(),
            command: vec!["bash".to_owned(), "-l".to_owned()],
        });
    }
    added
}

/// Turns the picker's ordered `(name, path)` pairs into the workspace the
/// session opens. The first pair is pane 1 and therefore the master.
fn workspace_of(state: &PickerState) -> Workspace {
    let projects: Vec<Project> = state
        .launch()
        .into_iter()
        .map(|(name, path)| Project {
            terminal: TerminalId::new(name),
            path,
            command: vec!["bash".to_owned(), "-l".to_owned()],
        })
        .collect();
    let root = projects
        .first()
        .map(|project| project.path.clone())
        .unwrap_or_default();
    let mut workspace = Workspace::discovered(root, projects);
    if !state.workspace().is_empty() {
        workspace.name = state.workspace().to_owned();
    }
    workspace
}

/// Runs an already validated workspace. Configuration is deliberately loaded
/// before this point, so no PTY exists when validation fails.
pub fn run(workspace: &Workspace) -> Result<(), Box<dyn Error>> {
    let _panic = PanicGuard::install();
    let _signals = SignalGuard::install()?;
    let outer = OuterTerminal::enter()?;
    let mut size = screen_size()?;
    // The workspace opened this list; `^g a` can lengthen it, so the session
    // owns it from here (#50 A3).
    let mut projects = workspace.projects.clone();
    // The configuration seeds the split; the divider owns it from there.
    let mut deck = DeckState::new(projects.len()).with_master_ratio(workspace.master_ratio.get());
    let mut engine = spawn_terminals(&projects, &deck, size)
        .map_err(|error| format!("cannot start workspace '{}': {error}", workspace.name))?;
    let mut terminal = Terminal::new(AnsiBackend::new()?)?;
    let mut input = Input::new(size.rows.saturating_sub(4));
    let mut keys = KeyReader::default();
    let mut last_click = None;
    // The preview whose disclosure marker is being pressed, if any.
    let mut marker_press: Option<usize> = None;
    // The pane whose close affordance is being pressed, if any (#84).
    let mut close_press: Option<usize> = None;
    // The runtime-add sheet, while it is open. It owns every key it sees.
    let mut sheet: Option<SheetState> = None;
    let browser = FsBrowse::new(crate::cli::picker_roots());
    let roots = Browse::roots(&browser);

    let mut dirty = true;
    'session: loop {
        if SIGNAL.swap(0, Ordering::SeqCst) != 0 {
            break;
        }
        let current_size = screen_size()?;
        if current_size != size {
            resize_terminals(&mut engine, &projects, &deck, current_size);
            input.set_page(current_size.rows.saturating_sub(4));
            size = current_size;
            terminal.autoresize()?;
            dirty = true;
        }
        dirty |= !engine.drain_events().is_empty();
        dirty |= input.expire(&mut deck, &projects, now());

        for event in keys.read(POLL_INTERVAL)? {
            dirty = true;
            // The sheet takes every key and every click while it is open, the
            // way a modal does: the session behind it stays live but is not
            // being driven (#50 A3).
            if let Some(open_sheet) = sheet.as_mut() {
                let rows = open_sheet.rows(&browser);
                let open = open_terminals(&projects);
                let reaction = match event {
                    InputEvent::Key(key) => {
                        picker::sheet_press(open_sheet, &rows, &roots, &open, key)
                    }
                    InputEvent::Mouse {
                        pointer,
                        action: action @ (MouseAction::Up | MouseAction::SecondaryUp),
                    } => {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let hit = Sheet {
                            state: open_sheet,
                            rows: &rows,
                            roots: &roots,
                            open: &open,
                            home: browser.home(),
                            next_pane: projects.len() + 1,
                        }
                        .hit(area, pointer);
                        match (hit, action) {
                            (Some(hit), MouseAction::Up) => {
                                picker::sheet_click(open_sheet, &rows, &roots, &open, hit)
                            }
                            (Some(hit), _) => {
                                picker::sheet_click_secondary(open_sheet, &rows, hit);
                                None
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                match reaction {
                    Some(PickerReaction::Launch) => {
                        for project in chosen(open_sheet, &projects) {
                            match add_terminal(&mut engine, project.clone(), &projects, &deck, size)
                            {
                                Ok(()) => {
                                    projects.push(project);
                                    deck.push_terminal();
                                }
                                // A terminal that will not start is not worth
                                // ending the session over; the rest still do.
                                Err(_) => continue,
                            }
                        }
                        sheet = None;
                    }
                    Some(PickerReaction::Quit) => sheet = None,
                    None => {}
                }
                continue;
            }
            match event {
                InputEvent::Key(key) => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    close_press = None;
                    deck.set_resizing(false);
                    let was_scrollback = deck.scrollback();
                    let reaction = input.press(key, &mut deck, &projects, now());
                    if was_scrollback
                        && key == Key::Escape
                        && !deck.scrollback()
                        && let Some(active) = deck.active()
                    {
                        dispatch_scroll(
                            &mut engine,
                            projects[active].terminal.clone(),
                            ScrollCommand::Bottom,
                        );
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
                                    projects[active].terminal.clone(),
                                    bytes,
                                );
                            }
                        }
                        Some(Reaction::Send(UserCommand::Action(_))) => {}
                        Some(Reaction::Scroll(command)) => {
                            if let Some(active) = deck.active() {
                                dispatch_scroll(
                                    &mut engine,
                                    projects[active].terminal.clone(),
                                    command,
                                );
                            }
                        }
                        // One page of the preview list is rendered geometry,
                        // so the window the renderer just drew answers it.
                        Some(Reaction::PageStack(pages)) => {
                            let window =
                                Deck {
                                    workspace: &workspace.name,
                                    projects: &projects,
                                    state: &deck,
                                    master_ratio: deck.master_ratio(),
                                    now: now(),
                                }
                                .stack_window(
                                    ratatui::layout::Rect::new(0, 0, size.columns, size.rows),
                                );
                            deck.set_stack_offset(window.paged(pages));
                        }
                        Some(Reaction::Respawn) => {
                            if let Some(active) = deck.active() {
                                engine.dispatch(EngineCommand::Respawn {
                                    terminal: projects[active].terminal.clone(),
                                });
                            }
                        }
                        Some(Reaction::AddTerminal) => sheet = Some(SheetState::new(&roots)),
                        // `^g x` closes the pane the master holds, which is
                        // the pane every other prefixed command acts on. The
                        // last one is the session, so it asks the quit
                        // confirmation instead, and leaves by that door (#84).
                        Some(Reaction::Close) => {
                            if let Some(active) = deck.active() {
                                request_close(&mut engine, &mut projects, &mut deck, active);
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
                    close_press = None;
                    deck.set_resizing(false);
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let pane = Deck {
                            workspace: &workspace.name,
                            projects: &projects,
                            state: &deck,
                            master_ratio: deck.master_ratio(),
                            now: now(),
                        };
                        let (terminal, list) = {
                            let pointed = pane.position_at(area, pointer);
                            let terminal = pointed
                                // A collapsed preview has no viewport, so there
                                // is nothing under the pointer to scroll.
                                .filter(|position| !deck.collapsed(*position))
                                .and_then(|position| projects.get(position))
                                .map(|project| project.terminal.clone());
                            // Off the previews, the wheel belongs to the list:
                            // the gutter, the gaps and the footer page it.
                            let list = (pointed.is_none() && pane.stack_scroll_at(area, pointer))
                                .then(|| pane.stack_window(area));
                            (terminal, list)
                        };
                        if let Some(terminal) = terminal {
                            // A full-screen app owns its grid: termdeck
                            // scrollback is inert there, so the wheel belongs
                            // to the app (#74). Line terminals keep the #25
                            // scrollback below.
                            match route_wheel(engine.metadata(&terminal), command) {
                                WheelRoute::Scrollback(command) => {
                                    dispatch_scroll(&mut engine, terminal, command);
                                }
                                WheelRoute::App { up, mouse } => {
                                    let (column, row) = pane
                                        .pane_cell(area, pointer)
                                        .map(|(_, column, row)| (column, row))
                                        .unwrap_or((0, 0));
                                    let size = engine
                                        .frame(&terminal)
                                        .map(|frame| frame.size)
                                        .unwrap_or(ScreenSize::new(1, 1));
                                    let column = column.min(size.columns.saturating_sub(1)) + 1;
                                    let row = row.min(size.rows.saturating_sub(1)) + 1;
                                    // App input, not live-shell typing: no
                                    // tail-resume preamble, which is inert on
                                    // the alternate screen anyway.
                                    engine.dispatch(EngineCommand::Input {
                                        terminal,
                                        bytes: app_wheel(up, column, row, mouse),
                                    });
                                }
                            }
                        } else if let Some(window) = list {
                            let items = match command {
                                ScrollCommand::Up(_) => -1,
                                _ => 1,
                            };
                            deck.set_stack_offset(window.scrolled(items));
                        }
                    }
                }
                // The deck has no gesture of its own for either, but the
                // press that came before one may have armed a drag or a
                // marker — a shift-click's own press decodes as an ordinary
                // one. So they end that state rather than leaving it for
                // whatever event happens to arrive next.
                InputEvent::Mouse {
                    action: action @ (MouseAction::SecondaryUp | MouseAction::RangeUp),
                    ..
                } => {
                    marker_press = None;
                    close_press = None;
                    mouse_action(&mut deck, None, action, now(), &mut last_click);
                }
                InputEvent::Mouse { pointer, action } => {
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let (marker, close, divider, split, position) = {
                            let pane = Deck {
                                workspace: &workspace.name,
                                projects: &projects,
                                state: &deck,
                                master_ratio: deck.master_ratio(),
                                now: now(),
                            };
                            (
                                pane.marker_at(area, pointer),
                                pane.close_at(area, pointer),
                                pane.divider_at(area, pointer),
                                pane.ratio_at(area, pointer.x),
                                match action {
                                    MouseAction::Down => pane.swap_position_at(area, pointer),
                                    // SecondaryUp never reaches here — the
                                    // event loop drops it before the deck.
                                    _ => pane.position_at(area, pointer),
                                },
                            )
                        };
                        // The status bar's `+` opens the same sheet `^g a`
                        // does — the pointer's half of the affordance.
                        let plus = {
                            let bar = Deck {
                                workspace: &workspace.name,
                                projects: &projects,
                                state: &deck,
                                master_ratio: deck.master_ratio(),
                                now: now(),
                            };
                            action == MouseAction::Up && bar.add_at(area, pointer)
                        };
                        if plus {
                            sheet = Some(SheetState::new(&roots));
                            continue;
                        }
                        // The divider is in the gutter, which belongs to no
                        // pane, so holding it can never be a pane drag. Once
                        // held it keeps the pointer until release, wherever
                        // the pointer travels.
                        if divider && action == MouseAction::Down || deck.resizing() {
                            deck.cancel_drag();
                            last_click = None;
                            marker_press = None;
                            close_press = None;
                            deck.set_resizing(action != MouseAction::Up);
                            if action != MouseAction::Down
                                && let Some(split) = split
                            {
                                deck.set_master_ratio(split);
                            }
                            continue;
                        }
                        // The close affordance owns its two cells the way
                        // the marker below owns its own, and for the same
                        // reason: the press there arms nothing else, and only
                        // a release still on it closes the pane. Anything
                        // that is neither of those falls through untouched.
                        let closing = match (action, close, close_press) {
                            (MouseAction::Down, Some(pressed), _) => {
                                deck.cancel_drag();
                                last_click = None;
                                marker_press = None;
                                close_press = Some(pressed);
                                true
                            }
                            (MouseAction::Move, _, Some(_)) => true,
                            (MouseAction::Up, _, Some(pressed)) => {
                                close_press = None;
                                if close == Some(pressed) {
                                    request_close(&mut engine, &mut projects, &mut deck, pressed);
                                }
                                true
                            }
                            _ => false,
                        };
                        if closing {
                            continue;
                        }
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
                                    deck.apply(&action, &projects, now());
                                }
                            }
                        }
                    }
                }
                InputEvent::Paste(text) => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    close_press = None;
                    deck.set_resizing(false);
                    if let Some(active) = deck.active()
                        && deck.modal().is_none()
                        && !deck.scrollback()
                    {
                        dispatch_live_input(
                            &mut engine,
                            projects[active].terminal.clone(),
                            text.into_bytes(),
                        );
                    }
                }
            }
        }
        dirty |= resize_terminals(&mut engine, &projects, &deck, size);
        if dirty {
            terminal.draw(|frame| {
                Deck {
                    workspace: &workspace.name,
                    projects: &projects,
                    state: &deck,
                    master_ratio: deck.master_ratio(),
                    now: now(),
                }
                .render(&engine, frame);
                if let Some(open_sheet) = sheet.as_ref() {
                    let rows = open_sheet.rows(&browser);
                    Sheet {
                        state: open_sheet,
                        rows: &rows,
                        roots: &roots,
                        open: &open_terminals(&projects),
                        home: browser.home(),
                        next_pane: projects.len() + 1,
                    }
                    .render(frame);
                }
            })?;
            dirty = false;
        }
    }
    // The interface is finished, so give the terminal back before the engine
    // takes its time: a pane that ignores the polite signals holds shutdown
    // for the whole grace period, and the user should not be looking at a
    // frozen deck while it does (#46).
    drop(outer);
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
