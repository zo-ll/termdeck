use std::collections::BTreeMap;

use crate::contracts::{
    Elapsed, EngineCommand, EngineEvent, ScreenSize, ScrollCommand, TerminalEngine, TerminalFrame,
    TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
};

#[derive(Debug)]
struct FakeTerminalState {
    frame: TerminalFrame,
    history: Vec<String>,
    metadata: TerminalMetadata,
    status: TerminalStatus,
}

/// Deterministic in-memory engine for UI development and tests.
#[derive(Debug)]
pub struct FakeEngine {
    input: Vec<(TerminalId, Vec<u8>)>,
    now: Timestamp,
    terminals: BTreeMap<TerminalId, FakeTerminalState>,
}

impl FakeEngine {
    pub fn new(terminals: impl IntoIterator<Item = TerminalId>) -> Self {
        Self {
            input: Vec::new(),
            now: Timestamp::default(),
            terminals: terminals
                .into_iter()
                .map(|terminal| {
                    let frame = TerminalFrame::blank(terminal.clone(), ScreenSize::new(80, 24), 0);
                    (
                        terminal,
                        FakeTerminalState {
                            frame,
                            history: Vec::new(),
                            metadata: TerminalMetadata::default(),
                            status: TerminalStatus::Starting,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Supplies a known frame to fixtures without simulating a terminal parser.
    pub fn set_frame(&mut self, frame: TerminalFrame) -> Option<EngineEvent> {
        let terminal = frame.terminal.clone();
        self.terminals.get_mut(&terminal)?.frame = frame.clone();
        Some(EngineEvent::FrameReady(frame))
    }

    pub fn set_status(
        &mut self,
        terminal: &TerminalId,
        status: TerminalStatus,
    ) -> Vec<EngineEvent> {
        let Some(state) = self.terminals.get_mut(terminal) else {
            return Vec::new();
        };
        state.status = status.clone();
        let mut events = vec![EngineEvent::StatusChanged {
            terminal: terminal.clone(),
            status: status.clone(),
        }];
        if matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        ) {
            state.metadata.last_exit_at = Some(self.now);
            state.metadata.process = None;
            events.push(EngineEvent::MetadataChanged {
                terminal: terminal.clone(),
                metadata: state.metadata.clone(),
            });
        }
        events
    }

    pub fn set_metadata(
        &mut self,
        terminal: &TerminalId,
        metadata: TerminalMetadata,
    ) -> Option<EngineEvent> {
        self.terminals.get_mut(terminal)?.metadata = metadata.clone();
        Some(EngineEvent::MetadataChanged {
            terminal: terminal.clone(),
            metadata,
        })
    }

    /// Records output without pretending to emulate terminal parsing.
    pub fn record_output(&mut self, terminal: &TerminalId, bytes: usize) -> Option<EngineEvent> {
        let state = self.terminals.get_mut(terminal)?;
        state.metadata.bytes_written = state.metadata.bytes_written.saturating_add(bytes as u64);
        state.metadata.output_idle = Some(Elapsed::default());
        Some(EngineEvent::MetadataChanged {
            terminal: terminal.clone(),
            metadata: state.metadata.clone(),
        })
    }

    /// Advances fake time and refreshes elapsed activity/process metadata.
    pub fn advance_time(&mut self, elapsed: Elapsed) -> Vec<EngineEvent> {
        self.now.unix_millis = self.now.unix_millis.saturating_add(elapsed.millis);
        self.terminals
            .iter_mut()
            .filter_map(|(terminal, state)| {
                let mut changed = false;
                if let Some(idle) = &mut state.metadata.output_idle {
                    idle.millis = idle.millis.saturating_add(elapsed.millis);
                    changed = true;
                }
                if let Some(process) = &mut state.metadata.process {
                    process.uptime.millis = process.uptime.millis.saturating_add(elapsed.millis);
                    changed = true;
                }
                changed.then(|| EngineEvent::MetadataChanged {
                    terminal: terminal.clone(),
                    metadata: state.metadata.clone(),
                })
            })
            .collect()
    }

    pub fn input(&self) -> &[(TerminalId, Vec<u8>)] {
        &self.input
    }

    /// Supplies retained output to ctl tests and fixtures.
    pub fn set_history_lines(&mut self, terminal: &TerminalId, lines: Vec<String>) -> bool {
        let Some(state) = self.terminals.get_mut(terminal) else {
            return false;
        };
        state.history = lines;
        true
    }
}

impl TerminalEngine for FakeEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent> {
        match command {
            EngineCommand::Input { terminal, bytes } => {
                if self.terminals.contains_key(&terminal) {
                    self.input.push((terminal, bytes));
                }
                Vec::new()
            }
            EngineCommand::Resize { terminal, size } => {
                let Some(state) = self.terminals.get_mut(&terminal) else {
                    return Vec::new();
                };
                state.frame = TerminalFrame::blank(terminal, size, state.frame.revision + 1);
                vec![EngineEvent::FrameReady(state.frame.clone())]
            }
            EngineCommand::Scroll { terminal, command } => {
                let Some(state) = self.terminals.get_mut(&terminal) else {
                    return Vec::new();
                };
                match command {
                    ScrollCommand::Up(lines) => {
                        let moved = state.metadata.scrollback.lines_above.min(u32::from(lines));
                        state.metadata.scrollback.lines_above -= moved;
                        state.metadata.scrollback.lines_below += moved;
                    }
                    ScrollCommand::Down(lines) => {
                        let moved = state.metadata.scrollback.lines_below.min(u32::from(lines));
                        state.metadata.scrollback.lines_above += moved;
                        state.metadata.scrollback.lines_below -= moved;
                    }
                    ScrollCommand::Bottom => {
                        state.metadata.scrollback.lines_above +=
                            state.metadata.scrollback.lines_below;
                        state.metadata.scrollback.lines_below = 0;
                    }
                }
                vec![
                    EngineEvent::MetadataChanged {
                        terminal,
                        metadata: state.metadata.clone(),
                    },
                    EngineEvent::FrameReady(state.frame.clone()),
                ]
            }
            EngineCommand::Respawn { terminal } => {
                let Some(state) = self.terminals.get_mut(&terminal) else {
                    return Vec::new();
                };
                state.frame = TerminalFrame::blank(
                    terminal.clone(),
                    state.frame.size,
                    state.frame.revision + 1,
                );
                state.status = TerminalStatus::Starting;
                state.metadata = TerminalMetadata {
                    last_exit_at: state.metadata.last_exit_at,
                    restarted_at: Some(self.now),
                    ..TerminalMetadata::default()
                };
                vec![
                    EngineEvent::StatusChanged {
                        terminal: terminal.clone(),
                        status: state.status.clone(),
                    },
                    EngineEvent::MetadataChanged {
                        terminal,
                        metadata: state.metadata.clone(),
                    },
                    EngineEvent::FrameReady(state.frame.clone()),
                ]
            }
            EngineCommand::Shutdown => {
                let mut events = Vec::new();
                for (terminal, state) in &mut self.terminals {
                    if matches!(
                        state.status,
                        TerminalStatus::Starting | TerminalStatus::Running
                    ) {
                        state.status = TerminalStatus::Exited { code: None };
                        state.metadata.last_exit_at = Some(self.now);
                        state.metadata.process = None;
                        events.push(EngineEvent::StatusChanged {
                            terminal: terminal.clone(),
                            status: state.status.clone(),
                        });
                        events.push(EngineEvent::MetadataChanged {
                            terminal: terminal.clone(),
                            metadata: state.metadata.clone(),
                        });
                    }
                }
                events
            }
        }
    }

    fn drain_events(&mut self) -> Vec<EngineEvent> {
        Vec::new()
    }

    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame> {
        self.terminals.get(terminal).map(|state| &state.frame)
    }

    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus> {
        self.terminals.get(terminal).map(|state| &state.status)
    }

    fn metadata(&self, terminal: &TerminalId) -> Option<&TerminalMetadata> {
        self.terminals.get(terminal).map(|state| &state.metadata)
    }

    fn history_lines(&self, terminal: &TerminalId, max: usize) -> Option<Vec<String>> {
        let state = self.terminals.get(terminal)?;
        let first = state.history.len().saturating_sub(max);
        Some(state.history[first..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use crate::contracts::{
        Elapsed, EngineCommand, EngineEvent, ProcessInfo, ScreenSize, ScrollCommand,
        ScrollbackPosition, TerminalEngine, TerminalId, TerminalMetadata, TerminalStatus,
        Timestamp,
    };

    use super::FakeEngine;

    #[test]
    fn respawn_resets_the_frame_and_status() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);
        let shutdown = engine.dispatch(EngineCommand::Shutdown);
        assert_eq!(
            engine
                .metadata(&terminal)
                .and_then(|metadata| metadata.last_exit_at),
            Some(Timestamp::default())
        );
        assert!(matches!(shutdown[1], EngineEvent::MetadataChanged { .. }));
        engine.advance_time(Elapsed { millis: 2_000 });

        let events = engine.dispatch(EngineCommand::Respawn {
            terminal: terminal.clone(),
        });

        assert_eq!(engine.status(&terminal), Some(&TerminalStatus::Starting));
        assert_eq!(engine.frame(&terminal).map(|frame| frame.revision), Some(1));
        assert_eq!(
            engine
                .metadata(&terminal)
                .and_then(|metadata| metadata.restarted_at),
            Some(Timestamp { unix_millis: 2_000 })
        );
        assert!(matches!(events[0], EngineEvent::StatusChanged { .. }));
        assert!(matches!(events[1], EngineEvent::MetadataChanged { .. }));
        assert!(matches!(events[2], EngineEvent::FrameReady(_)));
    }

    #[test]
    fn resize_emits_a_revisioned_frame() {
        let terminal = TerminalId::new("backend");
        let mut engine = FakeEngine::new([terminal.clone()]);

        let events = engine.dispatch(EngineCommand::Resize {
            terminal: terminal.clone(),
            size: ScreenSize::new(100, 30),
        });

        assert_eq!(
            engine.frame(&terminal).map(|frame| frame.size),
            Some(ScreenSize::new(100, 30))
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn scrolling_is_engine_owned_and_emits_a_new_viewport() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_metadata(
            &terminal,
            TerminalMetadata {
                scrollback: ScrollbackPosition {
                    lines_above: 20,
                    lines_below: 0,
                },
                ..TerminalMetadata::default()
            },
        );

        let events = engine.dispatch(EngineCommand::Scroll {
            terminal: terminal.clone(),
            command: ScrollCommand::Up(6),
        });

        assert_eq!(
            engine
                .metadata(&terminal)
                .map(|metadata| metadata.scrollback),
            Some(ScrollbackPosition {
                lines_above: 14,
                lines_below: 6,
            })
        );
        assert!(matches!(events[0], EngineEvent::MetadataChanged { .. }));
        assert!(matches!(events[1], EngineEvent::FrameReady(_)));
    }

    #[test]
    fn output_activity_ages_deterministically() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);

        engine.set_metadata(
            &terminal,
            TerminalMetadata {
                process: Some(ProcessInfo {
                    pid: 42,
                    uptime: Elapsed::default(),
                }),
                ..TerminalMetadata::default()
            },
        );
        engine.record_output(&terminal, 42);
        let events = engine.advance_time(Elapsed { millis: 1_500 });

        let metadata = engine.metadata(&terminal).unwrap();
        assert_eq!(metadata.bytes_written, 42);
        assert_eq!(metadata.output_idle, Some(Elapsed { millis: 1_500 }));
        assert_eq!(
            metadata.process,
            Some(ProcessInfo {
                pid: 42,
                uptime: Elapsed { millis: 1_500 },
            })
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn exit_code_lives_only_in_status() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.advance_time(Elapsed { millis: 500 });

        let events = engine.set_status(&terminal, TerminalStatus::Exited { code: Some(7) });

        assert_eq!(
            engine.status(&terminal),
            Some(&TerminalStatus::Exited { code: Some(7) })
        );
        assert_eq!(
            engine
                .metadata(&terminal)
                .and_then(|metadata| metadata.last_exit_at),
            Some(Timestamp { unix_millis: 500 })
        );
        assert!(matches!(events[1], EngineEvent::MetadataChanged { .. }));
    }

    #[test]
    fn fake_has_no_unsolicited_events() {
        let mut engine = FakeEngine::new([TerminalId::new("frontend")]);

        assert!(engine.drain_events().is_empty());
    }
}
