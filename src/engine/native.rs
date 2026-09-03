use std::{
    collections::BTreeSet,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    contracts::{
        Elapsed, EngineCommand, EngineEvent, ProcessInfo, Project, ScreenSize, TerminalEngine,
        TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
    },
    engine::{PtyEvent, PtyTransport, VtFrameAdapter},
};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

struct NativeTerminal {
    project: Project,
    transport: Option<PtyTransport>,
    adapter: VtFrameAdapter,
    frame: TerminalFrame,
    status: TerminalStatus,
    metadata: TerminalMetadata,
    started: Instant,
    last_output: Option<Instant>,
}

impl NativeTerminal {
    fn spawn(project: Project, size: ScreenSize) -> Result<Self, String> {
        let transport = PtyTransport::spawn(&project, size)?;
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
                self.frame = self.adapter.feed(&bytes);
                self.metadata.scrollback = self.adapter.scrollback_position();
                self.metadata.bytes_written = self
                    .metadata
                    .bytes_written
                    .saturating_add(bytes.len() as u64);
                self.last_output = Some(Instant::now());
                self.refresh_timing();
                events.push(EngineEvent::FrameReady(self.frame.clone()));
                events.push(EngineEvent::MetadataChanged {
                    terminal,
                    metadata: self.metadata.clone(),
                });
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

        let transport = PtyTransport::spawn(&self.project, self.frame.size)?;
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
        if projects.is_empty() {
            return Err("native engine requires at least one terminal".to_owned());
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
            .map(|project| NativeTerminal::spawn(project, size))
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
        self.terminals.push(NativeTerminal::spawn(project, size)?);
        Ok(())
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
                vec![EngineEvent::FrameReady(item.frame.clone())]
            }
            EngineCommand::Scroll { terminal, command } => {
                let Some(item) = self.terminal_mut(&terminal) else {
                    return Vec::new();
                };
                item.frame = item.adapter.scroll(command);
                item.metadata.scrollback = item.adapter.scrollback_position();
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
        CellContent, EngineCommand, EngineEvent, Project, ScreenSize, ScrollCommand,
        TerminalEngine, TerminalId, TerminalStatus,
    };

    use super::{NativeEngine, SHUTDOWN_GRACE};

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

    #[test]
    fn requires_at_least_one_terminal() {
        assert_eq!(
            NativeEngine::spawn(&[], ScreenSize::new(80, 24))
                .err()
                .as_deref(),
            Some("native engine requires at least one terminal")
        );
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
