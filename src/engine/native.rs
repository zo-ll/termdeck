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
    engine::{PtyEvent, PtyTransport, VtFrameAdapter},
};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

struct NativeTerminal {
    project: Project,
    socket: Option<std::path::PathBuf>,
    transport: Option<PtyTransport>,
    adapter: VtFrameAdapter,
    frame: TerminalFrame,
    status: TerminalStatus,
    metadata: TerminalMetadata,
    started: Instant,
    last_output: Option<Instant>,
}

impl NativeTerminal {
    fn spawn(project: Project, size: ScreenSize, socket: Option<&Path>) -> Result<Self, String> {
        let transport = PtyTransport::spawn_with_socket(&project, size, socket)?;
        let adapter = VtFrameAdapter::new(project.terminal.clone(), size);
        let frame = adapter.frame();

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
            started: Instant::now(),
            last_output: None,
        })
    }

    fn owns(&self, terminal: &TerminalId) -> bool {
        self.frame.terminal == *terminal
    }

    /// The viewport the renderer shows: history offset plus which screen
    /// the app owns and whether it wants the wheel as mouse reports (#74).
    fn refresh_viewport(&mut self) {
        self.metadata.scrollback = self.adapter.scrollback_position();
        self.metadata.alt_screen = self.adapter.alt_screen();
        self.metadata.mouse_reporting = self.adapter.mouse_reporting();
    }

    fn refresh_timing(&mut self) {
        if let Some(process) = &mut self.metadata.process {
            process.uptime = elapsed(self.started);
        }
        if let Some(last_output) = self.last_output {
            self.metadata.output_idle = Some(elapsed(last_output));
        }
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

    fn handle_pty_event(&mut self, event: PtyEvent, events: &mut Vec<EngineEvent>) {
        match event {
            PtyEvent::Output { terminal, bytes } if self.owns(&terminal) => {
                // Feeding at Alacritty's tail follows output itself; a
                // deliberate history offset remains untouched. Metadata still
                // refreshes below even when the visible cells do not change.
                self.frame = self.adapter.feed(&bytes);
                self.refresh_viewport();
                self.metadata.bytes_written = self
                    .metadata
                    .bytes_written
                    .saturating_add(bytes.len() as u64);
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
                let replies = self.adapter.take_pty_replies();
                if !replies.is_empty()
                    && let Some(transport) = self.transport.as_mut()
                    && let Err(message) = transport.write(&replies)
                {
                    events.extend(self.status_changed(TerminalStatus::Failed { message }));
                }
            }
            PtyEvent::StatusChanged { terminal, status } if self.owns(&terminal) => {
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
        self.adapter = VtFrameAdapter::new(self.project.terminal.clone(), self.frame.size);
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
}

impl NativeEngine {
    /// Starts one or more configured terminals at the requested cell dimensions.
    pub fn spawn(projects: &[Project], size: ScreenSize) -> Result<Self, String> {
        let sizes = vec![size; projects.len()];
        Self::spawn_sized(projects, &sizes)
    }

    /// Starts configured terminals at their rendered viewport dimensions.
    pub fn spawn_sized(projects: &[Project], sizes: &[ScreenSize]) -> Result<Self, String> {
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
            .map(|(project, size)| NativeTerminal::spawn(project, size, None))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { terminals })
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
        self.terminals
            .push(NativeTerminal::spawn(project, size, None)?);
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
        self.terminals
            .push(NativeTerminal::spawn(project, size, Some(socket))?);
        Ok(())
    }

    /// Starts a session whose children inherit its ctl rendezvous path.
    pub fn spawn_sized_with_socket(
        projects: &[Project],
        sizes: &[ScreenSize],
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
            .map(|(project, size)| NativeTerminal::spawn(project, size, Some(socket)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { terminals })
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
            if transport.is_process_group_alive()
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
}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl TerminalEngine for NativeEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent> {
        match command {
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
                    Some(Ok(())) | None => Vec::new(),
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
            let pty_events = terminal
                .transport
                .as_ref()
                .map(PtyTransport::drain_events)
                .unwrap_or_default();
            for event in pty_events {
                terminal.handle_pty_event(event, &mut events);
            }
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
}

fn elapsed(instant: Instant) -> Elapsed {
    Elapsed {
        millis: instant.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
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
        path::PathBuf,
        time::{Duration, Instant},
    };

    use crate::contracts::{
        CellContent, EngineCommand, EngineEvent, NotifyKind, Project, ScreenSize, ScrollCommand,
        TerminalEngine, TerminalId, TerminalMetadata, TerminalStatus,
    };

    use super::{NativeEngine, NativeTerminal, SHUTDOWN_GRACE, VtFrameAdapter};

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

        assert!(started.elapsed() >= SHUTDOWN_GRACE);
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

    #[test]
    fn requires_at_least_one_terminal() {
        assert_eq!(
            NativeEngine::spawn(&[], ScreenSize::new(80, 24))
                .err()
                .as_deref(),
            Some("native engine requires at least one terminal")
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
        };
        let mut adapter = VtFrameAdapter::new(terminal.clone(), ScreenSize::new(80, 24));
        adapter.feed(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
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
                last_output: None,
            }],
        };
        engine.dispatch(EngineCommand::Scroll {
            terminal: terminal.clone(),
            command: ScrollCommand::Up(1),
        });

        let metadata = engine.metadata(&terminal).unwrap();
        assert!(metadata.alt_screen, "the app owns the grid");
        assert!(metadata.mouse_reporting, "the app enabled the mouse");
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
        };
        let mut adapter = VtFrameAdapter::new(terminal.clone(), ScreenSize::new(4, 2));
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
                last_output: None,
            }],
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
    fn project(terminal: TerminalId, script: &str) -> Project {
        Project {
            terminal,
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
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
