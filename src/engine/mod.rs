//! Terminal engine implementations.

mod fake;
mod native;
pub(crate) mod pty;
mod shell_hook;
mod vt;

pub use fake::FakeEngine;
pub use native::NativeEngine;
pub use pty::{InputOutcome, PtyEvent, PtyTransport};
pub use vt::VtFrameAdapter;
