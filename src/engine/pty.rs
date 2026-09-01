use std::{
    io::{Read, Write},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::contracts::{Project, ScreenSize, TerminalId, TerminalStatus};

const EVENT_CAPACITY: usize = 16;
const READ_BUFFER_SIZE: usize = 4096;

/// Bytes and lifecycle changes from one PTY. This stays inside the engine;
/// callers at the UI boundary receive only the application-owned contracts.
#[derive(Debug)]
pub enum PtyEvent {
    Output {
        terminal: TerminalId,
        bytes: Vec<u8>,
    },
    StatusChanged {
        terminal: TerminalId,
        status: TerminalStatus,
    },
}

/// A single host shell and its bounded PTY transport.
pub struct PtyTransport {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    process_group: Option<u32>,
    events: Option<Receiver<PtyEvent>>,
    reader: Option<JoinHandle<()>>,
    waiter: Option<JoinHandle<()>>,
}

impl PtyTransport {
    /// Starts a configured host shell in its configured working directory.
    pub fn spawn(project: &Project, size: ScreenSize) -> Result<Self, String> {
        let Some((program, arguments)) = project.command.split_first() else {
            return Err(format!("{}: command must not be empty", project.terminal));
        };
        let mut command = CommandBuilder::new(program);
        command.args(arguments);
        command.cwd(&project.path);
        Self::spawn_command(project.terminal.clone(), command, size)
    }

    fn spawn_command(
        terminal: TerminalId,
        command: CommandBuilder,
        size: ScreenSize,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| error.to_string())?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        let process_group = child.process_id();
        let killer = child.clone_killer();
        let (sender, events) = mpsc::sync_channel(EVENT_CAPACITY);

        let reader_terminal = terminal.clone();
        let reader_sender = sender.clone();
        let reader = thread::spawn(move || read_output(reader, reader_terminal, reader_sender));
        let waiter_terminal = terminal.clone();
        let waiter = thread::spawn(move || {
            let status = match child.wait() {
                Ok(status) => TerminalStatus::Exited {
                    code: Some(status.exit_code() as i32),
                },
                Err(error) => TerminalStatus::Failed {
                    message: error.to_string(),
                },
            };
            let _ = sender.send(PtyEvent::StatusChanged {
                terminal: waiter_terminal,
                status,
            });
        });

        Ok(Self {
            master: pair.master,
            writer,
            killer,
            process_group,
            events: Some(events),
            reader: Some(reader),
            waiter: Some(waiter),
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
        self.writer.flush().map_err(|error| error.to_string())
    }

    pub fn resize(&mut self, size: ScreenSize) -> Result<(), String> {
        self.master
            .resize(pty_size(size))
            .map_err(|error| error.to_string())
    }

    pub fn process_id(&self) -> Option<u32> {
        self.process_group
    }

    pub fn drain_events(&self) -> Vec<PtyEvent> {
        self.events
            .as_ref()
            .map(|events| events.try_iter().collect())
            .unwrap_or_default()
    }

    /// Terminates one process group and waits through the normal grace period.
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.events.is_none() {
            return Ok(());
        }
        let result = self.request_shutdown();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while self.is_process_group_alive() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let kill_result = self
            .is_process_group_alive()
            .then(|| self.force_shutdown())
            .transpose();
        self.join();
        result.and(kill_result.map(|_| ()))
    }

    /// Starts shutdown without waiting. This permits a multi-terminal owner to
    /// send TERM to every process group before using one shared grace period.
    pub fn request_shutdown(&mut self) -> Result<(), String> {
        self.events.take();
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            return send_signal(process_group, libc::SIGTERM);
        }

        self.killer.kill().map_err(|error| error.to_string())
    }

    /// Reports whether the owned process group still has a live member.
    pub fn is_process_group_alive(&self) -> bool {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            return process_group_alive(process_group);
        }

        false
    }

    /// Kills a process group that survived the TERM grace period.
    pub fn force_shutdown(&mut self) -> Result<(), String> {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            return send_signal(process_group, libc::SIGKILL);
        }

        self.killer.kill().map_err(|error| error.to_string())
    }

    /// Joins the reader and waiter after their owner has ended the process.
    pub fn join(&mut self) {
        self.events.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn has_joined_threads(&self) -> bool {
        self.reader.is_none() && self.waiter.is_none()
    }
}

impl Drop for PtyTransport {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn pty_size(size: ScreenSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    terminal: TerminalId,
    sender: mpsc::SyncSender<PtyEvent>,
) {
    let mut buffer = [0; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => {
                if sender
                    .send(PtyEvent::Output {
                        terminal: terminal.clone(),
                        bytes: buffer[..count].to_vec(),
                    })
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                if is_end_of_pty(&error) {
                    return;
                }
                let _ = sender.send(PtyEvent::StatusChanged {
                    terminal,
                    status: TerminalStatus::Failed {
                        message: error.to_string(),
                    },
                });
                return;
            }
        }
    }
}

#[cfg(unix)]
fn is_end_of_pty(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::EIO)
}

#[cfg(not(unix))]
fn is_end_of_pty(_: &std::io::Error) -> bool {
    false
}

#[cfg(unix)]
fn send_signal(process_group: u32, signal: libc::c_int) -> Result<(), String> {
    // `portable-pty` creates the child as the session/process-group leader.
    let result = unsafe { libc::kill(-(process_group as libc::pid_t), signal) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(unix)]
fn process_group_alive(process_group: u32) -> bool {
    let result = unsafe { libc::kill(-(process_group as libc::pid_t), 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use std::path::PathBuf;

    use crate::contracts::{Project, ScreenSize, TerminalId, TerminalStatus};

    use super::{PtyEvent, PtyTransport};

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_forwards_input_applies_resize_and_reports_exit() {
        let terminal = TerminalId::new("transport");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned()],
        };
        let mut transport = PtyTransport::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        transport.resize(ScreenSize::new(101, 7)).unwrap();
        transport
            .write(b"printf 'TERMDECK-INPUT\n'; stty size; exit 7\n")
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut exited = None;
        while Instant::now() < deadline
            && !(exited.is_some()
                && String::from_utf8_lossy(&output).contains("TERMDECK-INPUT")
                && String::from_utf8_lossy(&output).contains("7 101"))
        {
            for event in transport.drain_events() {
                match event {
                    PtyEvent::Output {
                        terminal: event_terminal,
                        bytes,
                    } => {
                        assert_eq!(event_terminal, terminal);
                        output.extend(bytes);
                    }
                    PtyEvent::StatusChanged {
                        terminal: event_terminal,
                        status,
                    } => {
                        assert_eq!(event_terminal, terminal);
                        if matches!(status, TerminalStatus::Exited { .. }) {
                            exited = Some(status);
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("TERMDECK-INPUT"), "output: {output:?}");
        assert!(output.contains("7 101"), "output: {output:?}");
        assert_eq!(exited, Some(TerminalStatus::Exited { code: Some(7) }));
        transport.shutdown().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_terminates_the_shell_process_group() {
        let project = Project {
            terminal: TerminalId::new("transport"),
            path: PathBuf::from("/"),
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "sleep 30 & wait".to_owned(),
            ],
        };
        let mut transport = PtyTransport::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        let process_group = transport.process_group.unwrap();

        std::thread::sleep(Duration::from_millis(100));
        transport.shutdown().unwrap();

        assert!(!super::process_group_alive(process_group));
        assert!(transport.has_joined_threads());
    }
}
