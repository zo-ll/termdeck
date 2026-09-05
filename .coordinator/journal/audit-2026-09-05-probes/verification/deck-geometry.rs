// #140b: deck render geometry. Build TWICE -- debug and release disagree.
// Debug: panics at src/ui/deck.rs:1381 'attempt to subtract with overflow'.
// Release: [profile.release] sets no overflow-checks, so 80x4 panics inside
// ratatui's buffer bounds instead and 120x5 does not panic at all -- the wrap
// silently renders garbage. See the README beside this file for build lines.

use termdeck::contracts::{Project, TerminalId, Timestamp};
use termdeck::engine::FakeEngine;
use termdeck::ui::{Deck, DeckState, Notifications};

fn try_size(w: u16, h: u16) -> bool {
    let result = std::panic::catch_unwind(move || {
        let id = TerminalId::new("one");
        let engine = FakeEngine::new([id.clone()]);
        let projects = vec![Project { terminal: id, path: "/tmp".into(), command: vec!["sh".to_owned()], shell_hook: false }];
        let state = DeckState::new(projects.len());
        let notifies = Notifications::default();
        let deck = Deck {
            workspace: "w",
            projects: &projects,
            state: &state,
            notifies: &notifies,
            master_ratio: 0.85,
            now: Timestamp { unix_millis: 0 },
        };
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| deck.render(&engine, f)).unwrap();
    });
    result.is_err()
}

fn main() {
    std::panic::set_hook(Box::new(|i| eprintln!("  {i}")));
    for (w, h) in [
        (80u16, 4u16), (80, 5), (80, 6), (120, 4), (120, 5), (144, 5),
        (80, 24), (144, 42),
    ] {
        println!("{w}x{h} panic={}", try_size(w, h));
    }
}
