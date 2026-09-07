use super::*;

/// One row of the toast: what to call it, what it says, and where it sits in
/// the pane order for a stable tie-break. A session notice belongs to no
/// pane, so it carries its own name and sorts last among its contemporaries.
struct Toasted<'a> {
    order: usize,
    head: String,
    notify: &'a Notify,
}

impl Deck<'_> {
    /// Draws the whole screen: master, preview stack, key hints, status row.
    pub fn render(&self, engine: &dyn TerminalEngine, frame: &mut Frame) {
        let area = frame.area();
        Block::new()
            .style(Style::new().bg(CANVAS))
            .render(area, frame.buffer_mut());
        // The interface's own minimum, established before anything is laid
        // out: below it the deck says what it is waiting for and draws no
        // chrome, rather than laying out panes the canvas cannot hold (#140).
        if area.width < MIN_CANVAS.0 || area.height < MIN_CANVAS.1 {
            size_notice(frame.buffer_mut(), area);
            return;
        }

        // Rows: the status row, one blank row, then the body (#101).
        let body = body_of(area);
        let layout = self.layout(body);
        // The drawn panes, so an open modal knows which cells are pane chrome
        // and which are the key hints between them.
        let panes = match layout {
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                self.draw_active(
                    engine,
                    frame.buffer_mut(),
                    master,
                    self.master_pane(Pane::Master),
                );
                let mut panes = vec![master];
                panes.extend(self.stack(
                    engine,
                    frame.buffer_mut(),
                    Rect {
                        x: master.width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                ));
                if adjustable(body) {
                    self.divider(frame.buffer_mut(), body, master.x + master.width);
                }
                panes
            }
            Layout::Zoom => {
                self.draw_active(engine, frame.buffer_mut(), body, Pane::Zoomed);
                vec![body]
            }
            Layout::Single => {
                self.draw_active(
                    engine,
                    frame.buffer_mut(),
                    body,
                    self.master_pane(Pane::Master),
                );
                vec![body]
            }
            Layout::Narrow => vec![self.narrow(engine, frame.buffer_mut(), body)],
        };
        self.status_row(
            engine,
            frame.buffer_mut(),
            status_of(area),
            area.height,
            layout,
        );
        // A modal takes focus: the interface recedes behind it and the master
        // gives up both its accent border and its cursor.
        match self.state.modal() {
            Some(modal) => {
                dim(frame.buffer_mut(), body, &panes);
                self.modal(frame.buffer_mut(), area, modal);
            }
            None => {
                // The toast is not a modal: nothing is dimmed, the master
                // keeps its cursor, and every key still goes where it was
                // going (#97). It is only drawn for panes this layout leaves
                // nowhere else to say it — and never over a modal, which owns
                // the screen while it is open.
                let toast = self.toast_items(body);
                if !toast.is_empty() {
                    self.toast(frame.buffer_mut(), area, &toast);
                }
                self.place_cursor(engine, frame, panes[0]);
            }
        }
    }

    /// The notifications the current layout has nowhere to draw: their pane is
    /// hidden, so the toast is the only place they can appear. Newest first,
    /// which is the order the batch is read in.
    ///
    /// A session notice is here on the same terms and for a stronger reason:
    /// a terminal that never started has no pane in any layout, so the toast
    /// is not merely the only place left but the only place there is (#128).
    fn toast_items(&self, body: Rect) -> Vec<Toasted<'_>> {
        if self.notifies.is_empty() {
            return Vec::new();
        }
        let mut items: Vec<Toasted<'_>> = self
            .hidden(body)
            .into_iter()
            .filter_map(|position| {
                let project = self.projects.get(position)?;
                let notify = self.notifies.pending(&project.terminal)?;
                self.notifies
                    .toasting(&project.terminal, self.now)
                    .then(|| Toasted {
                        order: position,
                        head: format!("{} {}", position + 1, project.terminal),
                        notify,
                    })
            })
            .collect();
        // A notice belongs to no pane, so it sorts after the panes it shares
        // its moment with rather than into the middle of them.
        items.extend(
            self.notifies
                .toasting_notices(self.now)
                .map(|notice| Toasted {
                    order: usize::MAX,
                    head: notice.head.clone(),
                    notify: &notice.notify,
                }),
        );
        items.sort_by(|left, right| {
            right
                .notify
                .at
                .cmp(&left.notify.at)
                .then(left.order.cmp(&right.order))
        });
        items
    }

    /// The configured positions this layout draws no pane for.
    ///
    /// Zoom and the narrow fallback hide the whole stack; a column shorter
    /// than its list hides whatever the window left out. The master is never
    /// among them: every layout draws it.
    fn hidden(&self, body: Rect) -> Vec<usize> {
        match self.layout(body) {
            Layout::Single => Vec::new(),
            Layout::Zoom | Layout::Narrow => self.stack_items(),
            Layout::Stacked { stack, preview } => {
                let drawn: Vec<usize> = self
                    .stack_layout(
                        Rect {
                            x: body.width - GUTTER - stack,
                            width: stack,
                            ..body
                        },
                        preview,
                    )
                    .iter()
                    .map(|slot| slot.position)
                    .collect();
                self.stack_items()
                    .into_iter()
                    .filter(|position| !drawn.contains(position))
                    .collect()
            }
        }
    }

    /// Returns the terminal in the pane under `pointer`, excluding chrome
    /// outside the pane rectangles.
    pub fn terminal_at(&self, area: Rect, pointer: Position) -> Option<&TerminalId> {
        self.position_at(area, pointer)
            .and_then(|position| self.projects.get(position))
            .map(|project| &project.terminal)
    }

    /// Visible terminal-cell dimensions by configured project position.
    ///
    /// A hidden folded preview has no viewport, so it retains its last size
    /// until promotion or expansion gives it one again.
    pub fn terminal_sizes(&self, area: Rect) -> Vec<Option<ScreenSize>> {
        let mut sizes = vec![None; self.projects.len()];
        if area.width < GUTTER + 4 || area.height < 4 {
            return sizes;
        }
        let body = body_of(area);
        let mut set = |position: usize, pane: Rect| {
            if position < sizes.len() {
                sizes[position] = Some(inner_size(pane));
            }
        };
        let active = self.state.active();
        match self.layout(body) {
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                if let Some(position) = active {
                    set(position, master);
                }
                let stack = Rect {
                    x: master.x + master.width + GUTTER,
                    width: stack,
                    ..body
                };
                for slot in self.stack_layout(stack, preview) {
                    if !slot.collapsed {
                        set(slot.position, slot.rect);
                    }
                }
            }
            Layout::Zoom | Layout::Single => {
                if let Some(position) = active {
                    set(position, body);
                }
            }
            Layout::Narrow => {
                if let Some(position) = active {
                    set(
                        position,
                        Rect {
                            y: body.y + 2,
                            height: body.height.saturating_sub(2),
                            ..body
                        },
                    );
                }
            }
        }
        sizes
    }

    /// Terminal identities whose timing metadata is present in the deck's
    /// current drawing. This deliberately differs from [`Self::terminal_sizes`]:
    /// a folded preview has no PTY viewport to resize, but its drawn strip
    /// still reports its idle age.
    pub fn timing_terminals(&self, area: Rect) -> Vec<TerminalId> {
        if area.width < GUTTER + 4 || area.height < 4 {
            return Vec::new();
        }
        let body = body_of(area);
        let mut positions = self.state.active().into_iter().collect::<Vec<_>>();
        if let Layout::Stacked { stack, preview } = self.layout(body) {
            let master_width = body.width - GUTTER - stack;
            positions.extend(
                self.stack_layout(
                    Rect {
                        x: body.x + master_width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .into_iter()
                .map(|slot| slot.position),
            );
        }
        positions
            .into_iter()
            .filter_map(|position| self.projects.get(position))
            .map(|project| project.terminal.clone())
            .collect()
    }

    /// Returns the configured position in the pane under `pointer`.
    ///
    /// A collapsed preview still answers here, so promoting or swapping it by
    /// pointer keeps working; it is the caller's business to notice that a
    /// folded pane has no viewport to scroll.
    pub fn position_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = body_of(area);
        let active = || {
            self.state
                .active()
                .filter(|position| self.projects.get(*position).is_some())
        };
        match self.layout(body) {
            Layout::Zoom | Layout::Single if body.contains(pointer) => active(),
            Layout::Narrow => {
                let master = Rect {
                    y: body.y + 2,
                    height: body.height.saturating_sub(2),
                    ..body
                };
                master.contains(pointer).then(active).flatten()
            }
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                if master.contains(pointer) {
                    return active();
                }
                self.stack_layout(
                    Rect {
                        x: master.width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .into_iter()
                .find(|slot| slot.rect.contains(pointer))
                .map(|slot| slot.position)
            }
            _ => None,
        }
    }

    /// The terminal cell under `pointer`: the configured position plus the
    /// 0-based column and row from the viewport's top-left, past the border
    /// and the title inset. `None` outside a pane's drawn rectangle or over
    /// a folded preview, which has no viewport.
    ///
    /// Wheel forwarding into alternate-screen apps (#74) clamps these
    /// against the frame size; the app mostly scrolls wherever the tick
    /// lands, so title- and footer-row imprecision is harmless.
    pub fn pane_cell(&self, area: Rect, pointer: Position) -> Option<(usize, u16, u16)> {
        let position = self.position_at(area, pointer)?;
        if area.width < GUTTER + 4 || area.height < 4 || self.state.collapsed(position) {
            return None;
        }
        let body = body_of(area);
        let rect = match self.layout(body) {
            // An empty stack is the full body too: no stack, no divider,
            // so the master owns every column the zoomed master does.
            Layout::Zoom | Layout::Single => body,
            Layout::Narrow => Rect {
                y: body.y + 2,
                height: body.height.saturating_sub(2),
                ..body
            },
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                if master.contains(pointer) {
                    master
                } else {
                    self.stack_layout(
                        Rect {
                            x: master.x + master.width + GUTTER,
                            width: stack,
                            ..body
                        },
                        preview,
                    )
                    .into_iter()
                    .find(|slot| slot.rect.contains(pointer))
                    .map(|slot| slot.rect)?
                }
            }
        };
        Some((
            position,
            pointer.x.saturating_sub(rect.x + 1 + PADDING),
            pointer.y.saturating_sub(rect.y + 1),
        ))
    }

    /// The configured position whose disclosure marker sits under `pointer`.
    ///
    /// The marker owns two cells at the head of a stack pane's title: the
    /// export draws `▾ ` inside an open preview's top border and `▸ ` at the
    /// head of a collapsed strip, one column further left because the strip
    /// has no border to inset past.
    ///
    /// Every stack pane carries a marker, folded or not, so the cells are
    /// live from the first frame — that is the affordance a fresh run has to
    /// offer, and it takes precedence over drag and double-click there.
    pub fn marker_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = body_of(area);
        let Layout::Stacked { stack, preview } = self.layout(body) else {
            return None;
        };
        let master = body.width - GUTTER - stack;
        self.stack_layout(
            Rect {
                x: master + GUTTER,
                width: stack,
                ..body
            },
            preview,
        )
        .into_iter()
        .find(|slot| {
            // An open pane insets its title past the border; a strip does not.
            let head = slot.rect.x + if slot.collapsed { PADDING } else { PADDING + 1 };
            pointer.y == slot.rect.y && (pointer.x == head || pointer.x == head + 1)
        })
        .map(|slot| slot.position)
    }

    /// The configured position whose close affordance sits under `pointer`.
    ///
    /// Every drawn pane carries one — the master included, because closing it
    /// promotes the pane behind it — and it owns its two cells the way the
    /// disclosure marker owns its own: the press there starts no drag and
    /// arms no promotion. The narrow fallback draws none, for the reason the
    /// status bar's `+` is absent there too, so nothing answers here either.
    pub fn close_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = body_of(area);
        let hit = |position: usize, pane: Rect| {
            let column = close_column(pane)?;
            (pointer.y == pane.y && (pointer.x == column || pointer.x == column + 1))
                .then_some(position)
        };
        let active = || {
            self.state
                .active()
                .filter(|position| self.projects.get(*position).is_some())
        };
        match self.layout(body) {
            Layout::Narrow => None,
            Layout::Zoom | Layout::Single => active().and_then(|position| hit(position, body)),
            Layout::Stacked { stack, preview } => {
                let master = Rect {
                    width: body.width - GUTTER - stack,
                    ..body
                };
                if let Some(position) = active().and_then(|position| hit(position, master)) {
                    return Some(position);
                }
                self.stack_layout(
                    Rect {
                        x: master.width + GUTTER,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .into_iter()
                .find_map(|slot| hit(slot.position, slot.rect))
            }
        }
    }

    /// The part of the preview list the stack column is currently showing.
    ///
    /// The caller pages the list through this: the window knows how far it can
    /// scroll, which only the rendered geometry can say. A hidden stack has no
    /// window, and its offset never moves.
    pub fn stack_window(&self, area: Rect) -> StackWindow {
        if area.width < GUTTER + 4 || area.height < 4 {
            return StackWindow::default();
        }
        let body = body_of(area);
        let Layout::Stacked { preview, .. } = self.layout(body) else {
            return StackWindow::default();
        };
        self.stack_window_of(&self.stack_items(), body.height.saturating_sub(1), preview)
    }

    /// Whether `pointer` sits on the stack column's own chrome rather than on
    /// a preview: the gutter beside it, the blank rows between previews, the
    /// empty column below them, and the footer row.
    ///
    /// That is where the wheel pages the list. Over a preview the wheel still
    /// belongs to that preview's viewport, so the two gestures never fight.
    pub fn stack_scroll_at(&self, area: Rect, pointer: Position) -> bool {
        if !area.contains(pointer) || area.width < GUTTER + 4 || area.height < 4 {
            return false;
        }
        let body = body_of(area);
        let Layout::Stacked { stack, preview } = self.layout(body) else {
            return false;
        };
        let column = Rect {
            x: body.width - GUTTER - stack,
            width: GUTTER + stack,
            ..body
        };
        column.contains(pointer)
            && !self
                .stack_layout(
                    Rect {
                        x: body.width - stack,
                        width: stack,
                        ..body
                    },
                    preview,
                )
                .iter()
                .any(|slot| slot.rect.contains(pointer))
    }

    /// The column the split divider is drawn in, while the split can move.
    ///
    /// It is the gutter column beside the master, so the divider takes no
    /// columns from either pane and leaves the column beside the stack to the
    /// scroll track (#34b). Below [`WIDE_COLUMNS`] the export fixes the stack
    /// width, so there is nothing to move and there is no divider.
    fn divider_of(&self, area: Rect) -> Option<(Rect, u16)> {
        if area.width < GUTTER + 4 || area.height < 4 {
            return None;
        }
        let body = body_of(area);
        let Layout::Stacked { stack, .. } = self.layout(body) else {
            return None;
        };
        adjustable(body).then(|| (body, body.x + body.width - GUTTER - stack))
    }

    /// Whether `pointer` is on the divider, and so starts a resize rather than
    /// a pane drag. The divider sits in the gutter, which belongs to no pane,
    /// so the two gestures never contend for the same cell.
    pub fn divider_at(&self, area: Rect, pointer: Position) -> bool {
        self.divider_of(area)
            .is_some_and(|(body, column)| pointer.x == column && body.contains(pointer))
    }

    /// The split that puts the divider under `column`: the inverse of the
    /// layout, so dragging to a column and releasing leaves the divider under
    /// the pointer.
    ///
    /// The master keeps every column left of the divider, the gutter takes
    /// the next two and the stack takes the rest, so the master's share is
    /// `column + GUTTER`. The caller clamps it — [`DeckState`] owns the range.
    pub fn ratio_at(&self, area: Rect, column: u16) -> Option<f64> {
        let (body, _) = self.divider_of(area)?;
        let master = column.saturating_sub(body.x).min(body.width);
        Some(f64::from(master + GUTTER) / f64::from(body.width))
    }

    /// Whether `pointer` is on the status bar's `+`, which opens the
    /// runtime-add sheet — the pointer's half of `^g a` (#50 A3).
    ///
    /// The affordance sits after the workspace chip, the census and the
    /// active pane, so where it lands depends on their widths; this measures
    /// the same spans the row draws rather than guessing a column.
    pub fn add_at(&self, area: Rect, pointer: Position) -> bool {
        if area.height < 2 || pointer.y != area.y {
            return false;
        }
        let body = body_of(area);
        // The narrow fallback's row has no affordance to click.
        if self.layout(body) == Layout::Narrow {
            return false;
        }
        let start = area.x + PADDING + self.add_affordance_offset();
        let label = if area.width >= WIDE_COLUMNS { 4 } else { 0 };
        (start..=start + ADD_AFFORDANCE.chars().count() as u16 + label).contains(&pointer.x)
    }

    /// Columns before the `+`.
    fn add_affordance_offset(&self) -> u16 {
        let total = self.projects.len();
        let active = self
            .state
            .active()
            .and_then(|position| self.projects.get(position).zip(Some(position)))
            .map(|(project, position)| format!("> {} {}", position + 1, project.terminal))
            .unwrap_or_default();
        let width = format!(" {} ", self.workspace).chars().count()
            + format!("  {total} terminal{}   ", plural(total))
                .chars()
                .count()
            + active.chars().count()
            + "  ·  ".chars().count();
        width as u16
    }

    /// A draggable pane must have a visible master-and-stack counterpart.
    pub fn swap_position_at(&self, area: Rect, pointer: Position) -> Option<usize> {
        let body = body_of(area);
        matches!(self.layout(body), Layout::Stacked { .. })
            .then(|| self.position_at(area, pointer))
            .flatten()
    }

    fn master_pane(&self, pane: Pane) -> Pane {
        match self.state.active() {
            Some(position) if self.state.dragged() == Some(position) => pane.dragging(false),
            Some(position) if self.state.drag_target() == Some(position) => pane.dragging(true),
            _ => pane,
        }
    }

    /// Narrow fallback wins over zoom: below a usable preview width the stack
    /// is already hidden, so zoom has nothing left to hide. An empty stack
    /// wins over zoom the same way: with nothing stacked the master is full
    /// either way, and the deck reads as an ordinary deck rather than a zoom.
    fn layout(&self, body: Rect) -> Layout {
        let preview = if body.width >= WIDE_COLUMNS {
            PREVIEW_HEIGHT
        } else {
            COMPACT_PREVIEW_HEIGHT
        };
        // The stack needs one whole preview plus the hint row below it.
        if body.width < NARROW_COLUMNS || body.height < preview + 2 {
            return Layout::Narrow;
        }
        if self.stack_items().is_empty() {
            return Layout::Single;
        }
        if self.state.zoomed() {
            return Layout::Zoom;
        }
        Layout::Stacked {
            stack: stack_width(body.width, self.master_ratio),
            preview,
        }
    }

    /// The previews the list holds, top to bottom, dropping any configured
    /// position the workspace has no project for.
    fn stack_items(&self) -> Vec<usize> {
        self.state
            .stack()
            .iter()
            .copied()
            .filter(|position| self.projects.get(*position).is_some())
            .collect()
    }

    /// What one preview costs the column before any freed rows are handed out:
    /// a fold is a single title row, everything else is a whole preview.
    fn item_height(&self, position: usize, preview: u16) -> u16 {
        if self.state.collapsed(position) {
            COLLAPSED_HEIGHT
        } else {
            preview
        }
    }

    /// Which previews the column can show from the stored offset.
    ///
    /// `budget` is the stack area less its footer row, and every preview costs
    /// its height plus the blank row under it. The window takes previews from
    /// the offset while they fit, so the column always holds whole previews;
    /// the offset itself is clamped to the last window that still ends on the
    /// final preview, which is found by filling the same budget backwards.
    fn stack_window_of(&self, items: &[usize], budget: u16, preview: u16) -> StackWindow {
        let limit = items.len() - self.fill(items.iter().rev(), budget, preview);
        let offset = self.state.stack_offset().min(limit);
        StackWindow {
            offset,
            visible: self.fill(items[offset..].iter(), budget, preview),
            total: items.len(),
            limit,
        }
    }

    /// How many of `positions` fit in `budget` rows, each costing its height
    /// plus the blank row that follows it.
    fn fill<'a>(
        &self,
        positions: impl Iterator<Item = &'a usize>,
        budget: u16,
        preview: u16,
    ) -> usize {
        let mut used = 0;
        let mut count = 0;
        for position in positions {
            let cost = self.item_height(*position, preview) + 1;
            if used + cost > budget {
                break;
            }
            used += cost;
            count += 1;
        }
        count
    }

    /// Places the stack's children, top to bottom, from the scrolled window.
    ///
    /// A collapsed preview gives up every row but its title, and those rows go
    /// straight to the previews still open: the export's "freed rows
    /// redistribute to the panes still open, so one open preview grows to fill
    /// the column". The remainder goes to the topmost open panes so the column
    /// stays deterministic.
    ///
    /// When the whole list fits, each fold hands over exactly what it gave up,
    /// so the stack's used height never changes and a fold can never overflow
    /// a stack that fitted before it. When the list is longer than the column,
    /// the folds have already bought room for further previews, so what is
    /// handed out is only the room the window has left over.
    fn stack_layout(&self, area: Rect, preview: u16) -> Vec<StackSlot> {
        let items = self.stack_items();
        let budget = area.height.saturating_sub(1);
        let window = self.stack_window_of(&items, budget, preview);
        let drawn = &items[window.offset..window.offset + window.visible];
        let used: u16 = drawn
            .iter()
            .map(|position| self.item_height(*position, preview) + 1)
            .sum();
        let folded = drawn
            .iter()
            .filter(|position| self.state.collapsed(**position))
            .count();
        let open = window.visible - folded;
        let freed = (folded as u16 * (preview - COLLAPSED_HEIGHT)).min(budget - used);
        let (share, mut remainder) = if open > 0 {
            (freed / open as u16, freed % open as u16)
        } else {
            (0, 0)
        };

        let mut slots = Vec::with_capacity(window.visible);
        let mut top = area.y;
        for position in drawn.iter().copied() {
            let collapsed = self.state.collapsed(position);
            let height = if collapsed {
                COLLAPSED_HEIGHT
            } else {
                let extra = u16::from(remainder > 0);
                remainder = remainder.saturating_sub(1);
                preview + share + extra
            };
            slots.push(StackSlot {
                position,
                rect: Rect {
                    y: top,
                    height,
                    ..area
                },
                collapsed,
            });
            top += height + 1;
        }
        slots
    }

    fn draw_active(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        pane: Pane,
    ) {
        let Some(position) = self.state.active() else {
            return;
        };
        let Some(project) = self.projects.get(position) else {
            return;
        };
        self.draw_pane(engine, buffer, area, project, position, pane);
    }

    /// Draws the previews top to bottom and returns the rects they took.
    fn stack(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        preview: u16,
    ) -> Vec<Rect> {
        let demoted = self.state.demoted(self.now);
        let window =
            self.stack_window_of(&self.stack_items(), area.height.saturating_sub(1), preview);
        let mut drawn = Vec::new();
        for slot in self.stack_layout(area, preview) {
            let Some(project) = self.projects.get(slot.position) else {
                continue;
            };
            if slot.collapsed {
                // A strip has no border to carry a drag or demotion highlight,
                // and already sits on the demoted background.
                self.draw_strip(engine, buffer, slot.rect, project, slot.position);
            } else {
                let kind = if self.state.dragged() == Some(slot.position) {
                    Pane::Preview.dragging(false)
                } else if self.state.drag_target() == Some(slot.position) {
                    Pane::Preview.dragging(true)
                } else if self.notifies.flashing(&project.terminal, self.now) {
                    // A pane asking for you outranks one that has just
                    // settled: the drag is the only thing the pointer is
                    // holding, so it still comes first.
                    Pane::Notify
                } else if demoted == Some(slot.position) {
                    Pane::Demoted
                } else {
                    Pane::Preview
                };
                self.draw_pane(engine, buffer, slot.rect, project, slot.position, kind);
            }
            drawn.push(slot.rect);
        }
        self.scroll_track(buffer, area, window);
        buffer.set_line(
            area.x + 1,
            area.y + area.height - 1,
            &Line::from(self.stack_hints(window, area.width - 1)),
            area.width - 1,
        );
        drawn
    }

    /// The draggable divider between the master and the stack.
    ///
    /// A `│` in the separator colour for its whole height, with a three-cell
    /// grip at the middle in the hint colour: the affordance says both where
    /// the split is and that it can be taken hold of. While it is held the
    /// whole divider takes the accent, so the drag states itself the way a
    /// pane drag does.
    fn divider(&self, buffer: &mut Buffer, body: Rect, column: u16) {
        let middle = body.y + body.height / 2;
        let grip = middle.saturating_sub(1)..=middle + 1;
        for row in body.y..body.y + body.height {
            let Some(cell) = buffer.cell_mut((column, row)) else {
                continue;
            };
            let held = grip.contains(&row);
            cell.set_symbol(if held { "┃" } else { "│" }).set_style(
                Style::new()
                    .fg(match (self.state.resizing(), held) {
                        (true, _) => ACCENT,
                        (false, true) => HINT,
                        (false, false) => SEPARATOR,
                    })
                    .bg(CANVAS),
            );
        }
    }

    /// A one-column track in the gutter beside the stack, drawn only while the
    /// list is longer than the window.
    ///
    /// It takes no room from the previews, so a stack that fits looks exactly
    /// as the export draws it. The thumb's length and position are the
    /// window's share of the list, which states both how much is hidden and
    /// where the window sits in it.
    fn scroll_track(&self, buffer: &mut Buffer, area: Rect, window: StackWindow) {
        let track = usize::from(area.height.saturating_sub(1));
        if !window.overflows() || track == 0 || area.x == 0 {
            return;
        }
        let length = (window.visible * track)
            .div_ceil(window.total)
            .clamp(1, track);
        // Anchored at whichever end the window has reached, so "there is
        // nothing further down" is never a rounding question.
        let top = match window.below() {
            0 => track - length,
            _ => (window.offset * track / window.total).min(track - length),
        };
        for row in 0..track {
            let held = (top..top + length).contains(&row);
            let Some(cell) = buffer.cell_mut((area.x - 1, area.y + row as u16)) else {
                continue;
            };
            cell.set_symbol(if held { "┃" } else { "│" }).set_style(
                Style::new()
                    .fg(if held { HINT } else { IDLE_BORDER })
                    .bg(CANVAS),
            );
        }
    }

    /// One pane's scrollback position, drawn into its right border (#115).
    ///
    /// The border is the track and the thumb is that border drawn heavy, so
    /// the bar takes no column from the layout and no cell from the terminal:
    /// a scrollbar that resized the pane it measures would change the thing it
    /// describes every time it appeared.
    ///
    /// Three conditions, all of which must hold: the pane holds the master
    /// frame — its caller's guard, because a preview says the same thing in
    /// words on its own last row — it has scrollback to report, and it is in
    /// play: scrollback mode
    /// is on, which carries no countdown because the mode is the signal, or a
    /// scroll moved it inside [`crate::ui::SCROLLBAR_WINDOW`]. Otherwise the
    /// pane renders exactly as it did before this existed.
    ///
    /// The arithmetic is [`Deck::scroll_track`]'s, with the preview list's
    /// counts swapped for the viewport's, anchoring included: at the tail the
    /// thumb is flush with the bottom, so "there is nothing further down" is
    /// never a rounding question.
    fn scrollbar(
        &self,
        buffer: &mut Buffer,
        area: Rect,
        position: usize,
        scrollback: ScrollbackPosition,
        rows: u16,
        style: Style,
    ) {
        if area.height < 4 || area.width == 0 {
            return;
        }
        let raised = self.state.scrollback() || self.state.scrolling(self.now) == Some(position);
        if !raised {
            return;
        }
        let (above, below) = (
            scrollback.lines_above as usize,
            scrollback.lines_below as usize,
        );
        // Nothing above and nothing below is a viewport with no position to
        // report — the stack track's own rule, and the reason a fresh pane
        // draws no bar even in scrollback mode.
        if above + below == 0 {
            return;
        }
        let track = usize::from(area.height - 2);
        let window = usize::from(rows.max(1));
        let total = above + window + below;
        let length = (window * track).div_ceil(total).clamp(1, track);
        let top = match below {
            0 => track - length,
            _ => (above * track / total).min(track - length),
        };
        let column = area.x + area.width - 1;
        for row in top..top + length {
            let Some(cell) = buffer.cell_mut((column, area.y + 1 + row as u16)) else {
                continue;
            };
            cell.set_symbol(SCROLL_THUMB).set_style(style);
        }
    }

    /// A collapsed preview: one row, no box, on the export's `#101317`.
    ///
    /// `▸ {n} {name} · {dot} · {tail}`, where the tail is the pane's last
    /// meaningful line. The box goes and takes the cwd, the right-hand
    /// activity slot, the viewport, the exit footer and the scrollback marker
    /// with it; all of them come back when the pane expands.
    ///
    /// A strip already sits on the demotion tint, so it cannot flash by
    /// lifting its background the way an open pane does (#97). It inverts
    /// instead — warning ground, canvas ink — and swaps its status dot for a
    /// `!`, which is the mark the censuses use for the same state.
    fn draw_strip(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        project: &Project,
        position: usize,
    ) {
        let flashing = self.notifies.flashing(&project.terminal, self.now);
        let background = if flashing { WARNING } else { DEMOTED_BG };
        Block::new()
            .style(Style::new().bg(background))
            .render(area, buffer);
        let status = self.status(engine, project);
        let metadata = self.metadata(engine, project);
        let (glyph, glyph_colour) = if flashing {
            (NOTIFY_MARK, CANVAS)
        } else {
            status_glyph(&status, &metadata)
        };
        let ink = |colour: Color| {
            Style::new()
                .fg(if flashing { CANVAS } else { colour })
                .bg(background)
        };
        let separator = ink(SEPARATOR);

        let mut spans = vec![Span::styled("▸ ", ink(HINT))];
        if self.state.pinned() == Some(position) {
            spans.push(Span::styled(format!("{PIN_MARK} "), ink(ACCENT)));
        }
        spans.extend([
            Span::styled(
                format!("{} {}", position + 1, project.terminal),
                ink(PREVIEW_FG),
            ),
            Span::styled(" · ", separator),
            Span::styled(glyph, Style::new().fg(glyph_colour).bg(background)),
        ]);
        // The strip sits two columns in, per the export's `padding:0 2ch`, and
        // keeps the same inset on the right.
        let width = area.width.saturating_sub(2 * PADDING);
        // A folded preview closes like an open one (#84), so its row ends at
        // the same column the open panes put their mark in, with a blank
        // column before it holding the tail off.
        let close = close_column(area);
        let width = width.saturating_sub(u16::from(close.is_some()) * 3);
        let taken: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        // The tail is the first thing the strip gives up. At the minimum stack
        // width (#44) there is no room for it, and a separator with nothing
        // after it states nothing, so the two go together.
        let room = (width as usize).saturating_sub(taken + 3);
        if room > 1 {
            let (tail, colour) = match self.notifies.pending(&project.terminal) {
                // While it flashes the strip says what it is flashing about,
                // which is the one line it has to say anything in.
                Some(notify) if flashing => (notify_text(&notify.kind), MUTED),
                _ => self.strip_tail(engine, project, &status, &metadata),
            };
            spans.push(Span::styled(" · ", separator));
            spans.push(Span::styled(clip(&tail, room), ink(colour)));
        }
        buffer.set_line(area.x + PADDING, area.y, &Line::from(spans), width);
        if let Some(column) = close {
            buffer.set_line(
                column,
                area.y,
                &Line::from(Span::styled(CLOSE_AFFORDANCE, ink(HINT))),
                CLOSE_AFFORDANCE.chars().count() as u16,
            );
        }
    }

    /// The strip's trailing text: what the pane would say if it had one line
    /// left. An exit outranks an idle age, which outranks the live output.
    fn strip_tail(
        &self,
        engine: &dyn TerminalEngine,
        project: &Project,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
    ) -> (String, Color) {
        match status {
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => (
                status_label(status).unwrap_or_else(|| "exited".to_owned()),
                ERROR,
            ),
            TerminalStatus::Starting => ("starting".to_owned(), MUTED),
            TerminalStatus::Running => match metadata.output_idle {
                Some(idle) if idle >= ACTIVE_WINDOW => {
                    (format!("idle {}", age(idle.millis)), MUTED)
                }
                _ => (
                    engine
                        .frame(&project.terminal)
                        .map(last_line)
                        .unwrap_or_default(),
                    MUTED,
                ),
            },
        }
    }

    /// The stack footer states what the window hides, and otherwise says
    /// nothing. The declutter pass took out the promotion keys and the
    /// `promoted x · ^g 1 back` line: the demoted pane's own highlight
    /// already reports the swap, and the keys live in the help. The updated
    /// canvas (screen 05) took the fold census out with them — a fold is
    /// already declared by its own strip, its marker, and the status row's
    /// `N collapsed` beside an accented `^g c`, so the column said it twice.
    /// A preview the window has scrolled past says nothing about itself
    /// anywhere else, and so keeps the row.
    fn stack_hints(&self, window: StackWindow, width: u16) -> Vec<Span<'static>> {
        if self.state.scrollback() {
            return vec![
                Span::styled("scrollback", Style::new().fg(WARNING)),
                Span::styled(" · ", Style::new().fg(HINT)),
                Span::styled("esc", Style::new().fg(PREVIEW_FG)),
                Span::styled(" returns to live", Style::new().fg(HINT)),
            ];
        }
        if window.overflows() {
            return self.overflow_hints(window, width);
        }
        Vec::new()
    }

    /// `↑ 2 more · ↓ 3 more · ^g pgup/pgdn`, naming only the end that has
    /// something behind it. The keys are stated whenever the footer has the
    /// columns for them, and dropped first when it does not — the status
    /// bar's own rule.
    fn overflow_hints(&self, window: StackWindow, width: u16) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT);
        let key = Style::new().fg(PREVIEW_FG);
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (arrow, hidden) in [("↑", window.above()), ("↓", window.below())] {
            if hidden == 0 {
                continue;
            }
            if !spans.is_empty() {
                spans.push(Span::styled(" · ", hint));
            }
            spans.push(Span::styled(format!("{arrow} {hidden} "), key));
            spans.push(Span::styled("more", hint));
        }
        let taken: usize = spans.iter().map(Span::width).sum();
        if taken + PAGE_KEYS.chars().count() + 3 <= usize::from(width) {
            spans.push(Span::styled(" · ", hint));
            spans.push(Span::styled(PAGE_KEYS, key));
        }
        spans
    }

    /// Master-only fallback: a one-line pane strip on the first row, a blank
    /// row, then the master for the rest of the body. Returns the master rect.
    fn narrow(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, body: Rect) -> Rect {
        self.strip(engine, buffer, Rect { height: 1, ..body });
        let master = Rect {
            y: body.y + 2,
            height: body.height.saturating_sub(2),
            ..body
        };
        self.draw_active(engine, buffer, master, Pane::Compact);
        master
    }

    /// `1 frontend` `2 backend` · `3 app ✕1` · `4 worker ○`, in configured
    /// order so positions stay learnable when the stack is gone.
    fn strip(&self, engine: &dyn TerminalEngine, buffer: &mut Buffer, area: Rect) {
        let active = self.state.active();
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (position, project) in self.projects.iter().enumerate() {
            let master = active == Some(position);
            // The export separates chips, but never right after the accent
            // chip, which already stands apart.
            if position > 0 && active != Some(position - 1) {
                spans.push(Span::styled("·", Style::new().fg(HINT)));
            }
            let status = self.status(engine, project);
            let metadata = self.metadata(engine, project);
            // The narrow fallback hides every pane but the master, so its
            // chips are the only census it has: a pane that has asked for you
            // wears the mark until it is promoted (#97).
            let notified = !master && self.notifies.pending(&project.terminal).is_some();
            let label = format!(
                " {}{} {}{}{} ",
                // The narrow fallback has no stack for the pin to hold, so
                // its chip is where the state is stated until the width
                // comes back (#113).
                if self.state.pinned() == Some(position) {
                    format!("{PIN_MARK} ")
                } else {
                    String::new()
                },
                position + 1,
                project.terminal,
                chip_tag(&status, &metadata),
                if notified {
                    format!(" {NOTIFY_MARK}")
                } else {
                    String::new()
                }
            );
            let style = if master {
                Style::new()
                    .fg(CANVAS)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if notified {
                Style::new().fg(WARNING).bg(CHIP_BG)
            } else {
                Style::new().fg(chip_colour(&status, &metadata)).bg(CHIP_BG)
            };
            spans.push(Span::styled(label, style));
        }
        buffer.set_line(
            area.x + 1,
            area.y,
            &Line::from(spans),
            area.width.saturating_sub(1),
        );
    }

    /// Draws the open overlay, centred on the canvas at the supplement's size.
    ///
    /// The box takes the accent border the master has just given up, and it
    /// clears the cells beneath it: the reviewed dimming rule recedes the
    /// interface, it does not show through the modal.
    fn modal(&self, buffer: &mut Buffer, area: Rect, modal: Modal) {
        let (width, height) = match modal {
            Modal::Help => HELP_SIZE,
            Modal::Quit => QUIT_SIZE,
        };
        let rect = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width: width.min(area.width),
            height: height.min(area.height),
        };
        if rect.width < 2 * PADDING + 4 || rect.height < 3 {
            return;
        }
        Clear.render(rect, buffer);
        let style = Style::new().fg(ACCENT).bg(CANVAS);
        Block::bordered()
            .style(Style::new().bg(CANVAS))
            .border_style(style)
            .render(rect, buffer);

        let left = rect.x + 1 + PADDING;
        let right = rect.x + rect.width - 2 - PADDING;
        let width = right - left + 1;
        let (name, slot, lines) = match modal {
            Modal::Help => ("help", Some("^g ?"), help_lines()),
            Modal::Quit => ("quit", None, quit_lines(self.projects.len())),
        };
        buffer.set_line(
            left - 1,
            rect.y,
            &Line::from(clear_around(
                vec![Span::styled(name, Style::new().fg(ACCENT))],
                style,
            )),
            width + 2,
        );
        if let Some(slot) = slot {
            let slot_width = slot.chars().count() as u16;
            buffer.set_line(
                right - slot_width,
                rect.y,
                &Line::from(clear_around(
                    vec![Span::styled(slot, Style::new().fg(HINT))],
                    style,
                )),
                slot_width + 2,
            );
        }
        for (row, line) in lines.iter().enumerate().take(rect.height as usize - 2) {
            buffer.set_line(left, rect.y + 1 + row as u16, line, width);
        }
    }

    /// Draws the notification toast: the quit confirmation's size and shape,
    /// none of its authority (#97).
    ///
    /// It clears the cells beneath it like a modal, because a half-legible
    /// box says less than none, but it dims nothing, takes no key and leaves
    /// the master its cursor. It lists the batch newest first and counts the
    /// rest, so a fifth notification lengthens no box.
    fn toast(&self, buffer: &mut Buffer, area: Rect, items: &[Toasted<'_>]) {
        let (width, tallest) = TOAST_SIZE;
        // The box is the width of the quit confirmation and as tall as it
        // needs: borders, the row it opens with, a row per notification, the
        // count when there is one, and the row it closes with.
        let listed = items.len().min(TOAST_ROWS);
        let height = (4 + listed as u16 + u16::from(items.len() > listed)).min(tallest);
        let rect = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width: width.min(area.width),
            height: height.min(area.height),
        };
        if rect.width < 2 * PADDING + 4 || rect.height < 3 {
            return;
        }
        Clear.render(rect, buffer);
        let style = Style::new().fg(WARNING).bg(CANVAS);
        Block::bordered()
            .style(Style::new().bg(CANVAS))
            .border_style(style)
            .render(rect, buffer);

        let left = rect.x + 1 + PADDING;
        let right = rect.x + rect.width - 2 - PADDING;
        let width = right - left + 1;
        let name = format!("{} notification{}", items.len(), plural(items.len()));
        buffer.set_line(
            left - 1,
            rect.y,
            &Line::from(clear_around(
                vec![Span::styled(name, Style::new().fg(WARNING))],
                style,
            )),
            width + 2,
        );
        // The way out is stated the way the help overlay states its own, and
        // it is only one of the ways: a click or any deck command clears the
        // batch too, and it settles by itself.
        const DISMISS: &str = "esc";
        let slot = DISMISS.chars().count() as u16;
        buffer.set_line(
            right - slot,
            rect.y,
            &Line::from(clear_around(
                vec![Span::styled(DISMISS, Style::new().fg(HINT))],
                style,
            )),
            slot + 2,
        );

        let hint = Style::new().fg(HINT).bg(CANVAS);
        let separator = Style::new().fg(SEPARATOR).bg(CANVAS);
        // A canvas too short for the box it asked for lists fewer rather
        // than overrunning: the interior, less the rows the box opens and
        // closes with and the row the count may need.
        let listed = listed.min((rect.height as usize).saturating_sub(3));
        for (row, item) in items.iter().take(listed).enumerate() {
            let notify = item.notify;
            let age = age(self.now.unix_millis.saturating_sub(notify.at.unix_millis));
            let taken = item.head.chars().count() + age.chars().count() + 6;
            let spans = vec![
                Span::styled(item.head.clone(), Style::new().fg(MASTER_FG).bg(CANVAS)),
                Span::styled(" · ", separator),
                Span::styled(
                    clip(
                        &notify_text(&notify.kind),
                        (width as usize).saturating_sub(taken),
                    ),
                    Style::new()
                        .fg(match notify.kind {
                            NotifyKind::Attention => MUTED,
                            NotifyKind::Message { .. } => PREVIEW_FG,
                        })
                        .bg(CANVAS),
                ),
                Span::styled(" · ", separator),
                Span::styled(age, hint),
            ];
            buffer.set_line(left, rect.y + 2 + row as u16, &Line::from(spans), width);
        }
        if items.len() > listed {
            buffer.set_line(
                left,
                rect.y + 2 + listed as u16,
                &Line::styled(format!("+{} more", items.len() - listed), hint),
                width,
            );
        }
    }

    /// Draws one bordered pane: border, title chrome, terminal cells, footer.
    fn draw_pane(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        project: &Project,
        position: usize,
        pane: Pane,
    ) {
        let id = &project.terminal;
        let status = self.status(engine, project);
        let metadata = self.metadata(engine, project);
        let exited = matches!(
            status,
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. }
        );

        let (border, background) = match pane {
            Pane::Preview => (IDLE_BORDER, CANVAS),
            Pane::Demoted => (DEMOTED_BORDER, DEMOTED_BG),
            // The demotion idiom's lifted background with the warning border
            // a held pane wears: the same lift, saying "answer me" rather
            // than "that just happened" (#97).
            Pane::Notify => (WARNING, DEMOTED_BG),
            // A held pane turns warning-coloured; its only valid counterpart
            // gets the accent and a quiet lifted background.
            Pane::DragMasterSource | Pane::DragPreviewSource => (WARNING, CANVAS),
            Pane::DragMasterTarget | Pane::DragPreviewTarget => (ACCENT, DEMOTED_BG),
            _ => (ACCENT, CANVAS),
        };
        let border_style = Style::new().fg(border).bg(background);
        Block::bordered()
            .style(Style::new().bg(background))
            .border_style(border_style)
            .render(area, buffer);
        // The scrollbar is drawn into that border rather than over the
        // content, so it costs the terminal no cell and the layout no column
        // (#115). It is the master's alone: a preview says the same thing in
        // words, on its own last row.
        if pane.master() {
            self.scrollbar(
                buffer,
                area,
                position,
                metadata.scrollback,
                engine.frame(id).map_or(0, |frame| frame.size.rows),
                border_style,
            );
        }

        // Content columns, also the columns the title chrome aligns to. A
        // pane below [`MIN_PANE`] has none: it keeps the border it has just
        // been given and says nothing, which is all that fits (#140).
        let Some(content) = pane_content(area) else {
            return;
        };
        let left = content.x;
        let right = content.x + content.width - 1;

        // Title chrome aligns with the content columns and clears one border
        // cell on each side, matching the export's 2-column title inset.
        // The close affordance takes the right end of the title row (#84).
        // The narrow fallback's master has none, for the reason its right
        // slot goes too — and it is the only pane there, so the pointer
        // would be closing the session rather than a pane.
        let close = (pane.base() != Pane::Compact)
            .then(|| close_column(area))
            .flatten();
        // Its column, plus the blank one that keeps the slot off it.
        let reserved = u16::from(close.is_some()) * 2;
        let slot = self.right_slot(project, &status, &metadata, pane);
        let slot_width: u16 = slot.iter().map(|span| span.width() as u16).sum();
        let title = self.title(
            project,
            position,
            &status,
            &metadata,
            pane,
            content.width.saturating_sub(slot_width + 2 + reserved),
        );
        buffer.set_line(
            left - 1,
            area.y,
            &Line::from(clear_around(title, border_style)),
            content.width + 2,
        );
        // Right-aligned, and only where the columns are there: on a pane
        // too narrow for the slot and the blank that keeps it off the title,
        // the placement wrapped past the pane's own left edge (#140).
        if slot_width > 0
            && let Some(column) = right
                .checked_sub(reserved + slot_width)
                .filter(|column| *column > left)
        {
            buffer.set_line(
                column,
                area.y,
                &Line::from(clear_around(slot, border_style)),
                slot_width + 2,
            );
        }
        if let Some(column) = close {
            buffer.set_line(
                column - 1,
                area.y,
                &Line::from(clear_around(
                    vec![Span::styled(
                        CLOSE_AFFORDANCE,
                        Style::new().fg(HINT).bg(background),
                    )],
                    border_style,
                )),
                CLOSE_AFFORDANCE.chars().count() as u16 + 2,
            );
        }

        let default_fg = if pane.master() {
            MASTER_FG
        } else if exited {
            MUTED
        } else {
            PREVIEW_FG
        };
        let mut viewport = content;
        if pane.master() && self.state.scrollback() {
            // The mode owns the pane foot while it is active.
            viewport.height = content.height.saturating_sub(2);
            self.scroll_footer(buffer, content, background);
        } else if exited {
            viewport.height = content.height.saturating_sub(2);
            self.exit_footer(buffer, content, &status, &metadata, background);
        } else if !pane.master() && metadata.scrollback.lines_below > 0 {
            viewport.height = content.height.saturating_sub(1);
            self.scroll_marker(buffer, content, &metadata, background);
        }
        if let Some(terminal) = engine.frame(id) {
            draw_terminal(buffer, viewport, terminal, default_fg, background);
        }
    }

    /// `> {n} {name} · {dot} · {cmd}` for a full-width master, and
    /// `{n} {name} · {dot}` for a preview or a compact master. The declutter
    /// pass took the working directory out of every pane title: the pane
    /// number and name identify the terminal, and the path was the one field
    /// that shrank to nothing on a narrow pane anyway.
    fn title(
        &self,
        project: &Project,
        position: usize,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        pane: Pane,
        budget: u16,
    ) -> Vec<Span<'static>> {
        let (master, wide) = (pane.master(), pane.wide());
        let number = position + 1;
        let name = project.terminal.to_string();
        let separator = if wide { "  ·  " } else { " · " };
        // The canvas keeps the ordinary status dot on a flashing pane (#101):
        // its right slot names the notification, which is the same fact in
        // more words than a mark, so the dot is free to go on saying whether
        // the terminal is running. A folded strip has no such slot, so it
        // keeps the mark the censuses use (#97).
        let (glyph, glyph_colour) = status_glyph(status, metadata);
        let pinned = self.state.pinned() == Some(position);
        let mut spans = Vec::new();
        // Every stack pane declares its disclosure state, folded or not: the
        // open `▾` is the only thing on a fresh frame that says the stack
        // folds at all, and it is the cell the pointer toggles.
        if !master {
            spans.push(Span::styled("▾ ", Style::new().fg(HINT)));
        }
        // The pin leads the row behind that marker, which keeps its own two
        // cells and its column down the stack (#113). It is drawn on the
        // master too: the pin is a property of the terminal, so it is stated
        // wherever that terminal is drawn.
        if pinned {
            spans.push(Span::styled(
                format!("{PIN_MARK} "),
                Style::new().fg(ACCENT),
            ));
        }
        let prefix = if master {
            format!("> {number} ")
        } else {
            format!("{number} ")
        };
        let leading = (usize::from(!master) + usize::from(pinned)) * 2;
        let status_width = separator.chars().count()
            + glyph.chars().count()
            + status_label(status).map_or(0, |label| label.chars().count() + 1);
        let name = clip(
            &name,
            (budget as usize).saturating_sub(leading + prefix.chars().count() + status_width),
        );
        if master {
            spans.push(Span::styled(
                format!("{prefix}{name}"),
                Style::new().fg(ACCENT),
            ));
        } else {
            spans.push(Span::styled(
                format!("{prefix}{name}"),
                Style::new().fg(if matches!(pane.base(), Pane::Demoted | Pane::Notify) {
                    DEMOTED_FG
                } else {
                    PREVIEW_FG
                }),
            ));
        }
        if !name.is_empty() {
            spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));
        }

        spans.push(Span::styled(
            glyph,
            Style::new().fg(if master && glyph_colour == SUCCESS {
                ACCENT
            } else {
                glyph_colour
            }),
        ));
        if let Some(label) = status_label(status) {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::new().fg(glyph_colour),
            ));
        }
        if wide {
            // With the path gone the command is the only elastic field left,
            // so it takes what the budget holds and truncates right-first.
            let fixed: usize = spans.iter().map(|span| span.content.chars().count()).sum();
            let room = (budget as usize).saturating_sub(fixed + separator.chars().count());
            // A separator with nothing after it states nothing, so the two go
            // together — the rule the collapsed strip's tail already follows.
            // Below two columns all the command could say is a bare `…`.
            if room > 1 {
                spans.push(Span::styled(separator, Style::new().fg(SEPARATOR)));
                spans.push(Span::styled(
                    clip(&self.command(project, metadata, pane), room),
                    Style::new().fg(MUTED),
                ));
            }
        }
        spans
    }

    /// Zoom has the width to spend on the process behind the command.
    fn command(&self, project: &Project, metadata: &TerminalMetadata, pane: Pane) -> String {
        let command = project.command.join(" ");
        match (pane.base(), metadata.process) {
            (Pane::Zoomed, Some(process)) => format!(
                "{command} · pid {} · up {}",
                process.pid,
                age(process.uptime.millis)
            ),
            _ => command,
        }
    }

    /// The right-aligned slot: activity meter, idle age, exit age, and the
    /// ZOOM tag. A compact master has no room for it. The master carries no
    /// tag of its own — its border, caret, contrast and status-bar pointer
    /// already say which pane it is.
    fn right_slot(
        &self,
        project: &Project,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        pane: Pane,
    ) -> Vec<Span<'static>> {
        if pane.master() && self.state.scrollback() {
            return vec![Span::styled(
                " SCROLL ",
                Style::new()
                    .fg(CANVAS)
                    .bg(WARNING)
                    .add_modifier(Modifier::BOLD),
            )];
        }
        if pane.base() == Pane::Compact {
            return Vec::new();
        }
        // A flashing pane spends its right slot on what it is asking about
        // (#101): the canvas draws `job done` where the activity meter sits,
        // so the message is named in the title row rather than left to the
        // border colour alone. The meter comes back when the flash settles.
        if pane.base() == Pane::Notify
            && let Some(notify) = self.notifies.pending(&project.terminal)
        {
            return vec![Span::styled(
                clip(&notify_text(&notify.kind), NOTIFY_SLOT),
                Style::new().fg(WARNING),
            )];
        }
        let colour = match pane.base() {
            Pane::Master | Pane::Zoomed => ACCENT,
            Pane::Demoted => MUTED,
            _ => HINT,
        };
        let text = match status {
            TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. } => metadata
                .last_exit_at
                .map(|at| {
                    format!(
                        "{} ago",
                        age(self.now.unix_millis.saturating_sub(at.unix_millis))
                    )
                })
                .unwrap_or_else(|| "exited".to_owned()),
            _ => match metadata.output_idle {
                Some(idle) if idle >= ACTIVE_WINDOW => format!("idle {}", age(idle.millis)),
                Some(idle) => meter(idle.millis),
                None => meter(0),
            },
        };
        match pane.base() {
            Pane::Zoomed => vec![
                Span::styled(
                    " ZOOM ",
                    Style::new()
                        .fg(CANVAS)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {text}"), Style::new().fg(ACCENT)),
            ],
            _ => vec![Span::styled(text, Style::new().fg(colour))],
        }
    }

    /// Rule plus the navigation keys at the pane foot, reusing the accepted
    /// exited-pane pattern. Scroll position is stated once, in the status row,
    /// so the footer carries the keys alone.
    fn scroll_footer(&self, buffer: &mut Buffer, content: Rect, background: Color) {
        if content.height < 2 {
            return;
        }
        footer_rule(buffer, content, background, SEPARATOR);
        let hint = Style::new().fg(HINT).bg(background);
        let key = Style::new().fg(PREVIEW_FG).bg(background);
        let mut spans = Vec::new();
        // The last entry is help, which stays in the status bar.
        for (index, (name, label)) in SCROLLBACK_HINTS[..4].iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" · ", hint));
            }
            spans.push(Span::styled(*name, key));
            spans.push(Span::styled(format!(" {label}"), hint));
        }
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::from(spans),
            content.width,
        );
    }

    /// `↑ 214 lines above · ^g [` on a pane holding a detached viewport. A
    /// promoted terminal keeps its scroll position, so a demoted preview says
    /// how far back it is sitting.
    fn scroll_marker(
        &self,
        buffer: &mut Buffer,
        content: Rect,
        metadata: &TerminalMetadata,
        background: Color,
    ) {
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::styled(
                format!("↑ {} lines above · ^g [", metadata.scrollback.lines_above),
                Style::new().fg(HINT).bg(background),
            ),
            content.width,
        );
    }

    /// Rule plus `exited · code 1 · {time} · r restart` at the pane foot.
    fn exit_footer(
        &self,
        buffer: &mut Buffer,
        content: Rect,
        status: &TerminalStatus,
        metadata: &TerminalMetadata,
        background: Color,
    ) {
        if content.height < 2 {
            return;
        }
        footer_rule(buffer, content, background, ERROR);
        let reason = match status {
            TerminalStatus::Exited { code: Some(code) } => format!("exited · code {code}"),
            TerminalStatus::Exited { code: None } => "exited".to_owned(),
            _ => "failed".to_owned(),
        };
        let mut spans = vec![Span::styled(reason, Style::new().fg(ERROR).bg(background))];
        if let Some(at) = metadata.last_exit_at {
            spans.push(Span::styled(
                format!(" · {} · ", clock(at)),
                Style::new().fg(HINT).bg(background),
            ));
        } else {
            spans.push(Span::styled(" · ", Style::new().fg(HINT).bg(background)));
        }
        spans.push(Span::styled(
            "r restart",
            Style::new().fg(PREVIEW_FG).bg(background),
        ));
        buffer.set_line(
            content.x,
            content.y + content.height - 1,
            &Line::from(spans),
            content.width,
        );
    }

    fn status_row(
        &self,
        engine: &dyn TerminalEngine,
        buffer: &mut Buffer,
        area: Rect,
        rows: u16,
        layout: Layout,
    ) {
        Block::new()
            .style(Style::new().bg(STATUS_BG))
            .render(area, buffer);
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let workspace = Span::styled(
            format!(" {} ", self.workspace),
            Style::new()
                .fg(CANVAS)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        );

        let mut left = if layout == Layout::Narrow {
            // The compact line trades the terminal census for the reason the
            // stack is missing.
            Line::from(vec![
                workspace,
                Span::styled(format!("  {}×{}  ", area.width, rows), hint),
                Span::styled("stack hidden", Style::new().fg(WARNING).bg(STATUS_BG)),
            ])
        } else {
            let total = self.projects.len();
            let active = self
                .state
                .active()
                .and_then(|position| self.projects.get(position).zip(Some(position)))
                .map(|(project, position)| format!("> {} {}", position + 1, project.terminal))
                .unwrap_or_default();
            let mut spans = vec![
                workspace,
                Span::styled(format!("  {total} terminal{}   ", plural(total)), hint),
                Span::styled(active, Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
                // The runtime-add affordance the note puts here: `+ add`,
                // beside the pane summary and clickable (#50 A3). It says
                // the same thing `^g a` does.
                Span::styled("  ·  ", hint),
                Span::styled(ADD_AFFORDANCE, Style::new().fg(ACCENT).bg(STATUS_BG)),
            ];
            // Its label goes the way the key hints' labels go, and for the
            // same reason: below the wide threshold the row would rather
            // spend those columns on the keys themselves. The `+` stays,
            // because it is the only pointer path to the sheet.
            if area.width >= WIDE_COLUMNS {
                spans.push(Span::styled(" add", hint));
            }
            if self.state.scrollback() {
                spans.push(Span::styled("  ·  ", hint));
                spans.push(Span::styled(
                    "SCROLLBACK",
                    Style::new().fg(WARNING).bg(STATUS_BG),
                ));
                if let Some((line, total)) = self.scroll_position(engine) {
                    spans.push(Span::styled(format!("  ·  line {line}/{total}"), hint));
                }
            } else if layout == Layout::Zoom {
                spans.push(Span::styled("  ·  hidden: ", hint));
                spans.extend(self.hidden_summary(engine));
            } else if self.state.collapsed_count() > 0 {
                // The fold census replaces the stack count outright: the
                // export states it alone, with no exited summary after it.
                let collapsed = self.state.collapsed_count();
                spans.push(Span::styled(
                    format!("  ·  {} open  ·  ", self.state.stack().len() - collapsed),
                    hint,
                ));
                spans.push(Span::styled(
                    format!("{collapsed} collapsed"),
                    Style::new().fg(WARNING).bg(STATUS_BG),
                ));
            } else {
                // The stack count went with the declutter pass: the terminal
                // census above it already implies it, and the row would
                // rather spend the columns on what is not routine.
                spans.extend(self.exited_summary(engine));
            }
            Line::from(spans)
        };
        if let Some(notice) = self.state.notice() {
            left.spans.push(Span::styled("  ·  ", hint));
            left.spans.push(Span::styled(
                notice.to_owned(),
                Style::new().fg(WARNING).bg(STATUS_BG),
            ));
        }
        let taken = left.width() as u16;
        buffer.set_line(area.x + PADDING, area.y, &left, area.width);

        // Keys outlive their labels: take the widest form that still clears
        // the left text by a two-column gap.
        for right in self.key_hints(layout) {
            let width = right.width() as u16;
            if taken + width + 2 * PADDING + 2 <= area.width {
                buffer.set_line(area.x + area.width - PADDING - width, area.y, &right, width);
                break;
            }
        }
    }

    /// `line 2217/2431`: the last visible line, and every retained line.
    fn scroll_position(&self, engine: &dyn TerminalEngine) -> Option<(u32, u32)> {
        let project = self
            .state
            .active()
            .and_then(|position| self.projects.get(position))?;
        let frame = engine.frame(&project.terminal)?;
        let scrollback = self.metadata(engine, project).scrollback;
        let line = scrollback.lines_above + u32::from(frame.size.rows);
        Some((line, line + scrollback.lines_below))
    }

    /// `1 exited`, or `all running` when every terminal is alive.
    fn exited_summary(&self, engine: &dyn TerminalEngine) -> Vec<Span<'static>> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let exited = self
            .projects
            .iter()
            .filter(|project| {
                matches!(
                    engine.status(&project.terminal),
                    Some(TerminalStatus::Exited { .. } | TerminalStatus::Failed { .. })
                )
            })
            .count();
        if exited == 0 {
            return vec![Span::styled("  ·  all running", hint)];
        }
        vec![
            Span::styled("  ·  ", hint),
            Span::styled(
                format!("{exited} exited"),
                Style::new().fg(ERROR).bg(STATUS_BG),
            ),
        ]
    }

    /// `2● 3● 4○` — zoom hides the previews, so the status row keeps them
    /// accounted for.
    fn hidden_summary(&self, engine: &dyn TerminalEngine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for position in self.state.stack().iter().copied() {
            let Some(project) = self.projects.get(position) else {
                continue;
            };
            if !spans.is_empty() {
                spans.push(Span::styled(" ", Style::new().fg(HINT).bg(STATUS_BG)));
            }
            let status = self.status(engine, project);
            let metadata = self.metadata(engine, project);
            // The toast is dismissible and the flash settles; the census is
            // where a pane that has asked for you stays accounted for until
            // it is promoted (#97).
            let (glyph, colour) = match self.notifies.pending(&project.terminal) {
                Some(_) => (NOTIFY_MARK, WARNING),
                None => status_glyph(&status, &metadata),
            };
            // Zoom hides the stack, so the census is also where the pin
            // stays stated (#113).
            let pin = if self.state.pinned() == Some(position) {
                PIN_MARK
            } else {
                ""
            };
            spans.push(Span::styled(
                format!("{pin}{}{glyph}", position + 1),
                Style::new().fg(colour).bg(STATUS_BG),
            ));
        }
        spans
    }

    /// Candidate hint rows, widest first. The status bar drops shortcut
    /// labels before keys, per the export's responsive rule, and collapses
    /// outright once the layout goes narrow; the caller takes the first one
    /// that fits.
    fn key_hints(&self, layout: Layout) -> Vec<Line<'static>> {
        // A modal or mode states its own keys: nothing else is reachable while
        // it is open.
        if let Some(modal) = self.state.modal() {
            return vec![modal_hints(modal)];
        }
        if self.state.scrollback() {
            return vec![
                self.hints(&SCROLLBACK_HINTS, layout, true),
                self.hints(&SCROLLBACK_HINTS, layout, false),
            ];
        }
        let collapsed = Line::from(vec![
            Span::styled("^g", Style::new().fg(PREVIEW_FG).bg(STATUS_BG)),
            Span::styled(
                " j/k · N · z · [ · ? · q",
                Style::new().fg(HINT).bg(STATUS_BG),
            ),
        ]);
        if layout == Layout::Narrow {
            return vec![collapsed];
        }
        // While a fold is in play the row advertises the key that undoes it.
        // The export drops the scrollback hint and seats collapse ahead of
        // zoom rather than in the slot scrollback vacated.
        //
        // Zoom hides the stack outright, so the fold is inert and unstated
        // there — the spec's "the zoom status line ... says nothing about
        // collapse". Since #39 every run starts folded, so without this the
        // zoomed row would trade its live `^g [` for an inert `^g c`.
        let mut entries: Vec<(&str, &str)> = vec![KEY_HINTS[0]];
        if self.state.collapsed_count() > 0 && layout != Layout::Zoom {
            entries.push(("^g c", "collapse"));
        }
        // The same rule for the pin, and the explicit unpin #113 asks for:
        // the key is advertised exactly while it would unpin, which is while
        // the master is the pinned pane. Elsewhere the pinned pane wears its
        // own mark and the row keeps its columns.
        if self
            .state
            .pinned()
            .is_some_and(|pinned| self.state.active() == Some(pinned))
        {
            entries.push(("^g p", "unpin"));
        }
        entries.extend_from_slice(&KEY_HINTS[1..]);
        // No collapsed rung here: at four keys the bare row is already
        // narrower than `^g j/k · N · z · [ · ? · q`, so the collapsed form
        // has nothing left to save and belongs to the narrow layout alone.
        vec![
            self.hints(&entries, layout, true),
            self.hints(&entries, layout, false),
        ]
    }

    fn hints(&self, entries: &[(&str, &str)], layout: Layout, labels: bool) -> Line<'static> {
        let hint = Style::new().fg(HINT).bg(STATUS_BG);
        let key = Style::new().fg(PREVIEW_FG).bg(STATUS_BG);
        let mut spans = Vec::new();
        for (index, (name, label)) in entries.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled("  ", hint));
            }
            // An active mode names itself in accent: zoom while zoomed,
            // collapse while any preview is folded, and the pin while one
            // stands — the last two are only in the list at all while they
            // are in play.
            let folded = self.state.collapsed_count() > 0 && *name == "^g c";
            let zoom = layout == Layout::Zoom && *name == "^g z";
            let pinned = *name == "^g p";
            let held = zoom || folded || pinned;
            spans.push(Span::styled(
                (*name).to_owned(),
                if held {
                    Style::new().fg(ACCENT).bg(STATUS_BG)
                } else {
                    key
                },
            ));
            if labels {
                spans.push(Span::styled(
                    format!(" {}", if zoom { "unzoom" } else { *label }),
                    if held { key } else { hint },
                ));
            }
        }
        Line::from(spans)
    }

    fn place_cursor(&self, engine: &dyn TerminalEngine, frame: &mut Frame, master: Rect) {
        // The scrollback viewport is detached from the live tail, so there is
        // no cursor to draw.
        if self.state.scrollback() {
            return;
        }
        let Some(project) = self
            .state
            .active()
            .and_then(|position| self.projects.get(position))
        else {
            return;
        };
        let Some(terminal) = engine.frame(&project.terminal) else {
            return;
        };
        let Cursor {
            column,
            row,
            visible: true,
        } = terminal.cursor
        else {
            return;
        };
        let x = master.x + 1 + PADDING + column;
        let y = master.y + 1 + row;
        if x < (master.x + master.width).saturating_sub(1)
            && y < (master.y + master.height).saturating_sub(1)
        {
            frame.set_cursor_position(Position::new(x, y));
        }
    }

    fn status(&self, engine: &dyn TerminalEngine, project: &Project) -> TerminalStatus {
        engine
            .status(&project.terminal)
            .cloned()
            .unwrap_or(TerminalStatus::Starting)
    }

    fn metadata(&self, engine: &dyn TerminalEngine, project: &Project) -> TerminalMetadata {
        engine
            .metadata(&project.terminal)
            .cloned()
            .unwrap_or_default()
    }
}
