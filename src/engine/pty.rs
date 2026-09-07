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
/// Upper bound for confirming a force SIGKILL landed before reaping helper
/// threads (#119). SIGKILL death is prompt; this only absorbs scheduling
/// and reaping latency so the common path joins instead of detaching.
const FORCE_SETTLE: Duration = Duration::from_millis(500);
/// Upper bound for reaping one transport's helper threads in total (#119).
/// After the SIGKILL above, the reader sees EOF and the waiter reaps, so
/// both finish promptly; what cannot finish is detached, never waited out.
const JOIN_TIMEOUT: Duration = Duration::from_millis(500);
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
///
/// Process-ownership boundary (#119): every process in the spawned shell's
/// session (session id == shell pid — `portable-pty` makes the child a
/// session leader), plus any descendant that escaped the session outright
/// (nested sessions, `setsid` daemons), snapshotted while still parented.
/// An interactive shell's jobs live in SEPARATE process groups that survive
/// the shell's own group signals when they ignore SIGHUP, so the group
/// alone is not the boundary; the session is, with the snapshot behind it.
pub struct PtyTransport {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    process_group: Option<u32>,
    /// The shell's start time (Linux `/proc` clock ticks), recorded at
    /// spawn to validate the session still belongs to us before a
    /// session-wide signal: a pid can be reused after our shell dies.
    #[cfg(target_os = "linux")]
    shell_start: Option<u64>,
    /// Descendant (pid, start time) pairs snapshotted at shutdown start,
    /// while the tree is still parented. Catches processes that left our
    /// session (nested sessions, detached daemons); each kill revalidates
    /// the start time so a reused pid is never signalled.
    #[cfg(target_os = "linux")]
    descendants: Vec<(u32, u64)>,
    /// Bytes accepted but not yet written: the bounded queue between the
    /// session loop and a child that may have stopped reading (#118).
    /// `dispatch` appends without blocking; the frame pump (`flush_input`
    /// from `drain_events`) moves it to the PTY in nonblocking slices.
    pending_input: VecDeque<u8>,
    /// Hooked shells acknowledge their first prompt with a private marker.
    /// Input queued before that child-side signal stays out of the PTY (#136).
    input_ready: bool,
    input_ready_marker: Option<Vec<u8>>,
    input_ready_carry: Vec<u8>,
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
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        if let Some(bootstrap) = shell_hook.as_ref().and_then(ShellHook::bootstrap) {
            writer
                .write_all(bootstrap)
                .map_err(|error| error.to_string())?;
        }
        let process_group = child.process_id();
        let killer = child.clone_killer();
        // Recorded before anything can exit: the reuse guard below
        // compares against this baseline (#119).
        #[cfg(target_os = "linux")]
        let shell_start = process_group.and_then(proc_starttime);
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

        let input_ready = shell_hook.is_none();
        let input_ready_marker = shell_hook.as_ref().map(|hook| hook.ready_marker().to_vec());
        Ok(Self {
            master: pair.master,
            writer,
            killer,
            process_group,
            #[cfg(target_os = "linux")]
            shell_start,
            #[cfg(target_os = "linux")]
            descendants: Vec::new(),
            pending_input: VecDeque::new(),
            input_ready,
            input_ready_marker,
            input_ready_carry: Vec::new(),
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
            // A same-turn hooked-shell dispatch is always acknowledged as
            // queued. The frame pump releases it only after the shell's
            // first prompt emits its private ready marker (#136).
            if !self.input_ready {
                return Ok(InputOutcome::Queued);
            }
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
            if !self.input_ready {
                return Ok(InputOutcome::Queued);
            }
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

    pub(crate) fn waiting_for_input_ready(&self) -> bool {
        !self.input_ready
    }

    pub fn drain_events(&mut self) -> Vec<PtyEvent> {
        let events: Vec<PtyEvent> = self
            .events
            .as_ref()
            .map(|events| events.try_iter().collect())
            .unwrap_or_default();
        let mut filtered = Vec::with_capacity(events.len());
        for event in events {
            match event {
                PtyEvent::Output { terminal, bytes } => {
                    let bytes = self.consume_input_ready_marker(bytes);
                    if !bytes.is_empty() {
                        filtered.push(PtyEvent::Output { terminal, bytes });
                    }
                }
                PtyEvent::StatusChanged { terminal, status } => {
                    if !self.input_ready_carry.is_empty() {
                        filtered.push(PtyEvent::Output {
                            terminal: terminal.clone(),
                            bytes: std::mem::take(&mut self.input_ready_carry),
                        });
                    }
                    filtered.push(PtyEvent::StatusChanged { terminal, status });
                }
            }
        }
        filtered
    }

    fn consume_input_ready_marker(&mut self, bytes: Vec<u8>) -> Vec<u8> {
        let Some(marker) = self.input_ready_marker.as_ref() else {
            return bytes;
        };
        let mut combined = std::mem::take(&mut self.input_ready_carry);
        combined.extend(bytes);
        if let Some(start) = combined
            .windows(marker.len())
            .position(|window| window == marker)
        {
            combined.drain(start..start + marker.len());
            self.input_ready = true;
            return combined;
        }
        let tail = (1..marker.len().min(combined.len() + 1))
            .rev()
            .find(|&length| marker.starts_with(&combined[combined.len() - length..]))
            .unwrap_or(0);
        self.input_ready_carry = combined.split_off(combined.len() - tail);
        combined
    }

    /// Hangs up and terminates every owned process, bounded in time.
    ///
    /// Graceful first (HUP+TERM to the shell's group, up to two seconds),
    /// then SIGKILL to the whole session and every snapshotted descendant
    /// (#119), a short settle, and a bounded thread reap. Total shutdown is
    /// bounded by the grace period plus the two small constants above — a
    /// wedged or ignoring child can delay it, never stall it.
    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.events.is_none() {
            return Ok(());
        }
        let result = self.request_shutdown();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while self.is_process_group_alive() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let kill_result = (self.is_process_group_alive() || self.is_session_alive())
            .then(|| self.force_shutdown())
            .transpose();
        // Let the SIGKILL land: transient zombies (an init that has not
        // reaped yet) read "alive" to kill(2), and the helper threads
        // need the EOF/reap that follows the last death.
        let settle = std::time::Instant::now() + FORCE_SETTLE;
        while (self.is_process_group_alive() || self.is_session_alive())
            && std::time::Instant::now() < settle
        {
            thread::sleep(Duration::from_millis(20));
        }
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
        // Snapshot the owned tree at its most complete, while the shell is
        // still alive to parent it (#119). Anything that already orphaned
        // before this point stays covered by the session kill at force time.
        #[cfg(target_os = "linux")]
        {
            self.descendants = self
                .process_group
                .map(descendant_snapshot)
                .unwrap_or_default();
        }
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

    /// Whether the owned session still has a live member (#119).
    /// This is the ownership boundary the group check misses: jobs in
    /// their own groups, and orphans reparented before the snapshot, all
    /// keep the session alive until they die. Zombies do not count: they
    /// hold no descriptors and need only their reaper, not our signals.
    pub fn is_session_alive(&self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(session) = self.process_group {
            return !session_members(session).is_empty();
        }

        false
    }

    /// Kills what the grace period did not take (#119): the shell's group
    /// as before, plus the whole session (job groups, SIGHUP-ignorers,
    /// pre-shutdown orphans holding slave descriptors), plus every
    /// snapshotted descendant that escaped the session. Each wider step is
    /// guarded against pid reuse; see `kill_session`.
    pub fn force_shutdown(&mut self) -> Result<(), String> {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            let group = send_signal(process_group, libc::SIGKILL);
            let session = self.kill_session();
            let tree = self.kill_descendants();
            return group.and(session).and(tree);
        }

        self.killer.kill().map_err(|error| error.to_string())
    }

    /// SIGKILLs the whole owned session: job process groups, SIGHUP
    /// ignorers, and orphans reparented before the snapshot (#119).
    ///
    /// Linux has no session-signalling syscall (kill(-id) names a process
    /// GROUP), so this enumerates session members via `/proc` and signals
    /// each one. If the session id was reused by a new leader after our
    /// shell died, that leader's fresh tree is excluded; a live shell
    /// needs no guard since its pid cannot have been reused.
    #[cfg(unix)]
    fn kill_session(&self) -> Result<(), String> {
        #[cfg(not(target_os = "linux"))]
        {
            // Without `/proc` there is no session enumeration: the group
            // kill above is the whole force stage, as before.
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let Some(session) = self.process_group else {
                return Ok(());
            };
            let procs = all_processes();
            let excluded = pid_reused(session, self.shell_start).then(|| {
                let excluded = descendant_pids(session, &procs);
                eprintln!("DEBUG pid reused, excluding new tree: {excluded:?}");
                excluded
            });
            let own = std::process::id();
            let mut first_error = None;
            for info in &procs {
                if info.session != session
                    || info.pid == session
                    || info.pid == own
                    || excluded
                        .as_ref()
                        .is_some_and(|tree| tree.contains(&info.pid))
                {
                    continue;
                }
                if let Err(error) = send_signal_pid(info.pid, libc::SIGKILL) {
                    first_error.get_or_insert(error);
                }
            }
            first_error.map_or(Ok(()), Err)
        }
    }

    /// SIGKILLs the snapshotted descendants that escaped the session
    /// (nested sessions, detached daemons — #119). Best effort across the
    /// set: every kill is attempted, the first error reported. Each pid is
    /// revalidated against its snapshot start time first, so reuse since
    /// the snapshot can never aim at an innocent — nor at ourselves.
    #[cfg(unix)]
    fn kill_descendants(&self) -> Result<(), String> {
        #[cfg(not(target_os = "linux"))]
        {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let own = std::process::id();
            let shell = self.process_group.unwrap_or(0);
            let mut first_error = None;
            for (pid, starttime) in &self.descendants {
                if *pid == own || *pid == shell {
                    continue;
                }
                if proc_stat(*pid).is_some_and(|info| info.starttime == *starttime)
                    && let Err(error) = send_signal_pid(*pid, libc::SIGKILL)
                {
                    first_error.get_or_insert(error);
                }
            }
            first_error.map_or(Ok(()), Err)
        }
    }

    /// Joins the reader and waiter after their owner has ended the process.
    /// Bounded (#119): both threads finish promptly once the SIGKILL above
    /// lands — the reader on EOF, the waiter on the reap — so this waits up
    /// to `JOIN_TIMEOUT` and then detaches what cannot finish instead of
    /// stalling shutdown behind an unkillable child. A detached helper
    /// still exits on its own once its fd/child resolves.
    pub fn join(&mut self) {
        self.events.take();
        let deadline = std::time::Instant::now() + JOIN_TIMEOUT;
        for slot in [&mut self.reader, &mut self.waiter] {
            while slot.as_ref().is_some_and(|handle| !handle.is_finished())
                && std::time::Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(20));
            }
            if let Some(handle) = slot.take()
                && handle.is_finished()
            {
                let _ = handle.join();
                // Else the handle drops here and the thread detaches.
            }
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
    check_kill(unsafe { libc::kill(-(process_group as libc::pid_t), signal) })
}

/// Signals one process, tolerating the already-dead race (#119). Used for
/// per-PID sweeps where a member may exit between enumeration and kill.
#[cfg(unix)]
fn send_signal_pid(pid: u32, signal: libc::c_int) -> Result<(), String> {
    check_kill(unsafe { libc::kill(pid as libc::pid_t, signal) })
}

/// A kill(2) result is success when it landed or the target is already
/// gone (ESRCH); anything else is a real error.
#[cfg(unix)]
fn check_kill(result: libc::c_int) -> Result<(), String> {
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

/// One `/proc` process record: the identity a shutdown sweep needs.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct ProcInfo {
    pid: u32,
    ppid: u32,
    session: u32,
    starttime: u64,
    /// First letter of the stat state: zombies (`Z`) hold no descriptors
    /// and need only their reaper, so sweeps do not count or signal them.
    state: char,
}

/// Reads `/proc/<pid>/stat`. `comm` may hold spaces and parens, so the
/// fields after it are split off the LAST `)`. Times are raw clock ticks:
/// only ever compared for equality, never converted.
#[cfg(target_os = "linux")]
fn proc_stat(pid: u32) -> Option<ProcInfo> {
    let contents = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = contents.rfind(')')?;
    // state(0) ppid(1) pgrp(2) session(3) … starttime(19).
    let fields: Vec<&str> = contents[after_comm + 1..].split_whitespace().collect();
    Some(ProcInfo {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        starttime: fields.get(19)?.parse().ok()?,
        state: fields.first()?.chars().next()?,
    })
}

#[cfg(target_os = "linux")]
fn proc_starttime(pid: u32) -> Option<u64> {
    proc_stat(pid).map(|info| info.starttime)
}

/// Every process visible to us. Racy by nature — a member may exit mid-scan
/// (its record simply vanishes) or fork (missed until the next scan) — so
/// every kill site revalidates or tolerates ESRCH.
#[cfg(target_os = "linux")]
fn all_processes() -> Vec<ProcInfo> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter_map(proc_stat)
        .collect()
}

/// (pid, starttime) of every process parented under `root` (#119).
/// Taken while the tree is intact; kills revalidate the start time.
#[cfg(target_os = "linux")]
fn descendant_snapshot(root: u32) -> Vec<(u32, u64)> {
    descendant_pids(root, &all_processes())
        .into_iter()
        .filter_map(|pid| proc_stat(pid).map(|info| (pid, info.starttime)))
        .collect()
}

/// Breadth-first walk down ppid links from `root`, excluding `root` itself.
#[cfg(target_os = "linux")]
fn descendant_pids(root: u32, procs: &[ProcInfo]) -> Vec<u32> {
    let mut found = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for info in procs {
            if info.ppid == parent && info.pid != root && !found.contains(&info.pid) {
                found.push(info.pid);
                frontier.push(info.pid);
            }
        }
    }
    found
}

/// Live (non-zombie) members of one session. Racy like every scan here:
/// callers poll rather than assert instantly.
#[cfg(target_os = "linux")]
fn session_members(session: u32) -> Vec<ProcInfo> {
    all_processes()
        .into_iter()
        .filter(|info| info.session == session && info.state != 'Z')
        .collect()
}
/// Whether pid was reused since `recorded` (#119 reuse guard). A matching
/// start time means the same process, so the session is ours. A free pid
/// means no reuser exists, and whoever else sits in the session could only
/// have inherited it from our tree — also ours. Anything else is treated
/// as reused, and the fresh tree under the new leader is excluded from the
/// per-PID sweep.
#[cfg(target_os = "linux")]
fn pid_reused(pid: u32, recorded: Option<u64>) -> bool {
    let Some(recorded) = recorded else {
        // No baseline (unreadable `/proc` at spawn): assume the worst so
        // the group kill below stays off and the per-PID fallback decides.
        return true;
    };
    match proc_stat(pid) {
        Some(info) => info.starttime != recorded,
        None => false,
    }
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
    use crate::engine::shell_hook::ShellHook;

    /// Whether one pid is alive (kill(pid, 0): 0 and EPERM mean alive).
    #[cfg(target_os = "linux")]
    fn pid_alive(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    /// The process group of one pid, via `/proc`. Proves the job-control
    /// premise: monitor-mode jobs really leave the shell's group.
    #[cfg(target_os = "linux")]
    fn pid_pgrp(pid: u32) -> Option<u32> {
        let contents = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_comm = contents.rfind(')')?;
        contents[after_comm + 1..]
            .split_whitespace()
            .nth(2)?
            .parse()
            .ok()
    }

    /// A monitor-mode bash running `script`: background jobs get their own
    /// process groups (needs the PTY's controlling terminal — without one
    /// bash prints "no job control" and jobs stay in the shell's group).
    #[cfg(target_os = "linux")]
    fn bash_job_project(terminal: &str, script: &str) -> Project {
        Project {
            terminal: TerminalId::new(terminal),
            path: PathBuf::from("/"),
            command: vec![
                "/usr/bin/bash".to_owned(),
                "-m".to_owned(),
                "-c".to_owned(),
                script.to_owned(),
            ],
            shell_hook: false,
        }
    }

    /// Reads PTY output until `marker` appears (established 15s pattern).
    /// Returns everything seen.
    #[cfg(target_os = "linux")]
    fn read_until(transport: &mut PtyTransport, marker: &str) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut output = Vec::new();
        while Instant::now() < deadline && !String::from_utf8_lossy(&output).contains(marker) {
            for event in transport.drain_events() {
                if let PtyEvent::Output { bytes, .. } = event {
                    output.extend(bytes);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        output
    }

    /// Parses `PREFIX=<pid>` out of PTY output.
    #[cfg(target_os = "linux")]
    fn marked_pid(output: &[u8], prefix: &str) -> u32 {
        let text = String::from_utf8_lossy(output);
        let marker = format!("{prefix}=");
        let start = text
            .find(&marker)
            .unwrap_or_else(|| panic!("{prefix} never printed: {text:?}"));
        text[start + marker.len()..]
            .split(|character: char| !character.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Polls until `condition` holds or `timeout` passes. Shutdown asserts
    /// must poll: init reaping of zombies is async, so an instant check
    /// after shutdown races the reaper (the known `shutdown_terms` flake).
    #[cfg(target_os = "linux")]
    fn poll_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        condition()
    }

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

    /// #136: startup output is not enough to release hooked-shell input. The
    /// child deliberately emits output, waits before its first prompt, then
    /// receives the same-turn input whole after the private ready marker.
    #[cfg(target_os = "linux")]
    #[test]
    fn same_turn_input_waits_for_the_prompt_marker_then_arrives_whole() {
        let test_dir = std::env::temp_dir().join(format!(
            "termdeck-ready-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let marker = test_dir.join("ready");
        let home = test_dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".bashrc"),
            "printf 'STARTUP-OUTPUT\\n'\nwhile [ ! -e \"$TERMDECK_READY\" ]; do :; done\n",
        )
        .unwrap();
        let terminal = TerminalId::new("ready-gate");
        let mut command = CommandBuilder::new("bash");
        let hook = ShellHook::install("bash", &["--noprofile".to_owned()], &mut command).unwrap();
        command.env("HOME", &home);
        command.env("TERMDECK_READY", &marker);
        command.env("TERMDECK_SOCK", "/tmp/termdeck-termios.sock");
        command.env("TERMDECK_PANE", "ready-gate");
        let mut transport =
            PtyTransport::spawn_command(terminal, command, ScreenSize::new(80, 24), hook).unwrap();
        let input = b"printf 'WHOLE-STARTUP-INPUT\\n'\\r";
        assert_eq!(transport.write(input).unwrap(), InputOutcome::Queued);

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut output = Vec::new();
        while Instant::now() < deadline
            && !String::from_utf8_lossy(&output).contains("STARTUP-OUTPUT")
        {
            for event in transport.drain_events() {
                if let PtyEvent::Output { bytes, .. } = event {
                    output.extend(bytes);
                }
            }
        }
        assert!(
            String::from_utf8_lossy(&output).contains("STARTUP-OUTPUT"),
            "child never reached its pre-termios barrier"
        );
        assert!(transport.waiting_for_input_ready());
        assert_eq!(transport.pending_input_len(), input.len());
        assert_eq!(transport.flush_input().unwrap(), InputOutcome::Queued);
        assert_eq!(transport.pending_input_len(), input.len());

        std::fs::write(&marker, []).unwrap();
        while Instant::now() < deadline && transport.waiting_for_input_ready() {
            transport.drain_events();
        }
        assert!(
            !transport.waiting_for_input_ready(),
            "prompt marker did not open the gate"
        );
        assert_eq!(transport.flush_input().unwrap(), InputOutcome::Flushed);

        while Instant::now() < deadline
            && !String::from_utf8_lossy(&output).contains("WHOLE-STARTUP-INPUT")
        {
            for event in transport.drain_events() {
                if let PtyEvent::Output { bytes, .. } = event {
                    output.extend(bytes);
                }
            }
        }
        assert!(
            String::from_utf8_lossy(&output).contains("WHOLE-STARTUP-INPUT"),
            "the prompt-gated input must arrive whole: {:?}",
            String::from_utf8_lossy(&output)
        );
        transport.shutdown().unwrap();
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    /// #119 (audit repro): an interactive-shell background job in its own
    /// process group that ignores SIGHUP. Shutdown used to return with the
    /// job alive — group signals never reached its group, and the surviving
    /// slave holder then wedged the reader join without bound.
    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_kills_a_sighup_ignoring_background_job() {
        let mut transport = PtyTransport::spawn(
            &bash_job_project("bg-job", "trap '' HUP; sleep 60 & echo JOBPID=$!; wait"),
            ScreenSize::new(80, 24),
        )
        .unwrap();
        let shell_group = transport.process_group.unwrap();
        let output = read_until(&mut transport, "JOBPID=");
        let job = marked_pid(&output, "JOBPID");
        assert!(pid_alive(job), "the job must be running before shutdown");
        assert_ne!(
            pid_pgrp(job),
            Some(shell_group),
            "the premise: monitor-mode jobs leave the shell's group"
        );

        let started = Instant::now();
        transport.shutdown().unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "shutdown must stay bounded, took {elapsed:?}"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !pid_alive(job)),
            "the SIGHUP-ignoring job must die"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !transport.is_session_alive()),
            "no owned session member may survive"
        );
        assert!(transport.has_joined_threads());
    }

    /// #119: a foreground job that ignores HUP and TERM in its own group.
    /// It survives the whole grace period, so shutdown takes the full two
    /// seconds — then the session SIGKILL must still take it and the shell.
    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_kills_a_foreground_job_ignoring_hup_and_term() {
        let mut transport = PtyTransport::spawn(
            &bash_job_project("fg-job", "trap '' HUP TERM; sleep 60"),
            ScreenSize::new(80, 24),
        )
        .unwrap();
        let shell = transport.process_group.unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            pid_alive(shell),
            "the shell must be running before shutdown"
        );

        let started = Instant::now();
        transport.shutdown().unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "shutdown must stay bounded, took {elapsed:?}"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !pid_alive(shell)),
            "the ignoring shell must die at force time"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !transport.is_session_alive()),
            "no owned session member may survive"
        );
        assert!(transport.has_joined_threads());
    }

    /// #119: a grandchild orphaned before shutdown, holding the slave side
    /// open. Pre-fix this wedged the reader so the unconditional join hung
    /// forever; the session kill must take the orphan and release the PTY.
    #[cfg(target_os = "linux")]
    #[test]
    fn shutdown_kills_an_orphan_holding_the_slave_open() {
        let mut transport = PtyTransport::spawn(
            &bash_job_project("orphan", "sleep 60 & echo ORPHAN=$!; disown; wait"),
            ScreenSize::new(80, 24),
        )
        .unwrap();
        let output = read_until(&mut transport, "ORPHAN=");
        let orphan = marked_pid(&output, "ORPHAN");
        // Let bash leave: the waiter reaps it and init adopts the orphan,
        // which keeps the session — and the slave — alive on its own.
        assert!(
            poll_until(Duration::from_secs(5), || !pid_alive(
                transport.process_group.unwrap()
            )),
            "bash must exit on its own once disowned"
        );
        assert!(pid_alive(orphan), "the orphan must outlive its shell");

        let started = Instant::now();
        transport.shutdown().unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "shutdown must stay bounded past a wedged reader, took {elapsed:?}"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !pid_alive(orphan)),
            "the orphan must die"
        );
        assert!(
            poll_until(Duration::from_secs(5), || !transport.is_session_alive()),
            "no owned session member may survive"
        );
        assert!(transport.has_joined_threads());
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
