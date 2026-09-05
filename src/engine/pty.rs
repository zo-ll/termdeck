use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

#[cfg(unix)]
use std::os::unix::io::RawFd;

use crate::{
    contracts::{Project, ScreenSize, TerminalId, TerminalStatus},
    engine::shell_hook::ShellHook,
};

const EVENT_CAPACITY: usize = 16;
const READ_BUFFER_SIZE: usize = 4096;
/// Bytes one PTY may hold for a child that has stopped reading (#118). At
/// 1 MiB it comfortably accepts the 512 KiB audit paste while keeping a
/// wedged pane's footprint fixed. Single ctl.v1 requests are bounded far
/// below it (`MAX_LINE`), so one request always fits an empty queue and a
/// refusal is atomic — never a silent prefix.
pub const INPUT_QUEUE_CAP: usize = 1024 * 1024;
/// Largest single slice a nonblocking flush hands to the PTY. Small enough
/// that one frame's pump never outstays its welcome, large enough that a
/// healthy child drains in few passes.
const INPUT_WRITE_CHUNK: usize = 8192;

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

/// What one nonblocking input write did with its bytes (#118).
///
/// Input is "accepted" the moment it lands in the transport's bounded
/// queue: the caller never blocks, whatever the child is doing. "Written"
/// happens afterwards, in nonblocking slices, as the child reads. When the
/// queue is saturated the whole request is refused and nothing is stored —
/// a partial acceptance would leave the caller believing delivered bytes it
/// never sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputOutcome {
    /// Every byte reached the PTY; nothing is queued.
    Flushed,
    /// Bytes are queued behind a slow reader; the frame pump flushes them.
    Queued,
    /// The bounded queue had no room; nothing was stored or written.
    Refused,
}

/// A single host shell and its bounded PTY transport.
pub struct PtyTransport {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    process_group: Option<u32>,
    /// Bytes accepted but not yet written: the bounded queue between the
    /// session loop and a child that may have stopped reading (#118).
    /// `dispatch` appends without blocking; the frame pump (`flush_input`
    /// from `drain_events`) moves it to the PTY in nonblocking slices.
    pending_input: VecDeque<u8>,
    events: Option<Receiver<PtyEvent>>,
    reader: Option<JoinHandle<()>>,
    waiter: Option<JoinHandle<()>>,
    _shell_hook: Option<ShellHook>,
}

impl PtyTransport {
    /// Starts a configured host shell in its configured working directory.
    pub fn spawn(project: &Project, size: ScreenSize) -> Result<Self, String> {
        Self::spawn_with_socket(project, size, None)
    }

    /// Starts a project with the current session's ctl rendezvous variables.
    /// CommandBuilder inherits the parent environment, and these explicit
    /// assignments deliberately replace any outer Termdeck values. The shell
    /// hook marker is deliberately not inherited: a nested Termdeck must
    /// install its own hook in each of its child panes.
    pub fn spawn_with_socket(
        project: &Project,
        size: ScreenSize,
        socket: Option<&Path>,
    ) -> Result<Self, String> {
        let Some((program, arguments)) = project.command.split_first() else {
            return Err(format!("{}: command must not be empty", project.terminal));
        };
        let mut command = CommandBuilder::new(program);
        let shell_hook = if project.shell_hook {
            ShellHook::install(program, arguments, &mut command)?
        } else {
            None
        };
        if shell_hook.is_none() {
            command.args(arguments);
        }
        command.cwd(&project.path);
        inject_session_environment(&mut command, project, socket);
        Self::spawn_command(project.terminal.clone(), command, size, shell_hook)
    }

    fn spawn_command(
        terminal: TerminalId,
        command: CommandBuilder,
        size: ScreenSize,
        shell_hook: Option<ShellHook>,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        let read_fd: RawFd = pair
            .master
            .as_raw_fd()
            .ok_or_else(|| "pty master has no file descriptor".to_owned())?;
        // The whole master shares one open file description across the
        // reader, the writer and the handle below, so this one flag makes
        // every direction nonblocking at once (#118). The slave side the
        // child reads is a separate open, so the child's blocking semantics
        // are untouched; the reader below polls, and input goes out in
        // nonblocking slices from `flush_input`.
        #[cfg(unix)]
        set_nonblocking(read_fd)?;
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
        let reader = thread::spawn(move || {
            #[cfg(unix)]
            read_output(reader, reader_terminal, reader_sender, read_fd);
            #[cfg(not(unix))]
            read_output(reader, reader_terminal, reader_sender);
        });
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
            pending_input: VecDeque::new(),
            events: Some(events),
            reader: Some(reader),
            waiter: Some(waiter),
            _shell_hook: shell_hook,
        })
    }

    /// Queues input for the PTY without ever blocking the caller (#118).
    ///
    /// Bytes are accepted into the bounded queue first; what the child can
    /// take right now goes out in nonblocking slices before this returns,
    /// and the frame pump flushes the rest. A saturated queue refuses the
    /// whole request — never a prefix — so the caller always knows whether
    /// anything reached the child. Only a dead PTY is an error, as before.
    pub fn write(&mut self, bytes: &[u8]) -> Result<InputOutcome, String> {
        #[cfg(not(unix))]
        {
            self.writer
                .write_all(bytes)
                .map_err(|error| error.to_string())?;
            self.writer.flush().map_err(|error| error.to_string())?;
            return Ok(InputOutcome::Flushed);
        }
        #[cfg(unix)]
        {
            if self.pending_input.len().saturating_add(bytes.len()) > INPUT_QUEUE_CAP {
                return Ok(InputOutcome::Refused);
            }
            self.pending_input.extend(bytes);
            self.flush_input()
        }
    }

    /// Moves queued input to the PTY in nonblocking slices (#118). The
    /// frame pump calls this every drain, so bytes queued while the child
    /// was slow need no new keystroke to reach it. A write error means the
    /// child is gone: the undeliverable remainder is dropped and the caller
    /// learns it the same way a synchronous write used to report it.
    ///
    /// The master went nonblocking at spawn, so `write` on the owned writer
    /// reports `WouldBlock` instead of parking the session loop; partial
    /// writes advance the queue and the rest waits for the next pump.
    pub(crate) fn flush_input(&mut self) -> Result<InputOutcome, String> {
        #[cfg(not(unix))]
        {
            return Ok(InputOutcome::Flushed);
        }
        #[cfg(unix)]
        {
            while !self.pending_input.is_empty() {
                let (front, _) = self.pending_input.as_slices();
                let chunk = &front[..front.len().min(INPUT_WRITE_CHUNK)];
                match self.writer.write(chunk) {
                    Ok(0) => break,
                    Ok(written) => {
                        self.pending_input.drain(..written);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                        continue;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(error) => {
                        self.pending_input.clear();
                        return Err(error.to_string());
                    }
                }
            }
            Ok(if self.pending_input.is_empty() {
                InputOutcome::Flushed
            } else {
                InputOutcome::Queued
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_input_len(&self) -> usize {
        self.pending_input.len()
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

    /// Hangs up and terminates one process group, then waits through the
    /// normal grace period.
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
    /// signal every process group before using one shared grace period.
    ///
    /// Both HUP and TERM go out, in that order, because a workspace's panes
    /// are shells and **an interactive shell ignores SIGTERM by design**
    /// (POSIX: an interactive shell shall ignore SIGTERM so that a stray kill
    /// cannot drop the user's session). TERM alone therefore leaves every
    /// pane running until the grace period expires and SIGKILL lands, which
    /// is what made a confirmed quit hang for the full two seconds (#46).
    ///
    /// HUP is what a closing terminal delivers, and it is the signal a shell
    /// does answer: it exits, running its logout path on the way out. TERM
    /// still follows it, so a pane running something that cleans up on TERM
    /// gets what it expects, and the grace period and the SIGKILL escalation
    /// behind it are unchanged.
    pub fn request_shutdown(&mut self) -> Result<(), String> {
        self.events.take();
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            let hangup = send_signal(process_group, libc::SIGHUP);
            return hangup.and(send_signal(process_group, libc::SIGTERM));
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

fn inject_session_environment(
    command: &mut CommandBuilder,
    project: &Project,
    socket: Option<&Path>,
) {
    command.env_remove("TERMDECK_SHELL_HOOK");
    if let Some(socket) = socket {
        command.env("TERMDECK_SOCK", socket);
        command.env("TERMDECK_PANE", project.terminal.to_string());
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
    #[cfg(unix)] poll_fd: RawFd,
) {
    let mut buffer = [0; READ_BUFFER_SIZE];
    loop {
        // The master went nonblocking at spawn, so a bare `read` here
        // would spin on `WouldBlock`. Polling first keeps the old blocking
        // shape — sleep in the kernel until output, the hangup, or a short
        // timeout — without burning a thread per idle pane (#118).
        #[cfg(unix)]
        match wait_readable(poll_fd) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(message) => {
                let _ = sender.send(PtyEvent::StatusChanged {
                    terminal,
                    status: TerminalStatus::Failed { message },
                });
                return;
            }
        }
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
                // A polled read can still lose the race and find nothing;
                // that is a retry, not a failure.
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    continue;
                }
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

/// Sleeps in the kernel until the PTY has output, hung up, or 100 ms pass.
/// The timeout rejoins the loop so a dead child is noticed on the next
/// read; shutdown always kills the child first, which wakes the poll at
/// once through the hangup.
#[cfg(unix)]
fn wait_readable(fd: RawFd) -> Result<bool, String> {
    let mut watched = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `watched` is one valid descriptor record.
    let result = unsafe { libc::poll(&mut watched, 1, 100) };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error.to_string());
    }
    Ok(result > 0)
}

/// Makes the shared master description nonblocking (#118). Reader, writer
/// and handle are dups of one description, so one flag covers all three.
#[cfg(unix)]
fn set_nonblocking(fd: RawFd) -> Result<(), String> {
    // SAFETY: `fcntl` on an owned fd with integer arguments.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: as above; `O_NONBLOCK` keeps the other status flags.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
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
pub(crate) fn process_group_alive(process_group: u32) -> bool {
    let result = unsafe { libc::kill(-(process_group as libc::pid_t), 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use portable_pty::CommandBuilder;

    use crate::contracts::{Project, ScreenSize, TerminalId, TerminalStatus};

    use super::{
        INPUT_QUEUE_CAP, InputOutcome, PtyEvent, PtyTransport, inject_session_environment,
    };

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_forwards_input_applies_resize_and_reports_exit() {
        let terminal = TerminalId::new("transport");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned()],
            shell_hook: false,
        };
        let mut transport = PtyTransport::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        transport.resize(ScreenSize::new(101, 7)).unwrap();
        transport
            .write(b"printf 'TERMDECK-INPUT\n'; stty size; exit 7\n")
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(15);
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

    /// A child that never reads its slave side: `sleep` holds no
    /// descriptor open for reading, so the master's input buffers stay full.
    #[cfg(target_os = "linux")]
    fn stuck_project(terminal: &str) -> Project {
        Project {
            terminal: TerminalId::new(terminal),
            path: PathBuf::from("/"),
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "stty raw -echo; exec sleep 30".to_owned(),
            ],
            shell_hook: false,
        }
    }

    /// #118: the audit froze the engine's main thread beyond four seconds
    /// with a 512 KiB write to a raw-mode child that had stopped reading.
    /// The write must be accepted (queued) without blocking the caller, so
    /// a two-second bound fails the old synchronous `write_all` by a wide
    /// margin and passes the queue in milliseconds.
    #[cfg(target_os = "linux")]
    #[test]
    fn large_input_to_a_child_that_stopped_reading_never_blocks_the_caller() {
        let mut transport =
            PtyTransport::spawn(&stuck_project("stuck"), ScreenSize::new(80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let big = vec![b'x'; 512 * 1024];

        let started = Instant::now();
        let outcome = transport.write(&big).unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "a stuck child must not block input, took {elapsed:?}"
        );
        assert_eq!(
            outcome,
            InputOutcome::Queued,
            "512 KiB cannot fit a PTY buffer"
        );
        assert!(
            transport.pending_input_len() > 0,
            "accepted means queued behind the stuck child"
        );
        assert!(
            transport.pending_input_len() <= INPUT_QUEUE_CAP,
            "the queue stays bounded"
        );
        transport.shutdown().unwrap();
    }

    /// #118: once the bounded queue is saturated, further input is refused
    /// atomically — nothing stored, nothing written — instead of hanging.
    /// Refusal is the backpressure; it is never a silent prefix.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_saturated_input_queue_refuses_instead_of_hanging() {
        let mut transport =
            PtyTransport::spawn(&stuck_project("full"), ScreenSize::new(80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let chunk = vec![b'x'; 64 * 1024];
        let started = Instant::now();
        let mut refused = false;
        for _ in 0..64 {
            match transport.write(&chunk).unwrap() {
                InputOutcome::Refused => {
                    refused = true;
                    break;
                }
                InputOutcome::Flushed | InputOutcome::Queued => {}
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "queueing 4 MiB must take milliseconds, not block"
            );
        }
        assert!(refused, "a child that never reads must saturate the queue");
        assert!(
            transport.pending_input_len() <= INPUT_QUEUE_CAP,
            "the queue never exceeds its bound"
        );
        // A refusal stores nothing: a further full chunk still does not
        // fit while the child is stuck.
        assert_eq!(
            transport.write(&chunk).unwrap(),
            InputOutcome::Refused,
            "a saturated queue keeps refusing"
        );
        transport.shutdown().unwrap();
    }

    /// #118, the other half of accepted-vs-written: a child that reads gets
    /// a small input straight through, with nothing left queued.
    #[cfg(target_os = "linux")]
    #[test]
    fn small_input_to_a_reading_child_flushes_straight_through() {
        let project = Project {
            terminal: TerminalId::new("reader"),
            path: PathBuf::from("/"),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "exec cat".to_owned()],
            shell_hook: false,
        };
        let mut transport = PtyTransport::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        assert_eq!(
            transport.write(b"TERMDECK-QUEUED\n").unwrap(),
            InputOutcome::Flushed
        );
        assert_eq!(transport.pending_input_len(), 0);
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut output = Vec::new();
        while Instant::now() < deadline
            && !String::from_utf8_lossy(&output).contains("TERMDECK-QUEUED")
        {
            for event in transport.drain_events() {
                if let PtyEvent::Output { bytes, .. } = event {
                    output.extend(bytes);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            String::from_utf8_lossy(&output).contains("TERMDECK-QUEUED"),
            "the flushed bytes reached the child"
        );
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
            shell_hook: false,
        };
        let mut transport = PtyTransport::spawn(&project, ScreenSize::new(80, 24)).unwrap();
        let process_group = transport.process_group.unwrap();

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline && !super::process_group_alive(process_group) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(super::process_group_alive(process_group));
        transport.shutdown().unwrap();

        assert!(!super::process_group_alive(process_group));
        assert!(transport.has_joined_threads());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn child_environment_overrides_the_outer_session_and_clears_the_hook_marker() {
        let terminal = TerminalId::new("inner-pane");
        let project = Project {
            terminal: terminal.clone(),
            path: PathBuf::from("/"),
            command: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "printf '%s|%s|' \"$TERMDECK_SOCK\" \"$TERMDECK_PANE\"; if [ -z \"${TERMDECK_SHELL_HOOK+x}\" ]; then printf absent; else printf present; fi"
                    .to_owned(),
            ],
            shell_hook: false,
        };
        let mut command = CommandBuilder::new("/bin/sh");
        command.env("TERMDECK_SHELL_HOOK", "outer-hook");
        command.args(&project.command[1..]);
        inject_session_environment(
            &mut command,
            &project,
            Some(std::path::Path::new("/tmp/inner.sock")),
        );
        let mut transport =
            PtyTransport::spawn_command(terminal, command, ScreenSize::new(80, 24), None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut output = Vec::new();
        while Instant::now() < deadline && !String::from_utf8_lossy(&output).contains("inner-pane")
        {
            for event in transport.drain_events() {
                if let PtyEvent::Output { bytes, .. } = event {
                    output.extend(bytes);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            String::from_utf8_lossy(&output).contains("/tmp/inner.sock|inner-pane"),
            "output: {:?}",
            String::from_utf8_lossy(&output)
        );
        assert!(
            String::from_utf8_lossy(&output).contains("|absent"),
            "output: {:?}",
            String::from_utf8_lossy(&output)
        );
        transport.shutdown().unwrap();
    }
}
