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
    collections::BTreeSet,
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
        EngineCommand, EngineEvent, NotifyKind, Project, ScreenSize, ScrollCommand, TerminalEngine,
        TerminalId, TerminalMetadata, TerminalStatus, Timestamp, UserCommand,
    },
    engine::NativeEngine,
    ui::{
        Browse, Deck, DeckState, FsBrowse, Input, Key, Listing, Notifications, Picker,
        PickerReaction, PickerState, Reaction, Selection, Sheet, SheetState, picker,
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
    let mut size = screen_size()?;
    // What the last drawn frame was built from, kept across passes: an idle
    // loop that rebuilt them was walking the filesystem fifty times a second
    // for a screen it was not going to redraw (#127).
    let mut roots = Vec::new();
    let mut listing = Listing::default();
    let mut rows = Vec::new();
    let chosen = 'picker: loop {
        if SIGNAL.swap(0, Ordering::SeqCst) != 0 {
            break None;
        }
        // A resize is a reason to redraw by itself. Without this the picker
        // read the size every pass and never compared it, so a resized
        // window kept its old layout until the next keypress (#127).
        dirty |= resized(&mut size, screen_size()?);
        if dirty {
            roots = Browse::roots(&browser);
            listing = state.listing(&browser);
            rows = listing.entries.clone();
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

/// Whether the terminal has changed size since the last frame, remembering
/// the new size as it answers. A picker that has not been touched still has
/// to redraw when the window around it moves.
fn resized(size: &mut ScreenSize, current: ScreenSize) -> bool {
    let changed = *size != current;
    *size = current;
    changed
}

/// What a failed add says, as the one toast row it becomes: the terminal it
/// is about, and one bounded line about why (#128).
///
/// The panes that did start are already open and stay open — this is the
/// half of the batch that has nowhere else to appear. A sheet of twenty that
/// all fail says so in one row and counts the rest, and an error that
/// arrives as a paragraph is cut to its first line: the toast is four rows
/// of a fixed box, not a log.
fn add_failure(failed: &[(TerminalId, String)]) -> Option<(String, NotifyKind)> {
    let (terminal, error) = failed.first()?;
    let error = error.lines().next().unwrap_or_default().trim();
    let body = match failed.len() - 1 {
        0 => error.to_owned(),
        rest => format!("{error} · +{rest} more"),
    };
    Some((
        terminal.to_string(),
        NotifyKind::Message {
            title: "did not start".to_owned(),
            body,
        },
    ))
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

/// Allocates one runtime identity (#121): the path's base name made unique
/// against every id the session has EVER used — open or closed — and
/// recorded in `used` before returning. Display name and runtime id are one
/// string (no render or wire change), but the session-monotonic set means a
/// closed name is never handed out again: reopening a directory yields
/// `base-2`, never the tombstoned original, so a stale id cannot resolve
/// to a different live terminal.
fn alloc_identity(used: &mut BTreeSet<String>, base: &str) -> String {
    let taken: Vec<&str> = used.iter().map(String::as_str).collect();
    let name = picker::unique_name(&taken, base);
    used.insert(name.clone());
    name
}

/// The projects a committed sheet adds, named against every identity the
/// session has ever used so a reopened path becomes `base-2` rather than
/// reusing a tombstoned id the engine — or a stale agent retry — could
/// confuse with a live terminal (#121).
fn chosen(sheet: &SheetState, used: &mut BTreeSet<String>) -> Vec<Project> {
    let mut added = Vec::new();
    for instance in sheet.marked() {
        let base = instance
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| instance.name.clone());
        let name = alloc_identity(used, &base);
        added.push(Project {
            terminal: TerminalId::new(name),
            path: instance.path.clone(),
            command: vec!["bash".to_owned(), "-l".to_owned()],
            shell_hook: true,
        });
    }
    added
}

/// Builds the ordinary runtime-add project used by the ctl `open` verb.
/// The identity is session-unique (#121): a reopened directory never
/// reuses a tombstoned name, so a stale agent id cannot land on the new
/// pane.
fn opened(path: &str, used: &mut BTreeSet<String>) -> Result<Project, crate::ctl::Response> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        crate::ctl::Response::error(2, format!("bad request: open path: {error}"))
    })?;
    if !path.is_dir() {
        return Err(crate::ctl::Response::error(
            2,
            "bad request: open path must be a directory",
        ));
    }
    let base = path
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Ok(Project {
        terminal: TerminalId::new(alloc_identity(used, &base)),
        path,
        command: vec!["bash".to_owned(), "-l".to_owned()],
        shell_hook: true,
    })
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn dispatch_control(
    request: crate::ctl::Request,
    caller: Option<&str>,
    workspace: &Workspace,
    projects: &mut Vec<Project>,
    deck: &mut DeckState,
    engine: &mut NativeEngine,
    notifies: &mut Notifications,
    size: ScreenSize,
    sheet_open: bool,
    socket: &std::path::Path,
    allow_input: bool,
    closed: &mut BTreeSet<String>,
    used: &mut BTreeSet<String>,
    quit: &mut bool,
) -> crate::ctl::Response {
    if request.schema != crate::ctl::SCHEMA {
        return crate::ctl::Response::error(2, "bad request: unsupported schema");
    }
    let control = match request.control() {
        Ok(Some(control)) => control,
        Ok(None) => {
            return crate::ctl::dispatch(
                request,
                crate::ctl::State {
                    workspace,
                    projects,
                    deck,
                    engine,
                    size,
                    sheet_open,
                    notifies,
                    caller,
                    now: now(),
                },
            );
        }
        Err(response) => return response,
    };
    match control {
        crate::ctl::Control::Open { path } => {
            if sheet_open {
                return crate::ctl::Response::error(3, "refused: runtime-add sheet is open");
            }
            let project = match opened(&path, used) {
                Ok(project) => project,
                Err(response) => return response,
            };
            match add_terminal_with_socket(engine, project.clone(), projects, deck, size, socket) {
                Ok(()) => {
                    let id = project.terminal.to_string();
                    closed.remove(&id);
                    projects.push(project);
                    deck.push_terminal();
                    crate::ctl::Response::ok(serde_json::json!({ "id": id }))
                }
                Err(error) => crate::ctl::Response::error(1, error),
            }
        }
        crate::ctl::Control::Close { id, force } => {
            if caller == Some(id.as_str()) && !force {
                return crate::ctl::Response::error(
                    3,
                    "refused: cannot close caller pane without --force",
                );
            }
            let Some(position) = projects
                .iter()
                .position(|project| project.terminal.to_string() == id)
            else {
                return if closed.contains(&id) {
                    crate::ctl::Response::ok(
                        serde_json::json!({ "id": id, "last": false, "already": true }),
                    )
                } else {
                    crate::ctl::Response::error(2, "bad request: unknown terminal")
                };
            };
            let last = projects.len() == 1;
            if last {
                if !force {
                    return crate::ctl::Response::error(
                        3,
                        "refused: would end session; confirm --force",
                    );
                }
                *quit = true;
            } else {
                // The tombstone lands inside `request_close` (#121), the
                // same primitive keyboard and mouse closes share.
                request_close(engine, projects, deck, position, closed);
            }
            crate::ctl::Response::ok(serde_json::json!({ "id": id, "last": last }))
        }
        crate::ctl::Control::Promote { id } => {
            let Some(project) = projects
                .iter()
                .find(|project| project.terminal.to_string() == id)
            else {
                return crate::ctl::Response::error(2, "bad request: unknown terminal");
            };
            deck.apply(
                &crate::contracts::ActionCommand::Promote(project.terminal.clone()),
                projects,
                now(),
            );
            crate::ctl::Response::ok(serde_json::json!({ "id": id, "master": true }))
        }
        crate::ctl::Control::Zoom { on } => {
            if on.is_none_or(|on| on != deck.zoomed()) {
                deck.apply(
                    &crate::contracts::ActionCommand::ToggleZoom,
                    projects,
                    now(),
                );
            }
            crate::ctl::Response::ok(serde_json::json!({ "zoom": deck.zoomed() }))
        }
        crate::ctl::Control::Input {
            id,
            bytes,
            force,
            paste,
        } => {
            if !allow_input && !force {
                return crate::ctl::Response::error(3, "refused: input is disabled; use --force");
            }
            let Some(project) = projects
                .iter()
                .find(|project| project.terminal.to_string() == id)
            else {
                return if closed.contains(&id) {
                    crate::ctl::Response::error(3, "refused: terminal is closed")
                } else {
                    crate::ctl::Response::error(2, "bad request: unknown terminal")
                };
            };
            match engine.status(&project.terminal) {
                Some(TerminalStatus::Starting | TerminalStatus::Running) => {}
                Some(TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }) => {
                    return crate::ctl::Response::error(3, "refused: terminal is not live");
                }
                None => {
                    return crate::ctl::Response::error(1, "terminal is unavailable");
                }
            }
            // Agent `paste` stays a paste operation end to end (#120); the
            // wire is unchanged, only the internal flag distinguishes it.
            let bytes = if paste {
                let text = String::from_utf8(bytes)
                    .expect("ctl input bytes are built from UTF-8 request text");
                match encode_paste(engine, &project.terminal, text) {
                    Ok(bytes) => bytes,
                    Err(PasteEncodeError::EmbeddedCloser) => {
                        return crate::ctl::Response::error(
                            3,
                            "refused: paste payload contains a bracketed-paste closer",
                        );
                    }
                }
            } else {
                bytes
            };
            let events = dispatch_live_input(engine, project.terminal.clone(), bytes);
            // Truthful acknowledgement (#118): an accepted input is `ok` as
            // before, but a saturated queue refuses instead of hanging, and
            // the caller learns the bytes went nowhere.
            if events
                .iter()
                .any(|event| matches!(event, EngineEvent::InputDropped { .. }))
            {
                return crate::ctl::Response::error(
                    3,
                    "refused: terminal input queue is full; child is not reading",
                );
            }
            let queued = events
                .iter()
                .any(|event| matches!(event, EngineEvent::InputQueued { .. }));
            crate::ctl::Response::ok(serde_json::json!({ "id": id, "queued": queued }))
        }
    }
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
            shell_hook: true,
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
    // Bind after the outer terminal is active, then pass this inner-session
    // rendezvous to every child PTY.  Explicit CommandBuilder values replace
    // any outer TERMDECK_* values for nested sessions.
    #[cfg(unix)]
    let mut control = crate::ctl::Listener::bind()?;
    #[cfg(unix)]
    let allow_input = std::env::var("TERMDECK_ALLOW_INPUT").is_ok_and(|value| value == "1");
    let mut size = screen_size()?;
    // The workspace opened this list; `^g a` can lengthen it, so the session
    // owns it from here (#50 A3).
    let mut projects = workspace.projects.clone();
    // The configuration seeds the split; the divider owns it from there.
    let mut deck = DeckState::new(projects.len()).with_master_ratio(workspace.master_ratio.get());
    // What the terminals have asked for and the user has not seen (#97). It
    // lives beside the deck because it outlives every one of them: a bell is
    // still pending after the promotion, the resize and the fold that follow.
    let mut notifies = Notifications::new();
    #[cfg(unix)]
    let mut engine =
        spawn_terminals_with_socket(&projects, &deck, size, workspace.scrollback, control.path())
            .map_err(|error| format!("cannot start workspace '{}': {error}", workspace.name))?;
    #[cfg(not(unix))]
    let mut engine = spawn_terminals_with_scrollback(&projects, &deck, size, workspace.scrollback)
        .map_err(|error| format!("cannot start workspace '{}': {error}", workspace.name))?;
    // The engine begins with safe all-terminal timing visibility. Replace it
    // before the first drain with the panes this initial deck actually draws.
    resize_terminals(&mut engine, &projects, &deck, size);
    let mut terminal = Terminal::new(AnsiBackend::new()?)?;
    let mut input = Input::new(size.rows.saturating_sub(4));
    let mut keys = KeyReader::default();
    let mut last_click = None;
    // The preview whose disclosure marker is being pressed, if any.
    let mut marker_press: Option<usize> = None;
    // The pane whose close affordance is being pressed, if any (#84).
    let mut close_press: Option<usize> = None;
    // The pointer's text-selection gesture, and what it last copied (#148).
    // The copy goes to the host's clipboard by OSC 52, which no host
    // acknowledges and some refuse, so the session keeps the text: `^g v`
    // pastes it back into the active terminal.
    let mut selecting = Selecting::default();
    let mut copied = String::new();
    // The runtime-add sheet, while it is open. It owns every key it sees.
    let mut sheet: Option<SheetState> = None;
    // Closed pane identities, tombstoned on every close path (#121).
    let mut closed = BTreeSet::new();
    // Every identity ever allocated this session, open or closed (#121):
    // the allocator draws from this set, so a name is never handed out
    // twice and a stale id cannot resolve to a different live terminal.
    let mut used: BTreeSet<String> = projects
        .iter()
        .map(|project| project.terminal.to_string())
        .collect();
    let browser = FsBrowse::new(crate::cli::picker_roots());
    let roots = Browse::roots(&browser);

    let mut dirty = true;
    // A transient can become invisible without a new key or PTY event. Keep
    // one remembered active pass so crossing its deadline produces the final
    // frame that removes it, then go idle again.
    let mut expiry_repaint = false;
    'session: loop {
        if SIGNAL.swap(0, Ordering::SeqCst) != 0 {
            break;
        }
        let current_size = screen_size()?;
        if current_size != size {
            // The children reflow, so a selection's cells now hold something
            // else (#148).
            deck.clear_selection();
            resize_terminals(&mut engine, &projects, &deck, current_size);
            input.set_page(current_size.rows.saturating_sub(4));
            size = current_size;
            terminal.autoresize()?;
            dirty = true;
        }
        for event in engine.drain_events() {
            dirty = true;
            // A bell is the untaught tool's notification (#97). The master
            // frame is on screen in every layout, so `record` drops its own.
            if let crate::contracts::EngineEvent::Notify { terminal, kind } = event {
                notifies.record(&terminal, master_terminal(&deck, &projects), kind, now());
            }
        }
        dirty |= input.expire(&mut deck, &projects, now());
        dirty |= schedule_expiry_repaint(&mut expiry_repaint, &deck, &notifies, now());
        #[cfg(unix)]
        {
            let mut quit = false;
            let socket = control.path().to_path_buf();
            dirty |= control.poll_with(|request, caller| {
                dispatch_control(
                    request,
                    caller,
                    workspace,
                    &mut projects,
                    &mut deck,
                    &mut engine,
                    &mut notifies,
                    size,
                    sheet.is_some(),
                    &socket,
                    allow_input,
                    &mut closed,
                    &mut used,
                    &mut quit,
                )
            })?;
            if quit {
                break 'session;
            }
        }

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
                        let mut failed = Vec::new();
                        for project in chosen(open_sheet, &mut used) {
                            #[cfg(unix)]
                            let result = add_terminal_with_socket(
                                &mut engine,
                                project.clone(),
                                &projects,
                                &deck,
                                size,
                                control.path(),
                            );
                            #[cfg(not(unix))]
                            let result =
                                add_terminal(&mut engine, project.clone(), &projects, &deck, size);
                            match result {
                                Ok(()) => {
                                    projects.push(project);
                                    deck.push_terminal();
                                }
                                // A terminal that will not start is not worth
                                // ending the session over; the rest still do.
                                // It is worth saying so, though: dropping the
                                // error made an ignored selection and a
                                // process that could not start look exactly
                                // alike (#128).
                                Err(error) => failed.push((project.terminal, error)),
                            }
                        }
                        if let Some((head, summary)) = add_failure(&failed) {
                            notifies.notice(head, summary, now());
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
                    // The user has moved on, and typing changes what is under
                    // a standing highlight (#148).
                    deck.clear_selection();
                    deck.set_resizing(false);
                    let was_scrollback = deck.scrollback();
                    let application_cursor = deck
                        .active()
                        .and_then(|active| projects.get(active))
                        .and_then(|project| engine.metadata(&project.terminal))
                        .is_some_and(|metadata| metadata.application_cursor);
                    let reaction = input.press_with_application_cursor(
                        key,
                        &mut deck,
                        &projects,
                        now(),
                        application_cursor,
                    );
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
                        // Returning to the tail is a scroll like any other,
                        // so the bar stays for its window and shows the thumb
                        // snap to the bottom (#115).
                        deck.mark_scrolled(active, now());
                    }
                    // Esc and any outer-interface command clear the batch;
                    // typing into the master does not, because the toast is
                    // not in the way of it and settles by itself (#97).
                    if key == Key::Escape || !matches!(reaction, Some(Reaction::Send(_))) {
                        notifies.dismiss(now());
                    }
                    match reaction {
                        Some(Reaction::Send(UserCommand::Input(command))) => {
                            if let Some(active) = deck.active() {
                                // A keyboard paste stays a paste operation:
                                // bracketed while the child holds DEC 2004
                                // (#120), raw typed bytes otherwise.
                                let bytes = match command {
                                    crate::contracts::InputCommand::Bytes(bytes) => bytes,
                                    crate::contracts::InputCommand::Paste(text) => {
                                        let Ok(bytes) =
                                            encode_paste(&engine, &projects[active].terminal, text)
                                        else {
                                            continue;
                                        };
                                        bytes
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
                                deck.mark_scrolled(active, now());
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
                                    notifies: &notifies,
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
                                request_close(
                                    &mut engine,
                                    &mut projects,
                                    &mut deck,
                                    active,
                                    &mut closed,
                                );
                            }
                        }
                        // The pointer's copy, back out (#148). It is a
                        // paste operation like any other, so it is bracketed
                        // for a child that holds DEC 2004 (#120).
                        Some(Reaction::PasteCopy) => {
                            if let Some(active) = deck.active()
                                && !copied.is_empty()
                            {
                                let Ok(bytes) = encode_paste(
                                    &engine,
                                    &projects[active].terminal,
                                    copied.clone(),
                                ) else {
                                    continue;
                                };
                                dispatch_live_input(
                                    &mut engine,
                                    projects[active].terminal.clone(),
                                    bytes,
                                );
                            }
                        }
                        Some(Reaction::Quit) => break 'session,
                        None => {}
                    }
                }
                InputEvent::Wheel { pointer, command } => {
                    // The renderer borrows the deck while the wheel is being
                    // routed, so what it armed is applied once that is done.
                    let mut scrolled = None;
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
                    close_press = None;
                    // The content scrolls out from under the cells (#148).
                    deck.clear_selection();
                    deck.set_resizing(false);
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let pane = Deck {
                            workspace: &workspace.name,
                            projects: &projects,
                            state: &deck,
                            notifies: &notifies,
                            master_ratio: deck.master_ratio(),
                            now: now(),
                        };
                        let (pointed, terminal, list) = {
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
                            (pointed, terminal, list)
                        };
                        if let Some(terminal) = terminal {
                            // A full-screen app owns its grid: termdeck
                            // scrollback is inert there, so the wheel belongs
                            // to the app (#74). Line terminals keep the #25
                            // scrollback below.
                            match route_wheel(engine.metadata(&terminal), command) {
                                WheelRoute::Scrollback(command) => {
                                    dispatch_scroll(&mut engine, terminal, command);
                                    // The pane under the pointer, not the
                                    // master: wheeling a preview raises no
                                    // bar on the pane that has the frame
                                    // (#115).
                                    if let Some(position) = pointed {
                                        scrolled = Some(position);
                                    }
                                }
                                WheelRoute::App {
                                    up,
                                    mouse,
                                    application_cursor,
                                } => {
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
                                        bytes: app_wheel(
                                            up,
                                            column,
                                            row,
                                            mouse,
                                            application_cursor,
                                        ),
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
                    if let Some(position) = scrolled {
                        deck.mark_scrolled(position, now());
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
                    selecting.step(action, None, None);
                    mouse_action(&mut deck, None, action, now(), &mut last_click);
                }
                InputEvent::Mouse { pointer, action } => {
                    // A click anywhere is the batch read and answered.
                    if action == MouseAction::Up {
                        notifies.dismiss(now());
                    }
                    // Any press ends the selection before it: the next
                    // gesture starts here (#148).
                    if action == MouseAction::Down {
                        deck.clear_selection();
                    }
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let (marker, close, divider, split, position) = {
                            let pane = Deck {
                                workspace: &workspace.name,
                                projects: &projects,
                                state: &deck,
                                notifies: &notifies,
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
                                notifies: &notifies,
                                master_ratio: deck.master_ratio(),
                                now: now(),
                            };
                            // The release that ends a selection drag is not
                            // a click on the bar, wherever the pointer has
                            // wandered to by then (#148).
                            action == MouseAction::Up
                                && selecting.pane().is_none()
                                && bar.add_at(area, pointer)
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
                                    request_close(
                                        &mut engine,
                                        &mut projects,
                                        &mut deck,
                                        pressed,
                                        &mut closed,
                                    );
                                }
                                true
                            }
                            _ => false,
                        };
                        if closing {
                            continue;
                        }
                        // Where the press landed decides what the drag is:
                        // on pane content it selects text, on the title row
                        // or the border it reorders panes as it always did
                        // (#148). A press released without moving off its
                        // cell selects nothing, so the double-click that
                        // promotes a pane is untouched.
                        let step = {
                            let pane = Deck {
                                workspace: &workspace.name,
                                projects: &projects,
                                state: &deck,
                                notifies: &notifies,
                                master_ratio: deck.master_ratio(),
                                now: now(),
                            };
                            let content = pane.selection_cell(&engine, area, pointer);
                            let head = selecting
                                .pane()
                                .or(content.map(|(position, _, _)| position))
                                .and_then(|position| {
                                    pane.selection_head(&engine, area, position, pointer)
                                });
                            selecting.step(action, content, head)
                        };
                        match step {
                            SelectStep::Extend(selection) => {
                                deck.cancel_drag();
                                last_click = None;
                                marker_press = None;
                                close_press = None;
                                deck.set_selection(selection);
                                continue;
                            }
                            SelectStep::Copy => {
                                deck.cancel_drag();
                                last_click = None;
                                if let Some(text) =
                                    deck.selection(now()).as_ref().and_then(|selection| {
                                        selection_text(&engine, &projects, selection)
                                    })
                                {
                                    // Fire-and-forget: the host may refuse
                                    // OSC 52 and never says so, so the copy
                                    // is kept for `^g v` either way.
                                    let mut out = io::stdout();
                                    let _ = out.write_all(&clipboard_sequence(&text));
                                    let _ = out.flush();
                                    copied = text;
                                    // The highlight now reports a finished
                                    // copy rather than a gesture in progress,
                                    // so it starts its own countdown (#150).
                                    deck.mark_copied(now());
                                } else {
                                    // A drag over blank cells copies nothing
                                    // and leaves no highlight behind.
                                    deck.clear_selection();
                                }
                                continue;
                            }
                            SelectStep::Pass => {}
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
                    deck.clear_selection();
                    deck.set_resizing(false);
                    if let Some(active) = deck.active()
                        && deck.modal().is_none()
                        && !deck.scrollback()
                    {
                        // The outer paste stays a paste operation: bracketed
                        // while the child holds DEC 2004 (#120).
                        if let Ok(bytes) = encode_paste(&engine, &projects[active].terminal, text) {
                            dispatch_live_input(
                                &mut engine,
                                projects[active].terminal.clone(),
                                bytes,
                            );
                        }
                    }
                }
            }
        }
        // The master frame is the pane the user is looking at, so whatever it
        // was asking for has been seen. Every road to promotion passes here,
        // and none of them needs a clock (#97).
        if let Some(terminal) = master_terminal(&deck, &projects) {
            dirty |= notifies.clear(terminal);
        }
        dirty |= resize_terminals(&mut engine, &projects, &deck, size);
        if dirty {
            terminal.draw(|frame| {
                Deck {
                    workspace: &workspace.name,
                    projects: &projects,
                    state: &deck,
                    notifies: &notifies,
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

/// Continues repainting while a time-bound visual is active, then requests
/// exactly one final frame once it expires. Both state objects remain pure:
/// fixtures control their clocks through the `now` supplied here.
///
/// Three visuals end on a clock rather than on an event — the demotion
/// highlight, a notification's flash and toast, and the scrollbar that hides
/// itself after its window (#115). All three want the same thing from the
/// loop, and all three were getting it in different amounts: the scrollbar
/// asked only "am I up?", which keeps drawing while the bar is there and
/// then stops one pass too early, leaving the bar on screen until something
/// unrelated redrew (#132). The edge is what matters, so it is asked once,
/// for all of them.
fn schedule_expiry_repaint(
    was_active: &mut bool,
    deck: &DeckState,
    notifies: &Notifications,
    now: Timestamp,
) -> bool {
    let active = deck.demoted(now).is_some()
        || deck.scrolling(now).is_some()
        || deck.selection_expiring(now)
        || notifies.settling(now);
    let repaint = active || *was_active;
    *was_active = active;
    repaint
}

/// The terminal holding the master frame, which every layout draws.
fn master_terminal<'a>(deck: &DeckState, projects: &'a [Project]) -> Option<&'a TerminalId> {
    deck.active()
        .and_then(|position| projects.get(position))
        .map(|project| &project.terminal)
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
