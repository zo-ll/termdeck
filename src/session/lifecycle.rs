use super::*;
use std::path::Path;

#[cfg(any(test, not(unix)))]
pub(super) fn spawn_terminals(
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
    NativeEngine::spawn_sized(projects, &sizes, crate::contracts::DEFAULT_SCROLLBACK)
}

#[cfg(not(unix))]
pub(super) fn spawn_terminals_with_scrollback(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
    scrollback: usize,
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
    NativeEngine::spawn_sized(projects, &sizes, scrollback)
}

pub(super) fn spawn_terminals_with_socket(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
    scrollback: usize,
    socket: &Path,
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
    NativeEngine::spawn_sized_with_socket(projects, &sizes, scrollback, socket)
}

pub(super) fn resize_terminals(
    engine: &mut dyn TerminalEngine,
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> bool {
    let sizes = terminal_sizes(projects, deck, size);
    engine.dispatch(EngineCommand::SetTimingVisibility {
        terminals: timing_terminals(projects, deck, size).into_iter().collect(),
    });

    let mut resized = false;
    for (project, size) in projects.iter().zip(sizes) {
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

#[cfg(any(test, not(unix)))]
pub(super) fn add_terminal(
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

pub(super) fn add_terminal_with_socket(
    engine: &mut NativeEngine,
    project: Project,
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
    socket: &Path,
) -> Result<(), String> {
    let size = terminal_sizes(projects, deck, size)
        .into_iter()
        .flatten()
        .next()
        .unwrap_or(ScreenSize::new(1, 1));
    engine.add_with_socket(project, size, socket)
}

/// Closes one pane (#84): the engine ends its shell the way a confirmed quit
/// does, the session drops its identity, and the deck reflows around what is
/// left — promoting a new master when the closed pane held the frame.
///
/// The project list and the deck are renumbered together, so a pane's number
/// stays its place in the live list and `^g N` never points at a terminal
/// that has gone. An empty list afterwards is the caller's cue to end the
/// session; nothing here decides that.
///
/// The removed identity is tombstoned (#121): runtime ids are never reused
/// in a session, so a stale id held by an agent or retry can never resolve
/// to a different live terminal afterwards. Every close flows through here
/// — keyboard, mouse, and API alike — so the tombstone set is consistent
/// by construction rather than by per-site discipline.
pub(super) fn close_terminal(
    engine: &mut NativeEngine,
    projects: &mut Vec<Project>,
    deck: &mut DeckState,
    position: usize,
    closed: &mut BTreeSet<String>,
) -> bool {
    let Some(project) = projects.get(position) else {
        return false;
    };
    closed.insert(project.terminal.to_string());
    engine.close(&project.terminal);
    projects.remove(position);
    deck.close(position)
}

/// The close gesture, wherever it comes from: `^g x` or a pane's `×`.
///
/// The last pane is the session, so closing it asks the confirmation `^g q`
/// asks rather than ending the run outright — same modal, same `y`/`n`/`esc`,
/// and the exit is the quit path's own. Cancelling leaves the pane and its
/// shell exactly as they were, because nothing has been closed yet: the
/// question is asked before the engine is touched.
///
/// Anything else closes at once, since the deck it leaves behind still holds
/// a terminal. Returns whether a terminal was closed, which the confirmation
/// never is.
pub(super) fn request_close(
    engine: &mut NativeEngine,
    projects: &mut Vec<Project>,
    deck: &mut DeckState,
    position: usize,
    closed: &mut BTreeSet<String>,
) -> bool {
    if projects.get(position).is_none() {
        return false;
    }
    if projects.len() == 1 {
        deck.apply(
            &crate::contracts::ActionCommand::RequestQuit,
            projects,
            now(),
        );
        return false;
    }
    close_terminal(engine, projects, deck, position, closed)
}

/// The renderer owns pane geometry, so PTYs always receive precisely the
/// dimensions their applications can see.
pub(super) fn terminal_sizes(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> Vec<Option<ScreenSize>> {
    Deck {
        workspace: "",
        projects,
        state: deck,
        // Geometry only: what is pending changes no pane's dimensions.
        notifies: &crate::ui::Notifications::new(),
        master_ratio: deck.master_ratio(),
        now: now(),
    }
    .terminal_sizes(ratatui::layout::Rect::new(0, 0, size.columns, size.rows))
}

/// The deck decides which terminal metadata it renders. This is separate from
/// PTY geometry because a folded strip draws an idle age without a viewport.
pub(super) fn timing_terminals(
    projects: &[Project],
    deck: &DeckState,
    size: ScreenSize,
) -> Vec<TerminalId> {
    Deck {
        workspace: "",
        projects,
        state: deck,
        notifies: &crate::ui::Notifications::new(),
        master_ratio: deck.master_ratio(),
        now: now(),
    }
    .timing_terminals(ratatui::layout::Rect::new(0, 0, size.columns, size.rows))
}

pub(super) fn screen_size() -> io::Result<ScreenSize> {
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
