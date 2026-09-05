use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::render::display_path;
use super::{Browse, Entry, Listing};

use std::{fs, time::SystemTime};

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
        if is_repository(path) {
            let entry = Entry::repository(name, path);
            let branch = head_branch(path);
            match branch {
                Some(branch) => {
                    let age = commit_age(path).unwrap_or_else(|| "unknown".to_owned());
                    entry.git(branch, 0, age)
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
    path.join(".git").is_dir() || path.join(".git").is_file()
}

/// The checked-out branch, read straight out of `.git/HEAD`.
fn head_branch(path: &Path) -> Option<String> {
    let head = fs::read_to_string(path.join(".git/HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_owned(),
        None => head.chars().take(7).collect(),
    })
}

/// How long ago the branch last moved, from the ref's own mtime — one stat,
/// no repository opened. It is the last commit for any ordinary workflow.
fn commit_age(path: &Path) -> Option<String> {
    let head = path.join(".git/HEAD");
    let modified = fs::metadata(&head).and_then(|meta| meta.modified()).ok()?;
    let elapsed = SystemTime::now().duration_since(modified).ok()?.as_secs();
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
