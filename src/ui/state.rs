//! Selection, zoom, and modal state for the master-and-preview stack.
//!
//! This is the only mutable state the interface owns. It holds no terminal
//! data: everything it stores is a configured position, so terminal identity
//! stays with the engine and survives promotion untouched.

use crate::contracts::{ActionCommand, Elapsed, Project, Timestamp};

/// How long a just-demoted pane keeps its highlight, per the design export's
/// "holds ... for ~1.5s, then settles".
const DEMOTION_WINDOW: Elapsed = Elapsed { millis: 1_500 };

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
/// workspace and survives promotion".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeckState {
    order: Vec<usize>,
    collapsed: Vec<bool>,
    zoomed: bool,
    scrollback: bool,
    modal: Option<Modal>,
    demotion: Option<(usize, Timestamp)>,
}

impl DeckState {
    /// Starts with the first configured terminal as master, per the plan.
    pub fn new(terminals: usize) -> Self {
        Self {
            order: (0..terminals).collect(),
            collapsed: vec![false; terminals],
            zoomed: false,
            scrollback: false,
            modal: None,
            demotion: None,
        }
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

    /// Whether the preview at this configured position is folded to a single
    /// title row. The master is never collapsed, whatever the stored flag.
    pub fn collapsed(&self, position: usize) -> bool {
        self.active() != Some(position) && self.collapsed.get(position).copied().unwrap_or(false)
    }

    /// How many previews are currently folded. Drives the disclosure markers,
    /// the stack footer, and the status-bar census, all of which the export
    /// shows only once at least one preview is collapsed.
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

    #[test]
    fn collapse_all_folds_every_preview_then_expands_them_again() {
        let mut state = DeckState::new(4);

        assert!(state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 3);
        assert!(state.collapsed(1) && state.collapsed(2) && state.collapsed(3));

        assert!(state.toggle_collapse_all());
        assert_eq!(state.collapsed_count(), 0);
    }

    /// From a mixed stack the first press finishes the job rather than
    /// inverting it, so the key always means "collapse" until nothing is open.
    #[test]
    fn collapse_all_closes_what_is_left_open_before_it_reopens_anything() {
        let mut state = DeckState::new(4);
        state.toggle_collapse(2);

        assert!(state.toggle_collapse_all());

        assert_eq!(state.collapsed_count(), 3);
    }

    #[test]
    fn the_master_has_no_fold_to_toggle() {
        let mut state = DeckState::new(4);

        assert!(!state.toggle_collapse(0));
        assert!(!state.toggle_collapse(9));
        assert_eq!(state.collapsed_count(), 0);
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
        let mut state = DeckState::new(4);
        state.toggle_collapse_all();

        apply(&mut state, ActionCommand::SelectPosition(2));

        assert_eq!(state.active(), Some(2));
        assert!(!state.collapsed(2), "a master is never folded");
        assert!(state.collapsed(1) && state.collapsed(3));
        // The demoted old master lands in the vacated slot, still open.
        assert!(!state.collapsed(0));
    }

    /// The export: "collapse state persists per workspace and survives
    /// promotion."
    #[test]
    fn a_fold_travels_with_its_terminal_across_promotions() {
        let mut state = DeckState::new(4);
        state.toggle_collapse(3);

        apply(&mut state, ActionCommand::SelectPosition(1));
        apply(&mut state, ActionCommand::SelectPosition(2));

        assert!(
            state.collapsed(3),
            "worker is still folded wherever it sits"
        );
        assert_eq!(state.collapsed_count(), 1);
    }

    #[test]
    fn a_fold_survives_zoom() {
        let mut state = DeckState::new(4);
        state.toggle_collapse(2);

        apply(&mut state, ActionCommand::ToggleZoom);
        apply(&mut state, ActionCommand::ToggleZoom);

        assert!(state.collapsed(2));
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
