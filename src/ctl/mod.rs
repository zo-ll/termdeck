//! The local ctl.v1 adapter.  It deliberately sits outside the frozen engine/UI
//! contracts and is polled by the session's existing single-threaded loop.

use std::{
    env, fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
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
            let Some(lines) = state
                .engine
                .history_lines(&project.terminal, request.lines.unwrap_or(30))
            else {
                return Response::error(1, "history is unavailable for this terminal");
            };
            Response::ok(json!({
                "id": project.terminal.to_string(),
                "lines": lines,
                "alt_screen": state.engine.metadata(&project.terminal)
                    .is_some_and(|metadata| metadata.alt_screen),
            }))
        }
        "notify" => match request.msg {
            Some(message) => {
                dispatch_notify(&message, &mut state);
                Response::ok(json!({ "delivered": true }))
            }
            None => Response::error(2, "bad request: notify requires msg"),
        },
        "version" => {
            Response::ok(json!({ "schema": SCHEMA, "version": env!("CARGO_PKG_VERSION") }))
        }
        _ => Response::error(2, "bad request: unknown verb"),
    }
}

/// Routes an explicit notification into the session's notify state (#71/#97).
///
/// The caller names itself by running where it runs: every spawned PTY
/// inherits `TERMDECK_PANE`, so the pane the message belongs to is the pane
/// the connection came from.  A caller outside every pane (one holding only
/// `TERMDECK_SOCK`) has no pane to flash, and one naming a terminal this
/// session does not run has none either; both are dropped here.  The wire is
/// unchanged either way: `{delivered:true}` is what shipped, and the response
/// has never reported what the interface then did with it.
fn dispatch_notify<E: TerminalEngine>(message: &str, state: &mut State<'_, E>) -> bool {
    let Some(project) = state
        .caller
        .and_then(|pane| project_for_pane(pane, state.projects))
    else {
        return false;
    };
    let master = state
        .deck
        .active()
        .and_then(|position| state.projects.get(position))
        .map(|project| &project.terminal);
    state.notifies.record(
        &project.terminal,
        master,
        NotifyKind::Message {
            title: String::new(),
            body: message.to_owned(),
        },
        state.now,
    )
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
    bytes: Vec<u8>,
    pane: Option<String>,
}

/// Session-owned listener.  `poll` accepts at most one connection and serves
/// at most one request, so ctl work stays bounded by the UI frame cadence.
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
    uid: u32,
    pending: Option<Pending>,
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
            pending: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Polls one client without blocking the UI.  A partial request remains
    /// pending until its newline arrives; no second connection is accepted in
    /// that frame.
    pub fn poll<E: TerminalEngine>(&mut self, state: State<'_, E>) -> io::Result<bool> {
        self.poll_with(move |request, caller| dispatch(request, State { caller, ..state }))
    }

    /// Like [`Listener::poll`], but hands the caller's pane identity to the
    /// session for the Phase-2 close-self guard.
    pub fn poll_with(
        &mut self,
        dispatch_request: impl FnOnce(Request, Option<&str>) -> Response,
    ) -> io::Result<bool> {
        if self.pending.is_none() {
            match self.listener.accept() {
                Ok((stream, _)) if trusted_peer(&stream, self.uid) => {
                    stream.set_nonblocking(true)?;
                    self.pending = Some(Pending {
                        pane: caller_pane(&stream),
                        stream,
                        bytes: Vec::new(),
                    });
                }
                Ok((_stream, _)) => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) => return Err(error),
            }
        }

        let Some(mut pending) = self.pending.take() else {
            return Ok(false);
        };
        let result = read_request(&mut pending);
        let response = match result {
            Ok(Some(request)) => dispatch_request(request, pending.pane.as_deref()),
            Ok(None) => {
                self.pending = Some(pending);
                return Ok(false);
            }
            Err(message) => Response::error(2, format!("bad request: {message}")),
        };
        pending.stream.set_nonblocking(false)?;
        // The caller may abandon its one-shot connection after sending the
        // request. Its EPIPE/ECONNRESET is local to that caller, never a
        // reason to end the interactive session.
        let _ = write_response(&mut pending.stream, &response);
        Ok(true)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_request(pending: &mut Pending) -> Result<Option<Request>, String> {
    let mut buffer = [0_u8; 4096];
    loop {
        if let Some(end) = pending.bytes.iter().position(|byte| *byte == b'\n') {
            if end > MAX_LINE
                || pending.bytes[end + 1..]
                    .iter()
                    .any(|byte| !byte.is_ascii_whitespace())
            {
                return Err("request line exceeds protocol limits".to_owned());
            }
            return serde_json::from_slice(&pending.bytes[..end])
                .map(Some)
                .map_err(|error| error.to_string());
        }
        if pending.bytes.len() > MAX_LINE {
            return Err("request line exceeds 64 KiB".to_owned());
        }
        match pending.stream.read(&mut buffer) {
            Ok(0) => return Err("connection closed before newline".to_owned()),
            Ok(read) => pending.bytes.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn write_response(stream: &mut UnixStream, response: &Response) -> io::Result<()> {
    let mut encoded = serde_json::to_vec(response).map_err(io::Error::other)?;
    encoded.push(b'\n');
    stream.write_all(&encoded)
}

/// Dials a running session using the same one-request ctl.v1 transport as
/// `termctl`.  The caller owns formatting and exit-code selection.
pub fn call(socket: &Path, request: &Request) -> Result<Response, String> {
    let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
    let mut encoded = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    let response = response.trim().to_owned();
    serde_json::from_str(&response).map_err(|error| error.to_string())
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
        os::unix::{fs::PermissionsExt, net::UnixStream},
        path::PathBuf,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
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
        Control, Listener, Request, SCHEMA, State, dispatch, normalized_pane_path, peer_uid,
        socket_directory, trusted_peer,
    };

    static LISTENER_TEST_LOCK: Mutex<()> = Mutex::new(());

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
        engine.set_history_lines(
            &terminal,
            vec!["before viewport".to_owned(), "tail".to_owned()],
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
        assert_eq!(data["alt_screen"], true);
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
    fn control_requests_require_their_arguments_and_keep_input_kinds_distinct() {
        let missing = Request {
            schema: SCHEMA.to_owned(),
            verb: "close".to_owned(),
            ..Default::default()
        }
        .control()
        .unwrap_err();
        assert_eq!(missing.error.unwrap().code, 2);

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

    /// #97: the shipped wire is untouched — `notify` still answers
    /// `{delivered:true}` — and the message now lands in the session's notify
    /// state, addressed to the pane the connection came from.
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

        // The pane holding the master frame is on screen already, and a
        // caller in no pane at all has no pane to mark. Both still deliver:
        // the response has never reported what the interface did with it.
        assert!(call(&mut notifies, Some("one")).ok);
        assert!(notifies.pending(&TerminalId::new("one")).is_none());
        assert!(call(&mut notifies, None).ok);
        assert!(call(&mut notifies, Some("gone")).ok);

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
