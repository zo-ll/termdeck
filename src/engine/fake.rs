use std::collections::BTreeMap;

use crate::contracts::{
    EngineCommand, EngineEvent, ScreenSize, TerminalEngine, TerminalFrame, TerminalId,
    TerminalStatus,
};

/// Deterministic in-memory engine for UI development and tests.
#[derive(Debug)]
pub struct FakeEngine {
    frames: BTreeMap<TerminalId, TerminalFrame>,
    input: Vec<(TerminalId, Vec<u8>)>,
    statuses: BTreeMap<TerminalId, TerminalStatus>,
}

impl FakeEngine {
    pub fn new(terminals: impl IntoIterator<Item = TerminalId>) -> Self {
        let mut engine = Self {
            frames: BTreeMap::new(),
            input: Vec::new(),
            statuses: BTreeMap::new(),
        };
        for terminal in terminals {
            engine.frames.insert(
                terminal.clone(),
                TerminalFrame::blank(terminal.clone(), ScreenSize::new(80, 24), 0),
            );
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
    ) -> Option<EngineEvent> {
        if self.statuses.contains_key(terminal) {
            self.statuses.insert(terminal.clone(), status.clone());
            Some(EngineEvent::StatusChanged {
                terminal: terminal.clone(),
                status,
            })
        } else {
            None
        }
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
            EngineCommand::Respawn { terminal } => {
                let Some(frame) = self.frames.get_mut(&terminal) else {
                    return Vec::new();
                };
                *frame = TerminalFrame::blank(terminal.clone(), frame.size, frame.revision + 1);
                let status = TerminalStatus::Starting;
                self.statuses.insert(terminal.clone(), status.clone());
                vec![
                    EngineEvent::StatusChanged { terminal, status },
                    EngineEvent::FrameReady(frame.clone()),
                ]
            }
            EngineCommand::Shutdown => self
                .statuses
                .iter_mut()
                .filter_map(|(terminal, status)| {
                    if matches!(status, TerminalStatus::Starting | TerminalStatus::Running) {
                        *status = TerminalStatus::Exited { code: None };
                        Some(EngineEvent::StatusChanged {
                            terminal: terminal.clone(),
                            status: status.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    fn frame(&self, terminal: &TerminalId) -> Option<&TerminalFrame> {
        self.frames.get(terminal)
    }

    fn status(&self, terminal: &TerminalId) -> Option<&TerminalStatus> {
        self.statuses.get(terminal)
    }
}

#[cfg(test)]
mod tests {
    use crate::contracts::{
        EngineCommand, EngineEvent, ScreenSize, TerminalEngine, TerminalId, TerminalStatus,
    };

    use super::FakeEngine;

    #[test]
    fn respawn_resets_the_frame_and_status() {
        let terminal = TerminalId::new("frontend");
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_status(&terminal, TerminalStatus::Exited { code: Some(1) });

        let events = engine.dispatch(EngineCommand::Respawn {
            terminal: terminal.clone(),
        });

        assert_eq!(engine.status(&terminal), Some(&TerminalStatus::Starting));
        assert_eq!(engine.frame(&terminal).map(|frame| frame.revision), Some(1));
        assert!(matches!(events[0], EngineEvent::StatusChanged { .. }));
        assert!(matches!(events[1], EngineEvent::FrameReady(_)));
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
}
