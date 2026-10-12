//! Task title/body editors and the move-destination prompt.
//!
//! Port of the edit halves of Go `internal/tui/tasks_model.go`
//! (`enterEdit`/`enterNewTask`/`enterMove`, `updateMove`) plus
//! `tasks_edit.go`, `tasks_edit_body.go`, and the move half of
//! `tasks_write.go`.
//!
//! Every save opens the file through the core edit engine and calls
//! `verify_task` before staging a byte, so an externally modified task aborts
//! with the E17 status and writes nothing (SPEC.md §J). The body editor is
//! `ratatui-textarea`, which keeps cursor movement and wrapping Unicode/CJK
//! safe; [`App::editor_text`] mirrors its value for the save paths and the
//! frame snapshot.
//!
//! The append/move follow-ups refresh the in-memory item directly here; the
//! full scope reload (Go `reload`) belongs to todo 26's actions module.

use std::fmt;
use std::fs;
use std::io;

use mdfu_core::task::edit::{append_block, move_task, open, EditError};
use mdfu_core::task::grammar::parse_checkbox;
use mdfu_core::task::Task;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui_textarea::{CursorMove, TextArea};

use crate::view_model::TaskEditor;

use super::model::{App, Cmd, Mode};

/// Failure of one editor action: a fixed TUI message or an engine error.
#[derive(Debug)]
enum ActionError {
    /// A TUI-only message such as `invalid task header`.
    Message(&'static str),
    /// A failure from the byte-exact edit engine.
    Edit(EditError),
    /// An I/O failure reading or writing a task file.
    Io(io::Error),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::Edit(err) => err.fmt(f),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl From<EditError> for ActionError {
    fn from(err: EditError) -> Self {
        Self::Edit(err)
    }
}

impl From<io::Error> for ActionError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl App {
    /// Handles one key inside the title/body editor (Go `updateEditTitle` and
    /// `updateEditBody`): `ctrl+s` saves, `esc` cancels, `q`/`ctrl+c` quit,
    /// and every other key reaches the textarea.
    #[must_use = "the returned command drives quitting"]
    pub(crate) fn update_edit(&mut self, key: &KeyEvent) -> Option<Cmd> {
        if self.keys.save.matches(key) {
            self.run_save();
            return None;
        }
        if self.keys.cancel.matches(key) {
            "cancelled".clone_into(&mut self.status);
            self.mode = Mode::Browse;
            self.new_task = false;
            return None;
        }
        if self.keys.quit.matches(key) {
            self.aborted = true;
            return Some(Cmd::Quit);
        }
        let _ = self.textarea.input(*key);
        self.editor_text = editor_value(&self.textarea);
        None
    }

    /// Handles one key in the move prompt (Go `updateMove`): `enter` applies
    /// the target, `esc` cancels, `q`/`ctrl+c` quit, and everything else edits
    /// the destination.
    #[must_use = "the returned command drives quitting"]
    pub(crate) fn update_move(&mut self, key: &KeyEvent) -> Option<Cmd> {
        if key.code == KeyCode::Enter {
            if let Err(err) = self.commit_move() {
                self.record_error(&err);
            }
            return None;
        }
        if self.keys.cancel.matches(key) {
            "cancelled".clone_into(&mut self.status);
            self.mode = Mode::Browse;
            self.move_input.blur();
            return None;
        }
        if self.keys.quit.matches(key) {
            self.aborted = true;
            return Some(Cmd::Quit);
        }
        self.move_input.handle_event(&Event::Key(*key));
        None
    }

    /// Seeds the editor with the focused task's header (title mode) or body
    /// (body mode) and switches to `mode` (Go `enterEdit`).
    pub(crate) fn enter_edit(&mut self, mode: Mode) {
        let Some(task) = focused_task(self) else {
            return;
        };
        let seed = if mode == Mode::EditBody {
            task.body.clone()
        } else {
            task.header_raw.clone()
        };
        self.seed_editor(&seed, mode);
        self.new_task = false;
    }

    /// Seeds the editor with an empty task header as a new-task creation and
    /// switches to the title editor (Go `enterNewTask`).
    pub(crate) fn enter_new_task(&mut self) {
        self.seed_editor("- [ ] ", Mode::EditTitle);
        self.new_task = true;
        "new task".clone_into(&mut self.status);
    }

    /// Seeds the move prompt with the focused task's current file (Go
    /// `enterMove`).
    pub(crate) fn enter_move(&mut self) {
        let Some(task) = focused_task(self) else {
            return;
        };
        self.move_input.set_value(task.file.clone());
        self.mode = Mode::MovePrompt;
        self.move_input.focus();
    }

    /// Snapshots the editor frame: the buffer text, the parsed-header side
    /// panel rows (title mode), and the focused body indent (body mode).
    #[must_use]
    pub(crate) fn task_editor_snapshot(&self) -> TaskEditor {
        let is_body = self.mode == Mode::EditBody;
        TaskEditor {
            text: self.editor_text.clone(),
            meta: if is_body {
                Vec::new()
            } else {
                title_meta_rows(&self.editor_text)
            },
            is_body,
            body_indent: if is_body {
                focused_body_indent(self)
            } else {
                String::new()
            },
            status: self.status.clone(),
        }
    }

    /// Replaces the textarea state with `text` and records the seed as the
    /// pending edit (Go `editor.SetValue` + `pendingEdit`).
    fn seed_editor(&mut self, text: &str, mode: Mode) {
        self.textarea = TextArea::from(text.lines());
        self.textarea.move_cursor(CursorMove::End);
        text.clone_into(&mut self.editor_text);
        text.clone_into(&mut self.pending_edit);
        self.mode = mode;
    }

    /// Routes `ctrl+s` to the title or body save and reports a failure through
    /// the error/status fields (Go `updateEditTitle`).
    fn run_save(&mut self) {
        let result = if self.mode == Mode::EditBody {
            self.save_body()
        } else {
            self.save_title()
        };
        if let Err(err) = result {
            self.record_error(&err);
        }
    }

    /// Stores one action failure in the error and status fields.
    fn record_error(&mut self, err: &ActionError) {
        let message = err.to_string();
        self.error = Some(message.clone());
        self.status = message;
    }

    /// Parses the edited header and either replaces the focused task's header
    /// line or appends a new block (Go `saveTitle`). An unparseable header is
    /// refused with the `invalid task header` status and writes nothing.
    fn save_title(&mut self) -> Result<(), ActionError> {
        let header = self.editor_text.trim_end_matches(['\r', '\n']).to_owned();
        let Some(parsed) = parse_checkbox(&header) else {
            return Err(ActionError::Message("invalid task header"));
        };
        if self.new_task {
            return self.append_new_task(&header, &parsed);
        }
        self.replace_header_line(&header)
    }

    /// Swaps the single header line through the core edit engine after
    /// verifying the task still sits at its recorded line (Go
    /// `replaceHeaderLine`).
    fn replace_header_line(&mut self, header: &str) -> Result<(), ActionError> {
        let Some((index, path)) = focused_task_and_path(self) else {
            return Err(ActionError::Message("no task selected"));
        };
        if path.is_empty() {
            return Err(ActionError::Message("no task file"));
        }
        let task = self.tasks[index].clone();
        let mut edit = open(&path)?;
        edit.verify_task(&task)?;
        edit.replace_header_line(task.line, header.as_bytes())?;
        edit.commit()?;

        if let Some(mut parsed) = parse_checkbox(header) {
            parsed.body.clone_from(&task.body);
            parsed.body_indent.clone_from(&task.body_indent);
            parsed.file.clone_from(&task.file);
            parsed.line = task.line;
            update_focused_task(self, parsed);
        }
        self.new_task = false;
        self.mode = Mode::Browse;
        "saved".clone_into(&mut self.status);
        Ok(())
    }

    /// Appends the edited block to the append target through the core engine
    /// (Go `appendNewTask`). A task with no ID points the status at
    /// `mdfu task ids`.
    fn append_new_task(&mut self, header: &str, parsed: &Task) -> Result<(), ActionError> {
        let path = append_target_path(self);
        if path.is_empty() {
            return Err(ActionError::Message("no target file"));
        }
        append_block(&path, header.as_bytes())?;
        if parsed.id.is_empty() {
            "added task without an ID — run mdfu task ids".clone_into(&mut self.status);
        } else {
            self.status = format!("added {}", parsed.id);
        }
        self.new_task = false;
        self.mode = Mode::Browse;
        self.insert_appended(&path, header, parsed);
        Ok(())
    }

    /// Adds the just-appended task to the in-memory set so it is visible
    /// without a scope reload.
    fn insert_appended(&mut self, path: &str, header: &str, parsed: &Task) {
        let mut task = parsed.clone();
        path.clone_into(&mut task.file);
        task.line = last_header_line(path, header).unwrap_or(0);
        self.tasks.push(task);
        self.refilter();
    }

    /// Re-indents the editor text and replaces the block's body lines through
    /// the core edit engine, verifying the header first (Go `saveBody`).
    fn save_body(&mut self) -> Result<(), ActionError> {
        let Some((index, path)) = focused_task_and_path(self) else {
            return Err(ActionError::Message("no task selected"));
        };
        if path.is_empty() {
            return Err(ActionError::Message("no task file"));
        }
        let task = self.tasks[index].clone();
        let text = self.editor_text.trim_end_matches('\n').to_owned();
        let indent = if task.body_indent.is_empty() {
            "  ".to_owned()
        } else {
            task.body_indent.clone()
        };
        let body_lines = build_body_lines(&text, &indent);
        let start = task.line;
        let end = start + body_line_count(&task.body);
        let mut edit = open(&path)?;
        edit.verify_task(&task)?;
        edit.replace_body_range(start, end, &body_lines)?;
        edit.commit()?;

        let mut parsed = task;
        if text.is_empty() {
            parsed.body = String::new();
            parsed.body_indent = String::new();
        } else {
            parsed.body = text;
            parsed.body_indent = indent;
        }
        update_focused_task(self, parsed);
        self.mode = Mode::Browse;
        "saved body".clone_into(&mut self.status);
        Ok(())
    }

    /// Applies the move prompt target through the engine's `move_task` and
    /// refreshes the in-memory item (Go `commitMove`). An empty target is a
    /// TUI-only error and writes nothing.
    fn commit_move(&mut self) -> Result<(), ActionError> {
        let Some((index, _path)) = focused_task_and_path(self) else {
            self.mode = Mode::Browse;
            self.move_input.blur();
            return Ok(());
        };
        let target = self.move_input.value().trim().to_owned();
        if target.is_empty() {
            return Err(ActionError::Message("destination file required"));
        }
        let task = self.tasks[index].clone();
        move_task(&task, &target)?;
        "moved".clone_into(&mut self.status);
        self.mode = Mode::Browse;
        self.move_input.blur();
        if let Some(slot) = self.tasks.get_mut(index) {
            slot.file.clone_from(&target);
            slot.line = last_header_line(&target, &task.header_raw).unwrap_or(slot.line);
        }
        self.refilter();
        Ok(())
    }
}

/// The editor value as newline-joined text (Go `textarea.Value`).
fn editor_value(textarea: &TextArea<'_>) -> String {
    textarea.lines().join("\n")
}

/// The focused task, if the cursor is inside the filtered list.
fn focused_task(app: &App) -> Option<&Task> {
    app.filtered
        .get(app.cursor)
        .and_then(|index| app.tasks.get(*index))
}

/// The focused task's index and the file path to mutate (Go
/// `focusedTaskAndPath`). A blank path means the item carries no file; callers
/// fall back to in-memory behaviour.
fn focused_task_and_path(app: &App) -> Option<(usize, String)> {
    let index = *app.filtered.get(app.cursor)?;
    let task = app.tasks.get(index)?;
    let mut path = task.file.clone();
    if path.is_empty() && !app.base.is_empty() && is_file(&app.base) {
        path.clone_from(&app.base);
    }
    Some((index, path))
}

/// Replaces the focused item's engine task after a successful write (Go
/// `updateFocusedTask`).
fn update_focused_task(app: &mut App, task: Task) {
    if let Some(index) = app.filtered.get(app.cursor).copied() {
        if let Some(slot) = app.tasks.get_mut(index) {
            *slot = task;
        }
    }
}

/// Picks the file a newly created task is appended to: the `--path` file when
/// the scope is a single file, else the focused task's file (Go
/// `appendTargetPath`).
fn append_target_path(app: &App) -> String {
    if !app.base.is_empty() && is_file(&app.base) {
        return app.base.clone();
    }
    focused_task(app).map_or_else(String::new, |task| task.file.clone())
}

/// The indent the focused task's existing body uses, defaulting to two spaces
/// (Go `focusedBodyIndent`).
fn focused_body_indent(app: &App) -> String {
    focused_task(app).map_or_else(
        || "  ".to_owned(),
        |task| {
            if task.body_indent.is_empty() {
                "  ".to_owned()
            } else {
                task.body_indent.clone()
            }
        },
    )
}

/// Whether `path` exists and is not a directory (Go `os.Stat` + `IsDir`).
fn is_file(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|meta| !meta.is_dir())
}

/// Builds the title editor's side panel rows from the current header (Go
/// `renderTitleSidePanel`), parsed with the broadened grammar.
fn title_meta_rows(text: &str) -> Vec<(String, String)> {
    let Some(parsed) = parse_checkbox(text) else {
        return vec![("Header".to_owned(), "invalid task header".to_owned())];
    };
    let mut rows = Vec::with_capacity(8);
    let id = if parsed.id.is_empty() {
        "(none)".to_owned()
    } else {
        parsed.id.clone()
    };
    rows.push(("ID".to_owned(), id));
    rows.push((
        "Status".to_owned(),
        if parsed.checked { "done" } else { "open" }.to_owned(),
    ));
    if !parsed.title.is_empty() {
        rows.push(("Title".to_owned(), parsed.title.clone()));
    }
    if !parsed.tags.is_empty() {
        let tags = parsed
            .tags
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" ");
        rows.push(("Tags".to_owned(), tags));
    }
    if !parsed.priority.is_empty() {
        rows.push(("Priority".to_owned(), format!("!{}", parsed.priority)));
    }
    for key in &parsed.property_order {
        if let Some(value) = parsed.properties.get(key) {
            rows.push((format!("@{key}"), value.clone()));
        }
    }
    rows
}

/// Re-indents `text`: blank lines stay blank and unindented, every other line
/// is prefixed (Go `buildBodyLines`). Empty text yields no lines.
fn build_body_lines(text: &str, indent: &str) -> Vec<Vec<u8>> {
    if text.is_empty() {
        return Vec::new();
    }
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                Vec::new()
            } else {
                format!("{indent}{line}").into_bytes()
            }
        })
        .collect()
}

/// The number of lines a dedented body occupies: 0 when empty, else 1 plus the
/// newline count (Go `bodyLineCount`).
fn body_line_count(body: &str) -> usize {
    if body.is_empty() {
        0
    } else {
        1 + body.matches('\n').count()
    }
}

/// The 1-based line of the last line in `path` equal to `header` after
/// trimming CRLF, used to point the in-memory item at its appended location.
fn last_header_line(path: &str, header: &str) -> Option<usize> {
    let data = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&data);
    let wanted = header.trim_end_matches('\r');
    let mut found = None;
    for (index, line) in text.split('\n').enumerate() {
        if line.trim_end_matches('\r') == wanted {
            found = Some(index + 1);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use mdfu_core::task::config::Config as TaskConfig;
    use mdfu_core::task::scope;
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;
    use crate::tasks::model::{Config, Msg};
    use crate::widgets::test_support::{buffer_lines, render_buffer};

    /// Minimal self-cleaning temp dir (the crate has no `tempfile` dev-dep).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("mdfu-tui-edit-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_fixture(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).expect("write fixture");
        path
    }

    fn load_tasks(dir: &Path) -> Vec<Task> {
        let base = dir.to_string_lossy().into_owned();
        let no_env = |_key: &str| None::<String>;
        let (tasks, _) =
            scope::load_all(&base, &TaskConfig::default(), "", &no_env).expect("load task scope");
        tasks
    }

    fn app_for(dir: &Path) -> App {
        App::with_default_filter(
            load_tasks(dir),
            Config {
                show_done: true,
                ..Config::default()
            },
        )
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn rune_key(c: char) -> Msg {
        key(KeyCode::Char(c))
    }

    fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn apply(app: &mut App, msg: Msg) -> Option<Cmd> {
        app.update(msg)
    }

    fn type_str(app: &mut App, text: &str) {
        for c in text.chars() {
            let _ = apply(app, rune_key(c));
        }
    }

    fn set_editor(app: &mut App, text: &str) {
        app.textarea = TextArea::from(text.lines());
        app.editor_text = text.to_owned();
    }

    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).expect("read file")
    }

    #[test]
    fn task_edit_title_save_keeps_metadata_and_body() {
        let dir = TempDir::new();
        let path = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 Fix login #auth !P1 @owner:me\n  keep me\n- [ ] PRJ-002 second\n",
        );
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));
        assert_eq!(app.mode(), Mode::EditTitle);
        assert_eq!(
            app.editor_text(),
            "- [ ] PRJ-001 Fix login #auth !P1 @owner:me"
        );

        set_editor(&mut app, "- [ ] PRJ-001 Renamed task #auth !P1 @owner:me");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.mode(), Mode::Browse, "a save leaves the editor");
        assert_eq!(app.status(), "saved");
        let after = read_file(&path);
        assert!(
            after.starts_with("- [ ] PRJ-001 Renamed task #auth !P1 @owner:me\n  keep me\n"),
            "header not replaced or body lost:\n{after}"
        );

        let tasks = load_tasks(dir.path());
        let task = tasks
            .iter()
            .find(|task| task.id == "PRJ-001")
            .expect("reloaded task");
        assert_eq!(task.title, "Renamed task");
        assert_eq!(task.priority, "P1");
        assert_eq!(task.properties.get("owner").map(String::as_str), Some("me"));
        assert!(
            task.tags.contains(&"auth".to_owned()),
            "tags: {:?}",
            task.tags
        );
        assert_eq!(task.body, "keep me");
    }

    #[test]
    fn task_edit_title_save_keeps_tab_separator() {
        let dir = TempDir::new();
        let path = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 Fix login\t\t#auth\n  keep me\n",
        );
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));
        set_editor(&mut app, "- [ ] PRJ-001 Fix login done\t\t#auth");
        let _ = apply(&mut app, ctrl('s'));

        let after = read_file(&path);
        assert!(
            after.starts_with("- [ ] PRJ-001 Fix login done\t\t#auth\n  keep me\n"),
            "tab separator lost:\n{after:?}"
        );
        let task = load_tasks(dir.path())
            .into_iter()
            .find(|task| task.id == "PRJ-001")
            .expect("reloaded task");
        assert_eq!(task.title, "Fix login done");
        assert_eq!(task.tags, vec!["auth".to_owned()]);
    }

    #[test]
    fn task_edit_title_invalid_header_is_refused() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let before = read_file(&path);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));

        set_editor(&mut app, "not a task header");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.mode(), Mode::EditTitle, "invalid save keeps editing");
        assert!(
            app.status().contains("invalid task header"),
            "status: {}",
            app.status()
        );
        assert_eq!(read_file(&path), before, "invalid header must not write");
    }

    #[test]
    fn task_edit_title_cancel_writes_nothing() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let before = read_file(&path);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));
        set_editor(&mut app, "- [ ] PRJ-001 changed");
        let _ = apply(&mut app, key(KeyCode::Esc));

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.status(), "cancelled");
        assert_eq!(read_file(&path), before, "esc must not write");
    }

    #[test]
    fn task_edit_title_side_panel_tracks_metadata() {
        let dir = TempDir::new();
        write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));
        set_editor(&mut app, "- [ ] PRJ-001 tuned #side !P9 @k:v");

        let meta = app.task_editor_snapshot().meta;
        assert_eq!(
            meta,
            vec![
                ("ID".to_owned(), "PRJ-001".to_owned()),
                ("Status".to_owned(), "open".to_owned()),
                ("Title".to_owned(), "tuned".to_owned()),
                ("Tags".to_owned(), "#side".to_owned()),
                ("Priority".to_owned(), "!P9".to_owned()),
                ("@k".to_owned(), "v".to_owned()),
            ]
        );
        let frame = buffer_lines(&render_buffer(&app, 60, 8)).join("\n");
        for want in ["tuned", "#side", "!P9", "@k", "v", "PRJ-001"] {
            assert!(
                frame.contains(want),
                "side panel missing {want:?}:\n{frame}"
            );
        }
    }

    #[test]
    fn task_edit_title_invalid_side_panel_row() {
        let mut app = App::with_default_filter(Vec::new(), Config::default());
        let _ = apply(&mut app, rune_key('n'));
        set_editor(&mut app, "broken");
        assert_eq!(
            app.task_editor_snapshot().meta,
            vec![("Header".to_owned(), "invalid task header".to_owned())]
        );
    }

    #[test]
    fn task_edit_new_task_appends_block() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('n'));
        assert_eq!(app.mode(), Mode::EditTitle);
        assert_eq!(app.editor_text(), "- [ ] ");
        assert_eq!(app.status(), "new task");
        assert!(app.is_new_task());

        set_editor(&mut app, "- [ ] NEW-001 added by n");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.status(), "added NEW-001");
        assert!(!app.is_new_task());
        let after = read_file(&path);
        assert!(
            after.ends_with("- [ ] NEW-001 added by n\n"),
            "appended block missing:\n{after}"
        );
        let appended = app
            .tasks()
            .iter()
            .find(|task| task.id == "NEW-001")
            .expect("new task in memory");
        assert_eq!(appended.line, 2, "appended at the end of the file");
        assert_eq!(appended.file, path.to_string_lossy());
    }

    #[test]
    fn task_edit_new_task_without_id_reports_ids_hint() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('n'));
        set_editor(&mut app, "- [ ] no id at all");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.status(), "added task without an ID — run mdfu task ids");
        assert!(read_file(&path).contains("- [ ] no id at all\n"));
    }

    #[test]
    fn task_edit_new_task_cancel_clears_flag() {
        let dir = TempDir::new();
        write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('n'));
        assert!(app.is_new_task());
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.status(), "cancelled");
        assert!(!app.is_new_task());
    }

    #[test]
    fn task_edit_body_save_two_lines() {
        let dir = TempDir::new();
        let path = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 title\n  original\n\n- [ ] PRJ-002 other\n",
        );
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('e'));
        assert_eq!(app.mode(), Mode::EditBody);
        assert_eq!(
            app.editor_text(),
            "original",
            "seeded with the dedented body"
        );

        set_editor(&mut app, "line one\nline two");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.status(), "saved body");
        let after = read_file(&path);
        assert!(
            after.contains("- [ ] PRJ-001 title\n  line one\n  line two\n"),
            "body block wrong:\n{after:?}"
        );
    }

    #[test]
    fn task_edit_body_blank_lines_stay_blank() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 title\n  original\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('e'));
        set_editor(&mut app, "a\n\nb");
        let _ = apply(&mut app, ctrl('s'));

        let after = read_file(&path);
        assert!(
            after.contains("- [ ] PRJ-001 title\n  a\n\n  b\n"),
            "blank line not preserved:\n{after:?}"
        );
    }

    #[test]
    fn task_edit_body_empty_save_is_byte_identical() {
        let dir = TempDir::new();
        let path = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 title\n- [ ] PRJ-002 other\n",
        );
        let before = read_file(&path);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('j'));
        let _ = apply(&mut app, rune_key('e'));
        assert_eq!(app.editor_text(), "", "bodyless task seeds empty");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(read_file(&path), before, "empty save changed the file");
    }

    #[test]
    fn task_edit_body_four_space_indent_round_trips() {
        let dir = TempDir::new();
        let path = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 title\n    four space body\n",
        );
        let before = read_file(&path);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('e'));
        assert_eq!(app.editor_text(), "four space body");
        assert_eq!(app.task_editor_snapshot().body_indent, "    ");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(read_file(&path), before, "round-trip changed the file");
    }

    #[test]
    fn task_edit_body_cjk_types_through_textarea_and_round_trips() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 title\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('e'));

        type_str(&mut app, "你好世界");
        assert_eq!(app.textarea.lines(), ["你好世界"], "textarea holds CJK");

        let _ = apply(&mut app, ctrl('s'));
        let after = read_file(&path);
        assert!(
            after.contains("- [ ] PRJ-001 title\n  你好世界\n"),
            "CJK body not re-indented:\n{after:?}"
        );
    }

    #[test]
    fn task_edit_stale_title_aborts_with_zero_writes() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('t'));
        set_editor(&mut app, "- [ ] PRJ-001 renamed");

        let external = "# header moved\n- [ ] PRJ-001 alpha\n";
        fs::write(&path, external).expect("external change");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.mode(), Mode::EditTitle, "stale save keeps editing");
        assert!(
            app.status()
                .contains("file changed, task 'PRJ-001' not at expected line"),
            "status: {}",
            app.status()
        );
        assert_eq!(read_file(&path), external, "stale save must write nothing");
    }

    #[test]
    fn task_edit_stale_body_aborts_with_zero_writes() {
        let dir = TempDir::new();
        let path = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 title\n  original\n");
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('e'));
        set_editor(&mut app, "changed");

        let external = "# note\n- [ ] PRJ-001 title\n  original\n";
        fs::write(&path, external).expect("external change");
        let _ = apply(&mut app, ctrl('s'));

        assert_eq!(app.mode(), Mode::EditBody, "stale save keeps editing");
        assert!(
            app.status()
                .contains("file changed, task 'PRJ-001' not at expected line"),
            "status: {}",
            app.status()
        );
        assert_eq!(read_file(&path), external, "stale save must write nothing");
    }

    #[test]
    fn task_edit_move_prompt_relocates_block() {
        let dir = TempDir::new();
        let source = write_fixture(
            dir.path(),
            "tasks.md",
            "- [ ] PRJ-001 alpha\n  body one\n- [ ] PRJ-002 beta\n",
        );
        let target = write_fixture(dir.path(), "target.md", "# Target\n\n");
        let mut app = app_for(dir.path());

        let _ = apply(&mut app, rune_key('m'));
        assert_eq!(app.mode(), Mode::MovePrompt);
        assert_eq!(app.move_input().value(), source.to_string_lossy());
        assert!(app.move_input().focused());

        app.move_input.set_value(target.to_string_lossy());
        let _ = apply(&mut app, key(KeyCode::Enter));

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.status(), "moved");
        assert_eq!(read_file(&source), "- [ ] PRJ-002 beta\n");
        assert_eq!(
            read_file(&target),
            "# Target\n\n- [ ] PRJ-001 alpha\n  body one\n"
        );
        let focused = app.filtered_tasks()[app.cursor_index()].clone();
        assert_eq!(focused.id, "PRJ-001", "cursor follows the moved task");
        assert_eq!(focused.file, target.to_string_lossy());
    }

    #[test]
    fn task_edit_move_prompt_requires_destination() {
        let dir = TempDir::new();
        let source = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let before = read_file(&source);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('m'));
        app.move_input.set_value("");
        let _ = apply(&mut app, key(KeyCode::Enter));

        assert_eq!(app.mode(), Mode::MovePrompt, "empty target keeps prompting");
        assert_eq!(app.status(), "destination file required");
        assert_eq!(read_file(&source), before, "empty target must not write");
    }

    #[test]
    fn task_edit_move_prompt_cancel_writes_nothing() {
        let dir = TempDir::new();
        let source = write_fixture(dir.path(), "tasks.md", "- [ ] PRJ-001 alpha\n");
        let before = read_file(&source);
        let mut app = app_for(dir.path());
        let _ = apply(&mut app, rune_key('m'));
        let _ = apply(&mut app, key(KeyCode::Esc));

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.status(), "cancelled");
        assert_eq!(read_file(&source), before);
    }
}
