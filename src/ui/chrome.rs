use super::*;

/// Surrounds title chrome with one blank border cell on each side.
pub(super) fn clear_around(spans: Vec<Span<'static>>, style: Style) -> Vec<Span<'static>> {
    let mut padded = vec![Span::styled(" ", style)];
    padded.extend(spans);
    padded.push(Span::styled(" ", style));
    padded
}

/// Recedes the interface behind an open modal. The reviewed rule dims
/// foreground only, so the two accepted background values are never
/// supplemented with a scrim: pane borders fall back to idle, pane content
/// drops to [`UNDER_FG`], and the key hints between the panes drop further.
pub(super) fn dim(buffer: &mut Buffer, body: Rect, panes: &[Rect]) {
    for y in body.y..body.y + body.height {
        for x in body.x..body.x + body.width {
            let position = Position::new(x, y);
            let colour = match panes.iter().find(|pane| pane.contains(position)) {
                Some(pane) if on_border(*pane, position) => IDLE_BORDER,
                Some(_) => UNDER_FG,
                None => UNDER_HINT,
            };
            if let Some(cell) = buffer.cell_mut(position) {
                cell.fg = colour;
            }
        }
    }
}

pub(super) fn on_border(pane: Rect, position: Position) -> bool {
    position.x == pane.x
        || position.x + 1 == pane.x + pane.width
        || position.y == pane.y
        || position.y + 1 == pane.y + pane.height
}

/// The help overlay's body, in the supplement's order: a blank row, then each
/// section heading with its bindings, then the way out.
pub(super) fn help_lines() -> Vec<Line<'static>> {
    let key = Style::new().fg(PREVIEW_FG);
    let label = Style::new().fg(MUTED);
    let mut lines = vec![Line::default()];
    for (name, description) in HELP {
        if description.is_empty() {
            if lines.len() > 1 {
                lines.push(Line::default());
            }
            lines.push(Line::styled(
                name,
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{name:<HELP_KEYS$}"), key),
            Span::styled(description, label),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("esc  ", key),
        Span::styled("close", Style::new().fg(HINT)),
    ]));
    lines
}

/// The quit confirmation's body. It states how many terminals close, not which
/// ones, and confines the warning colour to the consequence line.
pub(super) fn quit_lines(terminals: usize) -> Vec<Line<'static>> {
    let key = Style::new().fg(PREVIEW_FG);
    let label = Style::new().fg(MUTED);
    // The supplement's ten-column gaps, less one: a drawn border costs a whole
    // cell where the supplement's hairline border costs none.
    let gap = Span::styled(" ".repeat(9), Style::new().fg(HINT));
    vec![
        Line::default(),
        Line::styled("quit termdeck?", Style::new().fg(MASTER_FG)),
        Line::default(),
        Line::styled(
            format!("{terminals} terminal{} will be closed.", plural(terminals)),
            key,
        ),
        Line::styled("SIGTERM, then SIGKILL after 2s.", Style::new().fg(WARNING)),
        Line::default(),
        Line::from(vec![
            Span::styled("y  ", key),
            Span::styled("quit", label),
            gap.clone(),
            Span::styled("n  ", key),
            Span::styled("cancel", label),
            gap,
            Span::styled("esc  ", key),
            Span::styled("cancel", label),
        ]),
    ]
}

/// The status row while a modal is open: it names the modal and its answers.
pub(super) fn modal_hints(modal: Modal) -> Line<'static> {
    let hint = Style::new().fg(HINT).bg(STATUS_BG);
    let key = Style::new().fg(PREVIEW_FG).bg(STATUS_BG);
    match modal {
        Modal::Help => Line::from(vec![
            Span::styled("^g ?", Style::new().fg(ACCENT).bg(STATUS_BG)),
            Span::styled(" help open", key),
            Span::styled("  ·  ", hint),
            Span::styled("esc", key),
            Span::styled(" close", hint),
        ]),
        Modal::Quit => Line::from(vec![
            Span::styled("confirm quit", Style::new().fg(WARNING).bg(STATUS_BG)),
            Span::styled("  ·  ", hint),
            Span::styled("y", key),
            Span::styled(" quit  ", hint),
            Span::styled("n", key),
            Span::styled(" cancel", hint),
        ]),
    }
}

/// The help overlay's bindings, from the plan. An empty description marks a
/// section heading.
pub(super) const HELP: [(&str, &str); 20] = [
    ("NAVIGATE", ""),
    ("^g j  ^g k", "promote next / previous"),
    ("^g ↓  ^g ↑", "same, with arrow keys"),
    ("^g N", "promote terminal number"),
    ("VIEW", ""),
    ("^g z", "toggle zoom"),
    ("^g c", "collapse / expand previews"),
    ("^g p", "pin / unpin at the stack top"),
    ("^g pgup/pgdn", "page the preview stack"),
    ("^g -  ^g =", "narrow / widen the master"),
    ("^g a", "open any folder / repo"),
    ("^g [", "enter scrollback mode"),
    ("TERMINAL", ""),
    ("^g r", "respawn active terminal"),
    ("^g x", "close active terminal"),
    ("^g v", "paste the last mouse copy"),
    ("^g ^g", "send a literal ^g"),
    ("SESSION", ""),
    ("^g ?", "this help"),
    ("^g q", "quit termdeck"),
];

/// Navigation keys captured while scrollback mode is active. The first four
/// are the pane footer; the whole list is the status bar.
pub(super) const SCROLLBACK_HINTS: [(&str, &str); 5] = [
    ("j/k ↑↓", "line"),
    ("pgup/pgdn", "page"),
    ("g/G", "ends"),
    ("esc", "live"),
    ("^g ?", "help"),
];

/// The wide status row names four keys. Select and scroll stay bound and stay
/// in the help and the collapsed row; the declutter pass took them out of the
/// bar, where the pane numbers and the scroll marker already point at them.
pub(super) const KEY_HINTS: [(&str, &str); 4] = [
    ("^g j/k", "switch"),
    ("^g z", "zoom"),
    ("^g ?", "help"),
    ("^g q", "quit"),
];

/// The column one pane's close affordance takes: [`PADDING`] in from the
/// right edge, which is the inset the title already keeps on the left, so the
/// marks line up down the stack column whether a pane is open or folded.
///
/// `None` for a pane too narrow to spare the columns, which no drawn pane is
/// — the guard is there so the arithmetic can never wrap.
pub(super) fn close_column(pane: Rect) -> Option<u16> {
    (pane.width > 2 * PADDING + 2).then(|| pane.x + pane.width - 2 - PADDING)
}

/// The content rect inside a bordered, padded pane, or `None` for a pane
/// below [`MIN_PANE`], which has no content rect at all.
///
/// Every caller measured this by subtraction until #140, and on a pane a
/// resize had made too small the subtraction wrapped: a panic in a debug
/// build, and in a release one — where `[profile.release]` sets no
/// `overflow-checks` — a rect wider and taller than the buffer it is drawn
/// into, which renders garbage until a write lands outside the buffer.
pub(super) fn pane_content(pane: Rect) -> Option<Rect> {
    (pane.width >= MIN_PANE.0 && pane.height >= MIN_PANE.1).then(|| Rect {
        x: pane.x + 1 + PADDING,
        y: pane.y + 1,
        width: pane.width - 2 - 2 * PADDING,
        height: pane.height - 2,
    })
}

/// The rows of `content` that draw terminal cells, once whatever footer this
/// pane is showing has taken its own.
///
/// Scrollback mode owns two rows of the master's foot, an exited pane spends
/// two on its exit summary, and a preview holding a detached viewport spends
/// one on its `↑ n lines below` marker. The rule lives here because two
/// callers need the same answer: [`super::Deck::draw_pane`] draws into it,
/// and the selection hit test (#148) must never offer a footer row as a
/// selectable cell.
pub(super) fn viewport_of(
    content: Rect,
    master: bool,
    exited: bool,
    metadata: &TerminalMetadata,
    scrollback: bool,
) -> Rect {
    let footer = if (master && scrollback) || exited {
        2
    } else if !master && metadata.scrollback.lines_below > 0 {
        1
    } else {
        0
    };
    Rect {
        height: content.height.saturating_sub(footer),
        ..content
    }
}

/// Where a right-aligned run of `width` columns starts inside `content`, or
/// `None` when the content cannot hold it clear of the left inset.
///
/// The right-hand affordances all placed themselves by subtracting their own
/// width from the right edge, which wraps once the content is narrower than
/// the run (#140). What does not fit is not drawn.
pub(super) fn right_aligned(content: Rect, width: u16) -> Option<u16> {
    let start = content.width.checked_sub(width.saturating_add(1))?;
    (start >= 1).then(|| content.x + start)
}

/// What the deck draws on a canvas below [`MIN_CANVAS`]: the geometry it is
/// waiting for, on the row nearest the middle, clipped to whatever columns
/// there are.
///
/// The interface has no smaller form to fall back to — the status row alone
/// costs two of the seven rows — and a partly drawn deck at these sizes is
/// what #140 was. So the fallback states the size rather than attempting one.
pub(super) fn size_notice(buffer: &mut Buffer, area: Rect) {
    if area.height == 0 {
        return;
    }
    let (columns, rows) = MIN_CANVAS;
    buffer.set_line(
        area.x,
        area.y + area.height / 2,
        &Line::styled(
            clip(&format!("{columns}×{rows} min"), area.width as usize),
            Style::new().fg(WARNING).bg(CANVAS),
        ),
        area.width,
    );
}

/// The status row: the canvas's first row (#101).
///
/// Every session frame in the export now opens with the bar — workspace chip,
/// census, then the keys — and the panes start two rows below it. The export's
/// specification panel still describes the pre-move grid (`status row 42`); the
/// screens are what the deck follows.
pub(super) fn status_of(area: Rect) -> Rect {
    Rect { height: 1, ..area }
}

/// The rows the deck draws panes into: everything under the status row and the
/// blank row that separates it from the panes.
///
/// Callers guard `area.height >= 4` before they lay anything out, so the
/// subtraction is only saturating for the ones that measure a pointer against
/// an area they were handed.
pub(super) fn body_of(area: Rect) -> Rect {
    Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(2),
        ..area
    }
}

/// Whether the split can move at this width. Below [`WIDE_COLUMNS`] the export
/// fixes the stack at [`COMPACT_STACK`], so the ratio has nothing to say and
/// the divider is neither drawn nor draggable.
pub(super) fn adjustable(body: Rect) -> bool {
    body.width >= WIDE_COLUMNS
}

/// At or above [`WIDE_COLUMNS`] the stack takes the ceiling of the non-master
/// share so the master never overruns: at 144 columns with the default 0.70
/// this is the design's 44. Below that the export fixes it instead.
pub(super) fn stack_width(width: u16, master_ratio: f64) -> u16 {
    if width < WIDE_COLUMNS {
        return COMPACT_STACK;
    }
    // The ceiling, but not fooled by a share that is a hair above a whole
    // column: the divider hands back the exact ratio for the column the
    // pointer is on, and without this the last bit of that division would
    // sometimes round the split one column past where it was dropped.
    let share = f64::from(width) * (1.0 - master_ratio) - 1e-9;
    let stack = share.ceil().max(0.0) as u16;
    stack.clamp(1, width.saturating_sub(GUTTER + 1))
}

/// The last non-blank row of a frame, as plain text. This is what a collapsed
/// preview shows for a terminal that is neither exited nor idle.
pub(super) fn last_line(frame: &TerminalFrame) -> String {
    (0..frame.size.rows)
        .rev()
        .map(|row| {
            (0..frame.size.columns)
                .filter_map(
                    |column| match frame.cell(column, row).map(|cell| &cell.content) {
                        Some(CellContent::Glyph { text, .. }) => Some(text.as_str()),
                        Some(CellContent::Empty) => Some(" "),
                        _ => None,
                    },
                )
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// What one notification says in a single line: the message a caller sent,
/// or the bare fact that a bell rang (#97).
pub(super) fn notify_text(kind: &NotifyKind) -> String {
    match kind {
        NotifyKind::Attention => "attention".to_owned(),
        NotifyKind::Message { title, body } if title.is_empty() => body.clone(),
        NotifyKind::Message { title, body } if body.is_empty() => title.clone(),
        NotifyKind::Message { title, body } => format!("{title} · {body}"),
    }
}

pub(super) fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub(super) fn status_glyph(
    status: &TerminalStatus,
    metadata: &TerminalMetadata,
) -> (&'static str, Color) {
    match status {
        TerminalStatus::Starting => ("○", WARNING),
        TerminalStatus::Running => match metadata.output_idle {
            Some(idle) if idle >= ACTIVE_WINDOW => ("○", HINT),
            _ => ("●", SUCCESS),
        },
        TerminalStatus::Exited { code: Some(0) } => ("✓", SUCCESS),
        TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => ("✕", ERROR),
    }
}

/// A title names an exit and nothing else: the glyph already carries a live
/// state, and the declutter pass dropped the word that repeated it.
pub(super) fn status_label(status: &TerminalStatus) -> Option<String> {
    match status {
        TerminalStatus::Starting | TerminalStatus::Running => None,
        TerminalStatus::Exited { code: Some(code) } => Some(format!("exit {code}")),
        TerminalStatus::Exited { code: None } => Some("exited".to_owned()),
        TerminalStatus::Failed { .. } => Some("failed".to_owned()),
    }
}

/// A strip chip states only what an ordinary running terminal does not need:
/// an exit code, or the idle ring.
pub(super) fn chip_tag(status: &TerminalStatus, metadata: &TerminalMetadata) -> String {
    match status {
        TerminalStatus::Exited { code: Some(code) } => format!(" ✕{code}"),
        TerminalStatus::Exited { code: None } | TerminalStatus::Failed { .. } => " ✕".to_owned(),
        _ => match status_glyph(status, metadata) {
            ("●", _) => String::new(),
            (glyph, _) => format!(" {glyph}"),
        },
    }
}

pub(super) fn chip_colour(status: &TerminalStatus, metadata: &TerminalMetadata) -> Color {
    match status {
        TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => ERROR,
        _ => match status_glyph(status, metadata) {
            ("●", _) => PREVIEW_FG,
            _ => MUTED,
        },
    }
}

/// `[####··]`, emptying one cell per fifth of the activity window.
pub(super) fn meter(idle_millis: u64) -> String {
    let step = ACTIVE_WINDOW.millis / METER_CELLS;
    let fill = METER_CELLS
        .saturating_sub(idle_millis / step)
        .min(METER_CELLS) as usize;
    format!(
        "[{}{}]",
        "#".repeat(fill),
        "·".repeat(METER_CELLS as usize - fill)
    )
}

pub(super) fn age(millis: u64) -> String {
    let seconds = millis / 1_000;
    match seconds {
        ..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3_600),
    }
}

/// `HH:MM:SS` in UTC. Termdeck has no time-zone database dependency.
pub(super) fn clock(at: Timestamp) -> String {
    let seconds = at.unix_millis / 1_000 % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Right-first truncation with an ellipsis, per the export's truncation rule.
pub(super) fn clip(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    match budget {
        0 => String::new(),
        _ => text
            .chars()
            .take(budget - 1)
            .chain(std::iter::once('…'))
            .collect(),
    }
}

/// The application-visible cells inside a bordered, padded terminal pane.
pub(super) fn inner_size(pane: Rect) -> ScreenSize {
    ScreenSize::new(
        pane.width.saturating_sub(2 + 2 * PADDING).max(1),
        pane.height.saturating_sub(2).max(1),
    )
}

/// The top row shared by scrollback and exited pane footers.
pub(super) fn footer_rule(buffer: &mut Buffer, content: Rect, background: Color, colour: Color) {
    buffer.set_line(
        content.x,
        content.y + content.height - 2,
        &Line::styled(
            "─".repeat(content.width as usize),
            Style::new().fg(colour).bg(background),
        ),
        content.width,
    );
}

/// Copies engine-owned cells into the buffer, clipping to the viewport.
pub(super) fn draw_terminal(
    buffer: &mut Buffer,
    area: Rect,
    terminal: &TerminalFrame,
    default_fg: Color,
    background: Color,
    selection: Option<&Selection>,
) {
    let clipped = terminal.size.columns > area.width;
    for row in 0..area.height.min(terminal.size.rows) {
        for column in 0..area.width.min(terminal.size.columns) {
            let Some(cell) = terminal.cell(column, row) else {
                continue;
            };
            let Some(target) = buffer.cell_mut((area.x + column, area.y + row)) else {
                continue;
            };
            let mut style = cell_style(&cell.style, default_fg, background);
            if selection.is_some_and(|selection| selection.contains(column, row)) {
                style = inverted(style);
            }
            match &cell.content {
                CellContent::Glyph { text, .. } => {
                    target.set_symbol(text);
                }
                CellContent::Empty | CellContent::Continuation => {
                    target.set_symbol(" ");
                }
            }
            target.set_style(style);
        }
        if clipped && area.width > 0 {
            buffer[(area.x + area.width - 1, area.y + row)]
                .set_symbol("…")
                .set_style(Style::new().fg(default_fg).bg(background));
        }
    }
}

pub(super) fn cell_style(style: &CellStyle, default_fg: Color, background: Color) -> Style {
    let mut result = Style::new()
        .fg(style.foreground.map_or(default_fg, colour))
        .bg(style.background.map_or(background, colour));
    for (enabled, modifier) in [
        (style.bold, Modifier::BOLD),
        (style.dim, Modifier::DIM),
        (style.italic, Modifier::ITALIC),
        (style.underline, Modifier::UNDERLINED),
        (style.inverse, Modifier::REVERSED),
    ] {
        if enabled {
            result = result.add_modifier(modifier);
        }
    }
    result
}

/// A selected cell, drawn as the negative of what is there (#148).
///
/// The palette has no selection colour to spare — `pin-terminal.md` §3 found
/// the border vocabulary spoken for and the same holds of the rest — and a
/// wash would be illegible over child output termdeck does not choose. So the
/// resolved colours are swapped, and the cell's own `REVERSED` attribute is
/// dropped with them: a cell the child had already inverted would otherwise
/// re-invert and read as the one cell in the range that is *not* selected.
pub(super) fn inverted(style: Style) -> Style {
    Style::new()
        .fg(style.bg.unwrap_or(Color::Reset))
        .bg(style.fg.unwrap_or(Color::Reset))
        .add_modifier(style.add_modifier.difference(Modifier::REVERSED))
        .remove_modifier(Modifier::REVERSED)
}

pub(super) const fn colour(rgb: Rgb) -> Color {
    Color::Rgb(rgb.red, rgb.green, rgb.blue)
}
