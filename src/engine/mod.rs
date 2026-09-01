//! Terminal engine implementations.

mod fake;
#[allow(dead_code)] // Wired into the native engine by the next slice.
pub(crate) mod pty;
mod vt;

pub use fake::FakeEngine;
pub use vt::VtFrameAdapter;
