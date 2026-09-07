use std::{
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    contracts::{
        Elapsed, EngineCommand, EngineEvent, NotifyKind, ProcessInfo, Project, ScreenSize,
        TerminalEngine, TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
    },
    engine::{InputOutcome, PtyEvent, PtyTransport, VtFrameAdapter},
};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Metadata ages independently of PTY output. One update per second is enough
/// for the displayed elapsed values while avoiding a redraw on every input
/// poll for an otherwise quiet workspace.
const TIMING_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const NOTIFY_PREFIX: &[u8] = b"\x1b]7777;termdeck;finished;";
const NOTIFY_PAYLOAD_CAP: usize = 512;

/// Removes complete private completion OSCs while preserving every other byte.
fn scan_notify(bytes: &[u8]) -> (Vec<NotifyKind>, Vec<u8>) {
    let mut notifies = Vec::new();
    let mut passthrough = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative) = bytes[cursor..]
            .windows(NOTIFY_PREFIX.len())
            .position(|window| window == NOTIFY_PREFIX)
        else {
            passthrough.extend_from_slice(&bytes[cursor..]);
            break;
        };
        let start = cursor + relative;
        passthrough.extend_from_slice(&bytes[cursor..start]);
        let payload_start = start + NOTIFY_PREFIX.len();
        let limit = (payload_start + NOTIFY_PAYLOAD_CAP + 2).min(bytes.len());
        let mut end = None;
        let mut next = payload_start;
        while next < limit {
            if bytes[next] == b'\x07' {
                end = Some((next, next + 1));
                break;
            }
            if bytes[next] == b'\x1b' && bytes.get(next + 1) == Some(&b'\\') {
                end = Some((next, next + 2));
                break;
            }
            next += 1;
        }
        let Some((payload_end, after)) = end else {
            passthrough.extend_from_slice(&bytes[start..]);
            break;
        };
        if let Some(kind) = parse_notify(&bytes[payload_start..payload_end]) {
            notifies.push(kind);
        } else {
            passthrough.extend_from_slice(&bytes[start..after]);
        }
        cursor = after;
    }
    (notifies, passthrough)
}

fn scan_notify_chunk(carry: &mut Vec<u8>, bytes: &[u8]) -> (Vec<NotifyKind>, Vec<u8>) {
    carry.extend_from_slice(bytes);
    let split = notify_partial_start(carry).unwrap_or(carry.len());
    let tail = carry.split_off(split);
    let (notifies, passthrough) = scan_notify(carry);
    *carry = tail;
    (notifies, passthrough)
}

/// Holds only a suffix which could still be our OSC. Once it exceeds the
/// bounded payload it is passed through unchanged on the next scan.
fn notify_partial_start(bytes: &[u8]) -> Option<usize> {
    if let Some(start) = bytes
        .windows(NOTIFY_PREFIX.len())
        .rposition(|window| window == NOTIFY_PREFIX)
    {
        let tail = &bytes[start..];
        if tail.len() <= NOTIFY_PREFIX.len() + NOTIFY_PAYLOAD_CAP + 1
            && !tail.windows(2).any(|pair| pair == b"\x1b\\")
            && !tail.contains(&b'\x07')
        {
            return Some(start);
        }
    }
    let start = bytes.iter().rposition(|byte| *byte == b'\x1b')?;
    NOTIFY_PREFIX.starts_with(&bytes[start..]).then_some(start)
}

fn parse_notify(payload: &[u8]) -> Option<NotifyKind> {
    let payload = std::str::from_utf8(payload).ok()?;
    let code = payload.strip_prefix("code=")?;
    let (code, payload) = code.split_once(";secs=")?;
    let (secs, cmd) = payload.split_once(";cmd=")?;
    let code = code.parse::<i32>().ok()?;
    let secs = secs.parse::<u64>().ok()?;
    let title: String = cmd
        .chars()
        .filter(|character| !character.is_control())
        .take(48)
        .collect();
    Some(NotifyKind::Message {
        title,
        body: if code == 0 {
            format!("done · {secs}s")
        } else {
            format!("exit {code} · {secs}s")
        },
    })
}

struct NativeTerminal {
    project: Project,
    socket: Option<std::path::PathBuf>,
    transport: Option<PtyTransport>,
    adapter: VtFrameAdapter,
    frame: TerminalFrame,
    status: TerminalStatus,
    metadata: TerminalMetadata,
    started: Instant,
    scrollback: usize,
    last_output: Option<Instant>,
    notify_carry: Vec<u8>,
}

impl NativeTerminal {
    fn spawn(
        project: Project,
        size: ScreenSize,
        scrollback: usize,
        socket: Option<&Path>,
    ) -> Result<Self, String> {
        let transport = PtyTransport::spawn_with_socket(&project, size, socket)?;
        let adapter = VtFrameAdapter::new(project.terminal.clone(), size, scrollback);
        let frame = adapter.frame();
        let started = Instant::now();

        Ok(Self {
            metadata: TerminalMetadata {
                process: transport.process_id().map(|pid| ProcessInfo {
                    pid,
                    uptime: Elapsed::default(),
                }),
                ..TerminalMetadata::default()
            },
            project,
            socket: socket.map(Path::to_path_buf),
            transport: Some(transport),
            adapter,
            frame,
            status: TerminalStatus::Running,
            started,
            scrollback,
            last_output: None,
            notify_carry: Vec::new(),
        })
    }

    fn owns(&self, terminal: &TerminalId) -> bool {
        self.frame.terminal == *terminal
    }

    /// The viewport the renderer shows: history offset plus which screen
    /// the app owns, whether it wants the wheel as mouse reports (#74),
    /// and whether it wants pastes bracketed (#120).
    fn refresh_viewport(&mut self) {
        self.metadata.scrollback = self.adapter.scrollback_position();
        self.metadata.alt_screen = self.adapter.alt_screen();
        self.metadata.mouse_reporting = self.adapter.mouse_reporting();
        self.metadata.mouse_protocol = self.adapter.mouse_protocol();
        self.metadata.application_cursor = self.adapter.application_cursor();
        self.metadata.bracketed_paste = self.adapter.bracketed_paste();
    }

    fn refresh_timing_at(&mut self, observed: Instant) {
        if let Some(process) = &mut self.metadata.process {
            process.uptime = elapsed(self.started, observed);
        }
        if let Some(last_output) = self.last_output {
            self.metadata.output_idle = Some(elapsed(last_output, observed));
        }
    }

    fn refresh_timing(&mut self) {
        self.refresh_timing_at(Instant::now());
    }

    fn status_changed(&mut self, status: TerminalStatus) -> Vec<EngineEvent> {
        self.refresh_timing();
        self.status = status.clone();
        if matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        ) {
            self.metadata.last_exit_at = Some(now());
            self.metadata.process = None;
        }
        vec![
            EngineEvent::StatusChanged {
                terminal: self.frame.terminal.clone(),
                status,
            },
            EngineEvent::MetadataChanged {
                terminal: self.frame.terminal.clone(),
                metadata: self.metadata.clone(),
            },
        ]
    }

    fn flush_notify_carry(&mut self, events: &mut Vec<EngineEvent>) {
        if self.notify_carry.is_empty() {
            return;
        }
        let bytes = std::mem::take(&mut self.notify_carry);
        self.frame = self.adapter.feed(&bytes);
        self.refresh_viewport();
        events.push(EngineEvent::FrameReady(self.frame.clone()));
        events.push(EngineEvent::MetadataChanged {
            terminal: self.frame.terminal.clone(),
            metadata: self.metadata.clone(),
        });
    }

    fn handle_pty_event(&mut self, event: PtyEvent, events: &mut Vec<EngineEvent>) {
        match event {
            PtyEvent::Output { terminal, bytes } if self.owns(&terminal) => {
                let raw_len = bytes.len();
                let (notifies, bytes) = scan_notify_chunk(&mut self.notify_carry, &bytes);
                // Feeding at Alacritty's tail follows output itself; a
                // deliberate history offset remains untouched. Metadata still
                // refreshes below even when the visible cells do not change.
                self.frame = self.adapter.feed(&bytes);
                self.refresh_viewport();
                self.metadata.bytes_written =
                    self.metadata.bytes_written.saturating_add(raw_len as u64);
                self.last_output = Some(Instant::now());
                self.refresh_timing();
                events.push(EngineEvent::FrameReady(self.frame.clone()));
                events.push(EngineEvent::MetadataChanged {
                    terminal: terminal.clone(),
                    metadata: self.metadata.clone(),
                });
                // A bell in that output is the untaught tool's notification
                // (#97). One event per drain, whatever the burst carried:
                // the session keeps a single slot per terminal anyway.
                if self.adapter.take_bells() > 0 {
                    events.push(EngineEvent::Notify {
                        terminal: terminal.clone(),
                        kind: NotifyKind::Attention,
                    });
                }
                events.extend(notifies.into_iter().map(|kind| EngineEvent::Notify {
                    terminal: terminal.clone(),
                    kind,
                }));
                let replies = self.adapter.take_pty_replies();
                if !replies.is_empty()
                    && let Some(transport) = self.transport.as_mut()
                {
                    // A saturated queue drops one auto-reply rather than
                    // blocking the loop or failing the pane: the child is
                    // stuck either way, and the query the app sends after it
                    // recovers is answered then.
                    if let Err(message) = transport.write(&replies) {
                        events.extend(self.status_changed(TerminalStatus::Failed { message }));
                    }
                }
            }
            PtyEvent::StatusChanged { terminal, status } if self.owns(&terminal) => {
                self.flush_notify_carry(events);
                events.extend(self.status_changed(status));
            }
            _ => {}
        }
    }

    fn respawn(&mut self) -> Result<Vec<EngineEvent>, String> {
        if !matches!(
            self.status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        ) {
            return Ok(Vec::new());
        }
        if let Some(mut transport) = self.transport.take() {
            transport.shutdown()?;
        }

        let transport = PtyTransport::spawn_with_socket(
            &self.project,
            self.frame.size,
            self.socket.as_deref(),
        )?;
        self.adapter = VtFrameAdapter::new(
            self.project.terminal.clone(),
            self.frame.size,
            self.scrollback,
        );
        self.frame = self.adapter.frame();
        self.status = TerminalStatus::Running;
        self.metadata = TerminalMetadata {
            last_exit_at: self.metadata.last_exit_at,
            restarted_at: Some(now()),
            process: transport.process_id().map(|pid| ProcessInfo {
                pid,
                uptime: Elapsed::default(),
            }),
            ..TerminalMetadata::default()
        };
        self.transport = Some(transport);
        self.started = Instant::now();
        self.last_output = None;
        self.notify_carry.clear();

        Ok(vec![
            EngineEvent::StatusChanged {
                terminal: self.frame.terminal.clone(),
                status: self.status.clone(),
            },
            EngineEvent::MetadataChanged {
                terminal: self.frame.terminal.clone(),
                metadata: self.metadata.clone(),
            },
            EngineEvent::FrameReady(self.frame.clone()),
        ])
    }
}

/// PTY-backed terminals behind the application-owned engine contract.
///
/// Terminals remain in configured order for their entire lifetime; commands
/// select them by their stable [`TerminalId`].
pub struct NativeEngine {
    terminals: Vec<NativeTerminal>,
    scrollback: usize,
    /// One deadline for all rendered timing values, rather than one per PTY.
    last_timing_refresh: Instant,
    timing_visible: BTreeSet<TerminalId>,
}

impl NativeEngine {
    /// Starts one or more configured terminals at the requested cell dimensions.
    pub fn spawn(projects: &[Project], size: ScreenSize) -> Result<Self, String> {
        let sizes = vec![size; projects.len()];
        Self::spawn_sized(projects, &sizes, crate::contracts::DEFAULT_SCROLLBACK)
    }

    /// Starts configured terminals at their rendered viewport dimensions.
    pub fn spawn_sized(
        projects: &[Project],
        sizes: &[ScreenSize],
        scrollback: usize,
    ) -> Result<Self, String> {
        if projects.is_empty() {
            return Err("native engine requires at least one terminal".to_owned());
        }
        if projects.len() != sizes.len() {
            return Err("native engine requires one size per terminal".to_owned());
        }
        let mut ids = BTreeSet::new();
        if projects
            .iter()
            .any(|project| !ids.insert(project.terminal.clone()))
        {
            return Err("native engine terminal identities must be unique".to_owned());
        }

        let terminals = projects
            .iter()
            .cloned()
            .zip(sizes.iter().copied())
            .map(|(project, size)| NativeTerminal::spawn(project, size, scrollback, None))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            timing_visible: terminals
                .iter()
                .map(|terminal| terminal.frame.terminal.clone())
                .collect(),
            terminals,
            scrollback,
            last_timing_refresh: Instant::now(),
        })
    }

    /// Starts one more terminal in an already-running engine (#50 A3).
    ///
    /// Everything else here iterates `terminals`, so a terminal added this
    /// way answers every command, drains its events, exits and respawns like
    /// any other, and shutdown reaches it without a further thought.
    pub fn add(&mut self, project: Project, size: ScreenSize) -> Result<(), String> {
        if self
            .terminals
            .iter()
            .any(|item| item.owns(&project.terminal))
        {
            return Err(format!(
                "terminal '{}' is already running",
                project.terminal
            ));
        }
        let terminal = project.terminal.clone();
        self.terminals
            .push(NativeTerminal::spawn(project, size, self.scrollback, None)?);
        self.timing_visible.insert(terminal);
        Ok(())
    }

    /// Starts one more terminal with this session's ctl rendezvous environment.
    pub fn add_with_socket(
        &mut self,
        project: Project,
        size: ScreenSize,
        socket: &Path,
    ) -> Result<(), String> {
        if self
            .terminals
            .iter()
            .any(|item| item.owns(&project.terminal))
        {
            return Err(format!(
                "terminal '{}' is already running",
                project.terminal
            ));
        }
        let terminal = project.terminal.clone();
        self.terminals.push(NativeTerminal::spawn(
            project,
            size,
            self.scrollback,
            Some(socket),
        )?);
        self.timing_visible.insert(terminal);
        Ok(())
    }

    /// Starts a session whose children inherit its ctl rendezvous path.
    pub fn spawn_sized_with_socket(
        projects: &[Project],
        sizes: &[ScreenSize],
        scrollback: usize,
        socket: &Path,
    ) -> Result<Self, String> {
        if projects.is_empty() {
            return Err("native engine requires at least one terminal".to_owned());
        }
        if projects.len() != sizes.len() {
            return Err("native engine requires one size per terminal".to_owned());
        }
        let mut ids = BTreeSet::new();
        if projects
            .iter()
            .any(|project| !ids.insert(project.terminal.clone()))
        {
            return Err("native engine terminal identities must be unique".to_owned());
        }
        let terminals = projects
            .iter()
            .cloned()
            .zip(sizes.iter().copied())
            .map(|(project, size)| NativeTerminal::spawn(project, size, scrollback, Some(socket)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            timing_visible: terminals
                .iter()
                .map(|terminal| terminal.frame.terminal.clone())
                .collect(),
            terminals,
            scrollback,
            last_timing_refresh: Instant::now(),
        })
    }

    /// Removes one terminal from a running engine (#84): the counterpart of
    /// [`NativeEngine::add`].
    ///
    /// Dropping the terminal drops its [`PtyTransport`], whose own `Drop` is
    /// the confirmed-quit path — HUP then TERM, one grace period, SIGKILL
    /// behind it, reader and waiter joined — so a closed pane leaves no
    /// orphaned process group behind and no thread still reading a dead PTY.
    ///
    /// This is what separates a close from an exit: a shell that exits on its
    /// own keeps its terminal here, with its last frame, its status and the
    /// `^g r` respawn that goes with them. A close takes the identity out of
    /// the engine altogether, so nothing answers for it afterwards.
    ///
    /// Returns whether a terminal by that name was running.
    pub fn close(&mut self, terminal: &TerminalId) -> bool {
        let Some(index) = self.terminals.iter().position(|item| item.owns(terminal)) else {
            return false;
        };
        self.terminals.remove(index);
        self.timing_visible.remove(terminal);
        true
    }

    fn terminal_mut(&mut self, terminal: &TerminalId) -> Option<&mut NativeTerminal> {
        self.terminals.iter_mut().find(|item| item.owns(terminal))
    }

    fn terminal(&self, terminal: &TerminalId) -> Option<&NativeTerminal> {
        self.terminals.iter().find(|item| item.owns(terminal))
    }

    fn shutdown(&mut self) -> Vec<EngineEvent> {
        let mut failures = vec![None; self.terminals.len()];
        for (index, terminal) in self.terminals.iter_mut().enumerate() {
            if let Some(transport) = &mut terminal.transport
                && let Err(message) = transport.request_shutdown()
            {
                failures[index] = Some(message);
            }
        }

        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline
            && self.terminals.iter().any(|terminal| {
                terminal
                    .transport
                    .as_ref()
                    .is_some_and(PtyTransport::is_process_group_alive)
            })
        {
            std::thread::sleep(Duration::from_millis(20));
        }

        for (index, terminal) in self.terminals.iter_mut().enumerate() {
            let Some(transport) = &mut terminal.transport else {
                continue;
            };
            // Force when anything owned survives — the shell's group OR the
            // wider session (#119). A dead shell with live jobs must still
            // reach `force_shutdown`; the group check alone misses it.
            if (transport.is_process_group_alive() || transport.is_session_alive())
                && let Err(message) = transport.force_shutdown()
            {
                failures[index].get_or_insert(message);
            }
            transport.join();
        }

        self.terminals
            .iter_mut()
            .enumerate()
            .filter(|(_, terminal)| {
                matches!(
                    terminal.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                )
            })
            .flat_map(|(index, terminal)| match failures[index].take() {
                Some(message) => terminal.status_changed(TerminalStatus::Failed { message }),
                None => terminal.status_changed(TerminalStatus::Exited { code: None }),
            })
            .collect()
    }

    fn refresh_timing_if_due(&mut self, observed: Instant) -> Option<EngineEvent> {
        if observed.saturating_duration_since(self.last_timing_refresh) < TIMING_REFRESH_INTERVAL {
            return None;
        }

        let mut refreshed = false;
        let mut metadata_changed = false;
        for terminal in &mut self.terminals {
            if !self.timing_visible.contains(&terminal.frame.terminal)
                || !matches!(
                    terminal.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                )
            {
                continue;
            }
            refreshed = true;
            terminal.refresh_timing_at(observed);
            metadata_changed |=
                terminal.metadata.process.is_some() || terminal.metadata.output_idle.is_some();
        }
        if refreshed {
            self.last_timing_refresh = observed;
        }
        metadata_changed.then_some(EngineEvent::TimingChanged)
    }
}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl TerminalEngine for NativeEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent> {
        match command {
            EngineCommand::SetTimingVisibility { terminals } => {
                self.timing_visible = terminals;
                Vec::new()
            }
            EngineCommand::Input { terminal, bytes } => {
                let Some(item) = self.terminal_mut(&terminal) else {
                    return Vec::new();
                };
                if !matches!(
                    item.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                ) {
                    return Vec::new();
                }
                match item
                    .transport
                    .as_mut()
                    .map(|transport| transport.write(&bytes))
                {
                    // Backpressure is an answer, not a failure (#118): the
                    // pane stays alive and the caller learns the bytes went
                    // nowhere.
                    Some(Ok(InputOutcome::Refused)) => vec![EngineEvent::InputDropped { terminal }],
                    Some(Ok(InputOutcome::Queued)) => {
                        vec![EngineEvent::InputQueued { terminal }]
                    }
                    Some(Ok(InputOutcome::Flushed)) | None => Vec::new(),
                    Some(Err(message)) => item.status_changed(TerminalStatus::Failed { message }),
                }
            }
            EngineCommand::Resize { terminal, size } => {
                let Some(item) = self.terminal_mut(&terminal) else {
                    return Vec::new();
                };
                if matches!(
                    item.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                ) && let Some(transport) = &mut item.transport
                    && let Err(message) = transport.resize(size)
                {
                    return item.status_changed(TerminalStatus::Failed { message });
                }
                item.frame = item.adapter.resize(size);
                item.refresh_viewport();
                vec![
                    EngineEvent::MetadataChanged {
                        terminal,
                        metadata: item.metadata.clone(),
                    },
                    EngineEvent::FrameReady(item.frame.clone()),
                ]
            }
            EngineCommand::Scroll { terminal, command } => {
                let Some(item) = self.terminal_mut(&terminal) else {
                    return Vec::new();
                };
                item.frame = item.adapter.scroll(command);
                item.refresh_viewport();
                vec![
                    EngineEvent::MetadataChanged {
                        terminal,
                        metadata: item.metadata.clone(),
                    },
                    EngineEvent::FrameReady(item.frame.clone()),
                ]
            }
            EngineCommand::Respawn { terminal } => {
                let Some(item) = self.terminal_mut(&terminal) else {
                    return Vec::new();
                };
                match item.respawn() {
                    Ok(events) => events,
                    Err(message) => item.status_changed(TerminalStatus::Failed { message }),
                }
            }
            EngineCommand::Shutdown => self.shutdown(),
        }
    }

    fn drain_events(&mut self) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        for terminal in &mut self.terminals {
            let waiting_for_input_ready = terminal
                .transport
                .as_mut()
                .is_some_and(PtyTransport::waiting_for_input_ready);
            // Hooked shells reset terminal state during startup. Their PTY
            // must report interactive termios before queued input is flushed
            // so that reset cannot discard its first bytes (#136).
            if !waiting_for_input_ready && let Some(transport) = terminal.transport.as_mut() {
                // Preserve the established flush-before-events order for
                // ordinary panes and hooked panes after startup.
                let _ = transport.flush_input();
            }
            let pty_events = terminal
                .transport
                .as_mut()
                .map(PtyTransport::drain_events)
                .unwrap_or_default();
            for event in pty_events {
                terminal.handle_pty_event(event, &mut events);
            }
            if waiting_for_input_ready && let Some(transport) = terminal.transport.as_mut() {
                let _ = transport.flush_input();
            }
        }
        if let Some(event) = self.refresh_timing_if_due(Instant::now()) {
            events.push(event);
        }
        events
    }

    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame> {
        self.terminal(terminal).map(|item| &item.frame)
    }

    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus> {
        self.terminal(terminal).map(|item| &item.status)
    }

    fn metadata(&self, terminal: &TerminalId) -> Option<&TerminalMetadata> {
        self.terminal(terminal).map(|item| &item.metadata)
    }

    fn history_lines(&self, terminal: &TerminalId, max: usize) -> Option<Vec<String>> {
        self.terminal(terminal)
            .map(|item| item.adapter.history_lines(max))
    }

    fn active_screen_lines(&self, terminal: &TerminalId, max: usize) -> Option<Vec<String>> {
        self.terminal(terminal)
            .map(|item| item.adapter.active_screen_lines(max))
    }
}

fn elapsed(instant: Instant, observed: Instant) -> Elapsed {
    Elapsed {
        millis: observed
            .saturating_duration_since(instant)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    }
}

fn now() -> Timestamp {
    Timestamp {
        unix_millis: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        path::PathBuf,
        time::{Duration, Instant},
    };

    use crate::contracts::{
        CellContent, DEFAULT_SCROLLBACK, Elapsed, EngineCommand, EngineEvent, MouseProtocol,
        NotifyKind, ProcessInfo, Project, ScreenSize, ScrollCommand, TerminalEngine, TerminalId,
        TerminalMetadata, TerminalStatus,
    };
    use crate::ui::{Deck, DeckState, Notifications};
    use ratatui::layout::Rect;

    use super::{
        NativeEngine, NativeTerminal, SHUTDOWN_GRACE, VtFrameAdapter, scan_notify,
        scan_notify_chunk,
    };

    #[test]
    fn private_notify_osc_is_consumed_and_sanitized() {
        let bytes = b"left\x1b]7777;termdeck;finished;code=1;secs=12;cmd=bad\x1btitle\x07right";
        let (notifies, passthrough) = scan_notify(bytes);
        assert_eq!(passthrough, b"leftright");
        assert_eq!(
            notifies,
            [NotifyKind::Message {
                title: "badtitle".to_owned(),
                body: "exit 1 · 12s".to_owned(),
            }]
        );
    }

    #[test]
    fn notify_scanner_reassembles_every_split_without_eating_hostile_output() {
        let valid = b"\x1b]7777;termdeck;finished;code=0;secs=3;cmd=build\x1b\\";
        let hostile = b"\x1b]7777;termdeck;finished;nope\x07";
        let mut source = b"one".to_vec();
        source.extend_from_slice(valid);
        source.extend_from_slice(hostile);
        source.extend_from_slice(b"two\x1b]777;notify;x\x07three");
        for split in 1..source.len() {
            let mut carry = Vec::new();
            let (mut notifies, mut output) = scan_notify_chunk(&mut carry, &source[..split]);
            let (later, rest) = scan_notify_chunk(&mut carry, &source[split..]);
            notifies.extend(later);
            output.extend(rest);
            output.extend(carry);
            assert_eq!(
                notifies,
                [NotifyKind::Message {
                    title: "build".to_owned(),
                    body: "done · 3s".to_owned(),
                }],
                "split {split}"
            );
            assert_eq!(
                output,
                [b"one".as_slice(), hostile, b"two\x1b]777;notify;x\x07three"].concat(),
                "split {split}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn configured_terminals_keep_identity_and_an_exited_terminal_respawns() {
        let frontend = TerminalId::new("frontend");
        let backend = TerminalId::new("backend");
        let projects = [
            project(
                frontend.clone(),
                "for n in $(seq 1 30); do printf 'FRONT-EXITED-%s\\n' \"$n\"; done; exit 7",
            ),
            project(backend.clone(), "printf 'BACK-RUNNING\\n'; sleep 30"),
        ];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 4)).unwrap();

        wait_for_exit(&mut engine, &frontend, 7);
        wait_for_frame(&mut engine, &frontend, "FRONT-EXITED");
        wait_for_frame(&mut engine, &backend, "BACK-RUNNING");
        assert!(frame_text(engine.frame(&frontend).unwrap()).contains("FRONT-EXITED"));
        assert!(frame_text(engine.frame(&backend).unwrap()).contains("BACK-RUNNING"));
        assert_eq!(engine.status(&backend), Some(&TerminalStatus::Running));
        engine.dispatch(EngineCommand::Scroll {
            terminal: frontend.clone(),
            command: ScrollCommand::Up(10),
        });
        assert!(frame_text(engine.frame(&frontend).unwrap()).contains("FRONT-EXITED"));
        assert_eq!(
            engine.status(&frontend),
            Some(&TerminalStatus::Exited { code: Some(7) })
        );

        let events = engine.dispatch(EngineCommand::Respawn {
            terminal: frontend.clone(),
        });
        assert!(matches!(
            events.as_slice(),
            [
                EngineEvent::StatusChanged { terminal, status: TerminalStatus::Running },
                EngineEvent::MetadataChanged { .. },
                EngineEvent::FrameReady(frame),
            ] if terminal == &frontend && frame.terminal == frontend
        ));
        assert!(engine.metadata(&frontend).unwrap().restarted_at.is_some());
        assert_eq!(engine.status(&frontend), Some(&TerminalStatus::Running));
        assert!(!frame_text(engine.frame(&frontend).unwrap()).contains("FRONT-EXITED"));

        engine.dispatch(EngineCommand::Shutdown);
        assert!(engine.terminals.iter().all(|terminal| {
            terminal
                .transport
                .as_ref()
                .is_some_and(|transport| transport.has_joined_threads())
        }));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_terms_all_groups_then_kills_survivors_and_joins_threads() {
        let terminal = TerminalId::new("stubborn");
        // Ignores both signals shutdown asks with (#46 added HUP beside
        // TERM), so it is still the pane that has to be killed.
        let projects = [project(
            terminal.clone(),
            "trap '' TERM HUP; printf READY; while :; do sleep 1; done",
        )];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        wait_for_frame(&mut engine, &terminal, "READY");
        let started = Instant::now();

        let events = engine.dispatch(EngineCommand::Shutdown);

        let elapsed = started.elapsed();
        let grace_slack = SHUTDOWN_GRACE / 4;
        // CI scheduling jitter makes a zero-tolerance lower bound flaky; this
        // window still proves we waited for the grace period instead of fast-pathing.
        assert!(
            elapsed >= SHUTDOWN_GRACE - grace_slack && elapsed < SHUTDOWN_GRACE * 2,
            "shutdown grace window was exceeded: {elapsed:?}"
        );
        assert!(matches!(
            events.as_slice(),
            [
                EngineEvent::StatusChanged { terminal: event_terminal, status: TerminalStatus::Exited { code: None } },
                EngineEvent::MetadataChanged { terminal: metadata_terminal, .. },
            ] if event_terminal == &terminal && metadata_terminal == &terminal
        ));
        let transport = engine.terminals[0].transport.as_ref().unwrap();
        assert!(!transport.is_process_group_alive());
        assert!(transport.has_joined_threads());
    }

    /// #119 through the integrated path: a pane whose background job
    /// ignores SIGHUP in its own process group is cleaned by engine
    /// shutdown — the shared grace, per-terminal force, and bounded join
    /// together leave no owned session member behind.
    #[cfg(target_os = "linux")]
    #[test]
    fn engine_shutdown_kills_a_sighup_ignoring_background_job() {
        let terminal = TerminalId::new("bg-job");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec![
                "/usr/bin/bash".to_owned(),
                "-m".to_owned(),
                "-c".to_owned(),
                "trap '' HUP; sleep 60 & echo JOBPID=$!; wait".to_owned(),
            ],
            shell_hook: false,
        }];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        wait_for_frame(&mut engine, &terminal, "JOBPID=");
        let text = frame_text(engine.frame(&terminal).unwrap());
        let marker = "JOBPID=";
        let job: u32 = text[text.find(marker).unwrap() + marker.len()..]
            .split(|character: char| !character.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(pid_alive(job), "the job must be running before shutdown");

        let started = Instant::now();
        engine.dispatch(EngineCommand::Shutdown);
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "engine shutdown must stay bounded, took {elapsed:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && pid_alive(job) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pid_alive(job), "the SIGHUP-ignoring job must die");
        let transport = engine.terminals[0].transport.as_ref().unwrap();
        assert!(
            !transport.is_session_alive(),
            "no session member may survive"
        );
        assert!(transport.has_joined_threads());
    }

    /// Issue #46: a confirmed quit waited out the whole grace period, because
    /// every pane is an interactive shell and an interactive shell ignores
    /// SIGTERM by design. Shutdown hangs up as well, which is the signal a
    /// shell answers, so a workspace of shells closes at once.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_workspace_of_interactive_shells_closes_without_waiting_out_the_grace() {
        // The real thing, not `sh -c`: only an interactive shell ignores TERM.
        let terminal = TerminalId::new("shell");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned()],
            shell_hook: false,
        }];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"printf READY\n".to_vec(),
        });
        wait_for_frame(&mut engine, &terminal, "READY");
        let started = Instant::now();

        engine.dispatch(EngineCommand::Shutdown);

        assert!(
            started.elapsed() < SHUTDOWN_GRACE / 2,
            "an interactive shell should hang up at once, took {:?}",
            started.elapsed()
        );
        let transport = engine.terminals[0].transport.as_ref().unwrap();
        assert!(!transport.is_process_group_alive(), "no orphaned shell");
        assert!(transport.has_joined_threads());
    }

    /// #84: closing takes the identity out of the engine and the process
    /// group with it, and it is not the same thing as a shell that exited.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_closed_terminal_leaves_no_process_group_and_no_identity() {
        let closed = TerminalId::new("closed");
        let kept = TerminalId::new("kept");
        let projects = [
            project(closed.clone(), "printf CLOSED; sleep 30"),
            project(kept.clone(), "printf KEPT; sleep 30"),
        ];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        wait_for_frame(&mut engine, &closed, "CLOSED");
        wait_for_frame(&mut engine, &kept, "KEPT");
        let group = engine.metadata(&closed).unwrap().process.unwrap().pid;
        assert!(crate::engine::pty::process_group_alive(group));

        assert!(engine.close(&closed));

        // Nothing answers for it any more: no frame, no status, no metadata,
        // and no second close either.
        assert_eq!(engine.frame(&closed), None);
        assert_eq!(engine.status(&closed), None);
        assert_eq!(engine.metadata(&closed), None);
        assert!(!engine.close(&closed));
        assert!(
            !crate::engine::pty::process_group_alive(group),
            "a closed pane leaves no orphaned process group"
        );
        // The pane beside it is undisturbed and still shuts down with the
        // engine, so a close is not a partial shutdown.
        assert_eq!(engine.status(&kept), Some(&TerminalStatus::Running));
        assert!(frame_text(engine.frame(&kept).unwrap()).contains("KEPT"));
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// The distinction the interface draws on (#84): a shell that exits on
    /// its own is kept, with its last frame and its `^g r`; closing it is
    /// what takes it away.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_exited_terminal_is_kept_until_it_is_closed() {
        let terminal = TerminalId::new("exits");
        let projects = [project(terminal.clone(), "printf GONE; exit 3")];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 4)).unwrap();

        wait_for_exit(&mut engine, &terminal, 3);

        assert_eq!(
            engine.status(&terminal),
            Some(&TerminalStatus::Exited { code: Some(3) }),
            "an exit keeps the terminal, as it always has"
        );
        assert!(engine.frame(&terminal).is_some());

        assert!(engine.close(&terminal));

        assert_eq!(engine.status(&terminal), None);
        assert_eq!(engine.frame(&terminal), None);
        assert!(engine.terminals.is_empty());
    }

    /// #50 A3: a terminal added to a running engine is a terminal like any
    /// other — it runs, it answers, and shutdown reaches it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_terminal_added_at_runtime_runs_and_is_shut_down_with_the_rest() {
        let first = TerminalId::new("first");
        let added = TerminalId::new("added");
        let projects = [project(first.clone(), "printf FIRST; sleep 30")];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        wait_for_frame(&mut engine, &first, "FIRST");

        engine
            .add(
                project(added.clone(), "printf ADDED; sleep 30"),
                ScreenSize::new(80, 24),
            )
            .unwrap();

        wait_for_frame(&mut engine, &added, "ADDED");
        assert_eq!(engine.status(&added), Some(&TerminalStatus::Running));
        assert_eq!(
            engine.status(&first),
            Some(&TerminalStatus::Running),
            "the one that was already here is undisturbed"
        );
        // Identities stay unique, which is what the engine keys everything by.
        assert!(
            engine
                .add(project(added.clone(), "sleep 1"), ScreenSize::new(80, 24))
                .is_err(),
            "the same identity twice is refused"
        );

        engine.dispatch(EngineCommand::Shutdown);

        assert!(
            engine.terminals.iter().all(|terminal| {
                terminal.transport.as_ref().is_some_and(|transport| {
                    !transport.is_process_group_alive() && transport.has_joined_threads()
                })
            }),
            "nothing is orphaned, the addition included"
        );
    }

    /// #118 at the engine seam: a terminal whose child stopped reading
    /// refuses input truthfully instead of hanging the loop — and the
    /// refusal is backpressure, not failure, so the pane stays `Running`.
    #[cfg(target_os = "linux")]
    #[test]
    fn input_to_a_terminal_that_stopped_reading_is_dropped_truthfully() {
        let terminal = TerminalId::new("wedged");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "stty raw -echo; exec sleep 30".to_owned(),
            ],
            shell_hook: false,
        }];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let chunk = vec![b'x'; 64 * 1024];
        let started = Instant::now();
        let mut dropped = false;
        for _ in 0..64 {
            let events = engine.dispatch(EngineCommand::Input {
                terminal: terminal.clone(),
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
        assert!(dropped, "a wedged terminal must report its backpressure");
        assert_eq!(
            engine.status(&terminal),
            Some(&TerminalStatus::Running),
            "a refused input is not a failed terminal"
        );
        // The queue stays saturated while the child is stuck: the next
        // full chunk is refused the same way, still without blocking or
        // failing.
        let events = engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: chunk.clone(),
        });
        assert!(
            events
                .iter()
                .any(|event| matches!(event, EngineEvent::InputDropped { .. })),
            "a full queue stays full"
        );
        assert_eq!(engine.status(&terminal), Some(&TerminalStatus::Running));
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// The configured capacity belongs to the engine session. Initial panes,
    /// runtime additions, and respawns all construct their adapters through
    /// this path. As in `VtFrameAdapter`, a complete peek contains at most
    /// `scrollback + viewport_rows` lines: the limit itself is off-screen
    /// history only.
    #[cfg(target_os = "linux")]
    #[test]
    fn configured_scrollback_is_preserved_for_initial_added_and_respawned_panes() {
        const HISTORY: usize = 1;
        let size = ScreenSize::new(20, 2);
        let initial = TerminalId::new("initial");
        let added = TerminalId::new("added");
        let script = |label: &str| {
            format!("i=0; while [ $i -lt 8 ]; do printf '{label}-%s\\n' \"$i\"; i=$((i + 1)); done")
        };
        let initial_project = project(initial.clone(), &script("initial"));
        let mut engine =
            NativeEngine::spawn_sized(std::slice::from_ref(&initial_project), &[size], HISTORY)
                .unwrap();

        wait_for_exit(&mut engine, &initial, 0);
        assert_retained_history(&engine, &initial, "initial-7", HISTORY, size);

        engine.dispatch(EngineCommand::Respawn {
            terminal: initial.clone(),
        });
        wait_for_exit(&mut engine, &initial, 0);
        assert_retained_history(&engine, &initial, "initial-7", HISTORY, size);

        engine
            .add(project(added.clone(), &script("added")), size)
            .unwrap();
        wait_for_exit(&mut engine, &added, 0);
        assert_retained_history(&engine, &added, "added-7", HISTORY, size);
    }

    /// #118, the accepted half: a large paste to a reading child is queued
    /// without blocking and the frame pump delivers it — no `InputDropped`,
    /// and the tail arrives on screen.
    #[cfg(target_os = "linux")]
    #[test]
    fn large_paste_to_a_reading_child_is_accepted_and_echoed() {
        let terminal = TerminalId::new("reader");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "stty raw -echo; exec cat".to_owned(),
            ],
            shell_hook: false,
        }];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let mut paste = vec![b'q'; 256 * 1024];
        paste.extend_from_slice(b"PUMP-TAIL-118\n");
        let started = Instant::now();
        let events = engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: paste,
        });
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a 256 KiB paste must be accepted without blocking"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EngineEvent::InputDropped { .. })),
            "a reading child leaves room in the queue"
        );
        wait_for_frame(&mut engine, &terminal, "PUMP-TAIL-118");
        assert_eq!(engine.status(&terminal), Some(&TerminalStatus::Running));
        engine.dispatch(EngineCommand::Shutdown);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn eight_terminals_spawn_and_shutdown() {
        let projects = (0..8)
            .map(|index| {
                let terminal = TerminalId::new(format!("terminal-{index}"));
                project(terminal, &format!("printf READY-{index}; sleep 30"))
            })
            .collect::<Vec<_>>();
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();

        for (index, project) in projects.iter().enumerate() {
            wait_for_frame(&mut engine, &project.terminal, &format!("READY-{index}"));
        }
        assert!(
            projects.iter().all(|project| {
                engine.status(&project.terminal) == Some(&TerminalStatus::Running)
            })
        );

        let events = engine.dispatch(EngineCommand::Shutdown);

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, EngineEvent::StatusChanged { .. }))
                .count(),
            projects.len()
        );
        assert!(engine.terminals.iter().all(|terminal| {
            terminal
                .transport
                .as_ref()
                .is_some_and(|transport| transport.has_joined_threads())
        }));
    }

    /// #97: a real child ringing a real bell reaches the session as one
    /// additive event. `tput bel` is what a shell hook or an untaught tool
    /// emits, and nothing had to be taught to emit it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_bell_from_a_real_pty_arrives_as_a_notification() {
        let terminal = TerminalId::new("beeper");
        let projects = [project(terminal.clone(), "printf 'RANG\\a\\n'; sleep 30")];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 4)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut rang = false;
        while Instant::now() < deadline && !rang {
            rang = engine.drain_events().iter().any(|event| {
                matches!(
                    event,
                    EngineEvent::Notify { terminal: rung, kind: NotifyKind::Attention }
                        if rung == &terminal
                )
            });
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(rang, "the bell never reached the engine seam");
        assert!(frame_text(engine.frame(&terminal).unwrap()).contains("RANG"));
        // The bell was taken with the output that carried it: a later drain
        // of quiet output raises nothing.
        assert!(
            !engine
                .drain_events()
                .iter()
                .any(|event| matches!(event, EngineEvent::Notify { .. }))
        );
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// The generated startup file is the whole feature boundary: this uses a
    /// real interactive bash, not a synthetic OSC writer.
    #[cfg(target_os = "linux")]
    #[test]
    fn bash_hook_reports_errors_and_long_completions() {
        let terminal = TerminalId::new("bash-hook");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["/usr/bin/bash".to_owned(), "-l".to_owned()],
            shell_hook: true,
        }];
        let mut engine = NativeEngine::spawn_sized_with_socket(
            &projects,
            &[ScreenSize::new(80, 24)],
            DEFAULT_SCROLLBACK,
            std::path::Path::new("/tmp/termdeck-hook-test.sock"),
        )
        .unwrap();
        engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"printf 'HOOK=%s\\n' \"$TERMDECK_SHELL_HOOK\"\nfalse\nTERMDECK_NOTIFY_LONG_SECS=0\nsleep 0.01\n".to_vec(),
        });

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut messages = Vec::new();
        while Instant::now() < deadline && messages.len() < 3 {
            messages.extend(
                engine
                    .drain_events()
                    .into_iter()
                    .filter_map(|event| match event {
                        EngineEvent::Notify {
                            terminal: rung,
                            kind: NotifyKind::Message { title, body },
                        } if rung == terminal => Some((title, body)),
                        _ => None,
                    }),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let frame = frame_text(engine.frame(&terminal).unwrap());
        assert!(frame.contains("HOOK=1"), "{messages:?}; {frame:?}");
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "false" && body.starts_with("exit 1 · ")),
            "{messages:?}; {frame:?}"
        );
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "sleep 0.01" && body.starts_with("done · ")),
            "{messages:?}; {frame:?}"
        );
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// #136: input dispatched in the same turn as a hooked shell spawn stays
    /// queued until its interactive termios is ready, then arrives whole.
    #[cfg(target_os = "linux")]
    #[test]
    fn hooked_shell_keeps_same_turn_input_until_termios_ready() {
        let terminal = TerminalId::new("startup-input");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec![
                "/usr/bin/bash".to_owned(),
                "--noprofile".to_owned(),
                "--norc".to_owned(),
            ],
            shell_hook: true,
        }];
        let mut engine = NativeEngine::spawn_sized_with_socket(
            &projects,
            &[ScreenSize::new(80, 4)],
            DEFAULT_SCROLLBACK,
            std::path::Path::new("/tmp/termdeck-hook-test.sock"),
        )
        .unwrap();

        let events = engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"printf 'WHOLE-STARTUP-INPUT\\n'\n".to_vec(),
        });
        assert!(
            events.iter().any(|event| matches!(
                event,
                EngineEvent::InputQueued { terminal: queued } if queued == &terminal
            )),
            "same-turn input must wait for shell startup: {events:?}"
        );
        wait_for_frame(&mut engine, &terminal, "WHOLE-STARTUP-INPUT");
        engine.dispatch(EngineCommand::Shutdown);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn zsh_hook_reports_errors_when_zsh_is_available() {
        if std::process::Command::new("zsh")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let terminal = TerminalId::new("zsh-hook");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["zsh".to_owned()],
            shell_hook: true,
        }];
        let mut engine = NativeEngine::spawn_sized_with_socket(
            &projects,
            &[ScreenSize::new(80, 4)],
            DEFAULT_SCROLLBACK,
            std::path::Path::new("/tmp/termdeck-hook-test.sock"),
        )
        .unwrap();
        engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"false\nTERMDECK_NOTIFY_LONG_SECS=0\nsleep 0.01\n".to_vec(),
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut messages = Vec::new();
        while Instant::now() < deadline && messages.len() < 3 {
            messages.extend(
                engine
                    .drain_events()
                    .into_iter()
                    .filter_map(|event| match event {
                        EngineEvent::Notify {
                            terminal: rung,
                            kind: NotifyKind::Message { title, body },
                        } if rung == terminal => Some((title, body)),
                        _ => None,
                    }),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "false" && body.starts_with("exit 1 · ")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "sleep 0.01" && body.starts_with("done · ")),
            "{messages:?}"
        );
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// The same for Fish, whose hook is a `vendor_conf.d` file rather than an
    /// rc: three shells are advertised and only two were ever exercised, so a
    /// green suite said nothing about the third (#130). Fish assigns with
    /// `set -x` and measures the command in `$CMD_DURATION`, so the input is
    /// its own dialect of the same three commands.
    ///
    /// Skips where Fish is absent, the way the Zsh case does. CI installs
    /// both, which is what turns these two from a promise into a check.
    #[cfg(target_os = "linux")]
    #[test]
    fn fish_hook_reports_errors_when_fish_is_available() {
        if std::process::Command::new("fish")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let terminal = TerminalId::new("fish-hook");
        let projects = [Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["fish".to_owned()],
            shell_hook: true,
        }];
        let mut engine = NativeEngine::spawn_sized_with_socket(
            &projects,
            &[ScreenSize::new(80, 4)],
            DEFAULT_SCROLLBACK,
            std::path::Path::new("/tmp/termdeck-hook-test.sock"),
        )
        .unwrap();
        engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"false\nset -x TERMDECK_NOTIFY_LONG_SECS 0\nsleep 0.01\n".to_vec(),
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut messages = Vec::new();
        while Instant::now() < deadline && messages.len() < 3 {
            messages.extend(
                engine
                    .drain_events()
                    .into_iter()
                    .filter_map(|event| match event {
                        EngineEvent::Notify {
                            terminal: rung,
                            kind: NotifyKind::Message { title, body },
                        } if rung == terminal => Some((title, body)),
                        _ => None,
                    }),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "false" && body.starts_with("exit 1 · ")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|(title, body)| title == "sleep 0.01" && body.starts_with("done · ")),
            "{messages:?}"
        );
        engine.dispatch(EngineCommand::Shutdown);
    }

    #[test]
    fn requires_at_least_one_terminal() {
        assert_eq!(
            NativeEngine::spawn(&[], ScreenSize::new(80, 24))
                .err()
                .as_deref(),
            Some("native engine requires at least one terminal")
        );
    }

    fn timing_terminal(terminal: TerminalId, observed: Instant) -> NativeTerminal {
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: Vec::new(),
            shell_hook: false,
        };
        let adapter = VtFrameAdapter::new(terminal, ScreenSize::new(80, 24), DEFAULT_SCROLLBACK);
        let frame = adapter.frame();
        NativeTerminal {
            project,
            socket: None,
            transport: None,
            adapter,
            frame,
            status: TerminalStatus::Running,
            metadata: TerminalMetadata {
                process: Some(ProcessInfo {
                    pid: 7,
                    uptime: Elapsed::default(),
                }),
                ..TerminalMetadata::default()
            },
            started: observed.checked_sub(Duration::from_millis(2_000)).unwrap(),
            scrollback: DEFAULT_SCROLLBACK,
            last_output: Some(observed.checked_sub(Duration::from_millis(1_500)).unwrap()),
            notify_carry: Vec::new(),
        }
    }

    #[test]
    fn timing_metadata_uses_one_visible_shared_tick() {
        let observed = Instant::now();
        let visible = TerminalId::new("visible");
        let also_visible = TerminalId::new("also-visible");
        let hidden = TerminalId::new("hidden");
        let mut engine = NativeEngine {
            terminals: vec![
                timing_terminal(visible.clone(), observed),
                timing_terminal(also_visible.clone(), observed),
                timing_terminal(hidden.clone(), observed),
            ],
            scrollback: DEFAULT_SCROLLBACK,
            last_timing_refresh: observed.checked_sub(Duration::from_secs(1)).unwrap(),
            timing_visible: BTreeSet::from([visible.clone(), also_visible.clone()]),
        };

        assert_eq!(
            engine.refresh_timing_if_due(observed),
            Some(EngineEvent::TimingChanged),
            "every rendered terminal shares one redraw signal"
        );
        assert_eq!(
            engine
                .metadata(&visible)
                .and_then(|metadata| metadata.process)
                .map(|process| process.uptime),
            Some(Elapsed { millis: 2_000 })
        );
        assert_eq!(
            engine
                .metadata(&visible)
                .and_then(|metadata| metadata.output_idle),
            Some(Elapsed { millis: 1_500 })
        );
        assert_eq!(
            engine
                .metadata(&also_visible)
                .and_then(|metadata| metadata.process)
                .map(|process| process.uptime),
            Some(Elapsed { millis: 2_000 }),
            "all rendered panes share the same observed instant"
        );
        assert_eq!(
            engine
                .metadata(&hidden)
                .and_then(|metadata| metadata.process)
                .map(|process| process.uptime),
            Some(Elapsed::default()),
            "a zoom-hidden or narrow-hidden pane is not refreshed"
        );
        assert_eq!(
            engine.refresh_timing_if_due(observed + Duration::from_millis(999)),
            None,
            "the shared tick is bounded to one second"
        );
        assert_eq!(
            engine.refresh_timing_if_due(observed + Duration::from_secs(1)),
            Some(EngineEvent::TimingChanged),
            "the visible terminal remains on the one-second cadence"
        );
        assert_eq!(
            engine
                .metadata(&visible)
                .and_then(|metadata| metadata.process)
                .map(|process| process.uptime),
            Some(Elapsed { millis: 3_000 })
        );

        engine.dispatch(EngineCommand::SetTimingVisibility {
            terminals: BTreeSet::new(),
        });
        assert_eq!(
            engine.refresh_timing_if_due(observed + Duration::from_secs(2)),
            None,
            "no hidden terminal schedules a timing redraw"
        );
    }

    #[test]
    fn a_folded_strip_keeps_its_idle_age_on_the_shared_tick() {
        let observed = Instant::now();
        let master = TerminalId::new("master");
        let folded = TerminalId::new("folded");
        let projects = [
            Project {
                terminal: master.clone(),
                path: PathBuf::from("/"),
                command: Vec::new(),
                shell_hook: false,
            },
            Project {
                terminal: folded.clone(),
                path: PathBuf::from("/"),
                command: Vec::new(),
                shell_hook: false,
            },
        ];
        let deck = DeckState::new(projects.len());
        let notifies = Notifications::new();
        let timing_visible = Deck {
            workspace: "",
            projects: &projects,
            state: &deck,
            notifies: &notifies,
            master_ratio: deck.master_ratio(),
            now: crate::contracts::Timestamp::default(),
        }
        .timing_terminals(Rect::new(0, 0, 144, 42))
        .into_iter()
        .collect();
        let mut engine = NativeEngine {
            terminals: vec![
                timing_terminal(master, observed),
                timing_terminal(folded.clone(), observed),
            ],
            scrollback: DEFAULT_SCROLLBACK,
            last_timing_refresh: observed.checked_sub(Duration::from_secs(1)).unwrap(),
            timing_visible,
        };

        assert_eq!(
            engine.refresh_timing_if_due(observed),
            Some(EngineEvent::TimingChanged)
        );
        assert_eq!(
            engine
                .metadata(&folded)
                .and_then(|metadata| metadata.output_idle),
            Some(Elapsed { millis: 1_500 }),
            "the folded strip draws this idle age"
        );
        assert_eq!(
            engine.refresh_timing_if_due(observed + Duration::from_secs(1)),
            Some(EngineEvent::TimingChanged)
        );
        assert_eq!(
            engine
                .metadata(&folded)
                .and_then(|metadata| metadata.output_idle),
            Some(Elapsed { millis: 2_500 }),
            "the drawn folded strip keeps advancing"
        );
    }

    /// The #9 review note was real: resizing can change Alacritty's visible
    /// history, so its metadata must be emitted with the replacement frame.
    /// #74: the viewport metadata follows the app — the session routes the
    /// wheel on these two flags, so they must refresh with every viewport
    /// change, without a PTY involved.
    #[test]
    fn viewport_metadata_tracks_the_alternate_screen_and_app_mouse() {
        let terminal = TerminalId::new("recording");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: Vec::new(),
            shell_hook: false,
        };
        let mut adapter = VtFrameAdapter::new(
            terminal.clone(),
            ScreenSize::new(80, 24),
            DEFAULT_SCROLLBACK,
        );
        adapter.feed(b"\x1b[?1049h\x1b[?1h\x1b[?1000h\x1b[?1006h");
        let frame = adapter.frame();
        let mut engine = NativeEngine {
            terminals: vec![NativeTerminal {
                project,
                socket: None,
                transport: None,
                adapter,
                frame,
                status: TerminalStatus::Running,
                metadata: TerminalMetadata::default(),
                started: Instant::now(),
                scrollback: DEFAULT_SCROLLBACK,
                last_output: None,
                notify_carry: Vec::new(),
            }],
            scrollback: DEFAULT_SCROLLBACK,
            last_timing_refresh: Instant::now(),
            timing_visible: BTreeSet::from([terminal.clone()]),
        };
        engine.dispatch(EngineCommand::Scroll {
            terminal: terminal.clone(),
            command: ScrollCommand::Up(1),
        });

        let metadata = engine.metadata(&terminal).unwrap();
        assert!(metadata.alt_screen, "the app owns the grid");
        assert!(metadata.mouse_reporting, "the app enabled the mouse");
        assert_eq!(metadata.mouse_protocol, MouseProtocol::Sgr);
        assert!(metadata.application_cursor, "the app enabled DECCKM");
    }

    /// #74 end to end: a live app entering its alternate screen surfaces
    /// through PTY output into the metadata the wheel route reads.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_app_reaching_its_alternate_screen_surfaces_in_metadata() {
        let terminal = TerminalId::new("app");
        let projects = [project(
            terminal.clone(),
            "printf '\x1b[?1049hAPP-SCREEN'; sleep 30",
        )];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            engine.drain_events();
            if engine
                .metadata(&terminal)
                .is_some_and(|metadata| metadata.alt_screen)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(
            engine
                .metadata(&terminal)
                .is_some_and(|metadata| metadata.alt_screen),
            "the app's alternate screen never surfaced"
        );
        engine.dispatch(EngineCommand::Shutdown);
    }

    /// `fzf` waits for this DSR reply before it draws. Keep the assertion at
    /// the PTY boundary: recording the reply in the emulator alone is not
    /// sufficient if it never reaches the child application.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_app_receives_its_device_status_reply() {
        let terminal = TerminalId::new("query");
        let projects = [project(
            terminal.clone(),
            "stty raw -echo; printf '\\033[6n'; reply=$(dd bs=1 count=6 status=none); stty sane; printf 'DSR:'; printf '%s' \"$reply\" | od -An -tx1",
        )];
        let mut engine = NativeEngine::spawn(&projects, ScreenSize::new(80, 24)).unwrap();

        wait_for_frame(&mut engine, &terminal, "DSR: 1b 5b 31 3b 31 52");
        engine.dispatch(EngineCommand::Shutdown);
    }

    #[test]
    fn resize_refreshes_scrollback_metadata_with_the_frame() {
        let terminal = TerminalId::new("recording");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: Vec::new(),
            shell_hook: false,
        };
        let mut adapter =
            VtFrameAdapter::new(terminal.clone(), ScreenSize::new(4, 2), DEFAULT_SCROLLBACK);
        let frame = adapter.feed(b"0\r\n1\r\n2\r\n3\r\n");
        let mut engine = NativeEngine {
            terminals: vec![NativeTerminal {
                project,
                socket: None,
                transport: None,
                adapter,
                frame,
                status: TerminalStatus::Running,
                metadata: TerminalMetadata::default(),
                started: Instant::now(),
                scrollback: DEFAULT_SCROLLBACK,
                last_output: None,
                notify_carry: Vec::new(),
            }],
            scrollback: DEFAULT_SCROLLBACK,
            last_timing_refresh: Instant::now(),
            timing_visible: BTreeSet::from([terminal.clone()]),
        };
        engine.dispatch(EngineCommand::Scroll {
            terminal: terminal.clone(),
            command: ScrollCommand::Up(1),
        });

        let events = engine.dispatch(EngineCommand::Resize {
            terminal: terminal.clone(),
            size: ScreenSize::new(4, 1),
        });
        let expected = engine.terminals[0].adapter.scrollback_position();
        assert!(matches!(
            events.as_slice(),
            [
                EngineEvent::MetadataChanged { terminal: event_terminal, metadata },
                EngineEvent::FrameReady(frame),
            ] if event_terminal == &terminal && metadata.scrollback == expected
                && frame.size == ScreenSize::new(4, 1)
        ));
        assert_eq!(engine.metadata(&terminal).unwrap().scrollback, expected);
    }

    #[cfg(target_os = "linux")]
    fn pid_alive(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(target_os = "linux")]
    fn project(terminal: TerminalId, script: &str) -> Project {
        Project {
            terminal,
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
            shell_hook: false,
        }
    }

    #[cfg(target_os = "linux")]
    fn wait_for_exit(engine: &mut NativeEngine, terminal: &TerminalId, code: i32) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            engine.drain_events();
            if engine.status(terminal) == Some(&TerminalStatus::Exited { code: Some(code) }) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{terminal} did not exit with {code}");
    }

    #[cfg(target_os = "linux")]
    fn wait_for_frame(engine: &mut NativeEngine, terminal: &TerminalId, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            engine.drain_events();
            if frame_text(engine.frame(terminal).unwrap()).contains(expected) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{terminal} did not print {expected}");
    }

    #[cfg(target_os = "linux")]
    fn assert_retained_history(
        engine: &NativeEngine,
        terminal: &TerminalId,
        last_line: &str,
        history: usize,
        size: ScreenSize,
    ) {
        let lines = engine.history_lines(terminal, usize::MAX).unwrap();
        assert!(
            lines.len() <= history + usize::from(size.rows),
            "{terminal} retained more than history plus viewport: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains(last_line)),
            "{terminal} lost its latest output: {lines:?}"
        );
        assert_eq!(
            engine.metadata(terminal).unwrap().scrollback.lines_above,
            history as u32,
            "{terminal} must retain exactly the configured off-screen history once full"
        );
    }

    #[cfg(target_os = "linux")]
    fn frame_text(frame: &crate::contracts::TerminalFrame) -> String {
        frame
            .cells
            .iter()
            .map(|cell| match &cell.content {
                CellContent::Glyph { text, .. } => text.as_str(),
                CellContent::Empty => " ",
                CellContent::Continuation => "",
            })
            .collect()
    }
}
