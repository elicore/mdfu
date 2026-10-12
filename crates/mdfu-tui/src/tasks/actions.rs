//! Task TUI actions: disk writes, `$EDITOR`, and scope reload.
//!
//! Port of the action half of the Go task TUI (`internal/tui/tasks_actions.go`
//! plus `commitMove`/`focusMoved`/`archiveFocused`/`taskBaseDir` from
//! `internal/tui/tasks_write.go`). Every mutation follows the write-then-update
//! ordering of the Go model:
//!
//! * `App::toggle_done_task` opens the focused task's file through the engine
//!   `FileEdit`, verifies the line is not stale, flips the checkbox, commits,
//!   and only then refreshes the in-memory task. A failed write leaves the
//!   model untouched and surfaces the error in the status line.
//! * `App::archive_focused` archives only a done task through `archive_tasks`
//!   and then reloads the scope.
//! * `App::open_in_editor_cmd` builds `$EDITOR +<line> <file>` (SPEC.md §F.5)
//!   and `App::editor_finished` reloads the scope when the editor exits.
//!
//! The edit/append/move flows (title, body, new task, move prompt) live in
//! [`super::edit`]; their files refresh the in-memory item directly, and the
//! scope reload provided here is what the editor-exit and archive paths use.
//!
//! # Reload loader
//!
//! Go's `TaskModel.reload` calls `task.LoadCheckboxes`, the **broadened**
//! loader that returns every markdown checkbox (any bullet/indent, `[X]`,
//! seeds, and unidentified lines). `mdfu-core` does not expose that loader, so
//! [`load_checkboxes`] assembles it from the public engine primitives —
//! `discover::walk`/`discover::filter`, `grammar::parse_checkbox`, and
//! `body::fence_mask`/`body::block_range_any`/`body::collect_body` — with the
//! same archive exclusion as `scope::load_all`. Paths stay exactly as the
//! walker reports them (no `rel_to_cwd`); a relative scope base therefore
//! yields the same relative spelling as Go, and an absolute base yields
//! absolute paths the engine resolves directly.
//!
//! Cursor identity across the reload is the model's task key (`id:` for
//! identified tasks, `file:line` for bare checkboxes), so a reload keeps the
//! cursor on the same task when it survives and never silently re-filters the
//! focused row away as part of a successful write.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use mdfu_core::task::config::DEFAULT_ARCHIVE_PATH;
use mdfu_core::task::{body, config as task_config, discover, edit, grammar, Task};
use ratatui::backend::Backend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

use super::{App, Cmd, Msg};

/// Loads every broadened checkbox in scope, mirroring Go `LoadCheckboxes`.
///
/// `base` is the resolved scope: an existing non-directory names a single file,
/// anything else is walked as a directory. Every markdown checkbox — any
/// bullet, any indent, `[ ]`/`[x]`/`[X]`, identified, seeded, or unidentified —
/// is returned with `file`, `line`, body, and body indent filled.
///
/// # Errors
///
/// Returns the walk/stat error when `base` cannot be walked, or
/// `cannot read <path>: <err>` when a discovered file cannot be read.
#[must_use = "the loaded tasks must be handled or refiltered"]
pub fn load_checkboxes(base: &str, cfg: &task_config::Config) -> Result<Vec<Task>, String> {
    let is_file = fs::metadata(base).is_ok_and(|metadata| !metadata.is_dir());
    let files = if is_file {
        vec![base.to_string()]
    } else {
        scope_files(base, cfg)?
    };

    let mut tasks = Vec::new();
    for path in files {
        let data = fs::read(&path).map_err(|err| format!("cannot read {path}: {err}"))?;
        collect_file_checkboxes(&path, &data, &mut tasks);
    }
    Ok(tasks)
}

/// Lists the files a directory scope covers: the walk, include/exclude globs,
/// and archive exclusion of Go `scopeFiles`.
fn scope_files(base: &str, cfg: &task_config::Config) -> Result<Vec<String>, String> {
    let mut options = discover::DiscoverOptions {
        follow_symlinks: true,
        exclude_dirs: discover::exclude_dirs_from_env(|key| std::env::var(key).ok()),
        warn: None,
    };
    let paths = discover::walk(base, &mut options).map_err(|err| err.to_string())?;
    let paths = discover::filter(&paths, base, &cfg.include, &cfg.exclude);
    let archive = resolve_archive(base, &cfg.archive_path);
    Ok(paths
        .into_iter()
        .filter(|path| !same_path(path, &archive))
        .collect())
}

/// Parses one file's unmasked lines into `out`, ending each body at the next
/// checkbox at any indent (Go `LoadCheckboxes` body filling).
fn collect_file_checkboxes(path: &str, data: &[u8], out: &mut Vec<Task>) {
    let text = String::from_utf8_lossy(data);
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let mask = body::fence_mask(&lines);
    for (index, raw) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        let Some(mut task) = grammar::parse_checkbox(raw) else {
            continue;
        };
        if let Some(end) = body::block_range_any(&lines, index) {
            let (body_text, body_indent) = body::collect_body(&lines, index, end);
            task.body = body_text;
            task.body_indent = body_indent;
        }
        task.file = path.to_string();
        task.line = index + 1;
        out.push(task);
    }
}

/// Resolves the archive destination against `base`, defaulting it when empty
/// (Go `resolveArchive`).
fn resolve_archive(base: &str, archive_path: &str) -> String {
    let name = if archive_path.is_empty() {
        DEFAULT_ARCHIVE_PATH
    } else {
        archive_path
    };
    if Path::new(name).is_absolute() {
        name.to_string()
    } else {
        Path::new(base).join(name).to_string_lossy().into_owned()
    }
}

/// Reports whether two paths name the same existing location (the archive
/// exclusion only ever compares paths the walker has just produced).
fn same_path(left: &str, right: &str) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Returns the directory component with Go `filepath.Dir` semantics.
fn go_dir(path: &str) -> String {
    let parent = Path::new(path)
        .parent()
        .map_or(String::new(), |dir| dir.to_string_lossy().into_owned());
    if parent.is_empty() {
        ".".to_string()
    } else {
        parent
    }
}

/// Builds the `$EDITOR` argv exactly like Go `openInEditorCmd`: the editor
/// string split on whitespace, then `+<line>` and the file appended after any
/// arguments already present (SPEC.md §F.5).
///
/// # Errors
///
/// Returns `"$EDITOR is not set"` when `editor` is empty or all whitespace.
#[must_use = "the argv is handed to Cmd::ExecEditor"]
pub fn editor_argv(editor: &str, line: usize, path: &str) -> Result<Vec<String>, String> {
    let mut fields = editor.split_whitespace();
    let Some(program) = fields.next() else {
        return Err("$EDITOR is not set".to_owned());
    };
    let mut argv = vec![program.to_owned()];
    argv.extend(fields.map(str::to_owned));
    argv.push(format!("+{line}"));
    argv.push(path.to_owned());
    Ok(argv)
}

/// Opens the focused task's file, verifies the header, and flips its checkbox.
fn flip_on_disk(task: &Task, path: &str) -> Result<(), String> {
    let mut file_edit = edit::open(path).map_err(|err| err.to_string())?;
    file_edit.verify_task(task).map_err(|err| err.to_string())?;
    file_edit
        .flip_checkbox(task.line)
        .map_err(|err| err.to_string())?;
    file_edit.commit().map_err(|err| err.to_string())
}

impl App {
    /// Returns the focused task and the file to mutate (Go
    /// `focusedTaskAndPath`). A blank path means the item carries no file
    /// (headless fixtures); callers fall back to in-memory behaviour.
    pub(crate) fn focused_task_and_path(&self) -> Option<(Task, String)> {
        let index = *self.filtered.get(self.cursor)?;
        let task = self.tasks.get(index)?.clone();
        let mut path = task.file.clone();
        if path.is_empty()
            && !self.base.is_empty()
            && fs::metadata(&self.base).is_ok_and(|metadata| !metadata.is_dir())
        {
            path.clone_from(&self.base);
        }
        Some((task, path))
    }

    /// Replaces the focused task in the backing slice after a successful write
    /// (Go `updateFocusedTask`). With index filtering the visible row and the
    /// backing task are the same slot, so one assignment covers both views.
    pub(crate) fn update_focused_task(&mut self, task: Task) {
        let Some(index) = self.filtered.get(self.cursor).copied() else {
            return;
        };
        if let Some(slot) = self.tasks.get_mut(index) {
            *slot = task;
        }
    }

    /// Flips the focused task's checkbox through the engine and refreshes the
    /// in-memory item only after a successful commit (Go `toggleDoneTask`).
    /// Without a file path it degrades to the in-memory toggle used by headless
    /// fixtures. A failed write surfaces the error and touches no state; it
    /// deliberately does not refilter, so a row that just became done cannot be
    /// silently dropped from under the cursor.
    pub(crate) fn toggle_done_task(&mut self) {
        let Some((task, path)) = self.focused_task_and_path() else {
            return;
        };
        if path.is_empty() {
            self.toggle_done();
            return;
        }
        if let Err(message) = flip_on_disk(&task, &path) {
            self.set_error(message);
            return;
        }
        let mut updated = task;
        updated.checked = !updated.checked;
        updated.status = if updated.checked { b'x' } else { b' ' };
        self.update_focused_task(updated);
    }

    /// Suspends the program to run `$EDITOR +<line> <file>` (Go
    /// `openInEditorCmd`). Without a usable `$EDITOR` the status line reports
    /// `"$EDITOR is not set"` and no command is emitted.
    pub(crate) fn open_in_editor_cmd(&mut self) -> Option<Cmd> {
        let (task, path) = self.focused_task_and_path()?;
        let editor = self.editor.as_deref().unwrap_or_default();
        match editor_argv(editor, task.line, &path) {
            Ok(argv) => Some(Cmd::ExecEditor(argv)),
            Err(message) => {
                self.set_error(message);
                None
            }
        }
    }

    /// Handles the external editor finishing (Go `taskEditorFinishedMsg`): an
    /// error lands in the status line, success reloads the scope so external
    /// edits appear.
    pub(crate) fn editor_finished(&mut self, error: Option<String>) {
        if let Some(message) = error {
            self.set_error(message);
            return;
        }
        self.reload();
    }

    /// Archives the focused task only when it is done (Go `archiveFocused`).
    /// An open task shows the TUI-only status `"task is not done"` and writes
    /// nothing; engine errors land in the status line without touching the
    /// model.
    pub(crate) fn archive_focused(&mut self) {
        let Some((task, _)) = self.focused_task_and_path() else {
            return;
        };
        if !task.checked {
            "task is not done".clone_into(&mut self.status);
            return;
        }
        let base_dir = self.task_base_dir();
        if let Err(err) =
            edit::archive_tasks(std::slice::from_ref(&task), &base_dir, &self.task_config)
        {
            self.set_error(err.to_string());
            return;
        }
        "archived".clone_into(&mut self.status);
        self.reload();
    }

    /// Resolves the directory the archive destination is measured against (Go
    /// `taskBaseDir`): a single-file scope uses that file's directory, and an
    /// unset base falls back to the focused task's directory.
    fn task_base_dir(&self) -> String {
        if self.base.is_empty() {
            return self.focused_task_and_path().map_or_else(
                || ".".to_owned(),
                |(_, path)| {
                    if path.is_empty() {
                        ".".to_owned()
                    } else {
                        go_dir(&path)
                    }
                },
            );
        }
        if fs::metadata(&self.base).is_ok_and(|metadata| !metadata.is_dir()) {
            return go_dir(&self.base);
        }
        self.base.clone()
    }

    /// Re-reads the task scope from disk through the loader and refilters,
    /// keeping the cursor on the same task when it survives (Go `reload`). An
    /// empty base only refilters, like Go.
    pub(crate) fn reload(&mut self) {
        if self.base.is_empty() {
            self.refilter();
            return;
        }
        match load_checkboxes(&self.base, &self.task_config) {
            Ok(tasks) => {
                self.tasks = tasks;
                self.refilter();
            }
            Err(message) => self.set_error(message),
        }
    }

    /// Stores one failure in both the status line and the error slot (Go sets
    /// `m.err` and `m.status` together for every action failure).
    pub(crate) fn set_error(&mut self, message: String) {
        self.status.clone_from(&message);
        self.error = Some(message);
    }
}

/// Runs the editor while the task TUI's terminal is suspended.
///
/// The driver mirrors the note picker's editor lifecycle: raw mode off and the
/// alternate screen left, the editor runs to completion with inherited stdio
/// (Go `tea.ExecProcess`), then the terminal is restored and
/// [`Msg::EditorFinished`] is returned for
/// [`App::update`](super::App::update). The task path is the argv tail placed
/// there by `editor_argv`, so the message needs no separate path.
#[must_use = "feed the returned message back into App::update"]
pub fn exec_editor<B: Backend<Error = io::Error>>(
    terminal: &mut Terminal<B>,
    argv: &[String],
) -> Msg {
    exec_editor_with(terminal, argv, suspend_terminal, resume_terminal)
}

/// [`exec_editor`] with the terminal enter/leave operations injected, so tests
/// drive the whole flow without touching the real terminal.
fn exec_editor_with<B: Backend>(
    terminal: &mut Terminal<B>,
    argv: &[String],
    suspend: impl FnOnce() -> io::Result<()>,
    resume: impl FnOnce(&mut Terminal<B>) -> io::Result<()>,
) -> Msg {
    // Go bubbletea aborts before starting the editor when the terminal cannot
    // be released; the editor is guaranteed not to be running when we resume.
    let run = suspend().and_then(|()| run_command(argv));
    // The editor has exited by now, so handing the terminal back is safe even
    // when it failed to start.
    let restored = resume(terminal);
    let error = match (run, restored) {
        (Err(error), _) | (Ok(_), Err(error)) => Some(error.to_string()),
        (Ok(_), Ok(())) => None,
    };
    Msg::EditorFinished { error }
}

/// Releases the terminal for the editor: raw mode off and the alternate screen
/// left (Go bubbletea `ReleaseTerminal`).
fn suspend_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)
}

/// Takes the terminal back after the editor exits: alternate screen on, raw
/// mode restored, the back buffer reset, and the cursor hidden.
fn resume_terminal<B: Backend<Error = io::Error>>(terminal: &mut Terminal<B>) -> io::Result<()> {
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    terminal.hide_cursor()
}

/// Builds the editor process with stdio inherited (Go `editorCommand`).
fn command_for(argv: &[String]) -> io::Result<Command> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "$EDITOR is not set"))?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    Ok(command)
}

/// Runs the editor to completion and turns a non-zero exit into an error whose
/// text matches Go's `exec.ExitError` (`exit status 1`).
fn run_command(argv: &[String]) -> io::Result<ExitStatus> {
    let status = command_for(argv)?.status()?;
    if status.success() {
        Ok(status)
    } else {
        Err(io::Error::other(exit_error_message(status)))
    }
}

/// Go `exec.ExitError.Error()` for the common cases.
fn exit_error_message(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => signal_description(status),
    }
}

/// Signal fallback; Unix reports the signal number where Go reports a name
/// (`signal: killed`), a documented divergence.
#[cfg(unix)]
fn signal_description(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;

    status
        .signal()
        .map_or_else(|| status.to_string(), |signal| format!("signal: {signal}"))
}

/// Signal fallback for non-Unix platforms.
#[cfg(not(unix))]
fn signal_description(status: ExitStatus) -> String {
    status.to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use mdfu_core::task::scope;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;

    use super::*;
    use crate::tasks::Config;

    /// Minimal temp vault (no `tempfile` dev-dependency in this crate).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
            let unique = format!(
                "mdfu-task-actions-{}-{sequence}-{label}",
                std::process::id()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn path_string(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }

        fn write(&self, name: &str, content: &str) {
            fs::write(self.path.join(name), content).expect("write fixture");
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.path.join(name)).expect("read fixture")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn no_env(_key: &str) -> Option<String> {
        None
    }

    fn config_for(base: &str) -> Config {
        Config {
            base: base.to_owned(),
            ..Config::default()
        }
    }

    /// The strict loader: Go `LoadAll` (`parse_header`, so `verify_task` is
    /// ID-based), which is what the Go action tests (`newTaskVault`) load. The
    /// empty `cwd` keeps the walked paths absolute, so the engine resolves them
    /// from the test process cwd (the Go tests `t.Chdir` instead).
    fn strict_tasks(base: &str) -> Vec<Task> {
        let (tasks, _) = scope::load_all(base, &task_config::Config::default(), "", &no_env)
            .expect("load strict tasks");
        tasks
    }

    fn broad_tasks(base: &str) -> Vec<Task> {
        load_checkboxes(base, &task_config::Config::default()).expect("load broad tasks")
    }

    fn app_for(tasks: Vec<Task>, base: &str, show_done: bool) -> App {
        App::with_default_filter(
            tasks,
            Config {
                base: base.to_owned(),
                show_done,
                ..Config::default()
            },
        )
    }

    fn apply(app: &mut App, msg: Msg) -> Option<Cmd> {
        app.update(msg)
    }

    fn rune_key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn down_key() -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
    }

    fn focused_id(app: &App) -> String {
        app.filtered_tasks()[app.cursor_index()].id.clone()
    }

    #[test]
    fn task_actions_toggle_writes_file_and_refreshes() {
        let vault = TempDir::new("toggle");
        vault.write(
            "tasks.md",
            "- [ ] PRJ-001 alpha\n  body\n- [ ] PRJ-002 beta\n",
        );
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);

        let _ = apply(&mut app, rune_key('x'));

        assert_eq!(
            vault.read("tasks.md"),
            "- [x] PRJ-001 alpha\n  body\n- [ ] PRJ-002 beta\n"
        );
        assert!(app.filtered_tasks()[0].checked);
        assert_eq!(app.filtered_tasks()[0].status, b'x');
        assert_eq!(app.status(), "");
        assert_eq!(app.error(), None);
    }

    #[test]
    fn task_actions_toggle_any_checkbox_bullets() {
        let cases = [
            ("  * [ ] buy milk\n", "  * [x] buy milk\n"),
            ("+ [ ] buy eggs\n", "+ [x] buy eggs\n"),
            ("- [X] shipped\n", "- [ ] shipped\n"),
        ];
        for (before, want) in cases {
            let vault = TempDir::new("broad");
            vault.write("tasks.md", before);
            let base = vault.path_string();
            let mut app = app_for(broad_tasks(&base), &base, true);

            let _ = apply(&mut app, rune_key('x'));

            assert_eq!(vault.read("tasks.md"), want, "input {before:?}");
            assert_eq!(app.status(), "", "input {before:?}");
        }
    }

    #[test]
    fn task_actions_toggle_stale_leaves_file_and_model() {
        let vault = TempDir::new("stale");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n  body\n");
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);
        let mutated = "- [ ] PRJ-009 intruder\n- [ ] PRJ-001 alpha\n  body\n";
        vault.write("tasks.md", mutated);

        let _ = apply(&mut app, rune_key('x'));

        assert!(
            app.status().contains("file changed"),
            "status = {:?}",
            app.status()
        );
        assert!(app
            .error()
            .is_some_and(|error| error.contains("file changed")));
        assert_eq!(
            vault.read("tasks.md"),
            mutated,
            "file modified on stale toggle"
        );
        assert!(
            !app.filtered_tasks()[0].checked,
            "model changed on stale toggle"
        );
    }

    #[cfg(unix)]
    #[test]
    fn task_actions_toggle_unwritable_surfaces_error_without_model_change() {
        use std::os::unix::fs::PermissionsExt as _;

        let vault = TempDir::new("unwritable");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);

        // A read-only file alone cannot fail the write: the engine replaces it
        // by rename, which only needs write access to the directory. The
        // read-only vault is the genuine failed-write case.
        fs::set_permissions(
            vault.path().join("tasks.md"),
            fs::Permissions::from_mode(0o444),
        )
        .expect("chmod file");
        fs::set_permissions(vault.path(), fs::Permissions::from_mode(0o555)).expect("chmod dir");
        // Root ignores the write bit; probe instead of failing the suite.
        if dir_writable(vault.path()) {
            fs::set_permissions(vault.path(), fs::Permissions::from_mode(0o755)).expect("restore");
            return;
        }

        let _ = apply(&mut app, rune_key('x'));

        fs::set_permissions(vault.path(), fs::Permissions::from_mode(0o755)).expect("restore");
        assert!(
            !app.status().is_empty(),
            "failed write must surface a status"
        );
        assert!(
            app.error().is_some(),
            "failed write must set the error slot"
        );
        assert_eq!(vault.read("tasks.md"), "- [ ] PRJ-001 alpha\n");
        assert!(
            !app.filtered_tasks()[0].checked,
            "model changed on failed write"
        );
        assert_eq!(app.error(), Some(app.status()));
    }

    #[cfg(unix)]
    fn dir_writable(dir: &Path) -> bool {
        let probe = dir.join(".mdfu-write-probe");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    }

    #[test]
    fn task_actions_toggle_then_archive_in_temp_vault() {
        let vault = TempDir::new("archive");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n  body\n");
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, true);

        let _ = apply(&mut app, rune_key('x'));
        assert_eq!(vault.read("tasks.md"), "- [x] PRJ-001 alpha\n  body\n");

        let _ = apply(&mut app, rune_key('a'));

        assert_eq!(vault.read("tasks.md"), "\n");
        assert_eq!(vault.read("_archive.md"), "- [x] PRJ-001 alpha\n  body\n");
        assert_eq!(app.status(), "archived");
        assert!(
            app.filtered().is_empty(),
            "archived row must leave the list"
        );
    }

    #[test]
    fn task_actions_archive_not_done_writes_nothing() {
        let vault = TempDir::new("open-archive");
        vault.write("tasks.md", "- [ ] PRJ-001 open\n");
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);

        let _ = apply(&mut app, rune_key('a'));

        assert_eq!(app.status(), "task is not done");
        assert_eq!(vault.read("tasks.md"), "- [ ] PRJ-001 open\n");
        assert!(!vault.path().join("_archive.md").exists());
    }

    #[test]
    fn task_actions_archive_broad_done_task() {
        let vault = TempDir::new("archive-broad");
        vault.write("tasks.md", "- [ ] open\n* [x] done broad\n  broad body\n");
        let base = vault.path_string();
        let mut app = app_for(broad_tasks(&base), &base, true);

        let _ = apply(&mut app, down_key());
        assert_eq!(app.filtered_tasks()[1].title, "done broad");
        let _ = apply(&mut app, rune_key('a'));

        assert_eq!(vault.read("tasks.md"), "- [ ] open\n");
        assert_eq!(
            vault.read("_archive.md"),
            "* [x] done broad\n  broad body\n"
        );
    }

    #[test]
    fn task_actions_duplicate_line_keys_stay_distinct() {
        let vault = TempDir::new("duplicates");
        vault.write("tasks.md", "- [ ] same\n- [ ] same\n");
        let base = vault.path_string();
        let tasks = broad_tasks(&base);
        assert_eq!(tasks.len(), 2);
        assert_ne!(
            (tasks[0].file.clone(), tasks[0].line),
            (tasks[1].file.clone(), tasks[1].line),
            "duplicate lines must not share a key"
        );

        let mut app = app_for(tasks, &base, true);
        let _ = apply(&mut app, down_key());
        let _ = apply(&mut app, rune_key('x'));

        assert_eq!(vault.read("tasks.md"), "- [ ] same\n- [x] same\n");
        // The cursor stays on the second, now-done duplicate across a reload.
        let _ = apply(&mut app, Msg::EditorFinished { error: None });
        assert_eq!(app.filtered_tasks()[app.cursor_index()].line, 2);
    }

    #[test]
    fn task_actions_editor_argv_appends_line_and_file() {
        assert_eq!(
            editor_argv("vim --wait", 7, "a.md").expect("usable editor"),
            ["vim", "--wait", "+7", "a.md"]
        );
        assert_eq!(
            editor_argv("code", 1, "sub/tasks.md").expect("usable editor"),
            ["code", "+1", "sub/tasks.md"]
        );
        for editor in ["", "   ", "\t\n"] {
            let error = editor_argv(editor, 1, "a.md").expect_err("blank editor");
            assert_eq!(error, "$EDITOR is not set");
        }
    }

    #[test]
    fn task_actions_open_sets_status_without_editor() {
        let vault = TempDir::new("no-editor");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        for editor in [None, Some(String::new()), Some("   ".to_owned())] {
            let mut config = config_for(&base);
            config.editor = editor;
            let mut app = App::with_default_filter(strict_tasks(&base), config);

            let cmd = apply(&mut app, rune_key('o'));

            assert!(cmd.is_none(), "no command for a blank $EDITOR");
            assert_eq!(app.status(), "$EDITOR is not set");
        }
    }

    #[test]
    fn task_actions_open_returns_exec_cmd_with_line_and_path() {
        let vault = TempDir::new("editor");
        vault.write("tasks.md", "text\n- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        let file = vault.path().join("tasks.md").to_string_lossy().into_owned();
        let mut config = config_for(&base);
        config.editor = Some("vim --wait".to_owned());
        let mut app = App::with_default_filter(strict_tasks(&base), config);

        let cmd = apply(&mut app, rune_key('o'));

        assert_eq!(
            cmd,
            Some(Cmd::ExecEditor(vec![
                "vim".to_owned(),
                "--wait".to_owned(),
                "+2".to_owned(),
                file,
            ]))
        );
        assert_eq!(app.status(), "");
    }

    #[test]
    fn task_actions_editor_finished_reloads_and_keeps_cursor() {
        let vault = TempDir::new("reload");
        vault.write(
            "tasks.md",
            "- [ ] PRJ-001 alpha\n- [ ] PRJ-002 beta\n- [ ] PRJ-003 gamma\n",
        );
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);
        let _ = apply(&mut app, down_key());
        assert_eq!(focused_id(&app), "PRJ-002");

        vault.write(
            "tasks.md",
            "- [ ] PRJ-001 alpha\n- [ ] PRJ-002 beta edited\n- [ ] PRJ-003 gamma\n",
        );
        let _ = apply(&mut app, Msg::EditorFinished { error: None });

        assert_eq!(focused_id(&app), "PRJ-002", "cursor follows the task");
        assert_eq!(
            app.filtered_tasks()[app.cursor_index()].title,
            "beta edited"
        );
        assert_eq!(app.status(), "");
    }

    #[test]
    fn task_actions_editor_finished_error_keeps_model() {
        let vault = TempDir::new("editor-error");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        let mut app = app_for(strict_tasks(&base), &base, false);

        let _ = apply(
            &mut app,
            Msg::EditorFinished {
                error: Some("exit status 1".to_owned()),
            },
        );

        assert_eq!(app.status(), "exit status 1");
        assert_eq!(app.tasks().len(), 1, "an editor error must not reload");
    }

    #[test]
    fn task_actions_reload_error_surfaces_status_and_keeps_model() {
        let vault = TempDir::new("reload-error");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        let missing = vault.path().join("missing").to_string_lossy().into_owned();
        let mut app = app_for(strict_tasks(&base), &missing, false);

        let _ = apply(&mut app, Msg::EditorFinished { error: None });

        assert!(
            app.status().contains("cannot stat"),
            "status = {:?}",
            app.status()
        );
        assert_eq!(
            app.tasks().len(),
            1,
            "the old list is kept on a reload error"
        );
    }

    #[test]
    fn task_actions_load_checkboxes_broad_and_archive_excluded() {
        let vault = TempDir::new("loader");
        vault.write(
            "tasks.md",
            "```\n- [ ] fenced\n```\n  * [X] upper\n- [ ] PRJ-1 id\n",
        );
        vault.write("_archive.md", "- [x] old\n");
        let base = vault.path_string();

        let tasks = load_checkboxes(&base, &task_config::Config::default()).expect("load");

        let titles: Vec<&str> = tasks.iter().map(|task| task.title.as_str()).collect();
        assert_eq!(titles, ["upper", "id"]);
        assert!(tasks[0].broad && tasks[0].checked);
        assert_eq!(tasks[1].line, 5);
    }

    #[test]
    fn task_actions_load_checkboxes_single_file_scope() {
        let vault = TempDir::new("single-file");
        vault.write("tasks.md", "- [ ] one\n");
        vault.write("other.md", "- [ ] other\n");
        let file = vault.path().join("tasks.md").to_string_lossy().into_owned();

        let tasks = load_checkboxes(&file, &task_config::Config::default()).expect("load");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "one");
        assert_eq!(tasks[0].file, file);
    }

    fn test_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).expect("TestBackend terminal")
    }

    #[test]
    fn task_actions_exec_editor_reports_success() {
        let mut terminal = test_terminal();
        let argv = vec!["true".to_owned(), "+1".to_owned(), "a.md".to_owned()];
        let msg = exec_editor_with(&mut terminal, &argv, || Ok(()), |_| Ok(()));
        assert_eq!(msg, Msg::EditorFinished { error: None });
    }

    #[cfg(unix)]
    #[test]
    fn task_actions_exec_editor_reports_exit_status() {
        let mut terminal = test_terminal();
        let argv = vec!["false".to_owned(), "+1".to_owned(), "a.md".to_owned()];
        let msg = exec_editor_with(&mut terminal, &argv, || Ok(()), |_| Ok(()));
        assert_eq!(
            msg,
            Msg::EditorFinished {
                error: Some("exit status 1".to_owned()),
            }
        );
    }

    #[test]
    fn task_actions_exec_editor_suspend_failure_skips_the_editor() {
        let mut terminal = test_terminal();
        // `false` would report `exit status 1` if it ever ran.
        let argv = vec!["false".to_owned(), "+1".to_owned(), "a.md".to_owned()];
        let msg = exec_editor_with(
            &mut terminal,
            &argv,
            || Err(io::Error::other("no tty")),
            |_| Ok(()),
        );
        assert_eq!(
            msg,
            Msg::EditorFinished {
                error: Some("no tty".to_owned()),
            }
        );
    }

    #[test]
    fn task_actions_exec_editor_empty_argv_errors_without_spawning() {
        let mut terminal = test_terminal();
        let msg = exec_editor_with(&mut terminal, &[], || Ok(()), |_| Ok(()));
        assert_eq!(
            msg,
            Msg::EditorFinished {
                error: Some("$EDITOR is not set".to_owned()),
            }
        );
    }
}
