//! Note-picker UI: the Elm-style model behind `mdfu`'s interactive note list.
//!
//! [`App`] carries the picker state, [`App::update`] is the message handler
//! (pure apart from the model it mutates, so tests drive it with synthetic key
//! events), and [`App::view`] paints one frame through the reusable
//! [`crate::widgets`] layer. The search filter is **injected** through
//! [`App::new`] (see [`FilterFn`]); there is deliberately no global or static
//! filter, and this module never references the task UI.
//!
//! Mode semantics mirror Go `internal/tui/tui.go`:
//! - The picker opens in **list mode** with the search bar hidden. Only an
//!   allowlist of keys is handled; printable runes and editing keys never
//!   reach the hidden input.
//! - `/` reveals and focuses the search bar (**search mode**). `esc` there
//!   returns to list mode keeping the query; in list mode `esc` first clears a
//!   non-empty query and only aborts when the query is already empty
//!   (`ctrl+c` always aborts).
//! - `tab` toggles multi-selection, `ctrl+a` archived visibility, `ctrl+p`
//!   the preview pane, `ctrl+f` the frontmatter panel, `ctrl+o` the first web
//!   link, `enter` confirms, and `e` runs the injected `$EDITOR` flow through
//!   [`exec_editor`] (suspend, run, resume, reload).
//!
//! Preview-pane wiring (markdown body window, side-by-side layout) is owned by
//! the follow-up preview integration; this module only tracks `show_preview`
//! and renders the list/search/status frame.

mod app;
mod editor;
mod view;

pub use app::{default_filter, editor_argv, App, Cmd, Config, FilterFn, FilterHit, Msg, ReloadFn};
pub use editor::exec_editor;
pub use view::{SEARCH_PLACEHOLDER, SEARCH_PROMPT};
