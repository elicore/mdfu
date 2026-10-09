//! Terminal user interface for `mdfu` (note picker and task views).
//!
//! Skeleton only; the ratatui widgets land in follow-up changes.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

/// The version of `mdfu-core` this UI is built against.
#[must_use]
pub fn core_version() -> &'static str {
    mdfu_core::version()
}
