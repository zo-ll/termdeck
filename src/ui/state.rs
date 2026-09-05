//! Selection, zoom, and modal state for the master-and-preview stack.
//!
//! This is the only mutable state the interface owns. It holds no terminal
//! data: everything it stores is a configured position, so terminal identity
//! stays with the engine and survives promotion untouched.

use std::collections::BTreeMap;

use crate::contracts::{ActionCommand, Elapsed, NotifyKind, Project, TerminalId, Timestamp};

/// How long a just-demoted pane keeps its highlight, per the design export's
/// "holds ... for ~1.5s, then settles".
const DEMOTION_WINDOW: Elapsed = Elapsed { millis: 1_500 };

/// How long a notified pane keeps its flash (#97).
///
/// Longer than the demotion highlight it borrows its idiom from, because a
/// notification is something to answer rather than something that just
/// happened, and shorter than the 30s activity window, so it settles rather
/// than becoming another permanent state.
pub const NOTIFY_WINDOW: Elapsed = Elapsed { millis: 4_000 };
/// How long the toast stays up before it settles by itself. Long enough to
/// read four lines, short enough not to sit over the master.
pub const TOAST_WINDOW: Elapsed = Elapsed { millis: 8_000 };
/// Within this window a bell behind an explicit message is the same event
/// told twice, so it is dropped rather than re-armed.
const COALESCE_WINDOW: Elapsed = Elapsed { millis: 2_000 };

/// One pane's pending notification: what it asked for, and when.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notify {
    pub kind: NotifyKind,
    pub at: Timestamp,
}

/// What the terminals have asked for and has not been seen yet (#97).
///
/// One slot per terminal, keyed by identity rather than by configured
/// position, so a notification survives the promotion and the renumbering a
/// close causes. Nothing here reads a clock: `now` arrives with every call,
/// the way [`DeckState::demoted`] already takes it, so a fixture can render
/// any frame of a flash it likes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Notifications {
    pending: BTreeMap<TerminalId, Notify>,
    /// When the batch was last dismissed. The marks it leaves behind outlive
    /// it: a dismissal closes the toast, it does not mean the pane was seen.
    dismissed: Option<Timestamp>,
}

impl Notifications {
    pub const fn new() -> Self {
        Self {
            pending: BTreeMap::new(),
            dismissed: None,
        }
    }

    /// Records what a terminal asked for. Returns whether anything changed.
    ///
    /// `master` is the terminal holding the master frame, which is drawn in
    /// every layout: a pane the user is already looking at has nothing to
    /// announce, so its bells and its messages are dropped here. That is also
    /// the "master bells are ignored" rule, kept in one place rather than at
    /// each of the two transports.
    ///
    /// Precedence and coalescing are the whole of the dedupe story, because
    /// the slot is single: a different explicit message overwrites and
    /// re-arms, while a repeat inside the visible toast window is dropped; a
    /// bell arriving behind a message younger than [`COALESCE_WINDOW`] is the
    /// same event twice and is dropped; any other bell re-arms what stands
    /// without downgrading a message to an attention.
    pub fn record(
        &mut self,
        terminal: &TerminalId,
        master: Option<&TerminalId>,
        kind: NotifyKind,
        now: Timestamp,
    ) -> bool {
        if master == Some(terminal) {
            return false;
        }
        let dismissed = self.dismissed;
        let Some(standing) = self.pending.get_mut(terminal) else {
            self.pending
                .insert(terminal.clone(), Notify { kind, at: now });
            return true;
        };
        match (&standing.kind, &kind) {
            (NotifyKind::Message { .. }, NotifyKind::Message { .. })
                if standing.kind == kind
                    && dismissed.is_none_or(|at| standing.at > at)
                    && elapsed(now, standing.at) < TOAST_WINDOW.millis =>
            {
                false
            }
            (_, NotifyKind::Message { .. }) => {
                *standing = Notify { kind, at: now };
                true
            }
            (NotifyKind::Message { .. }, NotifyKind::Attention)
                if elapsed(now, standing.at) < COALESCE_WINDOW.millis =>
            {
                false
            }
            _ => {
                standing.at = now;
                true
            }
        }
    }

    /// Clears one terminal's slot: the pane has been seen. The session calls
    /// this for whichever terminal holds the master frame, so a promotion
    /// answers a notification with no clock involved at all.
    pub fn clear(&mut self, terminal: &TerminalId) -> bool {
        self.pending.remove(terminal).is_some()
    }

    /// Closes the toast on everything standing now. Later notifications open
    /// it again; the census marks stay either way.
    pub fn dismiss(&mut self, now: Timestamp) -> bool {
        let closed = self
            .pending
            .values()
            .any(|notify| self.undismissed(notify.at));
        self.dismissed = Some(now);
        closed
    }

    /// What this terminal is still asking for, dismissed or not. This is the
    /// census mark: it outlives both the flash and the toast, and only
    /// [`Notifications::clear`] takes it away.
    pub fn pending(&self, terminal: &TerminalId) -> Option<&Notify> {
        self.pending.get(terminal)
    }

    /// Whether this pane is inside its flash window.
    pub fn flashing(&self, terminal: &TerminalId, now: Timestamp) -> bool {
        self.pending
            .get(terminal)
            .and_then(|notify| since(now, notify.at))
            .is_some_and(|elapsed| elapsed < NOTIFY_WINDOW.millis)
    }

    /// Whether this pane still belongs in the toast: never dismissed since it
    /// arrived, and inside the toast's own window.
    pub fn toasting(&self, terminal: &TerminalId, now: Timestamp) -> bool {
        self.pending.get(terminal).is_some_and(|notify| {
            self.undismissed(notify.at)
                && since(now, notify.at).is_some_and(|elapsed| elapsed < TOAST_WINDOW.millis)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Whether any window is still open, and so still owes the interface a
    /// frame to close in.
    ///
    /// A flash and a toast end by themselves, and a loop that only draws
    /// when something happens would leave the last one on screen until the
    /// next keystroke. This is what keeps it drawing until they have both
    /// settled — and no longer, because a notification whose windows have
    /// passed only lives in the censuses, which do not change on their own.
    pub fn settling(&self, now: Timestamp) -> bool {
        let window = NOTIFY_WINDOW.millis.max(TOAST_WINDOW.millis);
        self.pending
            .values()
            .filter_map(|notify| since(now, notify.at))
            .any(|elapsed| elapsed < window)
    }

    /// Whether a notification arrived after the last dismissal.
    fn undismissed(&self, at: Timestamp) -> bool {
        self.dismissed.is_none_or(|dismissed| at > dismissed)
    }
}

/// How long ago a notification arrived, or `None` if it has not arrived yet.
/// A frame drawn before one is a frame without it: that is what lets a
/// fixture hold one set of notifications and render every keyframe of it.
fn since(now: Timestamp, at: Timestamp) -> Option<u64> {
    now.unix_millis.checked_sub(at.unix_millis)
}

fn elapsed(now: Timestamp, at: Timestamp) -> u64 {
    now.unix_millis.saturating_sub(at.unix_millis)
}

/// The narrowest and widest share of the width the master pane may take.
///
/// This is deliberately the same range `defaults.master_ratio` accepts in the
/// configuration, so a split reached by dragging or nudging is always a value
/// the configuration file would also accept and the validator needs no
/// widening. The configuration owns its own copy, because the architecture
/// boundary keeps `src/config` out of the interface; the two copies are held
/// equal by `config::tests::the_interfaces_split_range_is_the_one_this_file_validates`,
/// which fails if either side moves.
pub const MIN_MASTER_RATIO: f64 = 0.55;
pub const MAX_MASTER_RATIO: f64 = 0.85;
/// One press of `^g -` / `^g =`. Six steps span the range end to end, and the
/// grid it lands on is the configuration's own two decimals.
pub const MASTER_RATIO_STEP: f64 = 0.05;
/// The split a deck starts at until the configuration says otherwise, and the
/// configuration's own default.
///
/// It is the top of the range on purpose (#44): a fresh run gives the stack
/// its minimum width and the master everything else, because the previews
/// start folded (#39) and a strip needs no more than its title. Dragging the
/// divider or `^g -` opens the stack back up, and a configured
/// `defaults.master_ratio` still overrides this outright.
pub const DEFAULT_MASTER_RATIO: f64 = MAX_MASTER_RATIO;

/// An overlay that takes focus from the deck. Only one can be open, and it
/// captures every key until it closes: focus stays singular, so the modal
/// takes the accent border and the master gives its own up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Modal {
    Help,
    Quit,
}

/// Which terminal holds the master pane, in which order the rest stack,
/// whether the stack is hidden, and which previews are folded to a title row.
///
/// `order` is a permutation of configured positions: `order[0]` is the master
/// and the remainder is the stack from top to bottom. Promotion swaps the
/// selected terminal with the master, so the old master lands in the slot the
/// new one vacated and configured numbers stay learnable.
///
/// `collapsed` is indexed by configured position rather than by slot, so a
/// fold travels with its terminal: the export's "collapse state persists per
/// workspace and survives promotion". Every terminal starts folded except the
/// one that opens as master, per issue #39.
///
/// `pinned` is the configured position of the terminal held at the top of the
/// stack (#113), or `None`. It is a property of a terminal rather than of a
/// slot: it holds while that terminal is master, and every reordering puts the
/// pinned terminal back at `stack()[0]` the moment it is demoted. Runtime only,
/// like the split ratio — nothing is written back to the configuration.
///
/// `stack_offset` is the index into `stack()` of the first preview the stack
/// column draws. The list can hold more previews than the column has rows, so
/// the column is a window onto it; the renderer clamps this to whatever the
/// current geometry can reach, and nothing here needs to know the geometry.
///
/// `master_ratio` is the share of the width the master pane takes. The
/// configuration seeds it and the divider moves it — by drag or by `^g -` /
/// `^g =`, which are the same adjustment reached two ways. It lives here
/// rather than in the renderer because it is state a gesture changes, and it
/// lives only here: the session holds it for as long as it runs and writes
/// nothing back to the configuration file.
#[derive(Clone, Debug, PartialEq)]
pub struct DeckState {
    order: Vec<usize>,
    collapsed: Vec<bool>,
    pinned: Option<usize>,
    zoomed: bool,
    scrollback: bool,
    modal: Option<Modal>,
    demotion: Option<(usize, Timestamp)>,
    drag: Option<(usize, Option<usize>)>,
    stack_offset: usize,
    master_ratio: f64,
    resizing: bool,
    notice: Option<String>,
}

impl DeckState {
    /// Starts with the first configured terminal as master, per the plan, and
    /// every preview folded to its strip, per issue #39.
    ///
    /// The invariant the fold flags carry is "a pane that has held the master
    /// frame is open": [`DeckState::promote`] clears the flag of whatever it
    /// promotes, and the terminal that opens as master has held the frame
    /// since the first one, so it carries no fold to come back to when it is
    /// demoted. Every other terminal has only ever been a preview, and a
    /// preview now starts folded.
    pub fn new(terminals: usize) -> Self {
        let mut collapsed = vec![true; terminals];
        if let Some(master) = collapsed.first_mut() {
            *master = false;
        }
        Self {
            order: (0..terminals).collect(),
            collapsed,
            pinned: None,
            zoomed: false,
            scrollback: false,
            modal: None,
            demotion: None,
            drag: None,
            stack_offset: 0,
            master_ratio: DEFAULT_MASTER_RATIO,
            resizing: false,
            notice: None,
        }
    }

    /// Seeds the split from the configuration. Out-of-range values are
    /// clamped rather than refused: the configuration has its own validator,
    /// and the interface will not draw a split it cannot also reach.
    #[must_use]
    pub fn with_master_ratio(mut self, ratio: f64) -> Self {
        self.set_master_ratio(ratio);
        self
    }

    /// The share of the width the master pane takes.
    pub fn master_ratio(&self) -> f64 {
        self.master_ratio
    }

    /// Moves the split, clamped to [`MIN_MASTER_RATIO`]..=[`MAX_MASTER_RATIO`].
    /// Returns whether it moved. This is what the divider drag calls, once per
    /// pointer move.
    pub fn set_master_ratio(&mut self, ratio: f64) -> bool {
        let ratio = if ratio.is_finite() {
            ratio.clamp(MIN_MASTER_RATIO, MAX_MASTER_RATIO)
        } else {
            return false;
        };
        let moved = (ratio - self.master_ratio).abs() > f64::EPSILON;
        self.master_ratio = ratio;
        moved
    }

    /// Steps the split by whole [`MASTER_RATIO_STEP`]s: the keyboard half of
    /// the divider, and the same adjustment the drag makes.
    ///
    /// The step lands on the grid rather than adding to whatever the drag
    /// left behind, so the keys always reach the same six splits however the
    /// pointer got there.
    ///
    /// With nothing stacked there is nothing to resize, so the keys are a
    /// silent no-op: the stored split is left alone for the stack a
    /// runtime-add may yet bring back. (The pointer half needs no guard —
    /// with no stack there is no divider column to take hold of.)
    pub fn nudge_master_ratio(&mut self, steps: i32) -> bool {
        if self.stack().is_empty() {
            return false;
        }
        let grid = ((self.master_ratio / MASTER_RATIO_STEP).round() + f64::from(steps))
            * MASTER_RATIO_STEP;
        self.set_master_ratio((grid * 100.0).round() / 100.0)
    }

    /// Whether the divider is being dragged, so it can say so while it moves.
    pub fn resizing(&self) -> bool {
        self.resizing
    }

    /// Holds and releases the divider. The pointer press and release bracket
    /// the drag; the ratio itself is set by the moves in between.
    pub fn set_resizing(&mut self, resizing: bool) {
        self.resizing = resizing;
    }

    /// A short status message for a rejected outer-interface command.
    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Appends a terminal to the deck and returns the position it took.
    ///
    /// It lands at the end of the stack, folded like every other preview
    /// (#39): the master keeps the frame, the order in front of it is
    /// untouched, and the new pane is one more strip at the bottom of the
    /// column. Nothing renumbers, so a promotion made before the addition
    /// still means what it meant.
    pub fn push_terminal(&mut self) -> usize {
        let position = self.collapsed.len();
        self.collapsed.push(true);
        self.order.push(position);
        position
    }

    /// Removes a terminal from the deck and reflows around it (#84).
    ///
    /// Closing renumbers, where adding deliberately does not: the numbers a
    /// pane wears are its position in the live list, so a hole would leave
    /// `^g N` pointing at a terminal that is no longer there. Every position
    /// above the closed one therefore steps down, and the caller drops the
    /// same index from its own project list so the two stay aligned.
    ///
    /// Closing the master promotes whatever was at the head of the stack,
    /// which takes the master frame open (the fold invariant
    /// [`DeckState::new`] states) and live, exactly as a promotion does.
    /// Closing anything else leaves the master, its mode and its fold alone.
    ///
    /// Returns whether a terminal was there to close. The last one can be
    /// closed like any other; what an empty deck means is the session's
    /// business, not the state's.
    pub fn close(&mut self, position: usize) -> bool {
        if position >= self.collapsed.len() {
            return false;
        }
        let was_master = self.active() == Some(position);
        self.collapsed.remove(position);
        self.order.retain(|index| *index != position);
        for index in &mut self.order {
            if *index > position {
                *index -= 1;
            }
        }
        // A pin belongs to a terminal, so it goes when the terminal does and
        // follows it down the renumbering otherwise (#113).
        self.pinned = match self.pinned {
            Some(pinned) if pinned == position => None,
            Some(pinned) if pinned > position => Some(pinned - 1),
            pinned => pinned,
        };
        // A highlight belongs to a pane, so it goes when the pane does and
        // follows it down otherwise.
        self.demotion = match self.demotion {
            Some((demoted, _)) if demoted == position => None,
            Some((demoted, at)) if demoted > position => Some((demoted - 1, at)),
            demotion => demotion,
        };
        // Whatever the pointer was holding is gone or has moved under it.
        self.drag = None;
        if was_master {
            if let Some(master) = self.order.first().copied() {
                self.collapsed[master] = false;
            }
            self.scrollback = false;
        }
        self.hold_pin();
        self.stack_offset = self.stack_offset.min(self.stack().len().saturating_sub(1));
        true
    }

    /// Configured position of the terminal holding the master pane.
    pub fn active(&self) -> Option<usize> {
        self.order.first().copied()
    }

    /// Configured positions of the stacked previews, top to bottom.
    pub fn stack(&self) -> &[usize] {
        self.order.get(1..).unwrap_or_default()
    }

    /// Configured position of the pinned terminal, if one is pinned (#113).
    ///
    /// Whenever it is not the master it is `stack()[0]`; while it is the
    /// master the pin is held rather than spent, and the terminal drops
    /// straight back into the pin slot when it is demoted.
    pub fn pinned(&self) -> Option<usize> {
        self.pinned
    }

    /// `^g p`: pins the terminal holding the master frame, or unpins it if it
    /// is the one already pinned.
    ///
    /// Only one terminal is pinned at a time, so pinning a second moves the
    /// pin. Like `^g c` this is the deck's own arrangement, so it carries no
    /// frozen action; and like the split it lives for the session only.
    ///
    /// Returns whether anything changed — nothing does on an empty deck, which
    /// has no master to pin.
    pub fn toggle_pin(&mut self) -> bool {
        let Some(active) = self.active() else {
            return false;
        };
        self.pinned = (self.pinned != Some(active)).then_some(active);
        self.hold_pin();
        true
    }

    /// Re-establishes the pin's one invariant: a pinned terminal that is not
    /// the master stands at the top of the stack.
    ///
    /// It lifts rather than swaps, so the panes it passes keep their order
    /// among themselves and only shift down one slot — the demoted master
    /// lands where an unpinned one would, one place lower.
    fn hold_pin(&mut self) {
        let Some(pinned) = self.pinned else {
            return;
        };
        let Some(slot) = self.order.iter().position(|index| *index == pinned) else {
            return;
        };
        if slot > 1 {
            self.order.remove(slot);
            self.order.insert(1, pinned);
        }
    }

    pub fn zoomed(&self) -> bool {
        self.zoomed
    }

    /// Index into [`DeckState::stack`] of the first preview the stack column
    /// draws.
    pub fn stack_offset(&self) -> usize {
        self.stack_offset
    }

    /// Scrolls the stack column's window to start at `offset`. The caller has
    /// the rendered geometry and so owns the clamping; this only refuses an
    /// offset with no preview left to show. Returns whether anything moved.
    pub fn set_stack_offset(&mut self, offset: usize) -> bool {
        let offset = offset.min(self.stack().len().saturating_sub(1));
        let moved = offset != self.stack_offset;
        self.stack_offset = offset;
        moved
    }

    /// Whether the preview at this configured position is folded to a single
    /// title row. The master is never collapsed, whatever the stored flag.
    pub fn collapsed(&self, position: usize) -> bool {
        self.active() != Some(position) && self.collapsed.get(position).copied().unwrap_or(false)
    }

    /// How many previews are currently folded. Drives the stack footer and
    /// the status-bar census, both of which the export shows only once at
    /// least one preview is collapsed. The disclosure markers themselves are
    /// unconditional, so they do not consult this.
    pub fn collapsed_count(&self) -> usize {
        self.stack()
            .iter()
            .filter(|position| self.collapsed(**position))
            .count()
    }

    /// Folds or unfolds one preview. The master has no fold to toggle.
    pub fn toggle_collapse(&mut self, position: usize) -> bool {
        if self.active() == Some(position) {
            return false;
        }
        let Some(flag) = self.collapsed.get_mut(position) else {
            return false;
        };
        *flag = !*flag;
        true
    }

    /// `^g c`: the master is always the selected pane, so this collapses every
    /// preview at once, and expands them all again once none is left open.
    /// Previews now start folded (#39), so on a fresh run the first press is
    /// the expand-all half of that toggle.
    pub fn toggle_collapse_all(&mut self) -> bool {
        let stack = self.stack().to_vec();
        if stack.is_empty() {
            return false;
        }
        let collapse = stack.iter().any(|position| !self.collapsed[*position]);
        for position in stack {
            self.collapsed[position] = collapse;
        }
        true
    }

    /// Whether the master pane shows its engine-owned scrollback instead of
    /// live output. Scrollback is a mode of the master pane, not an overlay:
    /// the previews stay live and the stack stays visible.
    pub fn scrollback(&self) -> bool {
        self.scrollback
    }

    /// The open overlay, if any.
    pub fn modal(&self) -> Option<Modal> {
        self.modal
    }

    /// Closes the open overlay. Returns whether one was open.
    pub fn close_modal(&mut self) -> bool {
        self.modal.take().is_some()
    }

    /// Starts a pane drag. Only the master and one preview can form a valid
    /// pair, because promotion is the frozen action that swaps those slots.
    pub fn begin_drag(&mut self, source: usize) -> bool {
        if self.order.contains(&source) {
            self.drag = Some((source, None));
            true
        } else {
            false
        }
    }

    /// Updates the highlighted drop target, rejecting stack-to-stack drops.
    pub fn update_drag(&mut self, target: Option<usize>) {
        let Some((source, _)) = self.drag else {
            return;
        };
        let target = target.filter(|target| {
            *target != source && (*target == self.order[0] || source == self.order[0])
        });
        if let Some(drag) = &mut self.drag {
            drag.1 = target;
        }
    }

    /// The source and valid target when a mouse button is released.
    pub fn finish_drag(&mut self) -> Option<(usize, Option<usize>)> {
        self.drag.take()
    }

    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }

    pub fn dragged(&self) -> Option<usize> {
        self.drag.map(|(source, _)| source)
    }

    pub fn drag_target(&self) -> Option<usize> {
        self.drag.and_then(|(_, target)| target)
    }

    /// The pane demoted by the most recent promotion, while its highlight
    /// lasts. `None` once the window has passed or nothing was promoted yet.
    pub fn demoted(&self, now: Timestamp) -> Option<usize> {
        let (position, at) = self.demotion?;
        let elapsed = now.unix_millis.saturating_sub(at.unix_millis);
        (elapsed < DEMOTION_WINDOW.millis).then_some(position)
    }

    /// Applies one outer-interface action. Returns whether anything changed.
    ///
    /// `SelectPosition` carries a zero-based configured position. The input
    /// layer turns the user's one-based terminal number into this value.
    pub fn apply(&mut self, action: &ActionCommand, projects: &[Project], now: Timestamp) -> bool {
        match action {
            ActionCommand::SelectNext => self.step(1, now),
            ActionCommand::SelectPrevious => self.step(-1, now),
            ActionCommand::SelectPosition(position) => self.promote(*position, now),
            ActionCommand::Promote(terminal) => projects
                .iter()
                .position(|project| &project.terminal == terminal)
                .is_some_and(|position| self.promote(position, now)),
            ActionCommand::ToggleZoom => {
                self.zoomed = !self.zoomed;
                true
            }
            ActionCommand::ToggleScrollback => {
                self.scrollback = !self.scrollback;
                true
            }
            ActionCommand::ShowHelp => self.open(Modal::Help),
            ActionCommand::RequestQuit => self.open(Modal::Quit),
            _ => false,
        }
    }

    fn open(&mut self, modal: Modal) -> bool {
        self.modal = Some(modal);
        true
    }

    /// Promotes a configured position to master and demotes the old master
    /// into the slot the promoted terminal vacated.
    pub fn promote(&mut self, position: usize, now: Timestamp) -> bool {
        let Some(slot) = self.order.iter().position(|index| *index == position) else {
            return false;
        };
        if slot == 0 {
            return false;
        }
        self.demotion = Some((self.order[0], now));
        self.order.swap(0, slot);
        // A collapsed pane expands as it takes the master frame; every other
        // fold is left alone, so the rest of the stack survives the swap.
        if let Some(flag) = self.collapsed.get_mut(position) {
            *flag = false;
        }
        // A promotion is the reordering the pin exists to survive: whatever
        // this swap did to the pinned terminal, it is back at the top of the
        // stack before anything reads the order (#113).
        self.hold_pin();
        // The mode belongs to the pane, and the new master is live.
        self.scrollback = false;
        true
    }

    /// Cycles by configured position so `j`/`k` walk every terminal rather
    /// than bouncing between the master and the top preview.
    fn step(&mut self, direction: isize, now: Timestamp) -> bool {
        let count = self.order.len();
        let Some(active) = self.active() else {
            return false;
        };
        if count < 2 {
            return false;
        }
        let offset = if direction < 0 { count - 1 } else { 1 };
        self.promote((active + offset) % count, now)
    }
}

#[cfg(test)]
mod tests {
    use super::{DeckState, Notifications};
    use crate::{
        contracts::{ActionCommand, Elapsed, NotifyKind, TerminalId, Timestamp},
        ui::fixture,
    };

    const NOW: Timestamp = Timestamp { unix_millis: 0 };

    fn later(millis: u64) -> Timestamp {
        Timestamp {
            unix_millis: millis,
        }
    }

    fn apply(state: &mut DeckState, action: ActionCommand) -> bool {
        state.apply(&action, &fixture::projects(), NOW)
    }

    #[test]
    fn the_first_configured_terminal_starts_as_master() {
        let state = DeckState::new(4);

        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [1, 2, 3]);
        assert!(!state.zoomed());
        assert_eq!(state.demoted(NOW), None);
    }

    #[test]
    fn promotion_swaps_the_old_master_into_the_vacated_slot() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::SelectPosition(1)));

        // The export's screen 02: backend is master and frontend takes the
        // top preview slot backend vacated; app and worker do not move.
        assert_eq!(state.active(), Some(1));
        assert_eq!(state.stack(), [0, 2, 3]);
        assert_eq!(state.demoted(NOW), Some(0));
    }

    #[test]
    fn promoting_a_lower_slot_leaves_the_higher_slots_alone() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::SelectPosition(2)));

        assert_eq!(state.active(), Some(2));
        assert_eq!(state.stack(), [1, 0, 3]);
    }

    #[test]
    fn promotion_keeps_terminal_identity_at_its_configured_position() {
        let projects = fixture::projects();
        let mut state = DeckState::new(projects.len());

        apply(&mut state, ActionCommand::SelectPosition(3));
        apply(&mut state, ActionCommand::SelectPosition(1));

        // Whatever the order, position 3 is still the worker terminal.
        assert_eq!(projects[3].terminal.to_string(), "worker");
        assert_eq!(
            projects[state.active().unwrap()].terminal.to_string(),
            "backend"
        );
        let names: Vec<_> = state
            .stack()
            .iter()
            .map(|position| projects[*position].terminal.to_string())
            .collect();
        assert_eq!(names, ["worker", "app", "frontend"]);
    }

    #[test]
    fn number_actions_promote_the_configured_position() {
        let projects = fixture::projects();
        for (position, name) in ["frontend", "backend", "app", "worker"].iter().enumerate() {
            let mut state = DeckState::new(projects.len());

            state.apply(&ActionCommand::SelectPosition(position), &projects, NOW);

            assert_eq!(
                projects[state.active().unwrap()].terminal.to_string(),
                *name
            );
        }
    }

    /// `j` and the down arrow both produce `SelectNext`; `k` and the up arrow
    /// both produce `SelectPrevious`.
    #[test]
    fn cycling_walks_every_terminal_in_configured_order() {
        let mut state = DeckState::new(4);
        let mut seen = vec![state.active()];

        for _ in 0..4 {
            assert!(apply(&mut state, ActionCommand::SelectNext));
            seen.push(state.active());
        }

        assert_eq!(
            seen,
            [Some(0), Some(1), Some(2), Some(3), Some(0)],
            "next must not bounce between the master and the top preview"
        );
    }

    #[test]
    fn cycling_backwards_wraps_to_the_last_terminal() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::SelectPrevious));

        assert_eq!(state.active(), Some(3));
        assert_eq!(state.stack(), [1, 2, 0]);
    }

    #[test]
    fn promoting_by_terminal_identity_finds_the_configured_position() {
        let projects = fixture::projects();
        let mut state = DeckState::new(projects.len());

        assert!(apply(
            &mut state,
            ActionCommand::Promote(projects[2].terminal.clone())
        ));

        assert_eq!(state.active(), Some(2));
    }

    /// #84: closing the master hands the frame to the top of the stack, and
    /// what is left renumbers so a pane's number is still its place in the
    /// live list.
    #[test]
    fn closing_the_master_promotes_the_top_of_the_stack_and_renumbers() {
        let mut state = DeckState::new(4);

        assert!(state.close(0));

        // The old 2, 3 and 4 are now 1, 2 and 3, in the order they stood in.
        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [1, 2]);
        // The new master holds the frame, so it holds it open.
        assert!(!state.collapsed(0));
        assert!(state.collapsed(1));
        assert!(state.collapsed(2));
    }

    /// A promotion made before the close still means what it meant: identity
    /// travels with the position, and the order is untouched around the hole.
    #[test]
    fn closing_a_preview_leaves_the_master_and_the_order_it_stands_in() {
        let mut state = DeckState::new(4);
        state.apply(&ActionCommand::SelectPosition(3), &fixture::projects(), NOW);
        assert_eq!(state.active(), Some(3));
        assert_eq!(state.stack(), [1, 2, 0]);

        // Close the pane numbered 2, which is above the master's own number.
        assert!(state.close(1));

        assert_eq!(state.active(), Some(2), "the old 4 is the new 3");
        assert_eq!(state.stack(), [1, 0]);
        assert!(!state.collapsed(2), "the master is untouched");
    }

    /// The last one closes like any other. What an empty deck means is the
    /// session's business, so nothing here refuses it.
    #[test]
    fn the_last_terminal_closes_and_leaves_an_empty_deck() {
        let mut state = DeckState::new(1);

        assert!(state.close(0));

        assert_eq!(state.active(), None);
        assert!(state.stack().is_empty());
        assert!(!state.close(0), "and there is nothing left to close");
    }

    /// Everything a pane was carrying goes with it: its demotion highlight,
    /// the drag it was in, and the scrollback mode it held as master. A
    /// scrolled column keeps a window it can still draw.
    #[test]
    fn closing_takes_the_state_the_pane_was_carrying_with_it() {
        let mut state = DeckState::new(4);
        state.apply(&ActionCommand::SelectPosition(1), &fixture::projects(), NOW);
        assert_eq!(state.demoted(NOW), Some(0));
        assert!(state.begin_drag(2));
        state.set_stack_offset(2);
        state.apply(&ActionCommand::ToggleScrollback, &fixture::projects(), NOW);

        // The demoted pane is position 0, closed here from the stack.
        assert!(state.close(0));

        assert_eq!(state.demoted(NOW), None);
        assert_eq!(state.dragged(), None);
        assert_eq!(state.stack_offset(), 1, "clamped to what is still drawable");
        assert!(
            state.scrollback(),
            "the master kept its frame, so it kept its mode"
        );

        // Closing the master itself does return the new one to live output.
        assert!(state.close(state.active().unwrap()));
        assert!(!state.scrollback());
    }

    /// A demotion highlight follows its pane down the renumbering.
    #[test]
    fn a_demotion_highlight_follows_the_pane_it_belongs_to() {
        let mut state = DeckState::new(4);
        state.apply(&ActionCommand::SelectPosition(3), &fixture::projects(), NOW);
        assert_eq!(state.demoted(NOW), Some(0));

        assert!(state.close(3), "close the pane that is not the master");

        assert_eq!(state.demoted(NOW), Some(0), "0 is below the closed 3");

        let mut state = DeckState::new(4);
        state.apply(&ActionCommand::SelectPosition(3), &fixture::projects(), NOW);
        assert!(state.close(1));
        assert_eq!(state.demoted(NOW), Some(0));
        assert!(state.close(0));
        assert_eq!(state.demoted(NOW), None, "the highlighted pane is gone");
    }

    #[test]
    fn selecting_the_master_or_an_unconfigured_position_changes_nothing() {
        let mut state = DeckState::new(4);

        assert!(!apply(&mut state, ActionCommand::SelectPosition(0)));
        assert!(!apply(&mut state, ActionCommand::SelectPosition(9)));
        assert!(!apply(&mut state, ActionCommand::RespawnActive));

        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [1, 2, 3]);
        assert_eq!(state.demoted(NOW), None);
    }

    #[test]
    fn dragging_only_marks_a_master_preview_pair_as_a_valid_drop() {
        let mut state = DeckState::new(4);

        assert!(state.begin_drag(2));
        state.update_drag(Some(3));
        assert_eq!(state.dragged(), Some(2));
        assert_eq!(state.drag_target(), None, "previews do not swap directly");

        state.update_drag(Some(0));
        assert_eq!(state.drag_target(), Some(0));
        assert_eq!(state.finish_drag(), Some((2, Some(0))));
        assert_eq!(state.dragged(), None);
    }

    #[test]
    fn a_single_terminal_has_nothing_to_cycle_to() {
        let mut state = DeckState::new(1);

        assert!(!apply(&mut state, ActionCommand::SelectNext));
        assert!(!apply(&mut state, ActionCommand::SelectPrevious));

        assert_eq!(state.active(), Some(0));
        assert!(state.stack().is_empty());
    }

    #[test]
    fn zoom_toggles_and_survives_promotion() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::ToggleZoom));
        assert!(state.zoomed());
        apply(&mut state, ActionCommand::SelectNext);
        assert!(state.zoomed());

        assert!(apply(&mut state, ActionCommand::ToggleZoom));
        assert!(!state.zoomed());
    }

    #[test]
    fn scrollback_toggles_and_survives_zoom() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::ToggleScrollback));
        assert!(state.scrollback());
        apply(&mut state, ActionCommand::ToggleZoom);
        assert!(state.scrollback(), "zoom and scrollback are independent");

        assert!(apply(&mut state, ActionCommand::ToggleScrollback));
        assert!(!state.scrollback());
    }

    #[test]
    fn promotion_leaves_scrollback_because_the_new_master_is_live() {
        let mut state = DeckState::new(4);

        apply(&mut state, ActionCommand::ToggleScrollback);
        apply(&mut state, ActionCommand::SelectNext);

        assert_eq!(state.active(), Some(1));
        assert!(!state.scrollback());
    }

    #[test]
    fn the_help_and_quit_actions_open_the_modal_they_name() {
        let mut state = DeckState::new(4);

        assert!(apply(&mut state, ActionCommand::ShowHelp));
        assert_eq!(state.modal(), Some(super::Modal::Help));

        // A modal replaces the one before it: focus stays singular.
        assert!(apply(&mut state, ActionCommand::RequestQuit));
        assert_eq!(state.modal(), Some(super::Modal::Quit));

        // The deck behind it is untouched.
        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [1, 2, 3]);
    }

    #[test]
    fn closing_reports_whether_a_modal_was_open() {
        let mut state = DeckState::new(4);

        assert!(!state.close_modal());
        apply(&mut state, ActionCommand::ShowHelp);

        assert!(state.close_modal());
        assert_eq!(state.modal(), None);
        assert!(!state.close_modal());
    }

    /// #39 inverted the default, so the same toggle now opens the stack on
    /// its first press and folds it again on the second.
    #[test]
    fn collapse_all_expands_every_preview_then_folds_them_again() {
        let mut state = DeckState::new(4);
        assert_eq!(state.collapsed_count(), 3, "a fresh deck is all strips");

        assert!(state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 0);

        assert!(state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 3);
        assert!(state.collapsed(1) && state.collapsed(2) && state.collapsed(3));
    }

    /// From a mixed stack the first press finishes the job rather than
    /// inverting it, so the key always means "collapse" until nothing is open.
    #[test]
    fn collapse_all_closes_what_is_left_open_before_it_reopens_anything() {
        let mut state = DeckState::new(4);
        // One preview open against two default folds is a mixed stack.
        state.toggle_collapse(2);
        assert_eq!(state.collapsed_count(), 2);

        assert!(state.toggle_collapse_all());

        assert_eq!(state.collapsed_count(), 3);
    }

    #[test]
    fn the_master_has_no_fold_to_toggle() {
        let mut state = DeckState::new(4);

        assert!(!state.toggle_collapse(0));
        assert!(!state.toggle_collapse(9));
        assert!(!state.collapsed(0), "the master is drawn open either way");
        assert_eq!(
            state.collapsed_count(),
            3,
            "the refused toggles left the stack as it started"
        );
    }

    /// #41: the split is state a gesture moves, and both gestures land it in
    /// the same place. The keys step the grid; the drag sets a value outright.
    #[test]
    fn the_split_steps_by_whole_notches_and_stops_at_the_range_ends() {
        let mut state = DeckState::new(4);
        assert_eq!(
            state.master_ratio(),
            super::MAX_MASTER_RATIO,
            "#44: a fresh deck starts with the stack at its minimum width"
        );
        assert!(
            !state.nudge_master_ratio(1),
            "there is nothing above the top of the range"
        );

        assert!(state.nudge_master_ratio(-1));
        assert_eq!(state.master_ratio(), 0.80);
        assert!(state.nudge_master_ratio(-3));
        assert_eq!(state.master_ratio(), 0.65);

        // Two more steps reach the other end of the range and stop there.
        for _ in 0..2 {
            state.nudge_master_ratio(-1);
        }
        assert_eq!(state.master_ratio(), super::MIN_MASTER_RATIO);
        assert!(
            !state.nudge_master_ratio(-1),
            "the end of the range reports no movement"
        );
        assert_eq!(state.master_ratio(), super::MIN_MASTER_RATIO);

        for _ in 0..99 {
            state.nudge_master_ratio(1);
        }
        assert_eq!(state.master_ratio(), super::MAX_MASTER_RATIO);
    }

    /// A drag lands wherever the pointer is, so the next keypress snaps back
    /// to the grid rather than carrying the remainder along for ever.
    #[test]
    fn a_dragged_split_is_clamped_and_the_keys_return_it_to_the_grid() {
        let mut state = DeckState::new(4);

        assert!(state.set_master_ratio(0.6944));
        assert_eq!(state.master_ratio(), 0.6944);
        assert!(state.nudge_master_ratio(1));
        assert_eq!(state.master_ratio(), 0.75, "0.6944 rounds to 0.70, then up");

        // The range is the configuration's, so neither gesture can leave it.
        assert!(state.set_master_ratio(0.99));
        assert_eq!(state.master_ratio(), super::MAX_MASTER_RATIO);
        assert!(state.set_master_ratio(0.10));
        assert_eq!(state.master_ratio(), super::MIN_MASTER_RATIO);
        assert!(!state.set_master_ratio(f64::NAN), "a bad value is refused");
        assert_eq!(state.master_ratio(), super::MIN_MASTER_RATIO);
    }

    /// The configuration seeds the split, within the same range, and what it
    /// says overrides the minimum-width default a fresh deck starts at (#44).
    #[test]
    fn the_configured_split_seeds_the_deck() {
        assert_eq!(
            DeckState::new(4).with_master_ratio(0.60).master_ratio(),
            0.60
        );
        assert_eq!(
            DeckState::new(4).with_master_ratio(0.70).master_ratio(),
            0.70,
            "a configured split is not the default it replaces"
        );
        assert_eq!(
            DeckState::new(4).with_master_ratio(0.95).master_ratio(),
            super::MAX_MASTER_RATIO,
            "clamped, because the interface will not draw what it cannot reach"
        );
    }

    /// Holding the divider is a mode of its own: it says so while it lasts,
    /// and it leaves the rest of the deck alone.
    #[test]
    fn holding_the_divider_states_itself_and_disturbs_nothing_else() {
        let mut state = DeckState::new(4);
        assert!(!state.resizing());

        state.set_resizing(true);
        assert!(state.resizing());
        state.set_master_ratio(0.60);

        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [1, 2, 3]);
        assert_eq!(state.collapsed_count(), 3);
        assert!(!state.zoomed());

        state.set_resizing(false);
        assert!(!state.resizing());
        assert_eq!(state.master_ratio(), 0.60, "the split it left behind");
    }

    /// #50 A3: a terminal added at runtime lands at the end of the stack,
    /// folded, and disturbs neither the master nor the order in front of it.
    #[test]
    fn a_pushed_terminal_joins_the_stack_without_moving_anything() {
        let mut state = DeckState::new(3);
        apply(&mut state, ActionCommand::SelectPosition(2));
        let order = state.stack().to_vec();
        assert_eq!(state.active(), Some(2));

        let position = state.push_terminal();

        assert_eq!(position, 3, "it took the next configured position");
        assert_eq!(state.active(), Some(2), "the master kept the frame");
        assert_eq!(
            state.stack(),
            [order[0], order[1], 3],
            "and it joined the end of the stack"
        );
        assert!(state.collapsed(3), "folded, like every other new preview");

        // It behaves like any other pane from there.
        apply(&mut state, ActionCommand::SelectPosition(3));
        assert_eq!(state.active(), Some(3));
        assert!(!state.collapsed(3), "a master is never folded");
    }

    #[test]
    fn a_lone_terminal_has_no_stack_to_fold() {
        let mut state = DeckState::new(1);

        assert!(!state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 0);
    }

    /// With nothing stacked there is nothing to resize: the keys leave the
    /// stored split alone for the stack a runtime-add may yet bring back.
    #[test]
    fn the_split_keys_are_inert_with_an_empty_stack() {
        let mut state = DeckState::new(1);

        assert!(!state.nudge_master_ratio(-1));
        assert!(!state.nudge_master_ratio(1));
        assert_eq!(state.master_ratio(), super::DEFAULT_MASTER_RATIO);

        state.push_terminal();
        assert!(state.nudge_master_ratio(-1));
        assert_eq!(state.master_ratio(), 0.80);
    }

    /// The export: "^g 2-4 promotes a collapsed pane directly — it expands as
    /// it takes the master frame."
    #[test]
    fn promoting_a_folded_preview_expands_it_and_leaves_the_others_folded() {
        // Since #39 a fresh deck is already the folded stack this describes.
        let mut state = DeckState::new(4);

        apply(&mut state, ActionCommand::SelectPosition(2));

        assert_eq!(state.active(), Some(2));
        assert!(!state.collapsed(2), "a master is never folded");
        assert!(state.collapsed(1) && state.collapsed(3));
        // The demoted old master lands in the vacated slot, still open: it
        // has held the master frame, so it carries no fold to come back to.
        assert!(!state.collapsed(0));
    }

    /// The export: "collapse state persists per workspace and survives
    /// promotion." Since #39 that cuts both ways: the fold a preview starts
    /// with travels, and so does an expansion the user made.
    #[test]
    fn a_fold_travels_with_its_terminal_across_promotions() {
        let mut state = DeckState::new(5);
        state.toggle_collapse(4);

        apply(&mut state, ActionCommand::SelectPosition(1));
        apply(&mut state, ActionCommand::SelectPosition(2));

        assert!(!state.collapsed(4), "the opened pane stays open");
        assert!(
            state.collapsed(3),
            "the untouched pane is still folded wherever it sits"
        );
        assert_eq!(state.collapsed_count(), 1);
    }

    #[test]
    fn a_fold_survives_zoom() {
        let mut state = DeckState::new(4);
        state.toggle_collapse(2);

        apply(&mut state, ActionCommand::ToggleZoom);
        apply(&mut state, ActionCommand::ToggleZoom);

        assert!(!state.collapsed(2), "the opened preview is still open");
        assert!(state.collapsed(3), "and the folded one is still folded");
    }

    /// The window onto the preview list is state; how far it can travel is
    /// geometry, so the renderer clamps it and this only refuses an offset
    /// with no preview left to show.
    #[test]
    fn the_stack_window_starts_at_the_head_of_the_list_and_moves_by_offset() {
        let mut state = DeckState::new(8);

        assert_eq!(state.stack_offset(), 0);
        assert!(state.set_stack_offset(3));
        assert_eq!(state.stack_offset(), 3);
        assert!(!state.set_stack_offset(3), "nothing moved");

        assert!(state.set_stack_offset(99));
        assert_eq!(state.stack_offset(), 6, "7 previews, so 6 is the last one");
    }

    #[test]
    fn a_deck_with_no_stack_has_no_window_to_move() {
        let mut state = DeckState::new(1);

        assert!(!state.set_stack_offset(4));
        assert_eq!(state.stack_offset(), 0);
    }

    #[test]
    fn the_demotion_highlight_settles_after_its_window() {
        let mut state = DeckState::new(4);

        apply(&mut state, ActionCommand::SelectPosition(1));

        assert_eq!(state.demoted(later(1_499)), Some(0));
        assert_eq!(state.demoted(later(1_500)), None);
        assert_eq!(super::DEMOTION_WINDOW, Elapsed { millis: 1_500 });
    }

    fn message(body: &str) -> NotifyKind {
        NotifyKind::Message {
            title: String::new(),
            body: body.to_owned(),
        }
    }

    fn rich_message(title: &str, body: &str) -> NotifyKind {
        NotifyKind::Message {
            title: title.to_owned(),
            body: body.to_owned(),
        }
    }

    /// #97: one slot per terminal. An explicit message always outranks a
    /// bell and re-arms the flash; a bell behind a fresh message is the same
    /// event told twice; any later bell re-arms what stands without
    /// downgrading the message to a bare attention.
    #[test]
    fn a_message_outranks_a_bell_and_a_bell_behind_one_is_coalesced() {
        let worker = TerminalId::new("worker");
        let mut notifies = Notifications::new();

        assert!(notifies.record(&worker, None, NotifyKind::Attention, NOW));
        assert_eq!(notifies.pending(&worker).map(|n| n.at), Some(NOW));

        // A bell on a bell re-arms the one slot rather than adding a second.
        assert!(notifies.record(&worker, None, NotifyKind::Attention, later(1_000)));
        assert_eq!(notifies.pending(&worker).map(|n| n.at), Some(later(1_000)));

        assert!(notifies.record(&worker, None, message("build done"), later(1_500)));
        assert_eq!(
            notifies.pending(&worker).map(|n| n.kind.clone()),
            Some(message("build done"))
        );

        // Inside the coalesce window the bell that follows a message says
        // nothing the message has not already said.
        assert!(!notifies.record(&worker, None, NotifyKind::Attention, later(2_000)));
        assert_eq!(notifies.pending(&worker).map(|n| n.at), Some(later(1_500)));

        // Past it, the bell is a fresh event: it re-arms the flash and the
        // message it re-arms is still the message.
        assert!(notifies.record(&worker, None, NotifyKind::Attention, later(4_000)));
        let pending = notifies.pending(&worker).unwrap();
        assert_eq!(pending.at, later(4_000));
        assert_eq!(pending.kind, message("build done"));

        // A failing loop repeats this exact message. While its toast is on
        // screen it must not keep pinning the flash; a distinct message is a
        // fresh event and still re-arms normally.
        assert!(!notifies.record(&worker, None, message("build done"), later(4_500)));
        assert_eq!(notifies.pending(&worker).map(|n| n.at), Some(later(4_000)));
        assert!(!notifies.flashing(&worker, later(8_000)));
        assert!(notifies.toasting(&worker, later(8_000)));

        assert!(notifies.record(&worker, None, message("tests failed"), later(8_000)));
        assert_eq!(notifies.pending(&worker).map(|n| n.at), Some(later(8_000)));
        assert!(notifies.flashing(&worker, later(8_000)));
    }

    #[test]
    fn an_identical_rich_message_inside_the_toast_window_does_not_rearm() {
        let worker = TerminalId::new("worker");
        let mut notifies = Notifications::new();
        let completion = rich_message("cargo build", "exit 1 · 84s");

        assert!(notifies.record(&worker, None, completion.clone(), NOW));
        assert!(!notifies.record(&worker, None, completion, later(5_000)));
        assert_eq!(notifies.pending(&worker).map(|notify| notify.at), Some(NOW));
        assert!(!notifies.flashing(&worker, later(5_000)));
        assert!(notifies.toasting(&worker, later(5_000)));

        assert!(notifies.record(
            &worker,
            None,
            rich_message("cargo test", "exit 1 · 84s"),
            later(5_000)
        ));
        assert_eq!(
            notifies.pending(&worker).map(|notify| notify.at),
            Some(later(5_000))
        );
    }

    /// The master frame is drawn in every layout, so the pane holding it has
    /// nothing to announce: that is the "master bells are ignored" rule, and
    /// it covers explicit messages for the same reason.
    #[test]
    fn the_pane_holding_the_master_frame_announces_nothing() {
        let master = TerminalId::new("frontend");
        let mut notifies = Notifications::new();

        assert!(!notifies.record(&master, Some(&master), NotifyKind::Attention, NOW));
        assert!(!notifies.record(&master, Some(&master), message("done"), NOW));

        assert!(notifies.is_empty());
    }

    /// Promotion answers a notification, and needs no clock to do it.
    #[test]
    fn promotion_clears_the_slot_whatever_the_time_is() {
        let worker = TerminalId::new("worker");
        let mut notifies = Notifications::new();
        notifies.record(&worker, None, NotifyKind::Attention, NOW);

        assert!(notifies.clear(&worker));

        assert!(!notifies.clear(&worker), "there is nothing left to clear");
        assert!(notifies.pending(&worker).is_none());
    }

    /// Both windows are read off the clock the caller hands in, so a fixture
    /// can render any frame of a flash it likes.
    #[test]
    fn the_flash_settles_before_the_toast_does() {
        let worker = TerminalId::new("worker");
        let mut notifies = Notifications::new();
        notifies.record(&worker, None, NotifyKind::Attention, NOW);

        assert!(notifies.settling(NOW));
        assert!(notifies.flashing(&worker, later(3_999)));
        assert!(!notifies.flashing(&worker, later(4_000)));
        assert!(notifies.toasting(&worker, later(7_999)));
        assert!(!notifies.toasting(&worker, later(8_000)));
        assert_eq!(super::NOTIFY_WINDOW, Elapsed { millis: 4_000 });
        assert_eq!(super::TOAST_WINDOW, Elapsed { millis: 8_000 });

        // The mark outlives both: only being seen takes it away. Once the
        // windows have passed nothing changes on its own any more, so the
        // loop has nothing left to redraw for.
        assert!(notifies.pending(&worker).is_some());
        assert!(!notifies.settling(later(8_000)));
    }

    /// A dismissal closes the batch that stands, and nothing else: a later
    /// notification opens the toast again, and the census marks never go.
    #[test]
    fn a_dismissal_closes_the_batch_and_leaves_the_marks() {
        let worker = TerminalId::new("worker");
        let app = TerminalId::new("app");
        let mut notifies = Notifications::new();
        notifies.record(&worker, None, NotifyKind::Attention, NOW);

        assert!(notifies.dismiss(later(1_000)));
        assert!(!notifies.toasting(&worker, later(1_000)));
        assert!(notifies.pending(&worker).is_some(), "the mark stays");
        assert!(!notifies.dismiss(later(2_000)), "nothing left to close");

        notifies.record(&app, None, message("failed"), later(3_000));
        assert!(notifies.toasting(&app, later(3_000)));
        assert!(
            !notifies.toasting(&worker, later(3_000)),
            "the dismissed one stays dismissed"
        );
    }

    /// #113: the pin is a property of a terminal. Whenever the pinned one is
    /// not the master it is the top of the stack, and promotions around it
    /// never move it.
    #[test]
    fn a_pinned_terminal_holds_the_top_of_the_stack_across_promotions() {
        let mut state = DeckState::new(4);
        apply(&mut state, ActionCommand::SelectPosition(3));
        assert!(state.toggle_pin(), "worker is master and now pinned");
        assert_eq!(state.pinned(), Some(3));
        assert_eq!(state.stack(), [1, 2, 0], "a pinned master holds no slot");

        // Demoting it drops it straight into the pin slot, not into the slot
        // the promoted pane vacated.
        apply(&mut state, ActionCommand::SelectPosition(0));
        assert_eq!(state.active(), Some(0));
        assert_eq!(state.stack(), [3, 1, 2]);

        // And every promotion around it leaves the slot alone.
        for position in [2, 1, 2] {
            apply(&mut state, ActionCommand::SelectPosition(position));
            assert_eq!(state.stack()[0], 3, "promoting {position} moved the pin");
        }
        // Cycling walks configured positions, so it reaches the pinned pane
        // like any other, and puts it back when it leaves.
        apply(&mut state, ActionCommand::SelectNext);
        apply(&mut state, ActionCommand::SelectNext);
        assert_eq!(state.stack()[0], 3);
    }

    /// The pin is held rather than spent while its terminal holds the frame,
    /// so the round trip is the whole of the model.
    #[test]
    fn promoting_the_pinned_terminal_holds_the_pin_rather_than_spending_it() {
        let mut state = DeckState::new(4);
        apply(&mut state, ActionCommand::SelectPosition(2));
        state.toggle_pin();

        apply(&mut state, ActionCommand::SelectPosition(2));
        assert_eq!(
            state.active(),
            Some(2),
            "already master: nothing to promote"
        );
        apply(&mut state, ActionCommand::SelectPosition(1));
        assert_eq!(state.stack(), [2, 0, 3], "back in the pin slot");
        apply(&mut state, ActionCommand::SelectPosition(2));
        assert_eq!(state.active(), Some(2), "and promotable again");
        assert_eq!(state.pinned(), Some(2), "with the pin still on it");
    }

    /// One pin at a time: the key unpins the pane it is on, and pinning a
    /// second terminal moves the pin rather than adding one.
    #[test]
    fn the_pin_key_toggles_the_master_and_only_one_pin_stands() {
        let mut state = DeckState::new(4);

        assert!(state.toggle_pin());
        assert_eq!(state.pinned(), Some(0));
        assert!(state.toggle_pin(), "the same key unpins it");
        assert_eq!(state.pinned(), None);
        assert_eq!(state.stack(), [1, 2, 3]);

        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(2));
        assert_eq!(state.stack(), [0, 1, 3], "the old pin holds its slot");
        state.toggle_pin();
        assert_eq!(state.pinned(), Some(2), "the pin moved to the new master");
        apply(&mut state, ActionCommand::SelectPosition(1));
        assert_eq!(state.stack(), [2, 0, 3], "and the old one is ordinary");
    }

    /// A pin is a property of a terminal, so closing that terminal takes it,
    /// and closing anything else renumbers it with everything else (#84).
    #[test]
    fn closing_takes_the_pin_with_its_terminal_and_renumbers_the_rest() {
        let mut state = DeckState::new(4);
        apply(&mut state, ActionCommand::SelectPosition(3));
        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(0));
        assert_eq!(state.stack(), [3, 1, 2]);

        assert!(state.close(1), "the pane below the pinned one goes");
        assert_eq!(state.pinned(), Some(2), "the old 4 is the new 3");
        assert_eq!(state.stack(), [2, 1]);

        assert!(state.close(2));
        assert_eq!(state.pinned(), None, "the pinned terminal is gone");
        assert_eq!(state.stack(), [1]);
    }

    /// Closing the master hands the frame on and the pin holds its slot in
    /// whatever is left.
    #[test]
    fn closing_the_master_leaves_the_pin_at_the_top_of_what_remains() {
        let mut state = DeckState::new(4);
        apply(&mut state, ActionCommand::SelectPosition(3));
        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(0));

        assert!(state.close(0), "close the master, which is not pinned");

        assert_eq!(state.active(), Some(2), "the old worker is the new 3");
        assert_eq!(
            state.pinned(),
            Some(2),
            "and it took the frame it was next to"
        );

        // Promote something else: the pin slot is waiting.
        apply(&mut state, ActionCommand::SelectPosition(0));
        assert_eq!(state.stack(), [2, 1]);
    }

    /// The modes the pin has to survive: zoom hides the stack, the narrow
    /// fallback has none, scrollback is a mode of the master, and a fold is
    /// disclosure rather than order. None of them touches the slot.
    #[test]
    fn zoom_scrollback_and_collapse_leave_the_pin_where_it_stands() {
        let mut state = DeckState::new(4);
        apply(&mut state, ActionCommand::SelectPosition(2));
        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(0));
        assert_eq!(state.stack(), [2, 1, 3]);

        apply(&mut state, ActionCommand::ToggleZoom);
        assert_eq!(state.stack(), [2, 1, 3], "zoom only hides it");
        apply(&mut state, ActionCommand::ToggleZoom);
        apply(&mut state, ActionCommand::ToggleScrollback);
        assert_eq!(state.stack(), [2, 1, 3]);
        assert!(state.toggle_pin(), "and the key still reaches the master");
        assert_eq!(state.pinned(), Some(0), "the pin moved to the master");
        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(2));
        state.toggle_pin();
        apply(&mut state, ActionCommand::SelectPosition(0));

        // A fold travels with its terminal and says nothing about order: the
        // pinned pane folds and unfolds like any other and keeps its slot
        // either way.
        assert!(state.toggle_collapse(2));
        assert!(state.collapsed(2), "the pinned pane folds like any other");
        assert_eq!(state.stack(), [2, 1, 3]);
        state.toggle_collapse_all();
        assert!(!state.collapsed(2), "and opens with the rest of them");
        assert_eq!(state.stack(), [2, 1, 3]);
    }

    /// Pinning is allowed before there is a stack to hold: a runtime-added
    /// terminal lands at the end and the pin slot is waiting for it.
    #[test]
    fn a_pin_survives_a_runtime_add_and_an_empty_deck_has_none_to_take() {
        let mut state = DeckState::new(1);
        assert!(state.toggle_pin());
        assert_eq!(state.pinned(), Some(0));
        assert!(state.stack().is_empty(), "nothing to hold yet");

        assert_eq!(state.push_terminal(), 1);
        assert_eq!(state.stack(), [1], "the new pane lands behind the master");
        apply(&mut state, ActionCommand::SelectPosition(1));
        assert_eq!(state.stack(), [0], "and the pin takes the slot it made");

        let mut empty = DeckState::new(0);
        assert!(!empty.toggle_pin(), "no master, nothing to pin");
        assert_eq!(empty.pinned(), None);
    }
}
