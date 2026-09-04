//! Shared, implementation-neutral foundations for Termdeck.

pub mod cli;
pub mod config;
pub mod contracts;
#[cfg(unix)]
pub mod ctl;
pub mod engine;
pub mod session;
pub mod ui;
