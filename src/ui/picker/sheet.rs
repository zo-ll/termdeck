use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};

use super::super::{Key, clip, palette::*, right_aligned};
use super::render::{COL_GLYPH, COL_NAME, INSET, display_path};
use super::{Browse, Entry, Instance, PickerReaction, PickerState};

/// A repository the session already holds, and the pane it is in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Open {
    pub path: PathBuf,
    pub pane: usize,
}

/// What `^g a` opens: the picker's language reduced to a sheet over the live
/// session (note §6).
///
/// It is the picker with two differences, all of them here rather than in
/// [`PickerState`], which it borrows wholesale: marks append rather than
/// order, and the master never changes. Already-open paths are ordinary
/// targets: marking one asks for another instance. `⇧⇥` cycles configured
/// roots in place.
#[derive(Clone, Debug)]
pub struct SheetState {
    picker: PickerState,
    root: usize,
}

impl SheetState {
    /// Opens on the first configured root.
    pub fn new(roots: &[Entry]) -> Self {
        Self {
            picker: roots
                .first()
                .map(|root| PickerState::at(&root.path))
                .unwrap_or_default(),
            root: 0,
        }
    }

    pub fn state(&self) -> &PickerState {
        &self.picker
    }

    pub fn state_mut(&mut self) -> &mut PickerState {
        &mut self.picker
    }

    pub fn root(&self) -> usize {
        self.root
    }

    /// `⇧⇥` — the next configured root, in place.
    ///
    /// What is marked survives the switch: the sheet's selection is as
    /// workspace-wide as the picker's, so a repository marked under one root
    /// is still going to be added after looking at another.
    pub fn next_root(&mut self, roots: &[Entry]) -> bool {
        if roots.len() < 2 {
            return false;
        }
        self.root = (self.root + 1) % roots.len();
        self.picker.go_to(roots[self.root].path.clone())
    }

    /// The current folder's listing, including selectable folders and
    /// repositories. Plain files and `..` remain navigation-only context.
    pub fn rows(&self, browser: &dyn Browse) -> Vec<Entry> {
        self.picker.rows(browser)
    }

    /// What the sheet will append, in the order it was marked.
    pub fn marked(&self) -> &[Instance] {
        self.picker.selection()
    }
}

/// Whether this row is already a running terminal, and which pane it is.
pub fn open_pane(open: &[Open], entry: &Entry) -> Option<usize> {
    open.iter()
        .find(|item| item.path == entry.path)
        .map(|item| item.pane)
}

/// The sheet, drawn over the session it will add to.
pub struct Sheet<'a> {
    pub state: &'a SheetState,
    pub rows: &'a [Entry],
    pub roots: &'a [Entry],
    pub open: &'a [Open],
    pub home: Option<&'a Path>,
    /// The pane number the first addition would take.
    pub next_pane: usize,
}

/// The sheet's own width, from the note: 78 columns centred on the session.
const SHEET_COLUMNS: u16 = 78;
/// Its listing grid is the picker's, with the separator pulled in to fit.
const SHEET_SEPARATOR: u16 = 30;
/// The instance slot's column: `+`, or `×N` once there is more than one.
pub(super) const SHEET_INSTANCE: u16 = SHEET_SEPARATOR - 3;
/// The rows the sheet's own chrome costs, which is the height it asks for
/// before it has a single row to list.
const SHEET_ROWS: u16 = 11;

impl Sheet<'_> {
    /// Where the sheet sits on `area`.
    pub fn rect(&self, area: Rect) -> Rect {
        let width = SHEET_COLUMNS.min(area.width);
        // A row per target plus the chrome, never below what the chrome
        // alone costs and never taller than the canvas — bounded in that
        // order, because on a canvas under thirteen rows the two bounds
        // cross and `clamp` panics on crossed bounds whatever it is given
        // (#140). The canvas wins there, and [`Sheet::render`] stops at the
        // border when what is left will not hold the listing.
        let height = u16::try_from(self.rows.len())
            .unwrap_or(u16::MAX)
            .saturating_add(9)
            .max(SHEET_ROWS)
            .min(area.height.saturating_sub(2));
        Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let buffer = frame.buffer_mut();
        // The session stays live behind it and recedes by foreground alone,
        // the way a modal's underlay does.
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_fg(UNDER_FG);
                }
            }
        }
        let rect = self.rect(area);
        Clear.render(rect, buffer);
        Block::bordered()
            .border_style(Style::new().fg(ACCENT))
            .style(Style::new().bg(CANVAS))
            .render(rect, buffer);
        let content = Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            width: rect.width.saturating_sub(2),
            height: rect.height.saturating_sub(2),
        };
        if content.width < 2 * INSET || content.height < 5 {
            return;
        }
        self.title(buffer, rect);
        self.header(buffer, content);
        self.rows(buffer, content);
        self.footer(buffer, content);
    }

    fn title(&self, buffer: &mut Buffer, rect: Rect) {
        let spans = vec![
            Span::styled(" ", Style::new().bg(CANVAS)),
            Span::styled("> add terminal", Style::new().fg(ACCENT)),
            Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
            Span::styled("^g a", Style::new().fg(MUTED)),
            Span::styled(" ", Style::new().bg(CANVAS)),
        ];
        buffer.set_line(rect.x + INSET, rect.y, &Line::from(spans), rect.width);
    }

    fn header(&self, buffer: &mut Buffer, content: Rect) {
        let root = self
            .state
            .state()
            .cwd()
            .map(|path| display_path(path, self.home))
            .unwrap_or_default();
        let open = self
            .rows
            .iter()
            .filter(|entry| open_pane(self.open, entry).is_some())
            .count();
        let mut spans = vec![
            Span::styled("ROOT ", Style::new().fg(MUTED)),
            Span::styled(root, Style::new().fg(MASTER_FG)),
            Span::styled("  ·  ", Style::new().fg(SEPARATOR)),
            Span::styled(
                format!("{} targets", self.rows.len()),
                Style::new().fg(MUTED),
            ),
        ];
        if open > 0 {
            spans.push(Span::styled(
                format!(" · {open} already open"),
                Style::new().fg(MUTED),
            ));
        }
        buffer.set_line(content.x + 1, content.y, &Line::from(spans), content.width);
        if self.roots.len() > 1 {
            let hint = "⇧⇥ switch root";
            if let Some(x) = right_aligned(content, hint.chars().count() as u16) {
                buffer.set_line(
                    x,
                    content.y,
                    &Line::from(Span::styled(hint, Style::new().fg(HINT))),
                    content.width,
                );
            }
        }
        buffer.set_line(
            content.x + 1,
            content.y + 1,
            &Line::from(Span::styled(
                "─".repeat(content.width as usize - 2),
                Style::new().fg(HINT),
            )),
            content.width,
        );
    }

    fn rows(&self, buffer: &mut Buffer, content: Rect) {
        let top = content.y + 2;
        let room = content.height.saturating_sub(6) as usize;
        if self.rows.is_empty() {
            buffer.set_line(
                content.x + 1,
                top,
                &Line::from(Span::styled(
                    match self.state.state().filter() {
                        Some(query) => format!("no match for {query}"),
                        None => "no targets in this folder".to_owned(),
                    },
                    Style::new().fg(MUTED),
                )),
                content.width,
            );
            return;
        }
        for slot in 0..self.rows.len().min(room) {
            let index = self.state.state().offset() + slot;
            let Some(entry) = self.rows.get(index) else {
                break;
            };
            self.row(buffer, content, top + slot as u16, index, entry);
        }
    }

    fn row(&self, buffer: &mut Buffer, content: Rect, y: u16, index: usize, entry: &Entry) {
        let cursor = index == self.state.state().cursor();
        let background = if cursor { DEMOTED_BG } else { CANVAS };
        if cursor {
            Block::new().style(Style::new().bg(background)).render(
                Rect {
                    y,
                    height: 1,
                    ..content
                },
                buffer,
            );
        }
        let put = |buffer: &mut Buffer, column: u16, spans: Vec<Span<'static>>| {
            buffer.set_line(
                content.x + 1 + column,
                y,
                &Line::from(spans),
                content.width.saturating_sub(column + 1),
            );
        };
        let open = open_pane(self.open, entry);
        let marks = self.state.state().instances(&entry.path);
        let (box_text, box_colour) = match (entry.selectable(), marks) {
            (true, 0) => ("[ ]".to_owned(), HINT),
            (true, _) => ("[+]".to_owned(), ACCENT),
            (false, _) => ("   ".to_owned(), HINT),
        };
        put(
            buffer,
            0,
            vec![Span::styled(
                box_text,
                Style::new().fg(box_colour).bg(background),
            )],
        );
        put(
            buffer,
            COL_GLYPH,
            vec![{
                let (glyph, colour) = super::render::glyph(entry.kind);
                Span::styled(glyph, Style::new().fg(colour).bg(background))
            }],
        );
        put(
            buffer,
            COL_NAME,
            vec![Span::styled(
                clip(&entry.name, (SHEET_SEPARATOR - COL_NAME - 3) as usize),
                Style::new().fg(MASTER_FG).bg(background),
            )],
        );
        // The instance slot is a `+` until there is more than one, then the
        // count. It stays on every terminal target so another instance is
        // explicit.
        let (badge, colour) = match (entry.selectable(), marks) {
            (true, 0) => ("+".to_owned(), HINT),
            (true, 1) => ("+".to_owned(), ACCENT),
            (true, _) => (format!("×{marks}"), ACCENT),
            (false, _) => (String::new(), HINT),
        };
        put(
            buffer,
            SHEET_INSTANCE,
            vec![Span::styled(badge, Style::new().fg(colour).bg(background))],
        );
        let meta = match (marks, open, entry.branch.as_deref()) {
            (_, Some(pane), _) if marks > 0 => vec![Span::styled(
                format!("another instance · pane {pane}"),
                Style::new().fg(MUTED).bg(background),
            )],
            (_, Some(pane), _) => vec![Span::styled(
                format!("already open · pane {pane}"),
                Style::new().fg(MUTED).bg(background),
            )],
            (_, None, Some(branch)) => vec![Span::styled(
                format!("git · {branch}"),
                Style::new().fg(MUTED).bg(background),
            )],
            (_, None, None) => Vec::new(),
        };
        if !meta.is_empty() {
            put(
                buffer,
                SHEET_SEPARATOR,
                vec![Span::styled("·", Style::new().fg(SEPARATOR).bg(background))],
            );
            put(buffer, SHEET_SEPARATOR + 2, meta);
        }
    }

    fn footer(&self, buffer: &mut Buffer, content: Rect) {
        let bottom = content.y + content.height;
        buffer.set_line(
            content.x + 1,
            bottom - 4,
            &Line::from(Span::styled(
                "─".repeat(content.width as usize - 2),
                Style::new().fg(HINT),
            )),
            content.width,
        );
        if let Some(query) = self.state.state().filter() {
            buffer.set_line(
                content.x + 1,
                bottom - 3,
                &Line::from(vec![
                    Span::styled("/", Style::new().fg(HINT)),
                    Span::styled(query.to_owned(), Style::new().fg(MASTER_FG)),
                ]),
                content.width,
            );
        }
        let marked = self.state.marked().len();
        if marked > 0 {
            let appends = format!("appends as pane {}", self.next_pane);
            if let Some(x) = right_aligned(content, appends.chars().count() as u16) {
                buffer.set_line(
                    x,
                    bottom - 3,
                    &Line::from(Span::styled(appends, Style::new().fg(HINT))),
                    content.width,
                );
            }
        }
        let label = self.button_label();
        buffer.set_line(
            content.x + 1,
            bottom - 2,
            &Line::from(vec![
                Span::styled(
                    label,
                    if marked > 0 {
                        Style::new()
                            .fg(CANVAS)
                            .bg(ACCENT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(HINT).bg(DEMOTED_BG)
                    },
                ),
                Span::styled("    ", Style::new()),
                Span::styled("esc cancel · master unchanged", Style::new().fg(HINT)),
            ]),
            content.width,
        );
        let keys = "↑↓ move  ⏎ mark  → inside  ← back  a all  o add";
        buffer.set_line(
            content.x + 1,
            bottom - 1,
            &Line::from(Span::styled(keys, Style::new().fg(HINT))),
            content.width,
        );
    }

    /// What the add button says, which is also how wide its target is.
    fn button_label(&self) -> String {
        match self.state.marked().len() {
            0 => " o  nothing marked ".to_owned(),
            1 => " o  Add 1 terminal ".to_owned(),
            marked => format!(" o  Add {marked} terminals "),
        }
    }

    /// What the pointer is over, in the same terms as the sheet's keys.
    ///
    /// Every action the sheet has now has a target: the row is `⏎`, the
    /// instance slot is `+` (and `-` on the secondary button), the header's
    /// switch label is `⇧⇥`, the query line is `/`, and the button is `o`.
    pub fn hit(&self, area: Rect, pointer: Position) -> Option<SheetHit> {
        let rect = self.rect(area);
        if !rect.contains(pointer) {
            return None;
        }
        let content = Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            width: rect.width.saturating_sub(2),
            height: rect.height.saturating_sub(2),
        };
        // Nothing is drawn below the render's own guard, so nothing there
        // can be clicked either: the two agree on the smallest sheet, and
        // neither measures one it did not draw (#140).
        if content.width < 2 * INSET || content.height < 5 {
            return None;
        }
        let bottom = content.y + content.height;
        // The header's own control, right-aligned where it is drawn.
        if pointer.y == content.y && self.roots.len() > 1 {
            let hint = "⇧⇥ switch root".chars().count() as u16;
            if let Some(start) = right_aligned(content, hint)
                && (start..start + hint).contains(&pointer.x)
            {
                return Some(SheetHit::Root);
            }
        }
        if pointer.y == bottom - 3 {
            return Some(SheetHit::Filter);
        }
        if pointer.y == bottom - 2 {
            let label = self.button_label().chars().count() as u16;
            let start = content.x + 1;
            return (start..start + label)
                .contains(&pointer.x)
                .then_some(SheetHit::Add);
        }
        let top = rect.y + 3;
        let room = rect.height.saturating_sub(8);
        if pointer.y < top || pointer.y >= top + room {
            return None;
        }
        let index = self.state.state().offset() + (pointer.y - top) as usize;
        if index >= self.rows.len() {
            return None;
        }
        let entry = self.rows.get(index)?;
        // The instance slot is two columns of the row and takes precedence
        // over it, the way the picker's marker cells do.
        let slot = content.x + 1 + SHEET_INSTANCE;
        Some(
            if entry.selectable() && (slot..slot + 2).contains(&pointer.x) {
                SheetHit::Instance(index)
            } else {
                SheetHit::Row(index)
            },
        )
    }
}

/// What the pointer is over inside the sheet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SheetHit {
    /// The row's body — `⏎`.
    Row(usize),
    /// Its instance slot — `+`, or `-` on the secondary button.
    Instance(usize),
    /// The header's `⇧⇥ switch root`.
    Root,
    /// The query line — `/`.
    Filter,
    /// The add button — `o`.
    Add,
}

/// One pointer gesture inside the sheet, in the same terms as its keys. Each
/// arm drives exactly the state its key drives; nothing here is a shortcut
/// the keyboard cannot take.
pub fn sheet_click(
    sheet: &mut SheetState,
    rows: &[Entry],
    roots: &[Entry],
    _open: &[Open],
    hit: SheetHit,
) -> Option<PickerReaction> {
    match hit {
        // `⏎`: mark the row, including another instance of an open path.
        SheetHit::Row(index) => {
            let entry = rows.get(index).cloned()?;
            sheet.state_mut().point_at(index, rows.len());
            sheet.state_mut().toggle(&entry);
        }
        // `+`: another instance of any selectable row.
        SheetHit::Instance(index) => {
            let entry = rows.get(index).cloned()?;
            sheet.state_mut().point_at(index, rows.len());
            sheet.state_mut().add(&entry);
        }
        SheetHit::Root => {
            sheet.next_root(roots);
        }
        SheetHit::Filter => {
            sheet.state_mut().begin_filter();
        }
        SheetHit::Add => {
            if !sheet.marked().is_empty() {
                return Some(PickerReaction::Launch);
            }
        }
    }
    None
}

/// The secondary button inside the sheet: on an instance slot it is `-`,
/// which sheds the most recent instance of that row. Everywhere else it does
/// nothing, so a stray right-click cannot change what is about to be added.
pub fn sheet_click_secondary(sheet: &mut SheetState, rows: &[Entry], hit: SheetHit) -> bool {
    let (SheetHit::Instance(index) | SheetHit::Row(index)) = hit else {
        return false;
    };
    let Some(entry) = rows.get(index).cloned() else {
        return false;
    };
    sheet.state_mut().point_at(index, rows.len());
    sheet.state_mut().drop_one(&entry)
}

/// One key inside the sheet. `⏎` marks the row, `+` marks another instance of
/// it, `o` commits, `esc` closes without adding anything.
pub fn sheet_press(
    sheet: &mut SheetState,
    rows: &[Entry],
    roots: &[Entry],
    _open: &[Open],
    key: Key,
) -> Option<PickerReaction> {
    if sheet.state().filtering()
        && let Key::Char(character) = key
        && !matches!(character, '+' | '-')
    {
        sheet.state_mut().push_filter(character);
        return None;
    }
    let cursor = rows.get(sheet.state().cursor()).cloned();
    match key {
        // As in the picker, `ctrl+j` is the control twin of `j` (#95).
        Key::Down | Key::Char('j') | Key::Ctrl('j') => {
            sheet.state_mut().move_cursor(1, rows.len());
        }
        Key::Up | Key::Char('k') => {
            sheet.state_mut().move_cursor(-1, rows.len());
        }
        Key::Enter | Key::Char(' ') => {
            if let Some(entry) = cursor.as_ref() {
                sheet.state_mut().toggle(entry);
            }
        }
        Key::Char('+') => {
            if let Some(entry) = cursor.as_ref() {
                sheet.state_mut().add(entry);
            }
        }
        Key::Char('-') => {
            if let Some(entry) = cursor.as_ref() {
                sheet.state_mut().drop_one(entry);
            }
        }
        Key::Right | Key::Char('l') => {
            if let Some(entry) = cursor.as_ref() {
                sheet.state_mut().enter(entry);
            }
        }
        Key::Left | Key::Char('h') => {
            sheet.state_mut().up(roots);
        }
        // `⇧⇥` is what the note names; plain `⇥` does the same, because a
        // sheet with one way through it should not be fussy about which.
        Key::ShiftTab | Key::Tab => {
            sheet.next_root(roots);
        }
        Key::Char('/') => {
            sheet.state_mut().begin_filter();
        }
        Key::Char('a') => {
            sheet.state_mut().select_all(rows);
        }
        Key::Backspace => {
            sheet.state_mut().pop_filter();
        }
        Key::Char('o') => {
            if !sheet.marked().is_empty() {
                return Some(PickerReaction::Launch);
            }
        }
        // The first `esc` clears the query; the second closes the sheet
        // without adding anything, exactly as the picker's does.
        Key::Escape if !sheet.state_mut().clear_filter() => return Some(PickerReaction::Quit),
        Key::Escape => {}
        _ => {}
    }
    None
}
