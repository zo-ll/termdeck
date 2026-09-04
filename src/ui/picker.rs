//! The repository picker: what `termdeck` draws when it is given no path.
//!
//! Design authority: `docs/design/termdeck/repository-picker.md`, itself read
//! off screens 06–08 of the export. This module owns the picker's state and
//! its view; it never walks a filesystem itself. Entries arrive already
//! classified and annotated through [`Browse`], and what leaves is an ordered
//! list of `(terminal name, path)` pairs plus a workspace name — the seam the
//! note fixes, which is what lets the same path appear more than once.

mod fs;
mod input;
mod render;
mod sheet;
mod state;

#[cfg(test)]
mod tests;

pub use fs::FsBrowse;
pub use input::{PickerReaction, click, click_range, click_secondary, descend, press};
pub use render::{Hit, Picker};
pub use sheet::{
    Open, Sheet, SheetHit, SheetState, open_pane, sheet_click, sheet_click_secondary, sheet_press,
};
pub use state::{
    Browse, Entry, EntryKind, Instance, Listing, PickerState, match_at, matches, unique_name,
};
