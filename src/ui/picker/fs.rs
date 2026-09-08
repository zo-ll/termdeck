use std::cell::RefCell;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::render::display_path;
use super::{Browse, Entry, Listing};

use std::{
    fs,
    time::{Duration, SystemTime},
};

/// What a row says when the fact it would otherwise state cannot be read. A
/// picker that guesses is worse than one that admits the gap: the note's
/// detail block is read as fact.
const UNKNOWN: &str = "unknown";

/// The real browser: `read_dir` plus what `.git` can be asked cheaply.
///
/// Everything here is one stat or one small read per row. No process is
/// spawned and no repository is opened, because the note requires the detail
/// block to never block the cursor.
///
/// What it answers is also remembered (#127). A listing costs a `read_dir`
/// for the folder and another for every row in it, and the picker's loop
/// asked for one fifty times a second whether or not anything had changed —
/// a fifth of a core in a folder of a hundred, for a screen nobody redrew.
/// A cached answer is checked against the folder's own modification time,
/// which is one stat.
pub struct FsBrowse {
    roots: Vec<PathBuf>,
    home: Option<PathBuf>,
    cache: RefCell<Cache>,
}

/// What the browser was last asked, and what it said.
///
/// One slot each, because the picker stands in one folder at a time: walking
/// into another is itself the invalidation, since the new path misses and
/// takes the slot over.
#[derive(Default)]
struct Cache {
    /// The root list, which is not a folder and so is keyed by nothing.
    roots: Option<(Stamp, Vec<Entry>)>,
    /// The folder listed last, and the folder searched last.
    listing: Option<(PathBuf, Stamp, Listing)>,
    search: Option<(PathBuf, Stamp, Vec<Entry>)>,
}

/// The modification times of the folders an answer was read from: one stat
/// each, against the `read_dir` per row that building it costs.
///
/// A folder's own time moves when a child is added, removed, or renamed,
/// which is what a listing draws. It does not move for a change further
/// down — a repository's branch, or what a child folder holds — so those
/// arrive when the picker next walks somewhere and back, or when
/// [`FsBrowse::refresh`] says so outright.
#[derive(Eq, PartialEq)]
struct Stamp(Vec<Option<SystemTime>>);

impl Stamp {
    fn of<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        Self(
            paths
                .into_iter()
                .map(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok())
                .collect(),
        )
    }
}

impl FsBrowse {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            home: std::env::var_os("HOME").map(PathBuf::from),
            cache: RefCell::new(Cache::default()),
        }
    }

    /// The home the picker abbreviates paths against.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Forgets every cached answer, so the next question goes to the
    /// filesystem. Walking to another folder invalidates by itself; this is
    /// for a caller that knows something moved under the folder it is
    /// standing in.
    pub fn refresh(&self) {
        *self.cache.borrow_mut() = Cache::default();
    }

    /// Every folder a search starts from, which is what its answer depends
    /// on: the folder being browsed, then the configured roots.
    fn search_from<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Path> {
        std::iter::once(path).chain(self.roots.iter().map(PathBuf::as_path))
    }

    fn entry(path: &Path, name: String) -> Entry {
        if let Some(git) = git_dir(path) {
            let entry = Entry::repository(name, path);
            match head(&git) {
                Some(head) => {
                    let age = commit_age(&git, &head).unwrap_or_else(|| UNKNOWN.to_owned());
                    // Nothing here can count what is uncommitted without
                    // opening the repository, and the picker opens nothing.
                    // So it says it does not know, rather than drawing the
                    // clean state a hardcoded zero used to claim (#128).
                    entry.git(head.label(), None, age)
                }
                None => entry,
            }
        } else if path.is_dir() {
            // A folder whose contents cannot be read claims nothing about
            // them: `0 items · no repos` would be the same lie the listing
            // body refuses to tell.
            match count(path) {
                Some((items, repos)) => Entry::folder(name, path).holding(items, repos),
                None => Entry::folder(name, path),
            }
        } else {
            Entry::file(name, path)
        }
    }

    /// The listing itself, read fresh.
    fn read(path: &Path) -> Listing {
        // The parent row goes in whatever happens: a folder that cannot be
        // read is one the user especially needs a way out of.
        let mut entries = Vec::new();
        if path.parent().is_some() {
            entries.push(Entry::parent(path.parent().unwrap_or(path).to_path_buf()));
        }
        // A read that fails is not a folder that is empty. Swallowing the
        // error would draw an empty listing and tell the user their folder
        // holds nothing, which is the one thing the picker must never say
        // about a folder it could not open.
        let read = match fs::read_dir(path) {
            Ok(read) => read,
            Err(error) => {
                return Listing {
                    entries,
                    error: Some(error.to_string()),
                    elsewhere: 0,
                };
            }
        };
        let mut read: Vec<_> = read
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        read.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
        // Folders and repositories interleaved by name, files last (§2.3).
        let (folders, files): (Vec<_>, Vec<_>) =
            read.into_iter().partition(|entry| entry.path().is_dir());
        for entry in folders.into_iter().chain(files) {
            entries.push(Self::entry(
                &entry.path(),
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
        Listing::of(entries)
    }

    /// The search itself, walked fresh.
    fn scan(&self, path: &Path) -> Vec<Entry> {
        // Configured roots may overlap — the default pair is the working
        // directory and the home that usually contains it — and a repository
        // inside two of them would otherwise be found once per root and
        // listed twice, in both partitions.
        //
        // So a repository belongs to exactly one searcher: the most specific
        // one that contains it. The folder being browsed claims its own
        // matches first, then the roots from the deepest outwards, and a path
        // already claimed is skipped. Paths are compared canonically, so a
        // symlinked or `..`-laden root cannot smuggle the same repository in
        // twice.
        let mut deepest: Vec<&Path> = self
            .roots
            .iter()
            .map(PathBuf::as_path)
            .filter(|root| *root != path)
            .collect();
        deepest.sort_by_key(|root| std::cmp::Reverse(root.components().count()));

        let mut found = Vec::new();
        let mut claimed = std::collections::HashSet::new();
        for start in std::iter::once(path).chain(deepest) {
            let mut batch = Vec::new();
            walk(start, 3, &mut batch);
            for entry in batch {
                if claimed.insert(canonical(&entry.path)) {
                    found.push(entry);
                }
            }
        }
        found
    }

    /// The root list itself, read fresh.
    fn read_roots(&self) -> Vec<Entry> {
        self.roots
            .iter()
            .map(|root| {
                let root_entry = Entry::folder(display_path(root, self.home.as_deref()), root);
                match count(root) {
                    Some((items, repos)) => root_entry.holding(items, repos),
                    None => root_entry,
                }
            })
            .collect()
    }
}

impl Browse for FsBrowse {
    fn list(&self, path: &Path) -> Listing {
        let stamp = Stamp::of([path]);
        if let Some((of, at, listing)) = self.cache.borrow().listing.as_ref()
            && of == path
            && *at == stamp
        {
            return listing.clone();
        }
        let listing = Self::read(path);
        self.cache.borrow_mut().listing = Some((path.to_path_buf(), stamp, listing.clone()));
        listing
    }

    fn search(&self, path: &Path) -> Vec<Entry> {
        // A search is a walk three folders deep from every root, which is far
        // too much to repeat per frame. Its stamp covers the folders it
        // starts from; anything deeper waits for a walk elsewhere or a
        // `refresh`, the same terms the listing keeps.
        let stamp = Stamp::of(self.search_from(path));
        if let Some((of, at, found)) = self.cache.borrow().search.as_ref()
            && of == path
            && *at == stamp
        {
            return found.clone();
        }
        let found = self.scan(path);
        self.cache.borrow_mut().search = Some((path.to_path_buf(), stamp, found.clone()));
        found
    }

    fn roots(&self) -> Vec<Entry> {
        let stamp = Stamp::of(self.roots.iter().map(PathBuf::as_path));
        if let Some((at, roots)) = self.cache.borrow().roots.as_ref()
            && *at == stamp
        {
            return roots.clone();
        }
        let roots = self.read_roots();
        self.cache.borrow_mut().roots = Some((stamp, roots.clone()));
        roots
    }
}

/// Repositories at or under `path`, to `depth` folders down.
fn walk(path: &Path, depth: usize, found: &mut Vec<Entry>) {
    if depth == 0 {
        return;
    }
    let Ok(read) = fs::read_dir(path) else {
        return;
    };
    let mut children: Vec<_> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|child| child.is_dir())
        .filter(|child| {
            !child
                .file_name()
                .map(|name| name.to_string_lossy().starts_with('.'))
                .unwrap_or(true)
        })
        .collect();
    children.sort();
    for child in children {
        let name = child
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_repository(&child) {
            found.push(FsBrowse::entry(&child, name));
        } else {
            walk(&child, depth - 1, found);
        }
    }
}

/// A path in the form two roots can be compared by. Falls back to the path
/// itself when it cannot be resolved, which still de-duplicates the ordinary
/// case.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_repository(path: &Path) -> bool {
    git_dir(path).is_some()
}

/// Where a repository keeps its administrative files.
///
/// Usually the `.git` folder itself. A linked worktree's `.git` is a *file*
/// holding `gitdir: <path>`, and a submodule's says the same — the
/// indirection the picker used to walk straight past. It recognised the
/// worktree as a repository and then looked for a HEAD that was not there,
/// so every worktree drew as a repository with no branch at all (#128).
fn git_dir(path: &Path) -> Option<PathBuf> {
    let git = path.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    let pointer = fs::read_to_string(&git).ok()?;
    let target = Path::new(pointer.trim().strip_prefix("gitdir:")?.trim());
    if target.as_os_str().is_empty() {
        return None;
    }
    // The pointer is written relative to the worktree when it is relative at
    // all. A pointer at nothing is not a repository: the folder it named has
    // been thrown away, and no branch will ever be read out of it.
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        path.join(target)
    };
    target.is_dir().then_some(target)
}

/// The folder every worktree of a repository shares, which is where the
/// branches and their logs live. A worktree's own gitdir names it in
/// `commondir`; an ordinary repository is already its own.
fn common_dir(git: &Path) -> PathBuf {
    let Ok(common) = fs::read_to_string(git.join("commondir")) else {
        return git.to_path_buf();
    };
    let common = Path::new(common.trim());
    if common.is_absolute() {
        common.to_path_buf()
    } else {
        git.join(common)
    }
}

/// What HEAD points at. A detached HEAD names no branch, and the two are
/// read out of different logs, so the difference is kept rather than
/// flattened into one string.
enum Head {
    Branch(String),
    Detached(String),
}

impl Head {
    /// What the row draws where a branch goes: the branch, or the short
    /// commit a detached HEAD sits on — which is what it drew before.
    fn label(&self) -> String {
        match self {
            Self::Branch(branch) | Self::Detached(branch) => branch.clone(),
        }
    }
}

/// The checked-out branch, read straight out of the gitdir's `HEAD`.
fn head(git: &Path) -> Option<Head> {
    let head = fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => Head::Branch(branch.to_owned()),
        None => Head::Detached(head.chars().take(7).collect()),
    })
}

/// How long ago the checked-out branch last took a commit.
///
/// Git writes a line into a ref's reflog every time its tip moves, and the
/// timestamp on a `commit` line is that commit's own committer date, written
/// as the commit was made. Without decompressing an object — which would
/// mean opening the repository, which the picker does not do — that is the
/// only commit time there is to read, so a tip that arrived any other way (a
/// clone, a pull, a reset, a rebase) has no age here and says so.
///
/// What was drawn before was `.git/HEAD`'s modification time, which is when
/// the branch was last *checked out*: a repository untouched for a year read
/// `just now` the moment you switched to it, and a commit made on the branch
/// you were already on moved it not at all (#128).
fn commit_age(git: &Path, head: &Head) -> Option<String> {
    let common = common_dir(git);
    let branch_log = match head {
        Head::Branch(branch) => Some(common.join("logs/refs/heads").join(branch)),
        Head::Detached(_) => None,
    };
    // The branch's own log first; the worktree's HEAD log is the same
    // event seen from the other side, and is all there is when the branch
    // keeps no log of its own.
    let when = branch_log
        .and_then(|log| last_commit(&log))
        .or_else(|| last_commit(&git.join("logs/HEAD")))?;
    age(when)
}

/// The time on the last line of a reflog, when that line is a commit.
fn last_commit(log: &Path) -> Option<SystemTime> {
    let line = last_line(log)?;
    // `<old> <new> <who> <email> <seconds> <zone>\t<action>: <message>`
    let (entry, action) = line.split_once('\t')?;
    if !action.starts_with("commit") {
        return None;
    }
    let mut fields = entry.split_whitespace().rev();
    let _zone = fields.next()?;
    let seconds: u64 = fields.next()?.parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
}

/// The last non-empty line of a file, reading only the tail of it: a reflog
/// grows without bound, and the picker promises one small read per row.
fn last_line(path: &Path) -> Option<String> {
    const TAIL: u64 = 4_096;
    let mut file = fs::File::open(path).ok()?;
    let end = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(end.saturating_sub(TAIL))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    // The window can open in the middle of a character, which is a reason to
    // drop that character and not the line it was in.
    String::from_utf8_lossy(&tail)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

/// How long ago something happened, phrased the way the tail column draws
/// it. A time in the future — a clock that disagrees with the one that
/// wrote it — is not an age, and says nothing rather than a wrong something.
fn age(when: SystemTime) -> Option<String> {
    let elapsed = SystemTime::now().duration_since(when).ok()?.as_secs();
    Some(match elapsed {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{}m ago", elapsed / 60),
        3600..=86_399 => format!("{}h ago", elapsed / 3600),
        86_400..=2_591_999 => format!("{}d ago", elapsed / 86_400),
        _ => format!("{}w ago", elapsed / 604_800),
    })
}

/// What a folder holds, for `9 items · 5 repos`. `None` when it could not be
/// read, which is not the same as holding nothing.
fn count(path: &Path) -> Option<(usize, usize)> {
    let read = fs::read_dir(path).ok()?;
    let children: Vec<_> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|child| {
            !child
                .file_name()
                .map(|name| name.to_string_lossy().starts_with('.'))
                .unwrap_or(true)
        })
        .collect();
    let repos = children
        .iter()
        .filter(|child| child.is_dir() && is_repository(child))
        .count();
    Some((children.len(), repos))
}
