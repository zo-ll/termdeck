//! The local ctl.v1 adapter.  It deliberately sits outside the frozen engine/UI
//! contracts and is polled by the session's existing single-threaded loop.

use std::{
    collections::VecDeque,
    env, fs,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    config::Workspace,
    contracts::{NotifyKind, Project, ScreenSize, TerminalEngine, TerminalStatus, Timestamp},
    ui::{DeckState, Notifications},
};

pub const SCHEMA: &str = "ctl.v1";
const MAX_LINE: usize = 64 * 1024;
/// A session only keeps a small, fixed number of ctl peers alive at once.
/// The kernel's listen queue absorbs the rest until one of these slots frees.
const MAX_CLIENTS: usize = 16;
/// No single ctl turn is allowed to monopolize the session loop.
const IO_CHUNK: usize = 16 * 1024;
/// The largest envelope the server will retain for a peer that has stopped
/// reading.  This deliberately permits useful `peek` replies without making
/// the session's memory depend on an untrusted client.
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
/// The pause between connect attempts while a session's listen queue is
/// full (#146). Short enough that a freed slot is taken almost at once,
/// long enough that waiting out a whole `CLIENT_TIMEOUT` costs a few
/// hundred syscalls rather than a spin.
const CONNECT_RETRY: Duration = Duration::from_millis(5);

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Request {
    pub schema: String,
    pub verb: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub on: Option<bool>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub paste: Option<String>,
    #[serde(default)]
    pub keys: Option<String>,
}

/// The Phase-2 part of ctl.v1.  The session applies these on its existing
/// lifecycle/input paths; parsing lives here so socket and CLI requests agree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Control {
    Save {
        name: Option<String>,
    },
    Restore {
        name: String,
    },
    Open {
        path: String,
    },
    Close {
        id: String,
        force: bool,
    },
    Promote {
        id: String,
    },
    Zoom {
        on: Option<bool>,
    },
    Input {
        id: String,
        bytes: Vec<u8>,
        force: bool,
        /// Whether the caller sent `paste` (vs `text`/`keys`). The wire is
        /// unchanged — all three arrive as bytes — but only a paste
        /// operation re-emits bracketed when the child holds DEC 2004
        /// (#120); typed text never wraps.
        paste: bool,
    },
}

impl Request {
    pub fn control(&self) -> Result<Option<Control>, Response> {
        let required = |name: &str, value: &Option<String>| {
            value.clone().ok_or_else(|| {
                Response::error(2, format!("bad request: {name} requires an argument"))
            })
        };
        match self.verb.as_str() {
            "save" => Ok(Some(Control::Save {
                name: self.id.clone(),
            })),
            "restore" => Ok(Some(Control::Restore {
                name: required("restore", &self.id)?,
            })),
            "open" => Ok(Some(Control::Open {
                path: required("open", &self.path)?,
            })),
            "close" => Ok(Some(Control::Close {
                id: required("close", &self.id)?,
                force: self.force,
            })),
            "promote" => Ok(Some(Control::Promote {
                id: required("promote", &self.id)?,
            })),
            "zoom" => Ok(Some(Control::Zoom { on: self.on })),
            "input" => {
                let values = [self.text.as_ref(), self.paste.as_ref(), self.keys.as_ref()];
                if values.iter().flatten().count() != 1 {
                    return Err(Response::error(
                        2,
                        "bad request: input requires exactly one of text, paste, or keys",
                    ));
                }
                Ok(Some(Control::Input {
                    id: required("input", &self.id)?,
                    bytes: values
                        .into_iter()
                        .flatten()
                        .next()
                        .unwrap()
                        .as_bytes()
                        .to_vec(),
                    force: self.force,
                    paste: self.paste.is_some(),
                }))
            }
            _ => Ok(None),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response {
    pub schema: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CtlError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CtlError {
    pub code: u8,
    pub message: String,
}

impl Response {
    pub fn ok(data: Value) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(code: u8, message: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            ok: false,
            data: None,
            error: Some(CtlError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// State read by the Phase 1 verbs while the session loop owns it.
///
/// `notify` is the one verb that writes: it lands in the session's notify
/// state (#97), which is why the caller's pane identity and the session's
/// clock arrive here with it. Time is an input everywhere in this feature.
pub struct State<'a, E> {
    pub workspace: &'a Workspace,
    pub projects: &'a [Project],
    pub deck: &'a DeckState,
    pub engine: &'a E,
    pub size: ScreenSize,
    pub sheet_open: bool,
    pub notifies: &'a mut Notifications,
    /// `TERMDECK_PANE` of the calling process, when it runs inside a pane.
    pub caller: Option<&'a str>,
    pub now: Timestamp,
}

pub fn dispatch<E: TerminalEngine>(request: Request, mut state: State<'_, E>) -> Response {
    if request.schema != SCHEMA {
        return Response::error(2, "bad request: unsupported schema");
    }
    match request.verb.as_str() {
        "status" => Response::ok(json!({
            "name": state.workspace.name,
            "pid": std::process::id(),
            "terminals": state.projects.len(),
            "master": state.deck.active().and_then(|index| state.projects.get(index))
                .map(|project| project.terminal.to_string()),
            "size": { "cols": state.size.columns, "rows": state.size.rows },
            "zoom": state.deck.zoomed(),
            "collapsed": state.deck.collapsed_count() > 0,
            "sheet": state.sheet_open,
        })),
        "list" => Response::ok(Value::Array(
            state
                .projects
                .iter()
                .enumerate()
                .map(|(index, project)| {
                    let live = state
                        .engine
                        .status(&project.terminal)
                        .is_some_and(|status| {
                            matches!(status, TerminalStatus::Starting | TerminalStatus::Running)
                        });
                    json!({
                        "id": project.terminal.to_string(),
                        "path": project.path,
                        "state": if live { "live" } else { "exited" },
                        "master": state.deck.active() == Some(index),
                        "active": state.deck.active() == Some(index),
                    })
                })
                .collect(),
        )),
        "peek" => {
            let Some(id) = request.id else {
                return Response::error(2, "bad request: peek requires id");
            };
            let Some(project) = state
                .projects
                .iter()
                .find(|project| project.terminal.to_string() == id)
            else {
                return Response::error(2, "bad request: unknown terminal");
            };
            let max = request.lines.unwrap_or(30);
            let Some(lines) = state
                .engine
                .active_screen_lines(&project.terminal, max)
                .or_else(|| state.engine.history_lines(&project.terminal, max))
            else {
                return Response::error(1, "active screen is unavailable for this terminal");
            };
            let alt_screen = state
                .engine
                .metadata(&project.terminal)
                .is_some_and(|metadata| metadata.alt_screen);
            Response::ok(json!({
                "id": project.terminal.to_string(),
                "lines": lines,
                "alt_screen": alt_screen,
                "screen": if alt_screen { "alt" } else { "main" },
            }))
        }
        "notify" => match request.msg {
            Some(message) => match dispatch_notify(&message, &mut state) {
                NotifyOutcome::Delivered => Response::ok(json!({ "delivered": true })),
                NotifyOutcome::Suppressed { reason } => {
                    Response::ok(json!({ "delivered": false, "reason": reason }))
                }
            },
            None => Response::error(2, "bad request: notify requires msg"),
        },
        "version" => {
            Response::ok(json!({ "schema": SCHEMA, "version": env!("CARGO_PKG_VERSION") }))
        }
        _ => Response::error(2, "bad request: unknown verb"),
    }
}

/// The outcome of routing an explicit notification into the session state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotifyOutcome {
    Delivered,
    Suppressed { reason: &'static str },
}

/// Routes an explicit notification into the session's notify state (#71/#97).
///
/// The caller names itself by running where it runs: every spawned PTY
/// inherits `TERMDECK_PANE`, so the pane the message belongs to is the pane
/// the connection came from. A caller outside every pane (one holding only
/// `TERMDECK_SOCK`) has no pane to flash, and one naming a terminal this
/// session does not run has none either; both are reported as suppressed.
fn dispatch_notify<E: TerminalEngine>(message: &str, state: &mut State<'_, E>) -> NotifyOutcome {
    let Some(project) = state
        .caller
        .and_then(|pane| project_for_pane(pane, state.projects))
    else {
        return NotifyOutcome::Suppressed {
            reason: "caller is not a live terminal pane",
        };
    };
    let master = state
        .deck
        .active()
        .and_then(|position| state.projects.get(position))
        .map(|project| &project.terminal);
    if master == Some(&project.terminal) {
        return NotifyOutcome::Suppressed {
            reason: "caller pane is already visible",
        };
    }
    if state.notifies.record(
        &project.terminal,
        master,
        NotifyKind::Message {
            title: String::new(),
            body: message.to_owned(),
        },
        state.now,
    ) {
        NotifyOutcome::Delivered
    } else {
        NotifyOutcome::Suppressed {
            reason: "notification was coalesced",
        }
    }
}

/// Finds the pane a child process names. The PTY supplies the stable terminal
/// id, but accepting the project's canonical directory as well keeps explicit
/// calls attributed when a client carries the `path` reported by `termctl
/// list`. Paths are only a fallback: terminal ids remain unambiguous, and an
/// ambiguous directory (two panes opened on the same path) is deliberately
/// not attributed to either one.
fn project_for_pane<'a>(pane: &str, projects: &'a [Project]) -> Option<&'a Project> {
    projects
        .iter()
        .find(|project| project.terminal.to_string() == pane)
        .or_else(|| {
            let pane = normalized_pane_path(pane, env::var_os("HOME").as_deref())?;
            let mut matches = projects.iter().filter(|project| {
                normalized_pane_path(&project.path.to_string_lossy(), None).as_ref() == Some(&pane)
            });
            let project = matches.next()?;
            matches.next().is_none().then_some(project)
        })
}

/// Resolves the spelling a caller can put in `TERMDECK_PANE`. In particular,
/// `~/project`, an absolute path, and a symlinked/opened spelling all identify
/// the same existing terminal directory.
fn normalized_pane_path(path: &str, home: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(home?).join(rest),
        None => PathBuf::from(path),
    };
    path.canonicalize().ok()
}

struct Pending {
    stream: UnixStream,
    pane: Option<String>,
    deadline: Instant,
    phase: PendingPhase,
}

enum PendingPhase {
    Reading { bytes: Vec<u8> },
    Writing { bytes: Vec<u8>, written: usize },
}

/// One listener at a time, for tests.
///
/// [`Listener::bind`] names its socket after the process, so every listener a
/// test binds wants the same path: two of them racing would unlink each
/// other's socket. The lock lives here rather than in this module's own test
/// module because the session's end-to-end test binds one too (#130).
#[cfg(test)]
pub(crate) static LISTENER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Session-owned listener. Connections remain nonblocking for their whole
/// lifetime. Every poll advances one peer in round-robin order and dispatches
/// at most one request, so a stalled client cannot freeze or starve the UI.
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
    uid: u32,
    pending: VecDeque<Pending>,
}

impl Listener {
    pub fn bind() -> io::Result<Self> {
        let uid = current_uid();
        let directory = socket_directory(uid)?;
        sweep_stale(&directory, uid);
        let path = directory
            .join(std::process::id().to_string())
            .join("ctl.sock");
        let parent = path.parent().expect("socket path has a parent");
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            uid,
            pending: VecDeque::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Polls ctl clients without blocking the UI. Requests and replies have
    /// independent deadlines; no more than one request is dispatched here.
    pub fn poll<E: TerminalEngine>(&mut self, state: State<'_, E>) -> io::Result<bool> {
        self.poll_with(move |request, caller| dispatch(request, State { caller, ..state }))
    }

    /// Like [`Listener::poll`], but hands the caller's pane identity to the
    /// session for the Phase-2 close-self guard.
    pub fn poll_with(
        &mut self,
        dispatch_request: impl FnOnce(Request, Option<&str>) -> Response,
    ) -> io::Result<bool> {
        let now = Instant::now();
        self.pending.retain(|pending| pending.deadline > now);

        while self.pending.len() < MAX_CLIENTS {
            match self.listener.accept() {
                Ok((stream, _)) if trusted_peer(&stream, self.uid) => {
                    stream.set_nonblocking(true)?;
                    self.pending.push_back(Pending {
                        pane: caller_pane(&stream),
                        stream,
                        deadline: now + REQUEST_TIMEOUT,
                        phase: PendingPhase::Reading { bytes: Vec::new() },
                    });
                }
                // Do not leave an untrusted peer at the front of the kernel
                // queue, but otherwise continue admitting ready peers.
                Ok((_stream, _)) => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }

        let Some(mut pending) = self.pending.pop_front() else {
            return Ok(false);
        };
        match &mut pending.phase {
            PendingPhase::Reading { bytes } => match read_request(&mut pending.stream, bytes) {
                Ok(Some(request)) => {
                    let response = dispatch_request(request, pending.pane.as_deref());
                    begin_response(&mut pending, response)?;
                    let complete = write_response(&mut pending);
                    if !complete {
                        self.pending.push_back(pending);
                    }
                    Ok(true)
                }
                Ok(None) => {
                    self.pending.push_back(pending);
                    Ok(false)
                }
                Err(message) => {
                    begin_response(
                        &mut pending,
                        Response::error(2, format!("bad request: {message}")),
                    )?;
                    let complete = write_response(&mut pending);
                    if !complete {
                        self.pending.push_back(pending);
                    }
                    Ok(true)
                }
            },
            PendingPhase::Writing { .. } => {
                if !write_response(&mut pending) {
                    self.pending.push_back(pending);
                }
                Ok(false)
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_request(stream: &mut UnixStream, bytes: &mut Vec<u8>) -> Result<Option<Request>, String> {
    let mut buffer = [0_u8; 4096];
    loop {
        if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            if end > MAX_LINE
                || bytes[end + 1..]
                    .iter()
                    .any(|byte| !byte.is_ascii_whitespace())
            {
                return Err("request line exceeds protocol limits".to_owned());
            }
            return serde_json::from_slice(&bytes[..end])
                .map(Some)
                .map_err(|error| error.to_string());
        }
        if bytes.len() > MAX_LINE {
            return Err("request line exceeds 64 KiB".to_owned());
        }
        match stream.read(&mut buffer) {
            Ok(0) => return Err("connection closed before newline".to_owned()),
            Ok(read) => bytes.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn begin_response(pending: &mut Pending, response: Response) -> io::Result<()> {
    let mut encoded = encode_response(&response).unwrap_or_else(|_| {
        // This is still a ctl.v1 envelope: response size is an availability
        // condition, not an excuse to silently drop a well-formed request.
        encode_response(&Response::error(1, "response exceeds protocol limits"))
            .expect("the fixed ctl error envelope fits MAX_RESPONSE")
    });
    encoded.push(b'\n');
    pending.deadline = Instant::now() + RESPONSE_TIMEOUT;
    pending.phase = PendingPhase::Writing {
        bytes: encoded,
        written: 0,
    };
    Ok(())
}

/// Serializes directly into a capped vector so an unusually large JSON value
/// cannot transiently allocate an unbounded encoded response on the session
/// thread.
fn encode_response(response: &Response) -> io::Result<Vec<u8>> {
    struct BoundedBuffer {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl Write for BoundedBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > self.limit {
                return Err(io::Error::other("response exceeds protocol limits"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut output = BoundedBuffer {
        bytes: Vec::with_capacity(1024),
        // Leave room for the newline added by the wire encoder.
        limit: MAX_RESPONSE.saturating_sub(1),
    };
    serde_json::to_writer(&mut output, response).map_err(io::Error::other)?;
    Ok(output.bytes)
}

/// Writes one bounded piece of a response. Socket errors are local to an
/// abandoning caller; they only discard that caller's slot.
fn write_response(pending: &mut Pending) -> bool {
    let PendingPhase::Writing { bytes, written } = &mut pending.phase else {
        return false;
    };
    let end = (*written + IO_CHUNK).min(bytes.len());
    match pending.stream.write(&bytes[*written..end]) {
        // A zero write moved nothing: retry on a later poll, exactly like
        // WouldBlock. The response deadline still bounds a peer that stalls
        // here, so this cannot wedge its slot (#117 NB-3).
        Ok(0) => false,
        Ok(count) => {
            *written += count;
            *written == bytes.len()
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
        Err(_) => true,
    }
}

/// Dials a running session using the same one-request ctl.v1 transport as
/// `termctl`.  The caller owns formatting and exit-code selection.
pub fn call(socket: &Path, request: &Request) -> Result<Response, String> {
    call_with_limits(socket, request, CLIENT_TIMEOUT, MAX_RESPONSE)
}

fn call_with_limits(
    socket: &Path,
    request: &Request,
    timeout: Duration,
    max_response: usize,
) -> Result<Response, String> {
    // One absolute deadline for the whole call: establishing the connection
    // is part of what `timeout` promises, not something that happens before
    // the clock starts (#146).
    let deadline = Instant::now() + timeout;
    let mut stream = connect_with_deadline(socket, deadline)?;
    let mut encoded = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    write_with_deadline(&mut stream, &encoded, deadline)?;
    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        set_read_deadline(&stream, deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len().saturating_add(read) > max_response {
                    return Err(format!("response exceeds {max_response} byte limit"));
                }
                response.extend_from_slice(&buffer[..read]);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let response = std::str::from_utf8(&response)
        .map_err(|error| error.to_string())?
        .trim();
    serde_json::from_str(response).map_err(|error| error.to_string())
}

/// Dials `socket` without ever waiting past `deadline` (#146).
///
/// `UnixStream::connect` blocks in the kernel while the listener's queue is
/// full, and that wait sits outside every socket timeout — the old client
/// started its clock only once connect returned, so against a saturated
/// session `termctl status` blocked with no bound at all. The socket is
/// created nonblocking instead: a full queue answers EAGAIN at once, which
/// is retried under the same deadline that write and read then share.
fn connect_with_deadline(socket: &Path, deadline: Instant) -> Result<UnixStream, String> {
    let address = socket_address(socket)?;
    loop {
        let stream = nonblocking_socket()?;
        // SAFETY: `address` is an initialised `sockaddr_un` of the length
        // passed, and the descriptor is owned by `stream` for this call.
        let dialled = unsafe {
            libc::connect(
                stream.as_raw_fd(),
                (&raw const address).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
        };
        let established = if dialled == 0 {
            true
        } else {
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                // A Unix socket refuses a full listen queue outright rather
                // than queueing a handshake, so nothing is in flight and the
                // next attempt starts from a fresh descriptor.
                Some(libc::EAGAIN) => false,
                // A handshake is under way: wait for writability and read
                // the outcome back out of SO_ERROR.
                Some(libc::EINPROGRESS) => wait_connected(&stream, deadline)?,
                _ => return Err(error.to_string()),
            }
        };
        if established {
            // Back to blocking: write and read carry their own timeouts,
            // both cut from the same deadline.
            stream
                .set_nonblocking(false)
                .map_err(|error| error.to_string())?;
            return Ok(stream);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "connect deadline exceeded".to_owned())?;
        std::thread::sleep(CONNECT_RETRY.min(remaining));
    }
}

/// The AF_UNIX address for one socket path, refusing a path that would not
/// fit `sun_path` rather than silently dialling a truncated name.
fn socket_address(socket: &Path) -> Result<libc::sockaddr_un, String> {
    // SAFETY: `sockaddr_un` is plain data; all-zero is a valid empty address.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let path = socket.as_os_str().as_bytes();
    if path.len() >= address.sun_path.len() {
        return Err(format!(
            "socket path exceeds {} bytes: {}",
            address.sun_path.len() - 1,
            socket.display()
        ));
    }
    for (slot, byte) in address.sun_path.iter_mut().zip(path) {
        *slot = *byte as libc::c_char;
    }
    Ok(address)
}

/// A fresh nonblocking, close-on-exec AF_UNIX stream socket.
fn nonblocking_socket() -> Result<UnixStream, String> {
    // SAFETY: `socket(2)` with constant arguments; it only returns a new
    // descriptor or -1.
    let descriptor = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: a fresh descriptor owned by nobody else; the stream takes it
    // over and closes it, including on every error path below.
    let stream = unsafe { UnixStream::from_raw_fd(descriptor) };
    // SAFETY: `fcntl` with integer arguments on the owned descriptor.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    Ok(stream)
}

/// Whether an in-flight connect completed before `deadline`. A timeout, an
/// interrupted poll or a refused queue all report "not yet" so the caller
/// retries under its own deadline; anything else is a real dial failure.
fn wait_connected(stream: &UnixStream, deadline: Instant) -> Result<bool, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| "connect deadline exceeded".to_owned())?;
    let mut watched = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    let millis = libc::c_int::try_from(remaining.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: `watched` is one valid descriptor record.
    let ready = unsafe { libc::poll(&mut watched, 1, millis) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error.to_string());
    }
    if ready == 0 {
        return Ok(false);
    }
    match stream.take_error().map_err(|error| error.to_string())? {
        None => Ok(true),
        Some(error) if error.raw_os_error() == Some(libc::EAGAIN) => Ok(false),
        Some(error) => Err(error.to_string()),
    }
}

fn write_with_deadline(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), String> {
    let mut written = 0;
    while written < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "request deadline exceeded".to_owned())?;
        stream
            .set_write_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        match stream.write(&bytes[written..]) {
            Ok(0) => return Err("socket closed while writing request".to_owned()),
            Ok(count) => written += count,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn set_read_deadline(stream: &UnixStream, deadline: Instant) -> Result<(), String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| "response deadline exceeded".to_owned())?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|error| error.to_string())
}

pub fn socket_from_environment() -> Result<PathBuf, String> {
    env::var_os("TERMDECK_SOCK")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "TERMDECK_SOCK is not set; pass --socket PATH".to_owned())
}

fn socket_directory(uid: u32) -> io::Result<PathBuf> {
    let base = match env::var_os("XDG_RUNTIME_DIR").filter(|path| !path.is_empty()) {
        Some(path) => PathBuf::from(path).join("termdeck"),
        None => env::var_os("TMPDIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join(format!("termdeck-{uid}")),
    };
    fs::create_dir_all(&base)?;
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700))?;
    Ok(base)
}

fn sweep_stale(directory: &Path, uid: u32) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<libc::pid_t>() else {
            continue;
        };
        if metadata.uid() == uid && !pid_alive(pid) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn pid_alive(pid: libc::pid_t) -> bool {
    // SAFETY: kill with signal zero does not deliver a signal.
    unsafe {
        libc::kill(pid, 0) == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: all pointers reference valid writable storage for SO_PEERCRED.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result == 0 {
        Ok(credentials.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn caller_pane(stream: &UnixStream) -> Option<String> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: all pointers reference valid writable storage for SO_PEERCRED.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return None;
    }
    let bytes = fs::read(format!("/proc/{}/environ", credentials.pid)).ok()?;
    bytes
        .split(|byte| *byte == 0)
        .find_map(|entry| entry.strip_prefix(b"TERMDECK_PANE="))
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}

fn trusted_peer(stream: &UnixStream, uid: u32) -> bool {
    peer_uid(stream).is_ok_and(|peer| peer == uid)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        os::{
            fd::AsRawFd,
            unix::{
                fs::PermissionsExt,
                net::{UnixListener, UnixStream},
            },
        },
        path::PathBuf,
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config::Workspace,
        contracts::{
            EngineCommand, EngineEvent, NotifyKind, Project, ScreenSize, TerminalEngine,
            TerminalFrame, TerminalId, TerminalMetadata, TerminalStatus, Timestamp,
        },
        engine::FakeEngine,
        ui::{DeckState, Notifications},
    };

    use super::{
        Control, LISTENER_TEST_LOCK, Listener, MAX_RESPONSE, Request, Response, SCHEMA, State,
        call_with_limits, connect_with_deadline, dispatch, normalized_pane_path, peer_uid,
        socket_directory, trusted_peer,
    };

    fn state<'a>(
        workspace: &'a Workspace,
        engine: &'a FakeEngine,
        projects: &'a [Project],
        deck: &'a DeckState,
        notifies: &'a mut Notifications,
    ) -> State<'a, FakeEngine> {
        State {
            workspace,
            projects,
            deck,
            engine,
            size: ScreenSize::new(80, 24),
            sheet_open: false,
            notifies,
            caller: None,
            now: Timestamp::default(),
        }
    }

    fn project(name: &str) -> Project {
        Project {
            terminal: TerminalId::new(name),
            path: std::env::temp_dir().join(name),
            command: vec!["sh".to_owned()],
            shell_hook: false,
        }
    }

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "termdeck-ctl-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    struct NoHistory;

    impl TerminalEngine for NoHistory {
        fn dispatch(&mut self, _: EngineCommand) -> Vec<EngineEvent> {
            Vec::new()
        }

        fn drain_events(&mut self) -> Vec<EngineEvent> {
            Vec::new()
        }

        fn frame(&self, _: &TerminalId) -> Option<&TerminalFrame> {
            None
        }

        fn status(&self, _: &TerminalId) -> Option<&TerminalStatus> {
            None
        }

        fn metadata(&self, _: &TerminalId) -> Option<&TerminalMetadata> {
            None
        }
    }

    #[test]
    fn read_verbs_return_ctl_v1_data() {
        let projects = vec![project("one")];
        let terminal = projects[0].terminal.clone();
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_active_screen_lines(
            &terminal,
            vec!["before viewport".to_owned(), "tail".to_owned()],
        );
        engine.set_metadata(
            &terminal,
            TerminalMetadata {
                ..Default::default()
            },
        );
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());

        let peek = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "peek".to_owned(),
                id: Some("one".to_owned()),
                lines: Some(1),
                msg: None,
                path: None,
                force: false,
                on: None,
                text: None,
                paste: None,
                keys: None,
            },
            state(&workspace, &engine, &projects, &deck, &mut notifies),
        );
        let data = peek.data.unwrap();
        assert_eq!(data["lines"], serde_json::json!(["tail"]));
        assert_eq!(data["alt_screen"], false);
        assert_eq!(data["screen"], "main");
        let version = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "version".to_owned(),
                id: None,
                lines: None,
                msg: None,
                path: None,
                force: false,
                on: None,
                text: None,
                paste: None,
                keys: None,
            },
            state(&workspace, &engine, &projects, &deck, &mut notifies),
        );
        assert!(version.ok);
    }

    #[test]
    fn peek_returns_active_alternate_screen_lines() {
        let projects = vec![project("one")];
        let terminal = projects[0].terminal.clone();
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_history_lines(&terminal, vec!["main history".to_owned()]);
        engine.set_active_screen_lines(
            &terminal,
            vec!["vim · src/main.rs".to_owned(), "fn main() {}".to_owned()],
        );
        engine.set_metadata(
            &terminal,
            TerminalMetadata {
                alt_screen: true,
                ..Default::default()
            },
        );
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());

        let peek = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "peek".to_owned(),
                id: Some("one".to_owned()),
                lines: Some(1),
                ..Default::default()
            },
            state(&workspace, &engine, &projects, &deck, &mut notifies),
        );

        let data = peek.data.unwrap();
        assert_eq!(data["lines"], serde_json::json!(["fn main() {}"]));
        assert_eq!(data["alt_screen"], true);
        assert_eq!(data["screen"], "alt");
    }

    /// #116 NB-1: an unset active screen is `None`, not an empty screen, so
    /// peek falls through to retained history through the real `or_else`
    /// path instead of answering with no lines.
    #[test]
    fn peek_falls_back_to_history_when_active_screen_is_unset() {
        let projects = vec![project("one")];
        let terminal = projects[0].terminal.clone();
        let mut engine = FakeEngine::new([terminal.clone()]);
        engine.set_history_lines(&terminal, vec!["old output".to_owned(), "tail".to_owned()]);
        // No `set_active_screen_lines`: the screen stays unset.
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());

        let peek = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "peek".to_owned(),
                id: Some("one".to_owned()),
                lines: Some(5),
                ..Default::default()
            },
            state(&workspace, &engine, &projects, &deck, &mut notifies),
        );

        assert!(peek.ok);
        let data = peek.data.unwrap();
        assert_eq!(data["lines"], serde_json::json!(["old output", "tail"]));
        assert_eq!(data["screen"], "main");
    }

    #[test]
    fn control_requests_require_their_arguments_and_keep_input_kinds_distinct() {
        let missing = Request {
            schema: SCHEMA.to_owned(),
            verb: "close".to_owned(),
            ..Default::default()
        }
        .control()
        .unwrap_err();
        assert_eq!(missing.error.unwrap().code, 2);

        assert!(matches!(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "save".to_owned(),
                ..Default::default()
            }
            .control(),
            Ok(Some(Control::Save { name: None }))
        ));
        assert!(matches!(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "restore".to_owned(),
                id: Some("checkpoint".to_owned()),
                ..Default::default()
            }
            .control(),
            Ok(Some(Control::Restore { name })) if name == "checkpoint"
        ));

        for request in [
            Request {
                schema: SCHEMA.to_owned(),
                verb: "input".to_owned(),
                id: Some("one".to_owned()),
                text: Some("text".to_owned()),
                ..Default::default()
            },
            Request {
                schema: SCHEMA.to_owned(),
                verb: "input".to_owned(),
                id: Some("one".to_owned()),
                paste: Some("paste".to_owned()),
                ..Default::default()
            },
            Request {
                schema: SCHEMA.to_owned(),
                verb: "input".to_owned(),
                id: Some("one".to_owned()),
                keys: Some("\u{1b}[A".to_owned()),
                ..Default::default()
            },
        ] {
            assert!(matches!(request.control(), Ok(Some(Control::Input { .. }))));
        }
    }

    /// #120: the three input kinds stay distinct past parsing — only
    /// `paste` re-emits bracketed at a paste-mode child. The wire is
    /// unchanged; this flag is the internal memory of which field arrived.
    #[test]
    fn input_control_marks_paste_requests_as_paste_operations() {
        let request = |text: Option<&str>, paste: Option<&str>, keys: Option<&str>| Request {
            schema: SCHEMA.to_owned(),
            verb: "input".to_owned(),
            id: Some("one".to_owned()),
            text: text.map(str::to_owned),
            paste: paste.map(str::to_owned),
            keys: keys.map(str::to_owned),
            ..Default::default()
        };
        assert!(matches!(
            request(Some("text"), None, None).control(),
            Ok(Some(Control::Input { paste: false, .. }))
        ));
        assert!(matches!(
            request(None, Some("paste"), None).control(),
            Ok(Some(Control::Input { paste: true, .. }))
        ));
        assert!(matches!(
            request(None, None, Some("keys")).control(),
            Ok(Some(Control::Input { paste: false, .. }))
        ));
    }

    /// #122: notify reports the state it actually reached. A caller in a
    /// background pane delivers, while a caller outside a live pane or the
    /// already-visible master receives an explicit suppression result.
    #[test]
    fn notify_routes_the_callers_pane_into_the_notify_state() {
        let projects = vec![project("one"), project("two")];
        let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
        let deck = DeckState::new(2);
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut notifies = Notifications::new();
        let request = || Request {
            schema: SCHEMA.to_owned(),
            verb: "notify".to_owned(),
            msg: Some("tests passed".to_owned()),
            ..Default::default()
        };
        let call = |notifies: &'_ mut Notifications, caller| {
            dispatch(
                request(),
                State {
                    workspace: &workspace,
                    projects: &projects,
                    deck: &deck,
                    engine: &engine,
                    size: ScreenSize::new(80, 24),
                    sheet_open: false,
                    notifies,
                    caller,
                    now: Timestamp { unix_millis: 10 },
                },
            )
        };

        let response = call(&mut notifies, Some("two"));

        assert!(response.ok);
        assert_eq!(response.data.unwrap()["delivered"], true);
        assert_eq!(
            notifies
                .pending(&TerminalId::new("two"))
                .map(|notify| notify.kind.clone()),
            Some(NotifyKind::Message {
                title: String::new(),
                body: "tests passed".to_owned(),
            })
        );

        // The pane holding the master frame is on screen already, so the
        // notification is intentionally suppressed rather than claimed as
        // delivered.
        let master = call(&mut notifies, Some("one"));
        assert!(master.ok);
        let master_data = master.data.unwrap();
        assert_eq!(master_data["delivered"], false);
        assert_eq!(master_data["reason"], "caller pane is already visible");
        assert!(notifies.pending(&TerminalId::new("one")).is_none());
        let outside = call(&mut notifies, None);
        assert!(outside.ok);
        assert_eq!(outside.data.as_ref().unwrap()["delivered"], false);
        assert_eq!(
            outside.data.unwrap()["reason"],
            "caller is not a live terminal pane"
        );
        let gone = call(&mut notifies, Some("gone"));
        assert!(gone.ok);
        assert_eq!(gone.data.unwrap()["delivered"], false);

        // And the argument check is where it was.
        let missing = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "notify".to_owned(),
                ..Default::default()
            },
            state(&workspace, &engine, &projects, &deck, &mut notifies),
        );
        assert_eq!(missing.error.unwrap().code, 2);
    }

    #[test]
    fn notify_normalizes_a_real_panes_directory_identity() {
        let root = test_path("notify-identity");
        let project_path = root.join("fitness-agent");
        let opened_path = root.join("opened-fitness-agent");
        fs::create_dir_all(&project_path).unwrap();
        std::os::unix::fs::symlink(&project_path, &opened_path).unwrap();

        let projects = vec![
            project("one"),
            Project {
                terminal: TerminalId::new("fitness-agent"),
                path: project_path.clone(),
                command: vec!["sh".to_owned()],
                shell_hook: false,
            },
        ];
        let engine = FakeEngine::new(projects.iter().map(|project| project.terminal.clone()));
        let deck = DeckState::new(projects.len());
        let workspace = Workspace::discovered(root.clone(), projects.clone());
        let mut notifies = Notifications::new();
        let response = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "notify".to_owned(),
                msg: Some("tests passed".to_owned()),
                ..Default::default()
            },
            State {
                workspace: &workspace,
                projects: &projects,
                deck: &deck,
                engine: &engine,
                size: ScreenSize::new(80, 24),
                sheet_open: false,
                notifies: &mut notifies,
                caller: opened_path.to_str(),
                now: Timestamp { unix_millis: 10 },
            },
        );

        assert!(response.ok);
        assert_eq!(
            notifies
                .pending(&TerminalId::new("fitness-agent"))
                .map(|notify| notify.kind.clone()),
            Some(NotifyKind::Message {
                title: String::new(),
                body: "tests passed".to_owned(),
            })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pane_path_normalization_expands_tilde_before_resolving() {
        let home = test_path("notify-home");
        let pane = home.join("fitness-agent");
        fs::create_dir_all(&pane).unwrap();

        assert_eq!(
            normalized_pane_path("~/fitness-agent", Some(home.as_os_str())),
            normalized_pane_path(pane.to_str().unwrap(), None),
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn socket_round_trip_and_mode_are_local() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let listener = Listener::bind().unwrap();
        let mode = fs::metadata(listener.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        let mut client = UnixStream::connect(listener.path()).unwrap();
        client
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();
        let projects = vec![project("one")];
        let engine = FakeEngine::new([projects[0].terminal.clone()]);
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck, &mut notifies))
                .unwrap()
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"ok\":true"));
        assert_eq!(peer_uid(&client).unwrap(), unsafe { libc::getuid() });
        assert!(!trusted_peer(
            &client,
            unsafe { libc::getuid() }.saturating_add(1)
        ));
    }

    #[test]
    fn stalled_client_does_not_starve_a_later_complete_request() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let mut listener = Listener::bind().unwrap();
        let _stalled = UnixStream::connect(listener.path()).unwrap();
        let mut complete = UnixStream::connect(listener.path()).unwrap();
        complete
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();

        // The first poll rotates the incomplete request. The next one must
        // reach the complete peer rather than keeping a single pending slot.
        assert!(
            !listener
                .poll_with(|_, _| panic!("the stalled request is incomplete"))
                .unwrap()
        );
        assert!(
            listener
                .poll_with(|_, _| Response::ok(serde_json::json!({ "served": true })))
                .unwrap()
        );

        let mut response = String::new();
        complete.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"served\":true"));
    }

    #[test]
    fn expired_request_slot_is_reclaimed_before_admitting_more_clients() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let mut listener = Listener::bind().unwrap();
        let _stalled = UnixStream::connect(listener.path()).unwrap();
        assert!(
            !listener
                .poll_with(|_, _| panic!("the request has no newline"))
                .unwrap()
        );
        listener.pending.front_mut().unwrap().deadline = Instant::now();

        let mut complete = UnixStream::connect(listener.path()).unwrap();
        complete
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();
        assert!(
            listener
                .poll_with(|_, _| Response::ok(serde_json::json!({ "served": true })))
                .unwrap()
        );
        let mut response = String::new();
        complete.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"served\":true"));
    }

    #[test]
    fn large_reply_to_a_nonreader_never_blocks_the_poll_loop() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let mut listener = Listener::bind().unwrap();
        let client = UnixStream::connect(listener.path()).unwrap();
        let receive_buffer: libc::c_int = 1024;
        // SAFETY: the socket and option storage are both valid for setsockopt.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    client.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    (&raw const receive_buffer).cast(),
                    std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
                )
            },
            0
        );
        let mut client = client;
        client
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();

        let started = Instant::now();
        assert!(
            listener
                .poll_with(|_, _| {
                    Response::ok(serde_json::json!({
                        "payload": "x".repeat(MAX_RESPONSE - 1024),
                    }))
                })
                .unwrap()
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a non-reading peer stalled the session poll loop"
        );
    }

    #[test]
    fn client_call_has_a_deadline_and_response_cap() {
        let timeout_path = test_path("client-timeout.sock");
        let timeout_listener = UnixListener::bind(&timeout_path).unwrap();
        let timeout_server = thread::spawn(move || {
            let _client = timeout_listener.accept().unwrap();
            // Far beyond the client deadline below: without a deadline the
            // call would stall here, with one it fails at the timeout. The
            // gap between the two is the whole assertion, so it is
            // deliberately wide — CI scheduling jitter must not close it
            // from either side (#117 NB-2).
            thread::sleep(Duration::from_millis(500));
        });
        let request = Request {
            schema: SCHEMA.to_owned(),
            verb: "version".to_owned(),
            ..Default::default()
        };
        let started = Instant::now();
        let timeout = call_with_limits(&timeout_path, &request, Duration::from_millis(50), 1024);
        assert!(timeout.is_err());
        assert!(started.elapsed() < Duration::from_millis(300));
        timeout_server.join().unwrap();
        fs::remove_file(&timeout_path).unwrap();

        let cap_path = test_path("client-cap.sock");
        let cap_listener = UnixListener::bind(&cap_path).unwrap();
        let cap_server = thread::spawn(move || {
            let (mut client, _) = cap_listener.accept().unwrap();
            let mut request = [0_u8; 256];
            let _ = client.read(&mut request).unwrap();
            client.write_all(&[b'x'; 128]).unwrap();
        });
        let capped = call_with_limits(&cap_path, &request, Duration::from_millis(100), 64);
        assert!(capped.unwrap_err().contains("limit"));
        cap_server.join().unwrap();
        fs::remove_file(&cap_path).unwrap();
    }

    /// #146: the client established its connection before its deadline
    /// existed, so the 2 s bound covered only write and read. Against a
    /// listener whose queue is full — an overloaded session — `connect`
    /// blocked in the kernel with no bound at all. One absolute deadline
    /// now spans connect, write and read together.
    #[test]
    fn client_call_returns_within_its_deadline_against_a_saturated_backlog() {
        let path = test_path("client-backlog.sock");
        // Bound and never accepted: every dial below stays in the queue.
        let listener = UnixListener::bind(&path).unwrap();
        // Re-listen with a queue of one, so saturation takes a handful of
        // dials instead of however many this host's `somaxconn` allows.
        // SAFETY: `listen(2)` on the descriptor the listener owns.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        let mut queued = Vec::new();
        while queued.len() < SATURATION_ATTEMPTS {
            let Ok(stream) =
                connect_with_deadline(&path, Instant::now() + Duration::from_millis(20))
            else {
                break;
            };
            queued.push(stream);
        }
        assert!(
            !queued.is_empty() && queued.len() < SATURATION_ATTEMPTS,
            "the listen queue never saturated after {} dials",
            queued.len()
        );

        let request = Request {
            schema: SCHEMA.to_owned(),
            verb: "version".to_owned(),
            ..Default::default()
        };
        let started = Instant::now();
        let blocked = call_with_limits(&path, &request, Duration::from_millis(200), 1024);
        let elapsed = started.elapsed();

        assert!(blocked.is_err(), "a saturated session cannot have answered");
        assert!(
            elapsed < Duration::from_secs(1),
            "the call outlived its advertised bound, took {elapsed:?}"
        );
        drop(queued);
        drop(listener);
        fs::remove_file(&path).unwrap();
    }

    /// Comfortably past the one-slot queue set above, so failing to
    /// saturate is a test failure rather than a silently trivial pass.
    const SATURATION_ATTEMPTS: usize = 64;

    #[test]
    fn oversized_line_returns_bad_request() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let listener = Listener::bind().unwrap();
        let mut client = UnixStream::connect(listener.path()).unwrap();
        client.write_all(&vec![b'x'; 64 * 1024 + 1]).unwrap();
        let projects = vec![project("one")];
        let engine = FakeEngine::new([projects[0].terminal.clone()]);
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck, &mut notifies))
                .unwrap()
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"code\":2"));
    }

    #[test]
    fn abandoned_reply_does_not_stop_the_next_request() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let listener = Listener::bind().unwrap();
        let mut abandoned = UnixStream::connect(listener.path()).unwrap();
        abandoned
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();
        abandoned.shutdown(std::net::Shutdown::Both).unwrap();
        let projects = vec![project("one")];
        let engine = FakeEngine::new([projects[0].terminal.clone()]);
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck, &mut notifies))
                .unwrap()
        );

        let mut client = UnixStream::connect(listener.path()).unwrap();
        client
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck, &mut notifies))
                .unwrap()
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"ok\":true"));
    }

    #[test]
    fn unavailable_history_is_a_code_one_envelope_over_the_socket() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let listener = Listener::bind().unwrap();
        let mut client = UnixStream::connect(listener.path()).unwrap();
        client
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"peek\",\"id\":\"one\"}\n")
            .unwrap();
        let projects = vec![project("one")];
        let engine = NoHistory;
        let deck = DeckState::new(1);
        let mut notifies = Notifications::new();
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(State {
                    workspace: &workspace,
                    projects: &projects,
                    deck: &deck,
                    engine: &engine,
                    size: ScreenSize::new(80, 24),
                    sheet_open: false,
                    notifies: &mut notifies,
                    caller: None,
                    now: Timestamp::default(),
                })
                .unwrap()
        );
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let response: super::Response = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(response.schema, SCHEMA);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, 1);
    }

    #[test]
    fn startup_unlinks_its_stale_socket() {
        let _lock = LISTENER_TEST_LOCK.lock().unwrap();
        let directory = socket_directory(unsafe { libc::getuid() }).unwrap();
        let parent = directory.join(std::process::id().to_string());
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join("ctl.sock");
        let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(stale);

        let listener = Listener::bind().unwrap();
        assert_eq!(listener.path(), path);
    }
}
