//! Terminal user interface for `mdfu` (note picker and task views).
//!
//! Skeleton only so far: [`view_model`] defines the plain-data boundary the
//! ratatui widgets will render from; the widgets themselves land in follow-up
//! changes.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

pub mod view_model;

/// The version of `mdfu-core` this UI is built against.
#[must_use]
pub fn core_version() -> &'static str {
    mdfu_core::version()
}
