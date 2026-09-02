//! Selection, zoom, and modal state for the master-and-preview stack.
//!
//! This is the only mutable state the interface owns. It holds no terminal
//! data: everything it stores is a configured position, so terminal identity
//! stays with the engine and survives promotion untouched.

use crate::contracts::{ActionCommand, Elapsed, Project, Timestamp};

/// How long a just-demoted pane keeps its highlight, per the design export's
/// "holds ... for ~1.5s, then settles".
const DEMOTION_WINDOW: Elapsed = Elapsed { millis: 1_500 };

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
    zoomed: bool,
    scrollback: bool,
    modal: Option<Modal>,
    demotion: Option<(usize, Timestamp)>,
    drag: Option<(usize, Option<usize>)>,
    stack_offset: usize,
    master_ratio: f64,
    resizing: bool,
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
            zoomed: false,
            scrollback: false,
            modal: None,
            demotion: None,
            drag: None,
            stack_offset: 0,
            master_ratio: DEFAULT_MASTER_RATIO,
            resizing: false,
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
    pub fn nudge_master_ratio(&mut self, steps: i32) -> bool {
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

    /// Configured position of the terminal holding the master pane.
    pub fn active(&self) -> Option<usize> {
        self.order.first().copied()
    }

    /// Configured positions of the stacked previews, top to bottom.
    pub fn stack(&self) -> &[usize] {
        self.order.get(1..).unwrap_or_default()
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
    /// `SelectPosition` carries a zero-based configured position: the `1..4`
    /// keys select `0..3`. `RespawnActive` needs the engine, so it is not this
    /// type's concern and is ignored.
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
    use super::DeckState;
    use crate::{
        contracts::{ActionCommand, Elapsed, Timestamp},
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

    #[test]
    fn a_lone_terminal_has_no_stack_to_fold() {
        let mut state = DeckState::new(1);

        assert!(!state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 0);
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
}
