//! Terminal engine implementations.

mod fake;
mod vt;

pub use fake::FakeEngine;
pub use vt::VtFrameAdapter;
