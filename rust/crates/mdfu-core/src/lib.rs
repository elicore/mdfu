//! Content logic for `mdfu`: scan, parse, query, rank, and format.
//!
//! Skeleton only; the package ports land in follow-up changes.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

/// The workspace version of `mdfu-core`.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
