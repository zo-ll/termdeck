use super::*;

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
    NativeEngine::spawn_sized(projects, &sizes)
}

pub(super) fn resize_terminals(
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

/// Closes one pane (#84): the engine ends its shell the way a confirmed quit
/// does, the session drops its identity, and the deck reflows around what is
/// left — promoting a new master when the closed pane held the frame.
///
/// The project list and the deck are renumbered together, so a pane's number
/// stays its place in the live list and `^g N` never points at a terminal
/// that has gone. An empty list afterwards is the caller's cue to end the
/// session; nothing here decides that.
pub(super) fn close_terminal(
    engine: &mut NativeEngine,
    projects: &mut Vec<Project>,
    deck: &mut DeckState,
    position: usize,
) -> bool {
    let Some(project) = projects.get(position) else {
        return false;
    };
    engine.close(&project.terminal);
    projects.remove(position);
    deck.close(position)
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
        master_ratio: deck.master_ratio(),
        now: now(),
    }
    .terminal_sizes(ratatui::layout::Rect::new(0, 0, size.columns, size.rows))
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
