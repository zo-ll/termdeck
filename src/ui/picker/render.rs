use std::path::Path;

use super::{Entry, EntryKind, Listing, PickerState, match_at};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Widget},
};

use super::super::palette::*;

/// Columns of the browse panel, the gutter and the selection panel: the
/// session's own 98 / 2 / 44, which is the point of the screen.
const BROWSE_COLUMNS: u16 = 98;
const GUTTER_COLUMNS: u16 = 2;
/// Inside a panel: one border column plus the export's `padding:0 2ch`.
pub(super) const INSET: u16 = 3;
/// Below this the selection panel has nowhere to stand.
const NARROW_PICKER: u16 = 100;

/// The listing grid, in columns from the content's left edge (§1.2).
pub(super) const COL_GLYPH: u16 = 4;
pub(super) const COL_NAME: u16 = 7;
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
    /// The per-row checkbox — toggle this path without descending.
    Checkbox(usize),
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
                ("⏎/⇥", " toggle  "),
                ("+/-", " instance  "),
                ("esc", " clear filter"),
            ]
        } else {
            &[
                ("↑↓", " move  "),
                ("⏎/⇥", " toggle  "),
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
        let box_style = if instances > 0 {
            Style::new().fg(ACCENT).bg(background)
        } else {
            Style::new().fg(HINT).bg(background)
        };
        // The checkbox is for paths that can actually be selected. `..` and
        // files have no checkbox because neither can open a terminal.
        let selection_box = match (entry.selectable(), instances > 0) {
            (true, true) => "[x]",
            (true, false) => "[ ]",
            (false, _) => "   ",
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
            let escape = "esc clear · ⏎/⇥ toggle";
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
            0..=2 if entry.selectable() => Hit::Checkbox(index),
            COL_BADGE..COL_SEPARATOR if self.state.instances(&entry.path) > 0 => Hit::Badge(index),
            _ => Hit::Row(index),
        })
    }
}

/// `~/code` rather than `/home/andrea/code`, as every screen draws it.
pub(super) fn display_path(path: &Path, home: Option<&Path>) -> String {
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

use super::super::clip;
