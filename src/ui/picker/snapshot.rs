//! The saved-session inventory the context picker browses.
//!
//! `termdeck` with no path asks one question before it asks any other: pick
//! up where you left off, or open a folder. The rows on the first half of
//! that screen come from here — one per `session.v1` file under the sessions
//! directory, recent first.
//!
//! What this module reads is deliberately thin. A snapshot file holds every
//! pane's transcript, which is the bulk of it and none of the picker's
//! business: the listing says a workspace's name, where it was taken, how
//! many panes it had and how long ago it was saved, and nothing else. The
//! header is therefore streamed off the file with the pane array skipped
//! token by token (`IgnoredAny`), so no transcript line is ever allocated to
//! draw a row.
//!
//! A file that cannot be read that way is skipped and counted. A picker that
//! panics on one corrupt snapshot takes every other saved session down with
//! it, which is exactly the robustness the audit asked for.
//!
//! The directory is the engine's — [`crate::session::snapshot::sessions_dir`]
//! names it, the engine writes it, and the browser is handed it. Resuming a
//! row is the engine's too: the file this reader skipped past is the file
//! `session::resume` opens in full.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;

use super::fs::phrase_age;
use super::render::display_path;
use super::{Browse, Entry, Listing};

/// The schema this reader understands. The engine lane owns the writer and
/// the format; anything else is a file the picker skips rather than guesses
/// at.
pub const SCHEMA: &str = "session.v1";

/// What the "Open a folder…" row says. The escape hatch out of the session
/// list and into the file explorer.
pub const OPEN_A_FOLDER: &str = "Open a folder…";

/// Everything the picker knows about a saved session without opening its
/// transcripts: the four facts a row is made of, plus the file they came out
/// of, which is how a resume is addressed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotHeader {
    /// The workspace name the session ran under.
    pub workspace: String,
    /// The directory it was opened on.
    pub root: PathBuf,
    /// When it was saved, in milliseconds since the epoch.
    pub saved_at: u64,
    /// How many panes the deck held.
    pub panes: usize,
    /// The `session.v1` file itself.
    pub file: PathBuf,
}

impl SnapshotHeader {
    /// How long ago this session was saved, phrased the way every other age
    /// in the picker is ("2h ago", "just now"). A snapshot stamped in the
    /// future — a clock that disagrees with the one that wrote it — has no
    /// age, and says nothing rather than a wrong something.
    pub fn age(&self, now: SystemTime) -> Option<String> {
        let saved = UNIX_EPOCH + Duration::from_millis(self.saved_at);
        Some(phrase_age(now.duration_since(saved).ok()?.as_secs()))
    }

    /// The row this header draws as.
    fn entry(&self, now: SystemTime) -> Entry {
        Entry::snapshot(self.workspace.clone(), self.file.clone()).saved(
            self.root.clone(),
            self.panes,
            self.age(now).unwrap_or_else(|| "unknown".to_owned()),
        )
    }
}

/// The header as it sits in the file. `panes` deserialises into a vector of
/// nothing: serde walks each pane's fields — transcript included — and keeps
/// none of them, so the count costs a scan and no allocation.
#[derive(Deserialize)]
struct Header {
    schema: String,
    workspace: String,
    root: PathBuf,
    saved_at: u64,
    #[serde(default)]
    panes: Vec<serde::de::IgnoredAny>,
}

/// The saved sessions, as a [`Browse`] the picker can list.
///
/// The inventory is read once, when the browser is built. Nothing is running
/// while the context picker is open — no session, no daemon — so the
/// directory cannot change under it, and the loop redraws from what it
/// already has rather than re-reading the disk fifty times a second (#127).
pub struct SnapshotBrowse {
    headers: Vec<SnapshotHeader>,
    unreadable: usize,
    rows: Vec<Entry>,
    home: Option<PathBuf>,
}

impl SnapshotBrowse {
    /// Reads the inventory of `dir`, as of now.
    pub fn new(dir: &Path) -> Self {
        Self::at(dir, SystemTime::now())
    }

    /// The same, against a clock the caller chooses, so a fixture's ages do
    /// not depend on when the test runs.
    pub fn at(dir: &Path, now: SystemTime) -> Self {
        let (headers, unreadable) = inventory(dir);
        let rows = headers
            .iter()
            .map(|header| header.entry(now))
            .chain(std::iter::once(Entry::escape(OPEN_A_FOLDER)))
            .collect();
        Self {
            headers,
            unreadable,
            rows,
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    /// The home the picker abbreviates paths against.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Whether there is nothing to resume. Zero saved sessions is not an
    /// empty context picker: it is no context picker at all, and the caller
    /// falls straight through to the file explorer.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// The saved sessions, recent first.
    pub fn headers(&self) -> &[SnapshotHeader] {
        &self.headers
    }

    /// The session a picked row stands for, addressed by its file.
    pub fn header(&self, file: &Path) -> Option<&SnapshotHeader> {
        self.headers.iter().find(|header| header.file == file)
    }

    /// What the picker says about the files it skipped, if it skipped any.
    /// The count is the honest part: naming them would be a log, and hiding
    /// them would be a lie about what is on disk.
    pub fn notice(&self) -> Option<String> {
        match self.unreadable {
            0 => None,
            1 => Some("1 unreadable snapshot skipped".to_owned()),
            count => Some(format!("{count} unreadable snapshots skipped")),
        }
    }
}

impl Browse for SnapshotBrowse {
    /// A saved session has nothing below it, so there is only ever the one
    /// level: whatever is asked for, the answer is the inventory.
    fn list(&self, _path: &Path) -> Listing {
        Listing::of(self.rows.clone())
    }

    fn search(&self, _path: &Path) -> Vec<Entry> {
        self.rows.clone()
    }

    fn roots(&self) -> Vec<Entry> {
        self.rows.clone()
    }
}

/// Every readable `session.v1` header in `dir`, recent first, and how many
/// files were skipped.
///
/// A missing directory is not an error: it is the ordinary state of a
/// termdeck that has never saved a session, and it holds no snapshots.
fn inventory(dir: &Path) -> (Vec<SnapshotHeader>, usize) {
    let Ok(read) = fs::read_dir(dir) else {
        return (Vec::new(), 0);
    };
    let mut headers = Vec::new();
    let mut unreadable = 0;
    for file in read.flatten().map(|entry| entry.path()) {
        if file.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        match header(&file) {
            Some(header) => headers.push(header),
            None => unreadable += 1,
        }
    }
    // Recent first, and by name where two were saved in the same
    // millisecond, so the list an eye learns is the list it keeps.
    headers.sort_by(|left, right| {
        right
            .saved_at
            .cmp(&left.saved_at)
            .then_with(|| left.workspace.cmp(&right.workspace))
    });
    (headers, unreadable)
}

/// One file's header, or `None` when it is not a `session.v1` this reader
/// understands — truncated, mis-typed, half-written, or another program's
/// JSON that happens to live here.
fn header(file: &Path) -> Option<SnapshotHeader> {
    let handle = fs::File::open(file).ok()?;
    let read: Header = serde_json::from_reader(std::io::BufReader::new(handle)).ok()?;
    if read.schema != SCHEMA || read.workspace.is_empty() {
        return None;
    }
    Some(SnapshotHeader {
        workspace: read.workspace,
        root: read.root,
        saved_at: read.saved_at,
        panes: read.panes.len(),
        file: file.to_path_buf(),
    })
}

/// A snapshot row's own line in the detail block: where the session was
/// taken and what it will cost to bring back.
pub(super) fn detail(entry: &Entry, home: Option<&Path>) -> String {
    let mut facts = Vec::new();
    if let Some(root) = entry.root.as_deref() {
        facts.push(display_path(root, home));
    }
    if let Some(panes) = entry.panes {
        facts.push(match panes {
            1 => "1 pane".to_owned(),
            panes => format!("{panes} panes"),
        });
    }
    if let Some(age) = entry.age.as_deref() {
        facts.push(format!("saved {age}"));
    }
    facts.join(" · ")
}
