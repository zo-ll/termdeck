use std::path::{Path, PathBuf};

/// What a listed entry is. The glyph and the colour follow from this, and so
/// does whether it can be selected at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// `◆` — a git repository.
    Repository,
    /// `▸` — a folder to enter.
    Folder,
    /// `▴` — the parent of the current folder.
    Parent,
    /// `·` — a plain file: listed, dimmed, never selectable.
    File,
}

/// One row of a listing, as the browser hands it over: classified, annotated,
/// and already in the order it should be drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
    pub path: PathBuf,
    /// A repository's branch, if the browser could read one.
    pub branch: Option<String>,
    /// Uncommitted files, drawn as `+3` beside the branch. `None` when
    /// nobody counted them: counting is opening the repository, which the
    /// browser does not do, and an absent `+3` therefore says "no count
    /// here" rather than "clean" (#128).
    pub dirty: Option<usize>,
    /// The last commit's age, already phrased ("2h ago", "just now").
    pub age: Option<String>,
    /// What a folder holds, drawn as `9 items` / `9 items · no repos`.
    pub items: Option<usize>,
    pub repos: Option<usize>,
}

impl Entry {
    pub fn repository(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            kind: EntryKind::Repository,
            name: name.into(),
            path: path.into(),
            branch: None,
            dirty: None,
            age: None,
            items: None,
            repos: None,
        }
    }

    pub fn folder(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            kind: EntryKind::Folder,
            ..Self::repository(name, path)
        }
    }

    pub fn parent(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: EntryKind::Parent,
            ..Self::repository("..", path)
        }
    }

    pub fn file(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            kind: EntryKind::File,
            ..Self::repository(name, path)
        }
    }

    pub fn git(
        mut self,
        branch: impl Into<String>,
        dirty: Option<usize>,
        age: impl Into<String>,
    ) -> Self {
        self.branch = Some(branch.into());
        self.dirty = dirty;
        self.age = Some(age.into());
        self
    }

    pub fn holding(mut self, items: usize, repos: usize) -> Self {
        self.items = Some(items);
        self.repos = Some(repos);
        self
    }

    /// Any directory can be a terminal's working directory, so a folder joins
    /// a workspace on the same terms as a repository. A plain file is only
    /// ever context, and `..` is a way out rather than a place to work.
    pub fn selectable(&self) -> bool {
        matches!(self.kind, EntryKind::Repository | EntryKind::Folder)
    }

    /// Whether `→` can go inside this row. Every directory can be entered,
    /// including a repository: a repository that holds `projects/` is a
    /// perfectly ordinary folder to look inside.
    pub fn enterable(&self) -> bool {
        matches!(
            self.kind,
            EntryKind::Repository | EntryKind::Folder | EntryKind::Parent
        )
    }
}

/// One folder, as the picker sees it.
///
/// A listing that could not be read is not an empty one, and the picker has
/// to be able to tell them apart: an unreadable folder that draws as empty is
/// a lie about the filesystem. `error` carries why, and the view says it where
/// the rows would have been.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Why this folder could not be read, when it could not be.
    pub error: Option<String>,
    /// How many of the trailing entries came from other roots, which the
    /// filter draws under its own rule.
    pub elsewhere: usize,
}

impl Listing {
    pub fn of(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            error: None,
            elsewhere: 0,
        }
    }

    /// A folder the picker could read and that holds nothing to show. The
    /// parent row does not count: it is a way out, not a child.
    pub fn is_empty_folder(&self) -> bool {
        self.error.is_none()
            && self
                .entries
                .iter()
                .all(|entry| entry.kind == EntryKind::Parent)
    }
}

/// Where listings come from. The picker asks; something behind this seam
/// walks the filesystem and reads git.
pub trait Browse {
    /// The entries of `path`, in draw order, with the parent row first when
    /// there is somewhere above to go.
    fn list(&self, path: &Path) -> Listing;

    /// Every repository at or below `path`, for the filter — the note's
    /// "a filter is a workspace-wide search, not a folder one".
    fn search(&self, path: &Path) -> Vec<Entry>;

    /// The configured roots, drawn as the picker's own top level.
    fn roots(&self) -> Vec<Entry>;
}

/// One selected terminal: a path, the name it will run under, and which
/// instance of that path it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instance {
    pub name: String,
    pub path: PathBuf,
}

/// Everything the picker holds while it is open.
///
/// `selection` is the pane order outright: index 0 is pane 1 and therefore the
/// master, per the note's "the ordinal is the pane number". A path may appear
/// in it more than once (§3.1), each appearance with its own name.
#[derive(Clone, Debug)]
pub struct PickerState {
    cwd: Option<PathBuf>,
    cursor: usize,
    selection: Vec<Instance>,
    filter: Option<String>,
    workspace: String,
    renaming: bool,
    offset: usize,
}

impl PickerState {
    /// Opens at the root list, which is the picker's own top level.
    pub fn new() -> Self {
        Self {
            cwd: None,
            cursor: 0,
            selection: Vec::new(),
            filter: None,
            workspace: String::new(),
            renaming: false,
            offset: 0,
        }
    }

    /// Opens inside a folder.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            workspace: folder_name(&path),
            cwd: Some(path),
            ..Self::new()
        }
    }

    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    pub fn filtering(&self) -> bool {
        self.filter.is_some()
    }

    pub fn renaming(&self) -> bool {
        self.renaming
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The workspace the selection will open as. Empty until a folder names
    /// it, so the launch line can say so.
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// The selection in pane order. `selection()[0]` is the master.
    pub fn selection(&self) -> &[Instance] {
        &self.selection
    }

    /// What the picker hands the session: ordered `(name, path)` pairs.
    pub fn launch(&self) -> Vec<(String, PathBuf)> {
        self.selection
            .iter()
            .map(|instance| (instance.name.clone(), instance.path.clone()))
            .collect()
    }

    /// What the listing shows, given what the browser says and whether a
    /// filter is on. Filtered rows come from the search, unfiltered ones from
    /// the folder — or from the roots when the picker is at its top level.
    ///
    /// A filter is a workspace-wide search (§4), so its results are ordered
    /// this root first and everything else after; `elsewhere` is how many fell
    /// in the second group, which is where the view draws its rule.
    pub fn listing(&self, browser: &dyn Browse) -> Listing {
        let Some(cwd) = self.cwd.as_deref() else {
            return Listing::of(browser.roots());
        };
        let Some(query) = self.filter.as_deref().filter(|query| !query.is_empty()) else {
            return browser.list(cwd);
        };
        let (here, elsewhere): (Vec<Entry>, Vec<Entry>) = browser
            .search(cwd)
            .into_iter()
            .filter(|entry| matches(&entry.name, query))
            .partition(|entry| entry.path.starts_with(cwd));
        let count = elsewhere.len();
        Listing {
            entries: here.into_iter().chain(elsewhere).collect(),
            error: None,
            elsewhere: count,
        }
    }

    /// The rows alone, for callers that only move a cursor over them.
    pub fn rows(&self, browser: &dyn Browse) -> Vec<Entry> {
        self.listing(browser).entries
    }

    /// Moves the cursor, clamped to the rows it has.
    pub fn move_cursor(&mut self, delta: isize, rows: usize) -> bool {
        if rows == 0 {
            return false;
        }
        let next = self.cursor.saturating_add_signed(delta).min(rows - 1);
        let moved = next != self.cursor;
        self.cursor = next;
        moved
    }

    /// Points the cursor straight at a row, which is what the pointer does.
    pub fn point_at(&mut self, row: usize, rows: usize) -> bool {
        if row >= rows {
            return false;
        }
        let moved = row != self.cursor;
        self.cursor = row;
        moved
    }

    /// Scrolls the listing window so the cursor stays inside it.
    pub fn follow_cursor(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
    }

    /// Enters a folder, or climbs to its parent. The cursor starts at the top
    /// of wherever it lands and the filter does not survive the move.
    pub fn enter(&mut self, entry: &Entry) -> bool {
        if !entry.enterable() {
            return false;
        }
        self.go(entry.path.clone())
    }

    /// Climbs one folder. From a root it lands on the root list rather than
    /// walking out into `/`.
    pub fn up(&mut self, roots: &[Entry]) -> bool {
        let Some(cwd) = self.cwd.clone() else {
            return false;
        };
        if roots.iter().any(|root| root.path == cwd) {
            return self.go_home();
        }
        match cwd.parent() {
            Some(parent) => self.go(parent.to_path_buf()),
            None => self.go_home(),
        }
    }

    /// `g` — the root of the tree the picker is standing in.
    pub fn go_root(&mut self, roots: &[Entry]) -> bool {
        let Some(cwd) = self.cwd.clone() else {
            return false;
        };
        match roots
            .iter()
            .find(|root| cwd.starts_with(&root.path))
            .map(|root| root.path.clone())
        {
            Some(root) if root != cwd => self.go(root),
            _ => false,
        }
    }

    /// `~` — back to the root list.
    pub fn go_home(&mut self) -> bool {
        if self.cwd.is_none() {
            return false;
        }
        self.cwd = None;
        self.cursor = 0;
        self.offset = 0;
        self.filter = None;
        true
    }

    /// Moves the listing to another folder, keeping the selection. The cursor
    /// starts at the top of wherever it lands and the query does not travel.
    pub fn go_to(&mut self, path: impl Into<PathBuf>) -> bool {
        self.go(path.into())
    }

    fn go(&mut self, path: PathBuf) -> bool {
        if self.workspace.is_empty() || self.selection.is_empty() {
            self.workspace = folder_name(&path);
        }
        self.cwd = Some(path);
        self.cursor = 0;
        self.offset = 0;
        self.filter = None;
        true
    }

    /// `space` — selects a repository, or drops every instance of one that is
    /// already selected. The note: "`space` and `x` address the whole path".
    pub fn toggle(&mut self, entry: &Entry) -> bool {
        if !entry.selectable() {
            return false;
        }
        if self.instances(&entry.path) > 0 {
            return self.remove(entry);
        }
        self.add(entry)
    }

    /// `+` — another terminal for the same path (§3.1). The new instance
    /// appends at the end of the order, exactly as a fresh selection would.
    pub fn add(&mut self, entry: &Entry) -> bool {
        if !entry.selectable() {
            return false;
        }
        let name = self.unique_name(&entry.name);
        self.selection.push(Instance {
            name,
            path: entry.path.clone(),
        });
        true
    }

    /// `-` — sheds the most recent instance of this path, leaving the rest.
    pub fn drop_one(&mut self, entry: &Entry) -> bool {
        let Some(index) = self
            .selection
            .iter()
            .rposition(|instance| instance.path == entry.path)
        else {
            return false;
        };
        self.selection.remove(index);
        true
    }

    /// `x` — the whole path leaves the selection, however many instances of it
    /// there are.
    pub fn remove(&mut self, entry: &Entry) -> bool {
        let before = self.selection.len();
        self.selection
            .retain(|instance| instance.path != entry.path);
        before != self.selection.len()
    }

    /// `X` — nothing is selected any more.
    pub fn clear(&mut self) -> bool {
        let had = !self.selection.is_empty();
        self.selection.clear();
        had
    }

    /// `a` — every selectable target in this listing, once each. A bulk key
    /// never multiplies what a deliberate one built, so an already-selected
    /// target is left at the count it has.
    pub fn select_all(&mut self, rows: &[Entry]) -> bool {
        let mut added = false;
        for entry in rows.iter().filter(|entry| entry.selectable()) {
            if self.instances(&entry.path) == 0 {
                added |= self.add(entry);
            }
        }
        added
    }

    /// `⇧↓` / `⇧↑` — everything selectable from the cursor to the end of the
    /// listing, or from the start of it to the cursor.
    ///
    /// The first selectable row in the span decides the operation: selected
    /// means remove every selectable path in the span; unselected means add
    /// every missing path once. Rows are taken in listing order whichever way
    /// the range runs, so newly added pane numbers read top to bottom the way
    /// the screen does.
    pub fn select_range(&mut self, rows: &[Entry], downward: bool) -> bool {
        if rows.is_empty() {
            return false;
        }
        self.select_between(rows, if downward { rows.len() - 1 } else { 0 })
    }

    /// The range itself: every selectable row between the cursor and `target`,
    /// inclusive, whichever of the two comes first. `⇧↓` and `⇧↑` hand it the
    /// end of the listing; a shift-click hands it the row that was clicked.
    ///
    /// It leaves the cursor alone — the caller decides whether the gesture
    /// moves the highlight, and only the pointer's does.
    pub fn select_between(&mut self, rows: &[Entry], target: usize) -> bool {
        if rows.is_empty() {
            return false;
        }
        let last = rows.len() - 1;
        let cursor = self.cursor.min(last);
        let target = target.min(last);
        let (first, final_row) = (cursor.min(target), cursor.max(target));
        let entries: Vec<_> = rows[first..=final_row]
            .iter()
            .filter(|entry| entry.selectable())
            .collect();
        let Some(first_entry) = entries.first() else {
            return false;
        };
        let remove = self.instances(&first_entry.path) > 0;
        entries.into_iter().fold(false, |changed, entry| {
            changed
                | if remove {
                    self.remove(entry)
                } else if self.instances(&entry.path) == 0 {
                    self.add(entry)
                } else {
                    false
                }
        })
    }

    /// `m` — the cursor's path takes the master frame: its first instance
    /// moves to pane 1 and everything above it shifts down.
    pub fn set_master(&mut self, entry: &Entry) -> bool {
        let Some(index) = self
            .selection
            .iter()
            .position(|instance| instance.path == entry.path)
        else {
            return false;
        };
        if index == 0 {
            return false;
        }
        let instance = self.selection.remove(index);
        self.selection.insert(0, instance);
        true
    }

    /// `K` / `J` inside the selection panel: reorder one instance, which is
    /// what makes the panel the instance list rather than a repo list.
    pub fn reorder(&mut self, index: usize, delta: isize) -> bool {
        let Some(target) = index.checked_add_signed(delta) else {
            return false;
        };
        if index >= self.selection.len() || target >= self.selection.len() {
            return false;
        }
        self.selection.swap(index, target);
        true
    }

    /// How many terminals this path will open.
    pub fn instances(&self, path: &Path) -> usize {
        self.selection
            .iter()
            .filter(|instance| instance.path == path)
            .count()
    }

    /// `/` opens the query line; typing narrows; `esc` clears it.
    pub fn begin_filter(&mut self) -> bool {
        if self.cwd.is_none() || self.filter.is_some() {
            return false;
        }
        self.filter = Some(String::new());
        self.cursor = 0;
        self.offset = 0;
        true
    }

    pub fn push_filter(&mut self, character: char) -> bool {
        let Some(query) = self.filter.as_mut() else {
            return false;
        };
        query.push(character);
        self.cursor = 0;
        self.offset = 0;
        true
    }

    pub fn pop_filter(&mut self) -> bool {
        let Some(query) = self.filter.as_mut() else {
            return false;
        };
        query.pop().is_some()
    }

    /// The first `esc`: the filter goes and the picker stays. Returns whether
    /// there was one to clear, so the caller knows whether this press was the
    /// one that quits.
    pub fn clear_filter(&mut self) -> bool {
        let had = self.filter.take().is_some();
        if had {
            self.cursor = 0;
            self.offset = 0;
        }
        had
    }

    /// `e` — the workspace name is a field, not a fact.
    pub fn begin_rename(&mut self) -> bool {
        if self.renaming {
            return false;
        }
        self.renaming = true;
        true
    }

    pub fn push_name(&mut self, character: char) -> bool {
        if !self.renaming {
            return false;
        }
        self.workspace.push(character);
        true
    }

    pub fn pop_name(&mut self) -> bool {
        if !self.renaming {
            return false;
        }
        self.workspace.pop().is_some()
    }

    pub fn finish_rename(&mut self) -> bool {
        let renaming = self.renaming;
        self.renaming = false;
        renaming
    }

    /// Whether `o` can launch. The note: with nothing selected the button
    /// renders `disabled`.
    pub fn launchable(&self) -> bool {
        !self.selection.is_empty()
    }

    /// The name a new instance of `base` takes: the plain name first, then
    /// `-2`, `-3`, skipping anything already spoken for. Names are what the
    /// engine keys a terminal by, so they leave the picker unique.
    fn unique_name(&self, base: &str) -> String {
        let taken: Vec<&str> = self
            .selection
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        unique_name(&taken, base)
    }
}

impl Default for PickerState {
    fn default() -> Self {
        Self::new()
    }
}

/// The name a new instance of `base` takes: the plain name first, then `-2`,
/// `-3`, skipping anything in `taken`.
///
/// The picker names what it is about to open; the runtime-add sheet (#50)
/// names against what is already running as well, so the rule lives here
/// rather than inside either of them.
pub fn unique_name(taken: &[&str], base: &str) -> String {
    if !taken.contains(&base) {
        return base.to_owned();
    }
    (2..)
        .map(|instance| format!("{base}-{instance}"))
        .find(|candidate| !taken.iter().any(|name| *name == candidate))
        .expect("an unused suffix exists")
}

/// Case-insensitive substring match, which is what the filter promises.
pub fn matches(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(&query.to_lowercase())
}

/// Where a query matches a name, for the accent the export draws on it.
pub fn match_at(name: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let start = name.to_lowercase().find(&query.to_lowercase())?;
    Some((start, start + query.len()))
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
