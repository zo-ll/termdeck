use std::collections::BTreeMap;

use crate::contracts::{
    Elapsed, EngineCommand, EngineEvent, ScreenSize, ScrollCommand, TerminalEngine, TerminalFrame,
    TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
};

/// Deterministic in-memory engine for UI development and tests.
#[derive(Debug)]
pub struct FakeEngine {
    frames: BTreeMap<TerminalId, TerminalFrame>,
    input: Vec<(TerminalId, Vec<u8>)>,
    metadata: BTreeMap<TerminalId, TerminalMetadata>,
    now: Timestamp,
    statuses: BTreeMap<TerminalId, TerminalStatus>,
}

impl FakeEngine {
    pub fn new(terminals: impl IntoIterator<Item = TerminalId>) -> Self {
        let mut engine = Self {
            frames: BTreeMap::new(),
            input: Vec::new(),
            metadata: BTreeMap::new(),
            now: Timestamp::default(),
            statuses: BTreeMap::new(),
        };
        for terminal in terminals {
            engine.frames.insert(
                terminal.clone(),
                TerminalFrame::blank(terminal.clone(), ScreenSize::new(80, 24), 0),
            );
            engine
                .metadata
                .insert(terminal.clone(), TerminalMetadata::default());
            engine.statuses.insert(terminal, TerminalStatus::Starting);
        }
        engine
    }

    /// Supplies a known frame to fixtures without simulating a terminal parser.
    pub fn set_frame(&mut self, frame: TerminalFrame) -> Option<EngineEvent> {
        if self.frames.contains_key(&frame.terminal) {
            self.frames.insert(frame.terminal.clone(), frame.clone());
            Some(EngineEvent::FrameReady(frame))
        } else {
            None
        }
    }

    pub fn set_status(
        &mut self,
        terminal: &TerminalId,
        status: TerminalStatus,
    ) -> Vec<EngineEvent> {
        if !self.statuses.contains_key(terminal) {
            return Vec::new();
        }
        self.statuses.insert(terminal.clone(), status.clone());
        let mut events = vec![EngineEvent::StatusChanged {
            terminal: terminal.clone(),
            status: status.clone(),
        }];
        if matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        ) {
            let metadata = self
                .metadata
                .get_mut(terminal)
                .expect("terminal metadata exists");
            metadata.last_exit_at = Some(self.now);
            metadata.process = None;
            events.push(EngineEvent::MetadataChanged {
                terminal: terminal.clone(),
                metadata: metadata.clone(),
            });
        }
        events
    }

    pub fn set_metadata(
        &mut self,
        terminal: &TerminalId,
        metadata: TerminalMetadata,
    ) -> Option<EngineEvent> {
        if self.metadata.contains_key(terminal) {
            self.metadata.insert(terminal.clone(), metadata.clone());
            Some(EngineEvent::MetadataChanged {
                terminal: terminal.clone(),
                metadata,
            })
        } else {
            None
        }
    }

    /// Records output without pretending to emulate terminal parsing.
    pub fn record_output(&mut self, terminal: &TerminalId, bytes: usize) -> Option<EngineEvent> {
        let metadata = self.metadata.get_mut(terminal)?;
        metadata.bytes_written = metadata.bytes_written.saturating_add(bytes as u64);
        metadata.output_idle = Some(Elapsed::default());
        Some(EngineEvent::MetadataChanged {
            terminal: terminal.clone(),
            metadata: metadata.clone(),
        })
    }

    /// Advances fake time and refreshes elapsed activity/process metadata.
    pub fn advance_time(&mut self, elapsed: Elapsed) -> Vec<EngineEvent> {
        self.now.unix_millis = self.now.unix_millis.saturating_add(elapsed.millis);
        self.metadata
            .iter_mut()
            .filter_map(|(terminal, metadata)| {
                let mut changed = false;
                if let Some(idle) = &mut metadata.output_idle {
                    idle.millis = idle.millis.saturating_add(elapsed.millis);
                    changed = true;
                }
                if let Some(process) = &mut metadata.process {
                    process.uptime.millis = process.uptime.millis.saturating_add(elapsed.millis);
                    changed = true;
                }
                changed.then(|| EngineEvent::MetadataChanged {
                    terminal: terminal.clone(),
                    metadata: metadata.clone(),
                })
            })
            .collect()
    }

    pub fn input(&self) -> &[(TerminalId, Vec<u8>)] {
        &self.input
    }
}

impl TerminalEngine for FakeEngine {
    fn dispatch(&mut self, command: EngineCommand) -> Vec<EngineEvent> {
        match command {
            EngineCommand::Input { terminal, bytes } => {
                if self.frames.contains_key(&terminal) {
                    self.input.push((terminal, bytes));
                }
                Vec::new()
            }
            EngineCommand::Resize { terminal, size } => self
                .frames
                .get_mut(&terminal)
                .map(|frame| {
                    *frame = TerminalFrame::blank(terminal, size, frame.revision + 1);
                    vec![EngineEvent::FrameReady(frame.clone())]
                })
                .unwrap_or_default(),
            EngineCommand::Scroll { terminal, command } => {
                let Some(metadata) = self.metadata.get_mut(&terminal) else {
                    return Vec::new();
                };
                match command {
                    ScrollCommand::Up(lines) => {
                        let moved = metadata.scrollback.lines_above.min(u32::from(lines));
                        metadata.scrollback.lines_above -= moved;
                        metadata.scrollback.lines_below += moved;
                    }
                    ScrollCommand::Down(lines) => {
                        let moved = metadata.scrollback.lines_below.min(u32::from(lines));
                        metadata.scrollback.lines_above += moved;
                        metadata.scrollback.lines_below -= moved;
                    }
                    ScrollCommand::Bottom => {
                        metadata.scrollback.lines_above += metadata.scrollback.lines_below;
                        metadata.scrollback.lines_below = 0;
                    }
                }
                let event = EngineEvent::MetadataChanged {
                    terminal: terminal.clone(),
                    metadata: metadata.clone(),
                };
                self.frames
                    .get(&terminal)
                    .map(|frame| vec![event, EngineEvent::FrameReady(frame.clone())])
                    .unwrap_or_default()
            }
            EngineCommand::Respawn { terminal } => {
                let Some(frame) = self.frames.get_mut(&terminal) else {
                    return Vec::new();
                };
                *frame = TerminalFrame::blank(terminal.clone(), frame.size, frame.revision + 1);
                let status = TerminalStatus::Starting;
                self.statuses.insert(terminal.clone(), status.clone());
                let metadata = self
                    .metadata
                    .get_mut(&terminal)
                    .expect("frame and metadata match");
                let last_exit_at = metadata.last_exit_at;
                *metadata = TerminalMetadata {
                    last_exit_at,
                    restarted_at: Some(self.now),
                    ..TerminalMetadata::default()
                };
                vec![
                    EngineEvent::StatusChanged { terminal, status },
                    EngineEvent::MetadataChanged {
                        terminal: frame.terminal.clone(),
                        metadata: metadata.clone(),
                    },
                    EngineEvent::FrameReady(frame.clone()),
                ]
            }
            EngineCommand::Shutdown => {
                let terminals: Vec<_> = self
                    .statuses
                    .iter()
                    .filter(|(_, status)| {
                        matches!(status, TerminalStatus::Starting | TerminalStatus::Running)
                    })
                    .map(|(terminal, _)| terminal.clone())
                    .collect();
                let mut events = Vec::new();
                for terminal in terminals {
                    let status = TerminalStatus::Exited { code: None };
                    self.statuses.insert(terminal.clone(), status.clone());
                    events.push(EngineEvent::StatusChanged {
                        terminal: terminal.clone(),
                        status,
                    });
                    let metadata = self
                        .metadata
                        .get_mut(&terminal)
                        .expect("terminal metadata exists");
                    metadata.last_exit_at = Some(self.now);
                    metadata.process = None;
                    events.push(EngineEvent::MetadataChanged {
                        terminal,
                        metadata: metadata.clone(),
                    });
                }
                events
            }
        }
    }

    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame> {
        self.frames.get(terminal)
    }

    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus> {
        self.statuses.get(terminal)
    }

    fn metadata(&self, terminal: &TerminalId) -> Option<&TerminalMetadata> {
        self.metadata.get(terminal)
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
}
