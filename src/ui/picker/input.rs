use super::{Entry, EntryKind, Hit, PickerState};

use super::super::Key;

/// What a key press asks the caller for. Everything else the picker does to
/// itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PickerReaction {
    /// Open the selection as a workspace.
    Launch,
    /// Resume the saved session the cursor settled on — the whole deck, not
    /// a pane of one. [`PickerState::resumed`] says which.
    Resume,
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
    // The context list borrows the browse keys rather than defining its own:
    // `↑↓` moves, `/` filters, `esc` leaves, and the keys that open something
    // — `⏎`, `⇥`, `space`, `o`, `→` — do the one thing there is to do to the
    // row under the cursor. A saved session is resumed whole; the way out is
    // the file explorer, which takes the screen on its own terms.
    if state.choosing()
        && let Some(entry) = cursor.as_ref().filter(|_| opens(key))
    {
        return match entry.kind {
            EntryKind::Snapshot => state.resume(entry).then_some(PickerReaction::Resume),
            EntryKind::Escape => {
                state.leave_context();
                None
            }
            _ => None,
        };
    }
    match key {
        // `ctrl+j` is the control twin of `j`, and no shell is listening
        // here, so it moves the cursor rather than falling through (#95).
        Key::Down | Key::Char('j') | Key::Ctrl('j') => {
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
        Key::Enter | Key::Tab | Key::Char(' ') => {
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
        Key::Right | Key::Char('l') => {
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

/// The keys that act on the row under the cursor rather than move to
/// another one. In the browse listing they select, descend, or launch; in the
/// context list there is one row and one thing to do with it, so they agree.
fn opens(key: Key) -> bool {
    matches!(
        key,
        Key::Enter | Key::Tab | Key::Char(' ') | Key::Char('o') | Key::Right | Key::Char('l')
    )
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

/// Shift held on a click: the pointer twin of `⇧↓` / `⇧↑`. It toggles every
/// selectable row between the highlight and the row that was clicked, then
/// moves the highlight there.
pub fn click_range(state: &mut PickerState, rows: &[Entry], hit: Hit) -> bool {
    let (Hit::Row(index) | Hit::Checkbox(index) | Hit::Badge(index)) = hit else {
        return false;
    };
    if index >= rows.len() {
        return false;
    }
    let selected = state.select_between(rows, index);
    state.point_at(index, rows.len());
    selected
}

/// The secondary button's own gesture: on the `×N` badge it is `-`, which
/// sheds one instance of that path. The parity table gives the primary button
/// the additions and the secondary button the subtraction, so a pointer can
/// reach both ends of §3.1 without a keyboard.
pub fn click_secondary(state: &mut PickerState, rows: &[Entry], hit: Hit) -> bool {
    let (Hit::Badge(index) | Hit::Row(index) | Hit::Checkbox(index)) = hit else {
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
    // Pointer parity in the context list is the same one gesture the keys
    // have: a click on a row is `⏎` on it.
    if state.choosing() {
        let (Hit::Row(index) | Hit::Checkbox(index) | Hit::Badge(index)) = hit else {
            if hit == Hit::Filter {
                state.begin_filter();
            }
            return None;
        };
        let entry = rows.get(index).cloned()?;
        state.point_at(index, rows.len());
        return match entry.kind {
            EntryKind::Snapshot => state.resume(&entry).then_some(PickerReaction::Resume),
            EntryKind::Escape => {
                state.leave_context();
                None
            }
            _ => None,
        };
    }
    match hit {
        Hit::Row(index) => {
            let entry = rows.get(index).cloned()?;
            state.point_at(index, rows.len());
            state.toggle(&entry);
        }
        Hit::Checkbox(index) => {
            let entry = rows.get(index).cloned()?;
            state.point_at(index, rows.len());
            state.toggle(&entry);
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
