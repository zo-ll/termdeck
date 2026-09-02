//! Key handling for the outer interface.
//!
//! Every binding is reached through the `ctrl+g` prefix. Most map onto a frozen
//! [`ActionCommand`]; `^g c` and `^g pgup`/`^g pgdn` are purely the deck's own
//! geometry, so the first applies straight to [`DeckState`] and the second
//! comes back for the caller that holds the rendered stack. Anything the deck
//! can carry out itself — selection, zoom, collapse, scrollback, the modals —
//! [`Input::press`] applies to the [`DeckState`] it is given; what is left over
//! needs the engine, the process, or the geometry, and comes back as a
//! [`Reaction`] for the caller.
//!
//! Keys arrive already decoded: this module names no terminal backend, so the
//! event loop can be written against any of them.

use crate::contracts::{
    ActionCommand, InputCommand, Project, ScrollCommand, Timestamp, UserCommand,
};

use super::state::{DeckState, Modal};

/// One key press. Modifiers other than control are carried by the character
/// itself, so `shift+g` arrives as `Char('G')`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Enter,
    Tab,
    Backspace,
    Escape,
}

impl Key {
    /// What the key sends to a terminal when it is forwarded.
    fn bytes(self) -> Vec<u8> {
        match self {
            Self::Char(character) => character.to_string().into_bytes(),
            // `ctrl+a` is 0x01, so the prefix's own literal byte is 0x07.
            Self::Ctrl(character) => vec![character.to_ascii_uppercase() as u8 & 0x1f],
            Self::Up => b"\x1b[A".to_vec(),
            Self::Down => b"\x1b[B".to_vec(),
            Self::Right => b"\x1b[C".to_vec(),
            Self::Left => b"\x1b[D".to_vec(),
            Self::PageUp => b"\x1b[5~".to_vec(),
            Self::PageDown => b"\x1b[6~".to_vec(),
            Self::Enter => b"\r".to_vec(),
            Self::Tab => b"\t".to_vec(),
            Self::Backspace => vec![0x7f],
            Self::Escape => vec![0x1b],
        }
    }
}

/// What one key press leaves for the caller once the deck has taken its part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reaction {
    /// Forward to the active terminal.
    Send(UserCommand),
    /// Move the active terminal's engine-owned scrollback viewport.
    Scroll(ScrollCommand),
    /// [`ActionCommand::RespawnActive`]: restarting a process is the engine's
    /// business, not the deck's.
    Respawn,
    /// Page the preview stack's window by whole screens, negative for up. How
    /// far one page reaches is rendered geometry, which only the caller holds,
    /// so it applies this through [`super::Deck::stack_window`].
    PageStack(isize),
    /// Quit confirmed at the confirmation modal that
    /// [`ActionCommand::RequestQuit`] opened.
    Quit,
}

/// The prefix state machine.
///
/// It holds the only thing a key press needs that the rendered state does not
/// carry: whether the previous key was the prefix, and how far one page key
/// moves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Input {
    prefix: bool,
    page: u16,
}

impl Input {
    /// `page` is the master viewport's height in rows, so `pgup`/`pgdn` move
    /// exactly one screen of scrollback.
    pub fn new(page: u16) -> Self {
        Self {
            prefix: false,
            page,
        }
    }

    /// Follows the master pane across a resize.
    pub fn set_page(&mut self, rows: u16) {
        self.page = rows;
    }

    /// Handles one key press, applying whatever it means to `deck`.
    pub fn press(
        &mut self,
        key: Key,
        deck: &mut DeckState,
        projects: &[Project],
        now: Timestamp,
    ) -> Option<Reaction> {
        if std::mem::take(&mut self.prefix) {
            return self.command(key, deck, projects, now);
        }
        // A modal owns every key while it is open, so the prefix is not
        // reachable from inside one.
        match deck.modal() {
            Some(Modal::Help) => {
                if key == Key::Escape {
                    deck.close_modal();
                }
                None
            }
            Some(Modal::Quit) => match key {
                Key::Char('y') => {
                    deck.close_modal();
                    Some(Reaction::Quit)
                }
                Key::Char('n') | Key::Escape => {
                    deck.close_modal();
                    None
                }
                _ => None,
            },
            None if key == Key::Ctrl('g') => {
                self.prefix = true;
                None
            }
            None if deck.scrollback() => self.scroll(key, deck, projects, now),
            None => Some(Reaction::Send(UserCommand::Input(InputCommand::Bytes(
                key.bytes(),
            )))),
        }
    }

    /// The second key of the prefix. An unbound one ends the prefix without
    /// reaching the shell, so a mistyped command never becomes stray input.
    fn command(
        &mut self,
        key: Key,
        deck: &mut DeckState,
        projects: &[Project],
        now: Timestamp,
    ) -> Option<Reaction> {
        let action = match key {
            // Doubling the prefix is the way to type it.
            Key::Ctrl('g') => {
                return Some(Reaction::Send(UserCommand::Input(InputCommand::Bytes(
                    Key::Ctrl('g').bytes(),
                ))));
            }
            Key::Char('j') | Key::Down => ActionCommand::SelectNext,
            Key::Char('k') | Key::Up => ActionCommand::SelectPrevious,
            // The `1..4` keys select the zero-based configured positions.
            Key::Char(digit @ '1'..='9') => {
                ActionCommand::SelectPosition(digit as usize - '1' as usize)
            }
            Key::Char('z') => ActionCommand::ToggleZoom,
            // The stack is a window onto a list that can be longer than the
            // column. Paging it is the deck's own geometry, so it carries no
            // frozen action either; the page keys are free under the prefix
            // because scrollback reads them unprefixed.
            Key::PageUp => return Some(Reaction::PageStack(-1)),
            Key::PageDown => return Some(Reaction::PageStack(1)),
            // Collapse is the deck's own geometry, so it needs no engine and
            // carries no frozen action of its own.
            Key::Char('c') => {
                deck.toggle_collapse_all();
                return None;
            }
            // The keyboard half of the split divider (#41): the same
            // adjustment the pointer makes by dragging it, in whole steps.
            // `=` widens the master, `-` narrows it; the shifted twins are
            // taken too, because `+` is what a hand reaches for.
            Key::Char('=' | '+') => {
                deck.nudge_master_ratio(1);
                return None;
            }
            Key::Char('-' | '_') => {
                deck.nudge_master_ratio(-1);
                return None;
            }
            Key::Char('[') => ActionCommand::ToggleScrollback,
            Key::Char('r') => ActionCommand::RespawnActive,
            Key::Char('?') => ActionCommand::ShowHelp,
            Key::Char('q') => ActionCommand::RequestQuit,
            _ => return None,
        };
        match action {
            ActionCommand::RespawnActive => Some(Reaction::Respawn),
            action => {
                deck.apply(&action, projects, now);
                None
            }
        }
    }

    /// Scrollback captures the navigation keys and swallows the rest: nothing
    /// reaches the hidden live shell until `esc` returns to it.
    fn scroll(
        &self,
        key: Key,
        deck: &mut DeckState,
        projects: &[Project],
        now: Timestamp,
    ) -> Option<Reaction> {
        let command = match key {
            Key::Char('j') | Key::Down => ScrollCommand::Down(1),
            Key::Char('k') | Key::Up => ScrollCommand::Up(1),
            Key::PageDown => ScrollCommand::Down(self.page),
            Key::PageUp => ScrollCommand::Up(self.page),
            // `G` is the live tail; `g` is as far back as the engine retains,
            // which it clamps this request to.
            Key::Char('G') => ScrollCommand::Bottom,
            Key::Char('g') => ScrollCommand::Up(u16::MAX),
            Key::Escape => {
                deck.apply(&ActionCommand::ToggleScrollback, projects, now);
                return None;
            }
            _ => return None,
        };
        Some(Reaction::Scroll(command))
    }
}

#[cfg(test)]
mod tests {
    use super::{Input, Key, Reaction};
    use crate::{
        contracts::{InputCommand, ScrollCommand, Timestamp, UserCommand},
        ui::{DeckState, Modal, fixture},
    };

    const NOW: Timestamp = Timestamp { unix_millis: 0 };
    /// The reference master viewport, so a page key moves one screen.
    const PAGE: u16 = 38;

    /// A four-terminal deck and the key handler driving it.
    struct Session {
        input: Input,
        deck: DeckState,
    }

    impl Session {
        fn new() -> Self {
            Self {
                input: Input::new(PAGE),
                deck: DeckState::new(4),
            }
        }

        fn press(&mut self, key: Key) -> Option<Reaction> {
            self.input
                .press(key, &mut self.deck, &fixture::projects(), NOW)
        }

        /// The prefix, which is always swallowed, and then one command key.
        fn command(&mut self, key: Key) -> Option<Reaction> {
            assert_eq!(self.press(Key::Ctrl('g')), None, "the prefix waits");
            self.press(key)
        }
    }

    fn sent(bytes: &[u8]) -> Option<Reaction> {
        Some(Reaction::Send(UserCommand::Input(InputCommand::Bytes(
            bytes.to_vec(),
        ))))
    }

    #[test]
    fn unprefixed_keys_reach_the_active_terminal() {
        let mut session = Session::new();

        assert_eq!(session.press(Key::Char('l')), sent(b"l"));
        assert_eq!(session.press(Key::Enter), sent(b"\r"));
        assert_eq!(session.press(Key::Up), sent(b"\x1b[A"));
        assert_eq!(session.press(Key::Ctrl('c')), sent(&[0x03]));
        assert_eq!(session.press(Key::Escape), sent(&[0x1b]));
        // Nothing the terminal sees changed the deck.
        assert_eq!(session.deck.active(), Some(0));
    }

    #[test]
    fn the_prefix_makes_the_next_key_a_command() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Char('j')), None);
        assert_eq!(session.deck.active(), Some(1));
        assert_eq!(session.command(Key::Char('k')), None);
        assert_eq!(session.deck.active(), Some(0));
        // Arrows are the same two commands.
        assert_eq!(session.command(Key::Down), None);
        assert_eq!(session.deck.active(), Some(1));
        assert_eq!(session.command(Key::Up), None);
        assert_eq!(session.deck.active(), Some(0));
    }

    #[test]
    fn the_number_keys_promote_the_configured_position() {
        let mut session = Session::new();

        session.command(Key::Char('3'));

        assert_eq!(session.deck.active(), Some(2));
        assert_eq!(session.deck.stack(), [1, 0, 3]);
    }

    #[test]
    fn the_view_commands_toggle_zoom_and_scrollback() {
        let mut session = Session::new();

        session.command(Key::Char('z'));
        assert!(session.deck.zoomed());
        session.command(Key::Char('z'));
        assert!(!session.deck.zoomed());

        session.command(Key::Char('['));
        assert!(session.deck.scrollback());
    }

    /// The key is a toggle-all either way; since #39 the stack it starts from
    /// is folded, so the first press is the expand-all half.
    #[test]
    fn the_collapse_key_folds_the_stack_without_a_frozen_action() {
        let mut input = Input::new(10);
        let mut deck = DeckState::new(4);
        let projects = fixture::projects();
        assert_eq!(deck.collapsed_count(), 3);

        input.press(Key::Ctrl('g'), &mut deck, &projects, NOW);
        assert_eq!(input.press(Key::Char('c'), &mut deck, &projects, NOW), None);

        assert_eq!(deck.collapsed_count(), 0);

        input.press(Key::Ctrl('g'), &mut deck, &projects, NOW);
        input.press(Key::Char('c'), &mut deck, &projects, NOW);

        assert_eq!(deck.collapsed_count(), 3);
    }

    /// #41: the keyboard half of the split divider. Like collapse, it is the
    /// deck's own geometry, so it carries no frozen action and reaches the
    /// shell not at all.
    #[test]
    fn the_split_keys_nudge_the_divider_without_a_frozen_action() {
        let mut input = Input::new(10);
        let mut deck = DeckState::new(4);
        let projects = fixture::projects();
        let press = |input: &mut Input, deck: &mut DeckState, key| {
            input.press(Key::Ctrl('g'), deck, &projects, NOW);
            input.press(key, deck, &projects, NOW)
        };

        assert_eq!(press(&mut input, &mut deck, Key::Char('-')), None);
        assert_eq!(deck.master_ratio(), 0.65);
        assert_eq!(press(&mut input, &mut deck, Key::Char('=')), None);
        assert_eq!(deck.master_ratio(), 0.70);
        // The shifted twins are the same keys: `+` is what a hand reaches for.
        press(&mut input, &mut deck, Key::Char('+'));
        assert_eq!(deck.master_ratio(), 0.75);
        press(&mut input, &mut deck, Key::Char('_'));
        assert_eq!(deck.master_ratio(), 0.70);

        // Unprefixed they are ordinary input and reach the shell untouched.
        assert_eq!(
            input.press(Key::Char('-'), &mut deck, &projects, NOW),
            Some(Reaction::Send(UserCommand::Input(InputCommand::Bytes(
                Key::Char('-').bytes()
            ))))
        );
        assert_eq!(deck.master_ratio(), 0.70);
    }

    /// The page keys are the keyboard half of the scrollable stack. Only the
    /// caller knows how far one page reaches, so they come back rather than
    /// applying themselves.
    #[test]
    fn the_page_keys_hand_the_stack_window_to_the_caller() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::PageDown), Some(Reaction::PageStack(1)));
        assert_eq!(session.command(Key::PageUp), Some(Reaction::PageStack(-1)));
        assert_eq!(session.deck.active(), Some(0), "paging promotes nothing");
    }

    /// Unprefixed they are still the shell's, and scrollback's, as before.
    #[test]
    fn an_unprefixed_page_key_is_not_a_stack_command() {
        let mut session = Session::new();

        assert_eq!(session.press(Key::PageDown), sent(b"\x1b[6~"));

        session.command(Key::Char('['));
        assert_eq!(
            session.press(Key::PageDown),
            Some(Reaction::Scroll(ScrollCommand::Down(PAGE)))
        );
    }

    #[test]
    fn a_doubled_prefix_sends_the_literal_byte() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Ctrl('g')), sent(&[0x07]));
        // The prefix is spent: the next key is ordinary input again.
        assert_eq!(session.press(Key::Char('j')), sent(b"j"));
    }

    #[test]
    fn an_unbound_command_key_ends_the_prefix_without_reaching_the_shell() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Char('x')), None);

        assert_eq!(session.press(Key::Char('x')), sent(b"x"));
    }

    #[test]
    fn respawn_is_left_to_the_caller_that_owns_the_engine() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Char('r')), Some(Reaction::Respawn));
        assert_eq!(session.deck.active(), Some(0));
    }

    #[test]
    fn help_opens_on_its_binding_and_closes_on_escape() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Char('?')), None);
        assert_eq!(session.deck.modal(), Some(Modal::Help));

        assert_eq!(session.press(Key::Escape), None);
        assert_eq!(session.deck.modal(), None);
    }

    #[test]
    fn an_open_help_modal_captures_every_other_key() {
        let mut session = Session::new();
        session.command(Key::Char('?'));

        // Nothing reaches the shell and nothing else opens.
        assert_eq!(session.press(Key::Char('a')), None);
        assert_eq!(session.press(Key::Enter), None);
        assert_eq!(session.press(Key::Ctrl('g')), None);
        assert_eq!(session.press(Key::Char('q')), None);

        assert_eq!(session.deck.modal(), Some(Modal::Help));
        assert_eq!(session.deck.active(), Some(0));
    }

    #[test]
    fn quit_confirmation_takes_y_to_confirm() {
        let mut session = Session::new();

        assert_eq!(session.command(Key::Char('q')), None);
        assert_eq!(session.deck.modal(), Some(Modal::Quit));

        assert_eq!(session.press(Key::Char('y')), Some(Reaction::Quit));
        assert_eq!(session.deck.modal(), None);
    }

    #[test]
    fn quit_confirmation_cancels_on_n_and_on_escape() {
        for key in [Key::Char('n'), Key::Escape] {
            let mut session = Session::new();
            session.command(Key::Char('q'));

            assert_eq!(session.press(key), None);
            assert_eq!(session.deck.modal(), None);
            // Cancelling returns the deck to ordinary input.
            assert_eq!(session.press(Key::Char('y')), sent(b"y"));
        }
    }

    #[test]
    fn an_open_quit_modal_captures_every_other_key() {
        let mut session = Session::new();
        session.command(Key::Char('q'));

        assert_eq!(session.press(Key::Char('Y')), None);
        assert_eq!(session.press(Key::Enter), None);

        assert_eq!(session.deck.modal(), Some(Modal::Quit));
    }

    #[test]
    fn scrollback_captures_the_navigation_keys() {
        let mut session = Session::new();
        session.command(Key::Char('['));

        let moves = [
            (Key::Char('j'), ScrollCommand::Down(1)),
            (Key::Down, ScrollCommand::Down(1)),
            (Key::Char('k'), ScrollCommand::Up(1)),
            (Key::Up, ScrollCommand::Up(1)),
            (Key::PageDown, ScrollCommand::Down(PAGE)),
            (Key::PageUp, ScrollCommand::Up(PAGE)),
            (Key::Char('G'), ScrollCommand::Bottom),
            (Key::Char('g'), ScrollCommand::Up(u16::MAX)),
        ];
        for (key, command) in moves {
            assert_eq!(
                session.press(key),
                Some(Reaction::Scroll(command)),
                "{key:?}"
            );
        }
        assert!(session.deck.scrollback());
    }

    #[test]
    fn scrollback_blocks_every_other_key_from_the_live_shell() {
        let mut session = Session::new();
        session.command(Key::Char('['));

        for key in [
            Key::Char('a'),
            Key::Char('3'),
            Key::Enter,
            Key::Backspace,
            Key::Ctrl('c'),
            Key::Left,
        ] {
            assert_eq!(session.press(key), None, "{key:?}");
        }
        assert!(session.deck.scrollback());
    }

    #[test]
    fn escape_leaves_scrollback_and_returns_to_live_input() {
        let mut session = Session::new();
        session.command(Key::Char('['));

        assert_eq!(session.press(Key::Escape), None);

        assert!(!session.deck.scrollback());
        assert_eq!(session.press(Key::Char('j')), sent(b"j"));
    }

    #[test]
    fn the_prefix_still_works_inside_scrollback() {
        let mut session = Session::new();
        session.command(Key::Char('['));

        // The status row offers `^g ?` while the mode runs, so it must reach.
        assert_eq!(session.command(Key::Char('?')), None);

        assert_eq!(session.deck.modal(), Some(Modal::Help));
        assert!(session.deck.scrollback());
    }

    #[test]
    fn a_page_key_follows_the_master_viewport() {
        let mut session = Session::new();
        session.command(Key::Char('['));
        session.input.set_page(12);

        assert_eq!(
            session.press(Key::PageUp),
            Some(Reaction::Scroll(ScrollCommand::Up(12)))
        );
    }
}
