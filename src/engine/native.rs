use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{
    contracts::{
        Elapsed, EngineCommand, EngineEvent, ProcessInfo, Project, ScreenSize, TerminalEngine,
        TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
    },
    engine::{PtyEvent, PtyTransport, VtFrameAdapter},
};

/// One PTY-backed terminal behind the application-owned engine contract.
pub struct NativeEngine {
    transport: Option<PtyTransport>,
    adapter: VtFrameAdapter,
    frame: TerminalFrame,
    status: TerminalStatus,
    metadata: TerminalMetadata,
    started: Instant,
    last_output: Option<Instant>,
}

impl NativeEngine {
    /// Starts one configured terminal at the requested cell dimensions.
    pub fn spawn(project: &Project, size: ScreenSize) -> Result<Self, String> {
        let transport = PtyTransport::spawn(project, size)?;
        let adapter = VtFrameAdapter::new(project.terminal.clone(), size);
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
            transport: Some(transport),
            adapter,
            frame,
            status: TerminalStatus::Running,
            started,
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
}

impl TerminalEngine for NativeEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent> {
        match command {
            EngineCommand::Input { terminal, bytes } if self.owns(&terminal) => {
                if matches!(
                    self.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                ) && let Some(transport) = &mut self.transport
                    && let Err(message) = transport.write(&bytes)
                {
                    return self.status_changed(TerminalStatus::Failed { message });
                }
                Vec::new()
            }
            EngineCommand::Resize { terminal, size } if self.owns(&terminal) => {
                if matches!(
                    self.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                ) && let Some(transport) = &mut self.transport
                    && let Err(message) = transport.resize(size)
                {
                    return self.status_changed(TerminalStatus::Failed { message });
                }
                self.frame = self.adapter.resize(size);
                vec![EngineEvent::FrameReady(self.frame.clone())]
            }
            EngineCommand::Scroll { terminal, command } if self.owns(&terminal) => {
                self.frame = self.adapter.scroll(command);
                self.metadata.scrollback = self.adapter.scrollback_position();
                vec![
                    EngineEvent::MetadataChanged {
                        terminal,
                        metadata: self.metadata.clone(),
                    },
                    EngineEvent::FrameReady(self.frame.clone()),
                ]
            }
            EngineCommand::Shutdown => {
                let result = self
                    .transport
                    .take()
                    .map(|mut transport| transport.shutdown())
                    .unwrap_or(Ok(()));
                if matches!(
                    self.status,
                    TerminalStatus::Starting | TerminalStatus::Running
                ) {
                    return match result {
                        Ok(()) => self.status_changed(TerminalStatus::Exited { code: None }),
                        Err(message) => self.status_changed(TerminalStatus::Failed { message }),
                    };
                }
                Vec::new()
            }
            // Respawn belongs to the lifecycle slice; this one-terminal engine
            // deliberately retains the exited terminal and its frame.
            EngineCommand::Respawn { .. } => Vec::new(),
            _ => Vec::new(),
        }
    }

    fn drain_events(&mut self) -> Vec<EngineEvent> {
        let pty_events = self
            .transport
            .as_ref()
            .map(PtyTransport::drain_events)
            .unwrap_or_default();
        let mut events = Vec::new();
        for event in pty_events {
            self.handle_pty_event(event, &mut events);
        }
        events
    }

    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame> {
        self.owns(terminal).then_some(&self.frame)
    }

    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus> {
        self.owns(terminal).then_some(&self.status)
    }

    fn metadata(&self, terminal: &TerminalId) -> Option<&TerminalMetadata> {
        self.owns(terminal).then_some(&self.metadata)
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
        CellContent, EngineCommand, EngineEvent, Project, ScreenSize, TerminalEngine, TerminalId,
        TerminalStatus,
    };

    use super::NativeEngine;

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_flows_through_input_output_resize_and_exit() {
        let terminal = TerminalId::new("native");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned()],
        };
        let mut engine = NativeEngine::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        assert_eq!(engine.status(&terminal), Some(&TerminalStatus::Running));

        let resized = engine.dispatch(EngineCommand::Resize {
            terminal: terminal.clone(),
            size: ScreenSize::new(101, 7),
        });
        assert!(
            matches!(resized.as_slice(), [EngineEvent::FrameReady(frame)] if frame.size == ScreenSize::new(101, 7))
        );
        engine.dispatch(EngineCommand::Input {
            terminal: terminal.clone(),
            bytes: b"printf 'TERMDECK-NATIVE\\n'; stty size; exit 7\n".to_vec(),
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_output_metadata = false;
        let mut saw_exit = false;
        while Instant::now() < deadline
            && !(saw_output_metadata
                && saw_exit
                && frame_text(engine.frame(&terminal).unwrap()).contains("TERMDECK-NATIVE"))
        {
            for event in engine.drain_events() {
                match event {
                    EngineEvent::MetadataChanged { metadata, .. } => {
                        saw_output_metadata |= metadata.bytes_written > 0;
                    }
                    EngineEvent::StatusChanged { status, .. } => {
                        saw_exit |= status == TerminalStatus::Exited { code: Some(7) };
                    }
                    EngineEvent::FrameReady(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let frame = engine.frame(&terminal).unwrap();
        let text = frame_text(frame);
        assert!(text.contains("TERMDECK-NATIVE"), "frame: {text:?}");
        assert!(text.contains("7 101"), "frame: {text:?}");
        assert!(saw_output_metadata);
        assert!(saw_exit);
        assert_eq!(
            engine.status(&terminal),
            Some(&TerminalStatus::Exited { code: Some(7) })
        );
        assert!(engine.metadata(&terminal).unwrap().last_exit_at.is_some());
        assert_eq!(engine.metadata(&terminal).unwrap().process, None);
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
