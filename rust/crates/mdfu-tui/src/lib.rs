//! Terminal user interface for `mdfu` (note picker and task views).
//!
//! [`view_model`] defines the plain-data boundary widgets render from;
//! [`widgets`] is the reusable ratatui widget layer built on top of it;
//! [`preview`] integrates the preview pane (layout, toggles, cached body
//! windows) for the note picker; [`notes`] is the note picker's Elm-style
//! model; and [`tasks`] is the task TUI's Elm-style model with its own
//! injected filter. Both models are updated with key/message events and
//! rendered through the widget layer.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

pub mod links;
pub mod notes;
pub mod preview;
pub mod tasks;
pub mod view_model;
pub mod widgets;

/// The version of `mdfu-core` this UI is built against.
#[must_use]
pub fn core_version() -> &'static str {
    mdfu_core::version()
}
