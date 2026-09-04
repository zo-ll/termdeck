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
    contracts::{Project, ScreenSize, TerminalEngine, TerminalStatus},
    ui::DeckState,
};

pub const SCHEMA: &str = "ctl.v1";
const MAX_LINE: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    pub schema: String,
    pub verb: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub msg: Option<String>,
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
    fn ok(data: Value) -> Self {
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
pub struct State<'a, E> {
    pub workspace: &'a Workspace,
    pub projects: &'a [Project],
    pub deck: &'a DeckState,
    pub engine: &'a E,
    pub size: ScreenSize,
    pub sheet_open: bool,
}

pub fn dispatch<E: TerminalEngine>(request: Request, state: State<'_, E>) -> Response {
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
                dispatch_notify(&message);
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

/// #71 owns the visual overlay.  Keeping this typed handoff here prevents ctl
/// dispatch from becoming a second UI path before that surface lands.
fn dispatch_notify(_message: &str) {}

struct Pending {
    stream: UnixStream,
    bytes: Vec<u8>,
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
        if self.pending.is_none() {
            match self.listener.accept() {
                Ok((stream, _)) if trusted_peer(&stream, self.uid) => {
                    stream.set_nonblocking(true)?;
                    self.pending = Some(Pending {
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
            Ok(Some(request)) => dispatch(request, state),
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

fn trusted_peer(stream: &UnixStream, uid: u32) -> bool {
    peer_uid(stream).is_ok_and(|peer| peer == uid)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        os::unix::{fs::PermissionsExt, net::UnixStream},
        sync::Mutex,
    };

    use crate::{
        config::Workspace,
        contracts::{
            EngineCommand, EngineEvent, Project, ScreenSize, TerminalEngine, TerminalFrame,
            TerminalId, TerminalMetadata, TerminalStatus,
        },
        engine::FakeEngine,
        ui::DeckState,
    };

    use super::{
        Listener, Request, SCHEMA, State, dispatch, peer_uid, socket_directory, trusted_peer,
    };

    static LISTENER_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn state<'a>(
        workspace: &'a Workspace,
        engine: &'a FakeEngine,
        projects: &'a [Project],
        deck: &'a DeckState,
    ) -> State<'a, FakeEngine> {
        State {
            workspace,
            projects,
            deck,
            engine,
            size: ScreenSize::new(80, 24),
            sheet_open: false,
        }
    }

    fn project(name: &str) -> Project {
        Project {
            terminal: TerminalId::new(name),
            path: std::env::temp_dir().join(name),
            command: vec!["sh".to_owned()],
        }
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
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());

        let peek = dispatch(
            Request {
                schema: SCHEMA.to_owned(),
                verb: "peek".to_owned(),
                id: Some("one".to_owned()),
                lines: Some(1),
                msg: None,
            },
            state(&workspace, &engine, &projects, &deck),
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
            },
            state(&workspace, &engine, &projects, &deck),
        );
        assert!(version.ok);
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
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck))
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
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck))
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
        let workspace = Workspace::discovered(std::env::temp_dir(), projects.clone());
        let mut listener = listener;
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck))
                .unwrap()
        );

        let mut client = UnixStream::connect(listener.path()).unwrap();
        client
            .write_all(b"{\"schema\":\"ctl.v1\",\"verb\":\"version\"}\n")
            .unwrap();
        assert!(
            listener
                .poll(state(&workspace, &engine, &projects, &deck))
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
