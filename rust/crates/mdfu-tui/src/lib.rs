//! Terminal user interface for `mdfu` (note picker and task views).
//!
//! [`view_model`] defines the plain-data boundary widgets render from;
//! [`widgets`] is the reusable ratatui widget layer built on top of it.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

pub mod links;
pub mod view_model;
pub mod widgets;

/// The version of `mdfu-core` this UI is built against.
#[must_use]
pub fn core_version() -> &'static str {
    mdfu_core::version()
}
