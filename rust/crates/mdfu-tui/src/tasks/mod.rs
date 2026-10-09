//! Task TUI: the Elm-style model behind `mdfu tasks`.
//!
//! This module is the Rust port of the Go task TUI (`internal/tui/tasks_model.go`,
//! `tasks_view.go`). [`App`] carries the task list, the injected
//! [`TaskFilterFn`], the input [`Mode`], the cursor/offset, and the frame
//! layout; [`App::update`] handles key/resize messages and [`App::view`] paints
//! one frame through the reusable [`crate::widgets`] layer.
//!
//! Filtering is **injected** through [`App::new`]/[`App::with_default_filter`].
//! [`TaskFilterFn`] is the task UI's own hook: it is deliberately not the note
//! picker's filter type, and this module shares no filter global with the
//! picker (the no-global-filter guard test lands with todo 27).
//!
//! Modes: `browse` (default), `filter` (`/`), `help` (`?`), and the edit
//! modes — title (`t`), body (`e`), new task (`n`), and the move prompt (`m`) —
//! whose key loops live in `edit`. Every disk write and the scope reload live
//! in `actions`: `x` flips the checkbox through the engine, `o` runs
//! `$EDITOR +<line> <file>`, and `a` archives a done task; [`Msg::EditorFinished`]
//! reloads the scope so external edits appear.
//!
//! Layout mirrors Go `renderTaskBrowse`: the filter line is the first row
//! (filter mode only), the status/help footer the last row, and the list and
//! detail panes are joined side by side at width >= [`WIDE_LAYOUT`] and stacked
//! below it. An empty list renders `(no tasks)`; all colors come from
//! [`mdfu_core::theme::Theme`] (never hardcoded).

mod actions;
mod edit;
mod model;

pub use actions::{exec_editor, load_checkboxes};
pub use model::{
    default_task_filter, default_task_key_map, key_name, App, Binding, Cmd, Config, KeyMap, Mode,
    Msg, TaskFilterFn, WIDE_LAYOUT,
};
