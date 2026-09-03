//! The interactive composition root: terminal mode, event loop, UI, and PTYs.

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
                        Some(Reaction::Quit) => break 'session,
                        None => {}
                    }
                }
                InputEvent::Wheel { pointer, command } => {
                    deck.cancel_drag();
                    last_click = None;
                    marker_press = None;
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
                    mouse_action(&mut deck, None, action, now(), &mut last_click);
                }
                InputEvent::Mouse { pointer, action } => {
                    if deck.modal().is_none() {
                        let area = ratatui::layout::Rect::new(0, 0, size.columns, size.rows);
                        let (marker, divider, split, position) = {
                            let pane = Deck {
                                workspace: &workspace.name,
                                projects: &projects,
                                state: &deck,
                                master_ratio: deck.master_ratio(),
                                now: now(),
                            };
                            (
                                pane.marker_at(area, pointer),
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
                            deck.set_resizing(action != MouseAction::Up);
                            if action != MouseAction::Down
                                && let Some(split) = split
                            {
                                deck.set_master_ratio(split);
                            }
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

/// Live shell input always resumes the active terminal at its tail. Wheel
/// scrolling deliberately has no modal state, unlike keyboard scrollback.
fn dispatch_live_input(
    engine: &mut dyn TerminalEngine,
    terminal: crate::contracts::TerminalId,
    bytes: Vec<u8>,
) {
    dispatch_scroll(engine, terminal.clone(), ScrollCommand::Bottom);
    engine.dispatch(EngineCommand::Input { terminal, bytes });
}

fn dispatch_scroll(engine: &mut dyn TerminalEngine, terminal: TerminalId, command: ScrollCommand) {
    engine.dispatch(EngineCommand::Scroll { terminal, command });
}

/// What a wheel tick over a pane becomes.
#[derive(Clone, Debug, Eq, PartialEq)]
enum WheelRoute {
    /// A line terminal: the engine-owned #25 scrollback viewport.
    Scrollback(ScrollCommand),
    /// An alternate-screen app: bytes for its PTY. `up` is the tick
    /// direction and `mouse` whether the app enabled mouse reporting.
    App { up: bool, mouse: bool },
}

/// Routes a wheel tick: alternate-screen apps scroll natively, everything
/// else keeps termdeck scrollback. Unknown terminals keep the old behavior.
fn route_wheel(metadata: Option<&TerminalMetadata>, command: ScrollCommand) -> WheelRoute {
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
fn app_wheel(up: bool, column: u16, row: u16, mouse: bool) -> Vec<u8> {
    if mouse {
        format!("\x1b[{};{column};{row}M", if up { "<64" } else { "<65" }).into_bytes()
    } else {
        let arrow = if up { b"\x1b[A" } else { b"\x1b[B" };
        arrow.repeat(usize::from(WHEEL_LINES))
    }
}

fn spawn_terminals(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> Result<NativeEngine, String> {
    let sizes = terminal_sizes(projects, deck, size);
    let fallback = sizes
        .iter()
        .flatten()
        .copied()
        .next()
        .unwrap_or(ScreenSize::new(1, 1));
    let sizes = sizes
        .into_iter()
        .map(|size| size.unwrap_or(fallback))
        .collect::<Vec<_>>();
    NativeEngine::spawn_sized(projects, &sizes)
}

fn resize_terminals(
    engine: &mut dyn TerminalEngine,
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> bool {
    let mut resized = false;
    for (project, size) in projects.iter().zip(terminal_sizes(projects, deck, size)) {
        let Some(size) = size else {
            continue;
        };
        if engine
            .frame(&project.terminal)
            .is_some_and(|frame| frame.size == size)
        {
            continue;
        }
        engine.dispatch(EngineCommand::Resize {
            terminal: project.terminal.clone(),
            size,
        });
        resized = true;
    }
    resized
}

fn add_terminal(
    engine: &mut NativeEngine,
    project: Project,
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> Result<(), String> {
    let size = terminal_sizes(projects, deck, size)
        .into_iter()
        .flatten()
        .next()
        .unwrap_or(ScreenSize::new(1, 1));
    engine.add(project, size)
}

/// The renderer owns pane geometry, so PTYs always receive precisely the
/// dimensions their applications can see.
fn terminal_sizes(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> Vec<Option<ScreenSize>> {
    Deck {
        workspace: "",
        projects,
        state: deck,
        master_ratio: deck.master_ratio(),
        now: now(),
    }
    .terminal_sizes(ratatui::layout::Rect::new(0, 0, size.columns, size.rows))
}

fn screen_size() -> io::Result<ScreenSize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: `size` points at valid writable storage for the ioctl result.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: TIOCGWINSZ initialized the value after the successful ioctl.
    let size = unsafe { size.assume_init() };
    if size.ws_col == 0 || size.ws_row == 0 {
        return Err(io::Error::other("terminal has no usable cell dimensions"));
    }
    Ok(ScreenSize::new(size.ws_col, size.ws_row))
}

struct OuterTerminal;

impl OuterTerminal {
    fn enter() -> io::Result<Self> {
        let mut previous = std::mem::MaybeUninit::<libc::termios>::zeroed();
        // SAFETY: stdin is a valid descriptor and `previous` is writable.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, previous.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let previous = unsafe { previous.assume_init() };
        let mut raw = previous;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: stdin is a valid descriptor and `raw` is a valid termios value.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        *saved_termios()
            .lock()
            .expect("terminal state lock poisoned") = Some(previous);
        let mut stdout = io::stdout();
        if let Err(error) =
            stdout.write_all(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h")
        {
            restore_outer_terminal();
            return Err(error);
        }
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for OuterTerminal {
    fn drop(&mut self) {
        restore_outer_terminal();
    }
}

fn saved_termios() -> &'static Mutex<Option<libc::termios>> {
    SAVED_TERMIOS.get_or_init(|| Mutex::new(None))
}

fn saved_panic_hook() -> &'static Mutex<Option<PanicHook>> {
    SAVED_PANIC_HOOK.get_or_init(|| Mutex::new(None))
}

fn restore_outer_terminal() {
    let _ =
        io::stdout().write_all(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?25h\x1b[?1049l");
    let _ = io::stdout().flush();
    if let Some(previous) = saved_termios()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take())
    {
        // SAFETY: `previous` came from tcgetattr for this process's stdin.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &previous) };
    }
}

struct PanicGuard;

impl PanicGuard {
    fn install() -> Self {
        let previous = panic::take_hook();
        *saved_panic_hook().lock().expect("panic hook lock poisoned") = Some(previous);
        panic::set_hook(Box::new(|info| {
            restore_outer_terminal();
            if let Ok(hooks) = saved_panic_hook().lock()
                && let Some(previous) = hooks.as_ref()
            {
                previous(info);
            }
        }));
        Self
    }
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        let _ = panic::take_hook();
        if let Some(previous) = saved_panic_hook()
            .lock()
            .ok()
            .and_then(|mut hook| hook.take())
        {
            panic::set_hook(previous);
        }
    }
}

extern "C" fn caught_signal(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

struct SignalGuard {
    #[cfg(unix)]
    previous_int: libc::sigaction,
    #[cfg(unix)]
    previous_term: libc::sigaction,
}

impl SignalGuard {
    fn install() -> io::Result<Self> {
        // SAFETY: the handler only performs an atomic store, which is signal-safe.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = caught_signal as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous_int = std::mem::zeroed();
            let mut previous_term = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, &action, &mut previous_int) != 0
                || libc::sigaction(libc::SIGTERM, &action, &mut previous_term) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                previous_int,
                previous_term,
            })
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: these are the handlers captured by `install`.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous_int, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.previous_term, std::ptr::null_mut());
        }
    }
}

#[derive(Default)]
struct KeyReader {
    bytes: Vec<u8>,
}

enum InputEvent {
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
enum MouseAction {
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

fn mouse_action(
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
    fn read(&mut self, timeout: Duration) -> io::Result<Vec<InputEvent>> {
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

    fn decode(&mut self, flush_escape: bool) -> Vec<InputEvent> {
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
                b'\r' | b'\n' => Key::Enter,
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

struct AnsiBackend {
    output: io::Stdout,
    cursor: Position,
}

impl AnsiBackend {
    fn new() -> io::Result<Self> {
        Ok(Self {
            output: io::stdout(),
            cursor: Position::ORIGIN,
        })
    }
    fn cursor_to(&mut self, x: u16, y: u16) -> io::Result<()> {
        write!(self.output, "\x1b[{};{}H", y + 1, x + 1)
    }
}

impl Backend for AnsiBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            self.cursor_to(x, y)?;
            write!(
                self.output,
                "\x1b[0m{}{}{}",
                colour(cell.fg, true),
                colour(cell.bg, false),
                modifiers(cell.modifier)
            )?;
            self.output.write_all(cell.symbol().as_bytes())?;
            self.cursor = Position { x, y };
        }
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[?25l")
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[?25h")
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.cursor_to(position.x, position.y)?;
        self.cursor = position;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.output.write_all(b"\x1b[2J\x1b[H")
    }
    fn size(&self) -> io::Result<Size> {
        let size = screen_size()?;
        Ok(Size::new(size.columns, size.rows))
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size()?,
            pixels: Size::ZERO,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

fn modifiers(modifier: Modifier) -> &'static str {
    if modifier.contains(Modifier::BOLD) {
        "\x1b[1m"
    } else if modifier.contains(Modifier::DIM) {
        "\x1b[2m"
    } else if modifier.contains(Modifier::ITALIC) {
        "\x1b[3m"
    } else if modifier.contains(Modifier::UNDERLINED) {
        "\x1b[4m"
    } else if modifier.contains(Modifier::REVERSED) {
        "\x1b[7m"
    } else {
        ""
    }
}

fn colour(colour: Color, foreground: bool) -> String {
    let base = if foreground { 30 } else { 40 };
    match colour {
        Color::Reset => if foreground { "\x1b[39m" } else { "\x1b[49m" }.to_owned(),
        Color::Black => format!("\x1b[{base}m"),
        Color::Red => format!("\x1b[{}m", base + 1),
        Color::Green => format!("\x1b[{}m", base + 2),
        Color::Yellow => format!("\x1b[{}m", base + 3),
        Color::Blue => format!("\x1b[{}m", base + 4),
        Color::Magenta => format!("\x1b[{}m", base + 5),
        Color::Cyan => format!("\x1b[{}m", base + 6),
        Color::Gray => format!("\x1b[{}m", base + 7),
        Color::DarkGray => format!("\x1b[{}m", base + 60),
        Color::LightRed => format!("\x1b[{}m", base + 61),
        Color::LightGreen => format!("\x1b[{}m", base + 62),
        Color::LightYellow => format!("\x1b[{}m", base + 63),
        Color::LightBlue => format!("\x1b[{}m", base + 64),
        Color::LightMagenta => format!("\x1b[{}m", base + 65),
        Color::LightCyan => format!("\x1b[{}m", base + 66),
        Color::White => format!("\x1b[{}m", base + 67),
        Color::Rgb(red, green, blue) => format!(
            "\x1b[{};2;{red};{green};{blue}m",
            if foreground { 38 } else { 48 }
        ),
        Color::Indexed(index) => format!("\x1b[{};5;{index}m", if foreground { 38 } else { 48 }),
    }
}

#[cfg(test)]
mod tests {
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
            bytes: "a\x03\x1b[A\x1b[200~paste\x1b[201~\x1b[<64;3;5M\x1b[<0;4;6M\x1b[<32;5;6M\x1b[<0;5;6m界"
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
}
