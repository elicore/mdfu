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
//! Modes: `browse` (default), `filter` (`/`), and `help` (`?`). The
//! `edit_title`/`edit_body`/`move_prompt` modes exist in [`Mode`] and their
//! state (`editor_text`, `pending_edit`, `move_input`, `new_task`, `base`,
//! `task_config`, `action`) is defined on [`App`], but their event loops and
//! every disk write belong to todos 25-26: the browse keys
//! `o`/`t`/`e`/`n`/`m`/`a` are reserved and carry no behavior in this slice.
//! [`Msg::EditorFinished`] and [`Cmd::ExecEditor`] are the reserved
//! message/command seam for the same todos.
//!
//! Layout mirrors Go `renderTaskBrowse`: the filter line is the first row
//! (filter mode only), the status/help footer the last row, and the list and
//! detail panes are joined side by side at width >= [`WIDE_LAYOUT`] and stacked
//! below it. An empty list renders `(no tasks)`; all colors come from
//! [`mdfu_core::theme::Theme`] (never hardcoded).

mod model;

pub use model::{
    default_task_filter, default_task_key_map, key_name, App, Binding, Cmd, Config, KeyMap, Mode,
    Msg, TaskFilterFn, WIDE_LAYOUT,
};
