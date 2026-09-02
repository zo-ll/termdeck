//! The repository picker: what `termdeck` draws when it is given no path.
//!
//! Design authority: `docs/design/termdeck/repository-picker.md`, itself read
//! off screens 06–08 of the export. This module owns the picker's state and
//! its view; it never walks a filesystem itself. Entries arrive already
//! classified and annotated through [`Browse`], and what leaves is an ordered
//! list of `(terminal name, path)` pairs plus a workspace name — the seam the
//! note fixes, which is what lets the same path appear more than once.

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
    /// Uncommitted files, drawn as `+3` beside the branch.
    pub dirty: usize,
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
            dirty: 0,
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

    pub fn git(mut self, branch: impl Into<String>, dirty: usize, age: impl Into<String>) -> Self {
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

    /// `a` — every repository in this listing, once each. A bulk key never
    /// multiplies what a deliberate one built, so an already-selected repo is
    /// left at the count it has.
    pub fn select_all(&mut self, rows: &[Entry]) -> bool {
        let mut added = false;
        for entry in rows
            .iter()
            .filter(|entry| entry.kind == EntryKind::Repository)
        {
            if self.instances(&entry.path) == 0 {
                added |= self.add(entry);
            }
        }
        added
    }

    /// `⇧↓` / `⇧↑` — everything selectable from the cursor to the end of the
    /// listing, or from the start of it to the cursor.
    ///
    /// Additive and idempotent, exactly like `a`: a row already selected keeps
    /// the instances it has, so leaning on the key cannot multiply what a
    /// deliberate `+` built. Rows are taken in listing order whichever way the
    /// range runs, so the pane numbers read top to bottom the way the screen
    /// does.
    pub fn select_range(&mut self, rows: &[Entry], downward: bool) -> bool {
        if rows.is_empty() {
            return false;
        }
        let cursor = self.cursor.min(rows.len() - 1);
        let range = if downward {
            &rows[cursor..]
        } else {
            &rows[..=cursor]
        };
        let mut added = false;
        for entry in range.iter().filter(|entry| entry.selectable()) {
            if self.instances(&entry.path) == 0 {
                added |= self.add(entry);
            }
        }
        added
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
        if !self.taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|instance| format!("{base}-{instance}"))
            .find(|candidate| !self.taken(candidate))
            .expect("an unused suffix exists")
    }

    fn taken(&self, name: &str) -> bool {
        self.selection.iter().any(|instance| instance.name == name)
    }
}

impl Default for PickerState {
    fn default() -> Self {
        Self::new()
    }
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

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};

use super::palette::*;

/// Columns of the browse panel, the gutter and the selection panel: the
/// session's own 98 / 2 / 44, which is the point of the screen.
const BROWSE_COLUMNS: u16 = 98;
const GUTTER_COLUMNS: u16 = 2;
/// Inside a panel: one border column plus the export's `padding:0 2ch`.
const INSET: u16 = 3;
/// Below this the selection panel has nowhere to stand.
const NARROW_PICKER: u16 = 100;

/// The listing grid, in columns from the content's left edge (§1.2).
const COL_GLYPH: u16 = 4;
const COL_NAME: u16 = 7;
const COL_BADGE: u16 = 30;
const COL_SEPARATOR: u16 = 33;
const COL_META: u16 = 35;
const COL_TAIL: u16 = 55;
/// The name field ends before the separator's own blank column.
const NAME_COLUMNS: usize = (COL_BADGE - COL_NAME) as usize;

/// What the pointer is over. The note gives every picker key a click, so the
/// regions are named after the keys they stand in for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hit {
    /// The row's body — `⏎`. A second click on the same row descends into it,
    /// which is the pointer's `→`.
    Row(usize),
    /// The selection box — `x`.
    Box(usize),
    /// The `×N` badge — `+`.
    Badge(usize),
    /// A pane number in the selection panel — `m`.
    Pane(usize),
    /// The launch button — `o`.
    Launch,
    /// The filter slot — `/`.
    Filter,
}

/// The picker, drawn.
pub struct Picker<'a> {
    pub state: &'a PickerState,
    /// What the browser said about the folder: its rows, how many of them
    /// came from elsewhere, and whether it could be read at all.
    pub listing: &'a Listing,
    /// Home directory, so paths read as `~/code` the way the export draws
    /// them.
    pub home: Option<&'a Path>,
    /// Roots, for the crumb and for what `h` climbs to.
    pub roots: &'a [Entry],
}

impl<'a> Picker<'a> {
    fn entries(&self) -> &'a [Entry] {
        &self.listing.entries
    }

    fn elsewhere(&self) -> usize {
        self.listing.elsewhere
    }
}

impl Picker<'_> {
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let buffer = frame.buffer_mut();
        Block::new()
            .style(Style::new().bg(CANVAS))
            .render(area, buffer);
        if area.width < 24 || area.height < 8 {
            return;
        }
        self.bar(buffer, Rect { height: 1, ..area }, true);
        self.bar(
            buffer,
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
            false,
        );
        let body = Rect {
            y: area.y + 2,
            height: area.height - 4,
            ..area
        };
        let (browse, selection) = self.panels(body);
        self.browse(buffer, browse);
        if let Some(selection) = selection {
            self.selection(buffer, selection);
        }
    }

    /// The two panels, or one when the canvas is too narrow to seat both.
    fn panels(&self, body: Rect) -> (Rect, Option<Rect>) {
        if body.width < NARROW_PICKER {
            return (body, None);
        }
        let browse = Rect {
            width: BROWSE_COLUMNS.min(body.width),
            ..body
        };
        let selection = Rect {
            x: body.x + browse.width + GUTTER_COLUMNS,
            width: body.width - browse.width - GUTTER_COLUMNS,
            ..body
        };
        (browse, Some(selection))
    }

    fn bar(&self, buffer: &mut Buffer, area: Rect, top: bool) {
        Block::new()
            .style(Style::new().bg(STATUS_BG))
            .render(area, buffer);
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let key = Style::new().fg(PREVIEW_FG).bg(STATUS_BG);
        let (left, right) = if top {
            (self.top_left(), self.top_right())
        } else {
            (self.keys(key, hint), self.actions(key, hint))
        };
        buffer.set_line(area.x + INSET, area.y, &Line::from(left), area.width);
        let width: usize = right.iter().map(Span::width).sum();
        let x = area
            .x
            .saturating_add(area.width)
            .saturating_sub(width as u16 + INSET);
        buffer.set_line(x, area.y, &Line::from(right), area.width);
    }

    fn top_left(&self) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let mut spans = vec![Span::styled(
            " termdeck ",
            Style::new()
                .fg(CANVAS)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )];
        let selected = self.state.selection().len();
        if self.state.filtering() {
            spans.push(Span::styled("  filtering  ·  ", hint));
            spans.push(Span::styled(
                format!("{selected} selected"),
                Style::new().fg(PREVIEW_FG).bg(STATUS_BG),
            ));
        } else if selected > 0 {
            spans.push(Span::styled("  ", hint));
            spans.push(Span::styled(
                format!("{selected} selected"),
                Style::new().fg(PREVIEW_FG).bg(STATUS_BG),
            ));
            spans.push(Span::styled("  ·  ", hint));
            spans.push(Span::styled("o opens them", hint));
        } else {
            spans.push(Span::styled("  no workspace open  ·  ", hint));
            spans.push(Span::styled(
                "pick repositories to start",
                Style::new().fg(PREVIEW_FG).bg(STATUS_BG),
            ));
        }
        spans
    }

    fn top_right(&self) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let crumb = match self.state.cwd() {
            Some(path) => display_path(path, self.home),
            None => "roots".to_owned(),
        };
        vec![
            Span::styled(crumb, Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
            Span::styled(" › ", Style::new().fg(SEPARATOR).bg(STATUS_BG)),
            Span::styled(
                if self.state.filtering() {
                    "all roots".to_owned()
                } else {
                    format!("{} roots configured", self.roots.len())
                },
                hint,
            ),
        ]
    }

    fn keys(&self, key: Style, hint: Style) -> Vec<Span<'static>> {
        let pairs: &[(&str, &str)] = if self.state.filtering() {
            &[
                ("type", " to narrow  "),
                ("↑↓", " move  "),
                ("⏎", " select  "),
                ("+/-", " instance  "),
                ("esc", " clear filter"),
            ]
        } else {
            &[
                ("↑↓", " move  "),
                ("⏎", " select  "),
                ("⇧↑↓", " range  "),
                ("→", " inside  "),
                ("←", " back  "),
                ("+/-", " instance  "),
                ("a", " all repos  "),
                ("/", " filter"),
            ]
        };
        pairs
            .iter()
            .flat_map(|(name, label)| {
                [
                    Span::styled((*name).to_owned(), key),
                    Span::styled((*label).to_owned(), hint),
                ]
            })
            .collect()
    }

    fn actions(&self, key: Style, hint: Style) -> Vec<Span<'static>> {
        vec![
            Span::styled(
                "o",
                if self.state.launchable() {
                    Style::new().fg(ACCENT).bg(STATUS_BG)
                } else {
                    hint
                },
            ),
            Span::styled(" open  ", hint),
            Span::styled("esc", key),
            Span::styled(
                if self.state.filtering() {
                    " esc quit"
                } else {
                    " quit"
                },
                hint,
            ),
        ]
    }
}

impl Picker<'_> {
    /// The browse panel: the listing, its header, the detail block under the
    /// cursor, and the filter's query line pinned to the bottom.
    fn browse(&self, buffer: &mut Buffer, area: Rect) {
        let title = match self.state.cwd() {
            Some(path) => display_path(path, self.home),
            None => "roots".to_owned(),
        };
        self.panel(buffer, area, true, |buffer, content| {
            let mut row = content.y;
            for span in self.listing_header() {
                buffer.set_line(content.x + INSET, row, &Line::from(span), content.width);
                row += 1;
            }
            let listing = Rect {
                y: row,
                height: content
                    .height
                    .saturating_sub(row - content.y)
                    .saturating_sub(self.footer_rows()),
                ..content
            };
            self.rows(buffer, listing);
            self.footer(buffer, content);
        });
        let hints = if self.state.filtering() {
            "⌫ edit · esc clear"
        } else {
            "← back · → inside · ~ home · g root"
        };
        self.inset(
            buffer,
            area,
            vec![
                Span::styled("> browse", Style::new().fg(ACCENT)),
                Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
                Span::styled(title, Style::new().fg(PREVIEW_FG)),
                Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
                Span::styled(hints.to_owned(), Style::new().fg(MUTED)),
            ],
            true,
        );
        self.inset(
            buffer,
            area,
            vec![
                Span::styled("filter", Style::new().fg(HINT)),
                Span::styled(
                    match self.state.filter() {
                        Some(query) => format!(" /{query}"),
                        None => " (/ to search)".to_owned(),
                    },
                    Style::new().fg(SEPARATOR),
                ),
            ],
            false,
        );
    }

    /// `ROOT ~/code · 9 items · 5 repos`, or the filter's `MATCH` line.
    fn listing_header(&self) -> Vec<Vec<Span<'static>>> {
        let hint = Style::new().fg(MUTED);
        let mut header = Vec::new();
        match (self.state.cwd(), self.state.filter()) {
            (Some(cwd), Some(query)) => {
                let here = self.entries().len().saturating_sub(self.elsewhere());
                header.push(vec![
                    Span::styled("MATCH ", hint),
                    Span::styled(query.to_owned(), Style::new().fg(ACCENT)),
                    Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
                    Span::styled(
                        format!(
                            "{here} matching · recursive from {}",
                            display_path(cwd, self.home)
                        ),
                        hint,
                    ),
                ]);
            }
            (Some(cwd), None) => header.push(vec![
                Span::styled("ROOT ", hint),
                Span::styled(display_path(cwd, self.home), Style::new().fg(MASTER_FG)),
                Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
                Span::styled(
                    format!(
                        "{} items · {} repos",
                        self.entries()
                            .iter()
                            .filter(|entry| entry.kind != EntryKind::Parent)
                            .count(),
                        self.entries()
                            .iter()
                            .filter(|entry| entry.kind == EntryKind::Repository)
                            .count()
                    ),
                    hint,
                ),
            ]),
            (None, _) => header.push(vec![Span::styled("ROOTS", hint)]),
        }
        header.push(vec![Span::styled("─".repeat(88), Style::new().fg(HINT))]);
        header
    }

    fn rows(&self, buffer: &mut Buffer, area: Rect) {
        // Whatever else is true, the rows that exist are drawn — an
        // unreadable or empty folder still shows the `..` that leads out of
        // it, exactly as the export's empty-folder card does.
        // The rule that separates this root's matches from the rest takes a
        // row of its own; it does not stand in for the match it introduces.
        let boundary = (self.elsewhere() > 0)
            .then(|| self.entries().len() - self.elsewhere())
            .filter(|first| *first >= self.state.offset());
        let mut drawn = 0usize;
        let mut y = area.y;
        for index in self.state.offset()..self.entries().len() {
            if y >= area.y + area.height {
                break;
            }
            if boundary == Some(index) {
                buffer.set_line(
                    area.x + INSET,
                    y,
                    &Line::from(Span::styled(
                        format!("─── also in other roots {}", "─".repeat(64)),
                        Style::new().fg(HINT),
                    )),
                    area.width,
                );
                y += 1;
                drawn += 1;
                if y >= area.y + area.height {
                    break;
                }
            }
            let Some(entry) = self.entries().get(index) else {
                break;
            };
            self.row(buffer, area, y, index, entry);
            y += 1;
            drawn += 1;
        }
        let visible = drawn;
        // Then what the listing has to say about itself, under them.
        for (index, (line, style)) in self.condition().into_iter().enumerate() {
            let y = area.y + (visible + index) as u16;
            if y >= area.y + area.height {
                break;
            }
            buffer.set_line(
                area.x + INSET,
                y,
                &Line::from(Span::styled(line, style)),
                area.width,
            );
        }
    }

    /// What the listing says when it has nothing ordinary to show: a read that
    /// failed, a folder that is genuinely empty, or a query that matched
    /// nothing. Each states the condition and the key that escapes it (§5).
    ///
    /// An unreadable folder is **not** an empty one. Saying so is the whole
    /// point of this: an error that draws as an empty listing tells the user
    /// their folder holds nothing, which is a lie about the filesystem.
    fn condition(&self) -> Vec<(String, Style)> {
        let muted = Style::new().fg(MUTED);
        if let Some(error) = self.listing.error.as_deref() {
            let path = self
                .state
                .cwd()
                .map(|cwd| display_path(cwd, self.home))
                .unwrap_or_default();
            return vec![
                (format!("cannot read {path}"), Style::new().fg(ERROR)),
                (error.to_owned(), muted),
                ("← back · ~ home".to_owned(), muted),
            ];
        }
        if self.state.filtering() && self.entries().is_empty() {
            let elsewhere = self.roots.len().saturating_sub(1);
            let root = self
                .state
                .cwd()
                .map(|cwd| display_path(cwd, self.home))
                .unwrap_or_default();
            return vec![
                (
                    format!("no match for {}", self.state.filter().unwrap_or_default()),
                    muted,
                ),
                (format!("in {root} or {elsewhere} other roots"), muted),
                (
                    format!("selection kept ({})", self.state.selection().len()),
                    muted,
                ),
            ];
        }
        if self.listing.is_empty_folder() {
            return vec![
                ("empty folder".to_owned(), muted),
                ("← back · ~ home".to_owned(), muted),
            ];
        }
        Vec::new()
    }

    /// One listing row on the §1.2 grid.
    fn row(&self, buffer: &mut Buffer, area: Rect, y: u16, index: usize, entry: &Entry) {
        let cursor = index == self.state.cursor();
        let instances = self.state.instances(&entry.path);
        let ordinal = self
            .state
            .selection()
            .iter()
            .position(|instance| instance.path == entry.path);
        if cursor {
            Block::new().style(Style::new().bg(DEMOTED_BG)).render(
                Rect {
                    y,
                    height: 1,
                    ..area
                },
                buffer,
            );
        }
        let background = if cursor { DEMOTED_BG } else { CANVAS };
        let put = |buffer: &mut Buffer, column: u16, spans: Vec<Span<'static>>| {
            buffer.set_line(
                area.x + column,
                y,
                &Line::from(spans),
                area.width.saturating_sub(column),
            );
        };
        let box_style = if ordinal.is_some() {
            Style::new().fg(ACCENT).bg(background)
        } else {
            Style::new().fg(HINT).bg(background)
        };
        // Screen 06 gives a box to every row but a plain file — including
        // `..` and folders, which simply never fill one.
        let selection_box = match (entry.kind, ordinal) {
            (_, Some(index)) => format!("[{}]", index + 1),
            (EntryKind::File, None) => "   ".to_owned(),
            (_, None) => "[ ]".to_owned(),
        };
        put(buffer, 0, vec![Span::styled(selection_box, box_style)]);
        let (glyph, colour) = match entry.kind {
            EntryKind::Repository => ("◆", ACCENT),
            EntryKind::Folder => ("▸", WARNING),
            EntryKind::Parent => ("▴", HINT),
            EntryKind::File => ("·", HINT),
        };
        put(
            buffer,
            COL_GLYPH,
            vec![Span::styled(glyph, Style::new().fg(colour).bg(background))],
        );
        let name_colour = if entry.kind == EntryKind::File {
            MUTED
        } else {
            MASTER_FG
        };
        put(buffer, COL_NAME, self.name(entry, name_colour, background));
        if instances > 1 {
            let badge = format!("×{instances}");
            put(
                buffer,
                COL_BADGE + 2 - badge.chars().count() as u16,
                vec![Span::styled(badge, Style::new().fg(ACCENT).bg(background))],
            );
        }
        let meta = self.meta(entry);
        if !meta.is_empty() {
            put(
                buffer,
                COL_SEPARATOR,
                vec![Span::styled("·", Style::new().fg(SEPARATOR).bg(background))],
            );
            put(buffer, COL_META, meta);
        }
        if let Some(age) = entry.age.as_deref().filter(|_| area.width > COL_TAIL) {
            put(
                buffer,
                COL_TAIL,
                vec![Span::styled(
                    age.to_owned(),
                    Style::new().fg(MUTED).bg(background),
                )],
            );
        }
    }

    /// The name, with the filter's match accented inside it.
    fn name(
        &self,
        entry: &Entry,
        colour: ratatui::style::Color,
        background: ratatui::style::Color,
    ) -> Vec<Span<'static>> {
        // A folder wears its slash, as every listing in the export does. The
        // root list is the exception: those rows are paths, not children.
        let name = match entry.kind {
            EntryKind::Folder if self.state.cwd().is_some() => format!("{}/", entry.name),
            _ => entry.name.clone(),
        };
        let name = clip(&name, NAME_COLUMNS);
        let plain = Style::new().fg(colour).bg(background);
        let Some((start, end)) = self.state.filter().and_then(|query| match_at(&name, query))
        else {
            return vec![Span::styled(name, plain)];
        };
        vec![
            Span::styled(name[..start].to_owned(), plain),
            Span::styled(
                name[start..end].to_owned(),
                Style::new().fg(ACCENT).bg(background),
            ),
            Span::styled(name[end..].to_owned(), plain),
        ]
    }

    fn meta(&self, entry: &Entry) -> Vec<Span<'static>> {
        let muted = Style::new().fg(MUTED);
        match entry.kind {
            EntryKind::Repository => {
                let Some(branch) = entry.branch.as_deref() else {
                    return Vec::new();
                };
                let mut spans = vec![Span::styled(format!("git · {branch}"), muted)];
                if entry.dirty > 0 {
                    spans.push(Span::styled(
                        format!(" +{}", entry.dirty),
                        Style::new().fg(WARNING),
                    ));
                }
                spans
            }
            EntryKind::Folder => match (entry.items, entry.repos) {
                // The root list is a list of roots: what matters about one is
                // how many repositories it holds — the export's
                // `~/code · 5 repos` — not how many entries it has.
                (_, Some(repos)) if self.state.cwd().is_none() => {
                    vec![Span::styled(format!("{repos} repos"), muted)]
                }
                (Some(items), Some(0)) => {
                    vec![Span::styled(format!("{items} items · no repos"), muted)]
                }
                (Some(items), _) => vec![Span::styled(format!("{items} items"), muted)],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Rows the listing gives up at its bottom: the detail block, or the
    /// filter's query line.
    fn footer_rows(&self) -> u16 {
        if self.state.filtering() { 2 } else { 4 }
    }

    fn footer(&self, buffer: &mut Buffer, content: Rect) {
        let bottom = content.y + content.height;
        let muted = Style::new().fg(MUTED);
        if let Some(query) = self.state.filter() {
            let y = bottom - 1;
            buffer.set_line(
                content.x + INSET,
                y,
                &Line::from(vec![
                    Span::styled("/", Style::new().fg(HINT)),
                    Span::styled(query.to_owned(), Style::new().fg(MASTER_FG)),
                ]),
                content.width,
            );
            let escape = "esc clear · ⏎ select";
            buffer.set_line(
                content.x + content.width - escape.chars().count() as u16 - INSET,
                y,
                &Line::from(Span::styled(escape, Style::new().fg(HINT))),
                content.width,
            );
            return;
        }
        let Some(entry) = self.entries().get(self.state.cursor()) else {
            return;
        };
        let rule = format!(
            "─── cursor on {} {}",
            entry.name,
            "─".repeat(70_usize.saturating_sub(entry.name.chars().count()))
        );
        for (index, spans) in [
            vec![Span::styled(rule, Style::new().fg(SEPARATOR))],
            vec![Span::styled(display_path(&entry.path, self.home), muted)],
            vec![Span::styled(self.facts(entry), muted)],
        ]
        .into_iter()
        .enumerate()
        {
            buffer.set_line(
                content.x + INSET,
                bottom - 3 + index as u16,
                &Line::from(spans),
                content.width,
            );
        }
    }

    fn facts(&self, entry: &Entry) -> String {
        let instances = self.state.instances(&entry.path);
        let mut facts = Vec::new();
        if let Some(age) = entry.age.as_deref() {
            facts.push(format!("last commit {age}"));
        }
        if entry.dirty > 0 {
            facts.push(format!("{} uncommitted", entry.dirty));
        }
        match instances {
            0 => {}
            1 => facts.push("selected".to_owned()),
            count => facts.push(format!("selected ×{count}")),
        }
        facts.join(" · ")
    }
}

impl Picker<'_> {
    /// The selection panel: instances in pane order, the keys that reorder
    /// them, the workspace name, and the launch button.
    fn selection(&self, buffer: &mut Buffer, area: Rect) {
        self.panel(buffer, area, false, |buffer, content| {
            let mut y = content.y;
            for (index, instance) in self.state.selection().iter().enumerate() {
                if y + 2 >= content.y + content.height.saturating_sub(9) {
                    buffer.set_line(
                        content.x + 1,
                        y,
                        &Line::from(Span::styled(
                            format!("… {} more", self.state.selection().len() - index),
                            Style::new().fg(MUTED),
                        )),
                        content.width,
                    );
                    break;
                }
                let mut spans = vec![
                    Span::styled(
                        format!("{} ", index + 1),
                        Style::new().fg(if index == 0 { ACCENT } else { PREVIEW_FG }),
                    ),
                    Span::styled(clip(&instance.name, 24), Style::new().fg(MASTER_FG)),
                ];
                if index == 0 {
                    spans.push(Span::styled("  MASTER", Style::new().fg(ACCENT)));
                }
                buffer.set_line(content.x + 1, y, &Line::from(spans), content.width);
                buffer.set_line(
                    content.x + 3,
                    y + 1,
                    &Line::from(Span::styled(
                        clip(
                            &display_path(&instance.path, self.home),
                            content.width as usize - 4,
                        ),
                        Style::new().fg(MUTED),
                    )),
                    content.width,
                );
                y += 3;
            }
            if self.state.selection().is_empty() {
                buffer.set_line(
                    content.x + 1,
                    y,
                    &Line::from(Span::styled(
                        "nothing selected · o disabled",
                        Style::new().fg(MUTED),
                    )),
                    content.width,
                );
            } else {
                let rule = "─".repeat(content.width as usize - 2);
                for (index, spans) in [
                    vec![Span::styled(rule, Style::new().fg(HINT))],
                    vec![
                        Span::styled("K/J", Style::new().fg(MUTED)),
                        Span::styled(" reorder · ", Style::new().fg(HINT)),
                        Span::styled("m", Style::new().fg(MUTED)),
                        Span::styled(" set master", Style::new().fg(HINT)),
                    ],
                    vec![
                        Span::styled("x", Style::new().fg(MUTED)),
                        Span::styled(" remove · ", Style::new().fg(HINT)),
                        Span::styled("X", Style::new().fg(MUTED)),
                        Span::styled(" clear all", Style::new().fg(HINT)),
                    ],
                ]
                .into_iter()
                .enumerate()
                {
                    buffer.set_line(
                        content.x + 1,
                        y + index as u16,
                        &Line::from(spans),
                        content.width,
                    );
                }
            }
            self.launch(buffer, content);
        });
        self.inset(
            buffer,
            area,
            vec![
                Span::styled("selected", Style::new().fg(PREVIEW_FG)),
                Span::styled(" · ", Style::new().fg(SEPARATOR)),
                Span::styled(
                    self.state.selection().len().to_string(),
                    Style::new().fg(ACCENT),
                ),
            ],
            true,
        );
        self.inset(
            buffer,
            area,
            vec![Span::styled("order = pane no.", Style::new().fg(HINT))],
            false,
        );
    }

    /// The workspace name and the launch button, pinned to the bottom of the
    /// selection panel.
    fn launch(&self, buffer: &mut Buffer, content: Rect) {
        let bottom = content.y + content.height;
        let rule = "─".repeat(content.width as usize - 2);
        buffer.set_line(
            content.x + 1,
            bottom - 5,
            &Line::from(Span::styled(rule, Style::new().fg(HINT))),
            content.width,
        );
        buffer.set_line(
            content.x + 1,
            bottom - 4,
            &Line::from(Span::styled("workspace name", Style::new().fg(MUTED))),
            content.width,
        );
        let name = if self.state.workspace().is_empty() {
            "unnamed".to_owned()
        } else {
            self.state.workspace().to_owned()
        };
        buffer.set_line(
            content.x + 1,
            bottom - 3,
            &Line::from(vec![
                Span::styled(
                    format!(" {name} "),
                    Style::new().fg(CANVAS).bg(if self.state.renaming() {
                        WARNING
                    } else {
                        ACCENT
                    }),
                ),
                Span::styled("  ", Style::new()),
                Span::styled("e", Style::new().fg(MUTED)),
                Span::styled(" rename", Style::new().fg(HINT)),
            ]),
            content.width,
        );
        let count = self.state.selection().len();
        let label = match count {
            // `o`, not `⏎`: since the picker's keys were simplified, `⏎` is
            // what selects a row and `o` is what opens the selection.
            0 => " o  nothing selected ".to_owned(),
            1 => " o  Open 1 as terminal ".to_owned(),
            _ => format!(" o  Open {count} as terminals "),
        };
        buffer.set_line(
            content.x + 1,
            bottom - 1,
            &Line::from(Span::styled(
                label,
                if self.state.launchable() {
                    Style::new()
                        .fg(CANVAS)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(HINT).bg(DEMOTED_BG)
                },
            )),
            content.width,
        );
    }

    /// A bordered panel, accent while it holds the cursor.
    fn panel(
        &self,
        buffer: &mut Buffer,
        area: Rect,
        focused: bool,
        draw: impl FnOnce(&mut Buffer, Rect),
    ) {
        let border = if focused { ACCENT } else { IDLE_BORDER };
        Block::bordered()
            .border_style(Style::new().fg(border))
            .style(Style::new().bg(CANVAS))
            .render(area, buffer);
        let content = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        if content.width > 2 * INSET && content.height > 4 {
            draw(buffer, content);
        }
    }

    /// A title inset into a panel's top border, as a pane's title is.
    fn inset(&self, buffer: &mut Buffer, area: Rect, mut spans: Vec<Span<'static>>, left: bool) {
        // The title floats clear of the rule it sits in, as a pane's does.
        let gap = Span::styled(" ", Style::new().bg(CANVAS));
        spans.insert(0, gap.clone());
        spans.push(gap);
        let width: usize = spans.iter().map(Span::width).sum();
        let x = if left {
            area.x + INSET
        } else {
            area.x + area.width - width as u16 - INSET
        };
        buffer.set_line(x, area.y, &Line::from(spans), width as u16);
    }

    /// Mouse parity: what the pointer is over, in the same terms as the keys.
    pub fn hit(&self, area: Rect, pointer: Position) -> Option<Hit> {
        if area.width < 24 || area.height < 8 {
            return None;
        }
        let body = Rect {
            y: area.y + 2,
            height: area.height - 4,
            ..area
        };
        let (browse, selection) = self.panels(body);
        if let Some(selection) = selection.filter(|panel| panel.contains(pointer)) {
            let row = pointer.y.saturating_sub(selection.y + 1);
            if pointer.y == selection.y + selection.height - 2 {
                return self.state.launchable().then_some(Hit::Launch);
            }
            if row.is_multiple_of(3) {
                let index = (row / 3) as usize;
                return (index < self.state.selection().len()).then_some(Hit::Pane(index));
            }
            return None;
        }
        if !browse.contains(pointer) {
            return None;
        }
        if pointer.y == browse.y && pointer.x > browse.x + browse.width / 2 {
            return Some(Hit::Filter);
        }
        let content = Rect {
            x: browse.x + 1,
            y: browse.y + 1,
            width: browse.width.saturating_sub(2),
            height: browse.height.saturating_sub(2),
        };
        let first = content.y + self.listing_header().len() as u16;
        let last = content.y + content.height - self.footer_rows();
        if pointer.y < first || pointer.y >= last {
            return None;
        }
        let index = self.state.offset() + (pointer.y - first) as usize;
        let entry = self.entries().get(index)?;
        let column = pointer.x.saturating_sub(content.x);
        Some(match column {
            0..=2 => Hit::Box(index),
            COL_BADGE..COL_SEPARATOR if self.state.instances(&entry.path) > 0 => Hit::Badge(index),
            _ => Hit::Row(index),
        })
    }
}

/// `~/code` rather than `/home/andrea/code`, as every screen draws it.
fn display_path(path: &Path, home: Option<&Path>) -> String {
    let text = path.to_string_lossy().into_owned();
    match home {
        Some(home) => match path.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => text,
        },
        None => text,
    }
}

use super::clip;

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

use super::Key;

/// What a key press asks the caller for. Everything else the picker does to
/// itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerReaction {
    /// Open the selection as a workspace.
    Launch,
    /// Leave without opening anything.
    Quit,
}

/// Applies one key. `rows` is what the listing is currently showing, which is
/// the only thing a key needs that the state does not hold.
///
/// Picker keys are bare — the note: "no `^g` prefix, since no terminal has
/// focus yet" — so nothing here consults a prefix and nothing reaches a shell.
pub fn press(
    state: &mut PickerState,
    rows: &[Entry],
    roots: &[Entry],
    key: Key,
) -> Option<PickerReaction> {
    if state.renaming() {
        match key {
            Key::Char(character) => {
                state.push_name(character);
            }
            Key::Backspace => {
                state.pop_name();
            }
            Key::Enter | Key::Escape => {
                state.finish_rename();
            }
            _ => {}
        }
        return None;
    }
    // While the query line is open the alphabet belongs to it, so the
    // selection keys are the ones that stay reachable.
    // While the query line is open the alphabet belongs to it, so only the
    // keys that are not letters stay reachable.
    if state.filtering()
        && let Key::Char(character) = key
        && !matches!(character, ' ' | '+' | '-')
    {
        state.push_filter(character);
        return None;
    }
    let cursor = rows.get(state.cursor()).cloned();
    match key {
        Key::Down | Key::Char('j') => {
            state.move_cursor(1, rows.len());
        }
        // Shift plus an arrow takes everything from here to that end of the
        // listing. The cursor stays put: the range is what moved, not it.
        Key::ShiftDown => {
            state.select_range(rows, true);
        }
        Key::ShiftUp => {
            state.select_range(rows, false);
        }
        Key::Up | Key::Char('k') => {
            state.move_cursor(-1, rows.len());
        }
        // `⏎` selects the row under the cursor, and a second press on the
        // same row lets it go again. `space` is the same key by another name.
        Key::Enter | Key::Char(' ') => {
            if let Some(entry) = cursor.as_ref() {
                state.toggle(entry);
            }
        }
        // `o` opens what has been selected. `⏎` used to, and cannot any
        // more: it is the select key now.
        Key::Char('o') => {
            if state.launchable() {
                return Some(PickerReaction::Launch);
            }
        }
        Key::Char('+') => {
            if let Some(entry) = cursor.as_ref() {
                state.add(entry);
            }
        }
        Key::Char('-') => {
            if let Some(entry) = cursor.as_ref() {
                state.drop_one(entry);
            }
        }
        // `→` goes inside whatever the cursor is on — a folder or a
        // repository, since a repository is a folder that also holds a `.git`.
        Key::Right | Key::Tab | Key::Char('l') => {
            if let Some(entry) = cursor.as_ref() {
                state.enter(entry);
            }
        }
        // `←` comes back out.
        Key::Left | Key::Char('h') => {
            state.up(roots);
        }
        Key::Char('~') => {
            state.go_home();
        }
        Key::Char('g') => {
            state.go_root(roots);
        }
        Key::Char('a') => {
            state.select_all(rows);
        }
        Key::Char('m') => {
            if let Some(entry) = cursor.as_ref() {
                state.set_master(entry);
            }
        }
        Key::Char('x') => {
            if let Some(entry) = cursor.as_ref() {
                state.remove(entry);
            }
        }
        Key::Char('X') => {
            state.clear();
        }
        Key::Char('e') => {
            state.begin_rename();
        }
        Key::Char('/') => {
            state.begin_filter();
        }
        Key::Backspace => {
            state.pop_filter();
        }
        // The note: `esc` clears the filter, and only a second one quits.
        Key::Escape if !state.clear_filter() => return Some(PickerReaction::Quit),
        Key::Escape => {}
        _ => {}
    }
    None
}

/// The pointer's `→`: a second click on a row it is already on goes inside
/// it. One click selects (the pointer's `⏎`), two descend.
pub fn descend(state: &mut PickerState, rows: &[Entry], hit: Hit) -> bool {
    let Hit::Row(index) = hit else {
        return false;
    };
    let Some(entry) = rows.get(index).cloned() else {
        return false;
    };
    // The first click of the pair selected it; going inside undoes that,
    // because the click was the user reaching for the folder, not for a pane.
    state.toggle(&entry);
    state.enter(&entry)
}

/// The secondary button's own gesture: on the `×N` badge it is `-`, which
/// sheds one instance of that path. The parity table gives the primary button
/// the additions and the secondary button the subtraction, so a pointer can
/// reach both ends of §3.1 without a keyboard.
pub fn click_secondary(state: &mut PickerState, rows: &[Entry], hit: Hit) -> bool {
    let (Hit::Badge(index) | Hit::Row(index) | Hit::Box(index)) = hit else {
        return false;
    };
    let Some(entry) = rows.get(index).cloned() else {
        return false;
    };
    state.point_at(index, rows.len());
    state.drop_one(&entry)
}

/// Applies one pointer gesture, in the same terms as the keys (§7).
pub fn click(state: &mut PickerState, rows: &[Entry], hit: Hit) -> Option<PickerReaction> {
    match hit {
        Hit::Row(index) => {
            let entry = rows.get(index).cloned()?;
            state.point_at(index, rows.len());
            state.toggle(&entry);
        }
        Hit::Box(index) => {
            let entry = rows.get(index).cloned()?;
            state.point_at(index, rows.len());
            state.remove(&entry);
        }
        Hit::Badge(index) => {
            let entry = rows.get(index).cloned()?;
            state.point_at(index, rows.len());
            state.add(&entry);
        }
        Hit::Pane(index) => {
            if let Some(instance) = state.selection().get(index).cloned() {
                let entry = Entry::repository(instance.name, instance.path);
                state.set_master(&entry);
            }
        }
        Hit::Filter => {
            state.begin_filter();
        }
        Hit::Launch => {
            if state.launchable() {
                return Some(PickerReaction::Launch);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The filesystem behind the seam
// ---------------------------------------------------------------------------

use std::{fs, time::SystemTime};

/// The real browser: `read_dir` plus what `.git` can be asked cheaply.
///
/// Everything here is one stat or one small read per row. No process is
/// spawned and no repository is opened, because the note requires the detail
/// block to never block the cursor.
pub struct FsBrowse {
    roots: Vec<PathBuf>,
    home: Option<PathBuf>,
}

impl FsBrowse {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    /// The home the picker abbreviates paths against.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
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
}

impl Browse for FsBrowse {
    fn list(&self, path: &Path) -> Listing {
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

    fn search(&self, path: &Path) -> Vec<Entry> {
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

    fn roots(&self) -> Vec<Entry> {
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

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Position};

    use super::*;

    /// The export's screen 06 folder, frozen so the snapshots do not depend on
    /// anyone's disk.
    struct Fixture;

    fn code() -> PathBuf {
        PathBuf::from("/home/dev/code")
    }

    impl Browse for Fixture {
        fn list(&self, path: &Path) -> Listing {
            // A folder that is genuinely empty, and one that cannot be read:
            // the two the picker must never confuse.
            if path == Path::new("/home/dev/code/vendor/tmp") {
                return Listing::of(vec![Entry::parent("/home/dev/code/vendor")]);
            }
            if path == Path::new("/home/dev/code/secret") {
                return Listing {
                    entries: vec![Entry::parent("/home/dev/code")],
                    error: Some("Permission denied (os error 13)".to_owned()),
                    elsewhere: 0,
                };
            }
            let entries = vec![
                Entry::parent("/home/dev"),
                Entry::folder("archive", code().join("archive")).holding(3, 0),
                Entry::repository("horizon-frontend", code().join("horizon-frontend"))
                    .git("main", 0, "2h ago"),
                Entry::repository("horizon-backend", code().join("horizon-backend"))
                    .git("main", 3, "18m ago"),
                Entry::repository("horizon-app", code().join("horizon-app")).git(
                    "feat/rn-0.75",
                    0,
                    "4d ago",
                ),
                Entry::repository("horizon-infra", code().join("horizon-infra"))
                    .git("main", 0, "3w ago"),
                Entry::folder("notes", code().join("notes")).holding(14, 0),
                Entry::repository("termdeck", code().join("termdeck")).git("main", 0, "just now"),
                Entry::folder("vendor", code().join("vendor")).holding(6, 0),
                Entry::file("README.md", code().join("README.md")),
            ];
            Listing::of(entries)
        }

        fn search(&self, path: &Path) -> Vec<Entry> {
            let mut found: Vec<Entry> = self
                .list(path)
                .entries
                .into_iter()
                .filter(Entry::selectable)
                .collect();
            found.push(
                Entry::repository("horizon-docs", "/home/dev/work/archive/horizon-docs")
                    .git("main", 0, "1y ago"),
            );
            found
        }

        fn roots(&self) -> Vec<Entry> {
            vec![
                Entry::folder("~/code", code()).holding(9, 5),
                Entry::folder("~/work", "/home/dev/work").holding(12, 12),
                Entry::folder("/srv", "/srv").holding(2, 2),
            ]
        }
    }

    fn render(state: &PickerState) -> Buffer {
        let listing = state.listing(&Fixture);
        let roots = Fixture.roots();
        let view = Picker {
            state,
            listing: &listing,
            roots: &roots,
            home: Some(Path::new("/home/dev")),
        };
        let mut terminal = Terminal::new(TestBackend::new(144, 42)).unwrap();
        terminal.draw(|frame| view.render(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buffer: &Buffer) -> String {
        let area = buffer.area();
        (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_snapshot(name: &str, buffer: &Buffer) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ui/testdata")
            .join(format!("{name}.txt"));
        let rendered = text(buffer);
        if std::env::var_os("TERMDECK_BLESS").is_some() {
            std::fs::write(&path, format!("{rendered}\n")).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap();
        assert_eq!(rendered, expected.trim_end_matches('\n'), "{name}");
    }

    fn browsing() -> PickerState {
        PickerState::at(code())
    }

    fn rows_of(state: &PickerState) -> Vec<Entry> {
        state.rows(&Fixture)
    }

    fn select(state: &mut PickerState, name: &str) {
        let rows = rows_of(state);
        let entry = rows
            .iter()
            .find(|entry| entry.name == name)
            .expect("the fixture lists it");
        assert!(state.toggle(entry));
    }

    /// A directory of this test's own, for the cases that need a real one.
    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "termdeck-picker-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn entry(state: &PickerState, name: &str) -> Entry {
        rows_of(state)
            .into_iter()
            .find(|entry| entry.name == name)
            .expect("the fixture lists it")
    }

    /// The note: "the ordinal is the pane number", and the first selected is
    /// the master.
    #[test]
    fn selection_order_is_pane_order_and_the_first_is_master() {
        let mut state = browsing();

        select(&mut state, "horizon-frontend");
        select(&mut state, "horizon-backend");
        select(&mut state, "horizon-app");

        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["horizon-frontend", "horizon-backend", "horizon-app"]
        );
        assert_eq!(state.launch()[0].0, "horizon-frontend", "pane 1 is master");

        // `m` promotes, and everything above it shifts down.
        assert!(state.set_master(&entry(&state, "horizon-app")));
        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["horizon-app", "horizon-frontend", "horizon-backend"]
        );
    }

    /// §3.1: a path may be selected repeatedly, each instance its own pane
    /// with its own name.
    #[test]
    fn the_same_path_can_open_more_than_one_terminal() {
        let mut state = browsing();
        let frontend = entry(&state, "horizon-frontend");

        select(&mut state, "horizon-frontend");
        assert!(state.add(&frontend));
        assert!(state.add(&frontend));

        assert_eq!(state.instances(&frontend.path), 3);
        let launch = state.launch();
        assert_eq!(
            launch
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "horizon-frontend",
                "horizon-frontend-2",
                "horizon-frontend-3"
            ]
        );
        assert!(
            launch.iter().all(|(_, path)| *path == frontend.path),
            "every instance runs in the same directory"
        );

        // `-` sheds the most recent; `space`/`x` take the whole path.
        assert!(state.drop_one(&frontend));
        assert_eq!(state.instances(&frontend.path), 2);
        assert!(state.toggle(&frontend));
        assert_eq!(state.instances(&frontend.path), 0);
    }

    /// A suffix another selection already holds is skipped, because names are
    /// what the engine keys a terminal by.
    #[test]
    fn instance_names_skip_the_ones_already_taken() {
        let mut state = browsing();
        let app = Entry::repository("horizon-app", code().join("horizon-app"));
        let decoy = Entry::repository("horizon-app-2", code().join("horizon-app-2"));

        assert!(state.add(&app));
        assert!(state.add(&decoy));
        assert!(state.add(&app));

        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        assert_eq!(names, ["horizon-app", "horizon-app-2", "horizon-app-3"]);
    }

    /// A bulk key must not multiply what a deliberate one built.
    #[test]
    fn selecting_every_repo_leaves_existing_instances_alone() {
        let mut state = browsing();
        let frontend = entry(&state, "horizon-frontend");
        state.add(&frontend);
        state.add(&frontend);

        assert!(state.select_all(&rows_of(&state)));

        assert_eq!(state.instances(&frontend.path), 2, "not multiplied");
        assert_eq!(state.selection().len(), 6, "5 repos, one of them twice");
    }

    /// Any directory can be a terminal, so a folder joins a workspace on the
    /// same terms as a repository. A plain file cannot, and neither can `..`.
    #[test]
    fn any_directory_can_be_selected_but_a_file_cannot() {
        let mut state = browsing();

        assert!(!state.toggle(&entry(&state, "README.md")), "a file");
        assert!(!state.toggle(&entry(&state, "..")), "a way out, not a pane");
        assert!(!state.launchable());

        assert!(state.toggle(&entry(&state, "archive")), "a plain folder");
        assert!(state.toggle(&entry(&state, "termdeck")), "a repository");
        assert_eq!(state.selection().len(), 2);
        assert!(state.launchable());
    }

    /// `⏎` selects the row under the cursor, and pressing it again lets go.
    #[test]
    fn enter_selects_the_row_and_a_second_press_deselects_it() {
        let mut state = browsing();
        let roots = Fixture.roots();
        let rows = rows_of(&state);
        let index = rows
            .iter()
            .position(|entry| entry.name == "horizon-frontend")
            .unwrap();
        state.point_at(index, rows.len());

        assert_eq!(press(&mut state, &rows, &roots, Key::Enter), None);
        assert_eq!(state.selection().len(), 1);
        assert_eq!(state.selection()[0].name, "horizon-frontend");

        assert_eq!(press(&mut state, &rows, &roots, Key::Enter), None);
        assert!(state.selection().is_empty(), "the second press lets go");
        assert_eq!(
            state.cwd(),
            Some(code().as_path()),
            "and neither press moved anywhere"
        );
    }

    /// `→` goes inside whatever the cursor is on, repository or not — the
    /// case a repository holding `projects/` used to make impossible.
    #[test]
    fn the_right_arrow_goes_inside_a_repository_as_well_as_a_folder() {
        let roots = Fixture.roots();
        for (name, expected) in [
            ("termdeck", code().join("termdeck")),
            ("archive", code().join("archive")),
        ] {
            let mut state = browsing();
            let rows = rows_of(&state);
            let index = rows.iter().position(|entry| entry.name == name).unwrap();
            state.point_at(index, rows.len());

            press(&mut state, &rows, &roots, Key::Right);

            assert_eq!(state.cwd(), Some(expected.as_path()), "inside {name}");
            assert!(state.selection().is_empty(), "going inside selects nothing");
        }
    }

    /// `←` comes back out again.
    #[test]
    fn the_left_arrow_goes_back_one_level() {
        let mut state = PickerState::at(code().join("termdeck"));
        let roots = Fixture.roots();
        let rows = rows_of(&state);

        press(&mut state, &rows, &roots, Key::Left);

        assert_eq!(state.cwd(), Some(code().as_path()));
    }

    /// `o` is what opens the selection now that `⏎` selects.
    #[test]
    fn o_opens_the_selection_and_only_once_there_is_one() {
        let mut state = browsing();
        let roots = Fixture.roots();
        let rows = rows_of(&state);

        assert_eq!(
            press(&mut state, &rows, &roots, Key::Char('o')),
            None,
            "nothing selected, nothing to open"
        );

        select(&mut state, "termdeck");
        assert_eq!(
            press(&mut state, &rows, &roots, Key::Char('o')),
            Some(PickerReaction::Launch)
        );
    }

    /// Navigation: `l` enters, `h` climbs, `~` returns to the roots, and none
    /// of it disturbs the selection.
    #[test]
    fn navigation_keeps_the_selection_and_the_filter_does_not_survive_it() {
        let mut state = browsing();
        select(&mut state, "termdeck");
        let vendor = entry(&state, "vendor");
        state.begin_filter();
        state.push_filter('h');

        assert!(state.enter(&vendor));

        assert_eq!(state.cwd(), Some(code().join("vendor").as_path()));
        assert!(!state.filtering(), "a move clears the query");
        assert_eq!(state.selection().len(), 1, "selection is workspace-wide");

        assert!(state.up(&Fixture.roots()));
        assert_eq!(state.cwd(), Some(code().as_path()));
        assert!(state.go_home());
        assert_eq!(state.cwd(), None, "~ lands on the root list");
        assert_eq!(state.selection().len(), 1);
    }

    /// `h` from a configured root lands on the root list rather than walking
    /// out into `/`.
    #[test]
    fn climbing_out_of_a_root_lands_on_the_root_list() {
        let mut state = browsing();

        assert!(state.up(&Fixture.roots()));

        assert_eq!(state.cwd(), None);
    }

    /// `g` is root and `/` is filter — the contradiction the note flagged,
    /// resolved by the key map board.
    #[test]
    fn g_goes_to_the_root_and_slash_opens_the_filter() {
        let mut state = PickerState::at(code().join("vendor/tmp"));

        assert!(state.go_root(&Fixture.roots()));
        assert_eq!(state.cwd(), Some(code().as_path()));

        assert!(state.begin_filter());
        assert_eq!(state.filter(), Some(""));
        let rows = rows_of(&state);
        press(&mut state, &rows, &Fixture.roots(), Key::Char('g'));
        assert_eq!(
            state.filter(),
            Some("g"),
            "typing narrows, it does not move"
        );
    }

    /// The first `esc` clears the filter; only the second one quits.
    #[test]
    fn escape_clears_the_filter_before_it_quits() {
        let mut state = browsing();
        let roots = Fixture.roots();
        state.begin_filter();
        state.push_filter('h');

        let rows = rows_of(&state);
        assert_eq!(press(&mut state, &rows, &roots, Key::Escape), None);
        assert!(!state.filtering());

        assert_eq!(
            press(&mut state, &rows, &roots, Key::Escape),
            Some(PickerReaction::Quit)
        );
    }

    /// The filter reaches every configured root, not just this folder.
    #[test]
    fn the_filter_searches_beyond_the_current_folder() {
        let mut state = browsing();
        state.begin_filter();
        for character in "hor".chars() {
            state.push_filter(character);
        }

        let rows = rows_of(&state);
        let names: Vec<_> = rows.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"horizon-docs"), "{names:?}");
        assert!(!names.contains(&"termdeck"), "{names:?}");
    }

    /// `⇧↓` takes everything selectable from the cursor to the end of the
    /// listing — folders as much as repositories, since any directory can be
    /// a terminal — and leaves files and `..` alone.
    #[test]
    fn shift_down_selects_everything_below_the_cursor() {
        let mut state = browsing();
        let roots = Fixture.roots();
        let rows = rows_of(&state);
        let index = rows
            .iter()
            .position(|entry| entry.name == "horizon-infra")
            .unwrap();
        state.point_at(index, rows.len());

        press(&mut state, &rows, &roots, Key::ShiftDown);

        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        // From the cursor down: the repo it is on, the folder under it, the
        // next repo, the last folder. README.md is a file, so it is not here.
        assert_eq!(
            names,
            ["horizon-infra", "notes", "termdeck", "vendor"],
            "{names:?}"
        );
        assert!(
            !names.contains(&"README.md") && !names.contains(&".."),
            "a file and the way out are not terminals"
        );
        assert!(
            names.contains(&"notes") && names.contains(&"vendor"),
            "folders are in the range"
        );

        // Additive and idempotent: pressing it again changes nothing, and a
        // deliberate instance is not multiplied by a bulk key.
        let before = state.selection().len();
        press(&mut state, &rows, &roots, Key::ShiftDown);
        assert_eq!(state.selection().len(), before);
    }

    /// `⇧↑` is the same thing upward, and takes its rows in listing order so
    /// the pane numbers read the way the screen does.
    #[test]
    fn shift_up_selects_everything_above_the_cursor() {
        let mut state = browsing();
        let roots = Fixture.roots();
        let rows = rows_of(&state);
        let index = rows
            .iter()
            .position(|entry| entry.name == "horizon-backend")
            .unwrap();
        state.point_at(index, rows.len());

        press(&mut state, &rows, &roots, Key::ShiftUp);

        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["archive", "horizon-frontend", "horizon-backend"],
            "{names:?}"
        );
        assert_eq!(
            state.selection()[0].name,
            "archive",
            "the topmost of the range is pane 1, so the panes read downwards"
        );
    }

    /// A range that starts on an already-selected row keeps what is there and
    /// adds the rest — the two keys accumulate rather than replace.
    #[test]
    fn a_range_adds_to_what_is_already_selected() {
        let mut state = browsing();
        let roots = Fixture.roots();
        let rows = rows_of(&state);
        select(&mut state, "termdeck");
        let index = rows
            .iter()
            .position(|entry| entry.name == "horizon-app")
            .unwrap();
        state.point_at(index, rows.len());

        press(&mut state, &rows, &roots, Key::ShiftDown);

        let names: Vec<_> = state
            .selection()
            .iter()
            .map(|instance| instance.name.as_str())
            .collect();
        assert_eq!(names[0], "termdeck", "what was already selected stays put");
        assert_eq!(state.instances(&entry(&state, "termdeck").path), 1);
        assert!(names.contains(&"horizon-app"), "{names:?}");
        assert!(names.contains(&"vendor"), "{names:?}");
    }

    /// The pointer selects a folder exactly as `⏎` does, and a second click
    /// on it descends — the row semantics do not care what kind it is.
    #[test]
    fn a_click_selects_a_folder_and_a_second_click_goes_inside() {
        let mut state = browsing();
        let rows = rows_of(&state);
        let index = rows
            .iter()
            .position(|entry| entry.name == "archive")
            .unwrap();

        click(&mut state, &rows, Hit::Row(index));

        assert_eq!(state.selection().len(), 1, "a folder is selectable");
        assert_eq!(state.selection()[0].name, "archive");

        assert!(descend(&mut state, &rows, Hit::Row(index)));
        assert_eq!(state.cwd(), Some(code().join("archive").as_path()));
        assert!(state.selection().is_empty(), "the click was navigation");
    }

    /// Every column of a folder's row is the row, since the name stopped
    /// being its own hit region when a click came to mean select.
    #[test]
    fn every_column_of_a_folder_row_is_the_row() {
        let state = browsing();
        let listing = state.listing(&Fixture);
        let roots = Fixture.roots();
        let picker = Picker {
            state: &state,
            listing: &listing,
            roots: &roots,
            home: Some(Path::new("/home/dev")),
        };
        let area = ratatui::layout::Rect::new(0, 0, 144, 42);

        // `archive/` is the second row drawn, under the header and its rule.
        for column in [1u16, 8, 20, 40] {
            assert_eq!(
                picker.hit(area, Position::new(column, 6)),
                Some(if column <= 2 {
                    Hit::Box(1)
                } else {
                    Hit::Row(1)
                }),
                "column {column}"
            );
        }
    }

    /// The pointer follows the same two keys: one click selects, a second on
    /// the same row goes inside it.
    #[test]
    fn a_second_click_on_a_row_goes_inside_it() {
        let mut state = browsing();
        let rows = rows_of(&state);
        let index = rows
            .iter()
            .position(|entry| entry.name == "termdeck")
            .unwrap();

        click(&mut state, &rows, Hit::Row(index));
        assert_eq!(state.selection().len(), 1, "one click selects");

        assert!(descend(&mut state, &rows, Hit::Row(index)));
        assert_eq!(state.cwd(), Some(code().join("termdeck").as_path()));
        assert!(
            state.selection().is_empty(),
            "the click that opened it was not a selection after all"
        );
    }

    /// Mouse parity (§7): the pointer reaches what the keys reach.
    #[test]
    fn the_pointer_toggles_enters_and_adds_an_instance() {
        let mut state = browsing();
        let rows = rows_of(&state);
        let roots = Fixture.roots();

        // Row 2 of the listing is horizon-frontend; its body toggles it.
        let hit = {
            let listing = state.listing(&Fixture);
            let picker = Picker {
                state: &state,
                listing: &listing,
                roots: &roots,
                home: Some(Path::new("/home/dev")),
            };
            // The listing starts under its header and rule: row 0 is `..`,
            // so horizon-frontend is the third row drawn.
            picker.hit(
                ratatui::layout::Rect::new(0, 0, 144, 42),
                Position::new(60, 7),
            )
        };
        assert_eq!(hit, Some(Hit::Row(2)));
        click(&mut state, &rows, hit.unwrap());
        assert_eq!(state.selection().len(), 1);
        assert_eq!(state.selection()[0].name, "horizon-frontend");

        // The badge cells add another instance of the same path.
        click(&mut state, &rows, Hit::Badge(2));
        assert_eq!(state.selection().len(), 2);
        assert_eq!(state.selection()[1].name, "horizon-frontend-2");

        // The box removes the path outright, however many instances it has.
        click(&mut state, &rows, Hit::Box(2));
        assert!(state.selection().is_empty());
    }

    #[test]
    fn the_launch_button_only_answers_once_it_is_enabled() {
        let mut state = browsing();
        let rows = rows_of(&state);

        assert_eq!(click(&mut state, &rows, Hit::Launch), None);
        select(&mut state, "termdeck");
        assert_eq!(
            click(&mut state, &rows, Hit::Launch),
            Some(PickerReaction::Launch)
        );
    }

    /// The blocking case: a folder the picker cannot read must say so. An
    /// error that draws as an empty listing tells the user their folder holds
    /// nothing, which is a lie about the filesystem.
    #[test]
    fn an_unreadable_folder_says_why_instead_of_looking_empty() {
        let state = PickerState::at("/home/dev/code/secret");

        let listing = state.listing(&Fixture);
        assert!(listing.error.is_some(), "the browser reported the failure");
        assert!(
            !listing.is_empty_folder(),
            "a folder that could not be read is not an empty one"
        );

        let rendered = text(&render(&state));
        assert!(rendered.contains("cannot read ~/code/secret"), "{rendered}");
        assert!(rendered.contains("Permission denied"), "{rendered}");
        assert!(rendered.contains("← back · ~ home"), "the way out");
        assert!(!rendered.contains("empty folder"), "{rendered}");
        // And the row that leads out of it is still drawn.
        assert!(rendered.contains("▴  .."), "{rendered}");
    }

    /// The other half of the same distinction: a folder that really is empty
    /// gets the designed empty state, which the error case must not take.
    #[test]
    fn a_genuinely_empty_folder_shows_the_empty_state() {
        let state = PickerState::at("/home/dev/code/vendor/tmp");

        let listing = state.listing(&Fixture);
        assert!(listing.error.is_none());
        assert!(listing.is_empty_folder());

        let rendered = text(&render(&state));
        assert!(rendered.contains("empty folder"), "{rendered}");
        assert!(rendered.contains("← back · ~ home"), "{rendered}");
        assert!(!rendered.contains("cannot read"), "{rendered}");
        assert!(rendered.contains("▴  .."), "{rendered}");
    }

    /// The same distinction against the real filesystem, so the branch the
    /// fixture describes is the branch `read_dir` actually produces.
    #[test]
    fn the_filesystem_browser_separates_empty_from_unreadable() {
        let root = temp_root("states");
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let browser = FsBrowse::new(vec![root.clone()]);

        let listing = browser.list(&empty);
        assert!(listing.error.is_none(), "an empty folder reads fine");
        assert!(
            listing.is_empty_folder(),
            "and holds nothing but its parent"
        );

        let missing = root.join("does-not-exist");
        let listing = browser.list(&missing);
        assert!(
            listing.error.is_some(),
            "a folder that cannot be read reports why"
        );
        assert!(!listing.is_empty_folder(), "and is not called empty");
        assert!(
            listing
                .entries
                .iter()
                .any(|entry| entry.kind == EntryKind::Parent),
            "the way out is still there"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Overlapping roots — the default pair is the working directory and the
    /// home above it — must not list the same repository twice. The nearer
    /// root claims it; a repository no root contains still shows up under the
    /// filter's own rule.
    #[test]
    fn a_repository_inside_two_roots_is_listed_once() {
        let root = temp_root("overlap");
        let outer = root.join("outer");
        let inner = outer.join("inner");
        let shared = inner.join("repo-shared");
        let only_outer = outer.join("repo-outer");
        for repository in [&shared, &only_outer] {
            std::fs::create_dir_all(repository.join(".git")).unwrap();
            std::fs::write(repository.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        }
        // Both roots contain `repo-shared`; only the outer contains
        // `repo-outer`.
        let browser = FsBrowse::new(vec![outer.clone(), inner.clone()]);

        let found = browser.search(&inner);

        let names: Vec<_> = found.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names.iter().filter(|name| **name == "repo-shared").count(),
            1,
            "the nearer root claims it: {names:?}"
        );
        assert_eq!(
            names.iter().filter(|name| **name == "repo-outer").count(),
            1,
            "{names:?}"
        );

        // And through the filter, the shared repository sits in the browsed
        // folder's own partition while the outer one is ruled off as
        // elsewhere — each exactly once.
        let mut state = PickerState::at(&inner);
        state.begin_filter();
        for character in "repo".chars() {
            state.push_filter(character);
        }
        let listing = state.listing(&browser);

        let names: Vec<_> = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["repo-shared", "repo-outer"], "{names:?}");
        assert_eq!(listing.elsewhere, 1, "only the unclaimed one is elsewhere");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Mouse parity's other half (§7): the secondary button is the `-`.
    #[test]
    fn the_secondary_button_sheds_an_instance() {
        let mut state = browsing();
        let rows = rows_of(&state);
        let frontend = entry(&state, "horizon-frontend");
        state.add(&frontend);
        state.add(&frontend);
        let index = rows
            .iter()
            .position(|entry| entry.name == "horizon-frontend")
            .unwrap();

        assert!(click_secondary(&mut state, &rows, Hit::Badge(index)));

        assert_eq!(state.instances(&frontend.path), 1, "one shed, one kept");
        assert!(click_secondary(&mut state, &rows, Hit::Badge(index)));
        assert!(state.selection().is_empty());
        assert!(
            !click_secondary(&mut state, &rows, Hit::Badge(index)),
            "nothing left to shed"
        );
    }

    /// The filter's own rule: matches from other roots are counted, and the
    /// view draws them under their own heading.
    #[test]
    fn matches_from_other_roots_are_counted_and_ruled_off() {
        let mut state = browsing();
        state.begin_filter();
        for character in "hor".chars() {
            state.push_filter(character);
        }

        let listing = state.listing(&Fixture);

        assert_eq!(listing.elsewhere, 1, "horizon-docs lives under ~/work");
        assert_eq!(
            listing.entries.last().map(|entry| entry.name.as_str()),
            Some("horizon-docs"),
            "and it sorts after everything from this root"
        );
        let rendered = text(&render(&state));
        assert!(rendered.contains("also in other roots"), "{rendered}");
        // The rule introduces the match; it does not replace it.
        assert!(
            rendered.contains("horizon-docs"),
            "the elsewhere match is drawn under its rule: {rendered}"
        );
        let rule = rendered.find("also in other roots").unwrap();
        let docs = rendered.find("horizon-docs").unwrap();
        assert!(rule < docs, "the rule comes first");
    }

    #[test]
    fn browse_matches_the_picker_canvas() {
        assert_snapshot("picker-browse", &render(&browsing()));
    }

    #[test]
    fn a_doubled_selection_shows_its_instance_badge() {
        let mut state = browsing();
        select(&mut state, "horizon-frontend");
        select(&mut state, "horizon-backend");
        let frontend = entry(&state, "horizon-frontend");
        state.add(&frontend);

        let buffer = render(&state);
        let rendered = text(&buffer);

        assert!(rendered.contains("×2"), "{rendered}");
        assert!(
            rendered.contains("horizon-frontend-2"),
            "the panel lists it"
        );
        assert!(rendered.contains("o  Open 3 as terminals"), "{rendered}");
        assert_snapshot("picker-selected", &buffer);
    }

    #[test]
    fn a_filter_with_no_match_says_so_and_keeps_the_selection() {
        let mut state = browsing();
        select(&mut state, "termdeck");
        state.begin_filter();
        for character in "zzq".chars() {
            state.push_filter(character);
        }

        let buffer = render(&state);
        let rendered = text(&buffer);

        assert!(rendered.contains("no match for zzq"), "{rendered}");
        assert!(rendered.contains("selection kept (1)"), "{rendered}");
        assert_snapshot("picker-no-match", &buffer);
    }

    #[test]
    fn the_root_list_is_the_pickers_own_top_level() {
        let state = PickerState::new();

        let buffer = render(&state);
        let rendered = text(&buffer);

        assert!(rendered.contains("ROOTS"), "{rendered}");
        assert!(
            rendered.contains("nothing selected · o disabled"),
            "{rendered}"
        );
        assert_snapshot("picker-roots", &buffer);
    }
}
