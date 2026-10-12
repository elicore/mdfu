//! External `$EDITOR` driver for the note picker.
//!
//! Pressing `e` in list mode makes [`App::update`](super::App::update) return
//! [`Cmd::ExecEditor`](super::Cmd::ExecEditor); the run loop hands that argv to
//! [`exec_editor`], which:
//!
//! 1. **Suspends** the picker: raw mode off, alternate screen left, exactly
//!    reversing what `ratatui::init` set up.
//! 2. **Runs the editor** to completion with stdio inherited, so the editor
//!    owns the terminal and the picker never draws or reads input while it is
//!    open (Go `tea.ExecProcess`).
//! 3. **Resumes** the picker and returns
//!    [`Msg::EditorFinished`](super::Msg::EditorFinished), which
//!    `App::update` turns into a re-parse through the injected
//!    [`ReloadFn`](super::ReloadFn), a document replacement, a preview-cache
//!    invalidation, and a refilter.
//!
//! The argv is built by [`editor_argv`](super::editor_argv) (Go `editorArgv`):
//! the `$EDITOR` string split on whitespace, then the document path appended
//! last. The driver reads the path from that argv tail, so the path never
//! needs to be threaded through the command separately.
//!
//! `mdfu task open` and the tasks TUI open at a line with `+<line> <file>`
//! appended after the editor arguments (`internal/task/SPEC.md` §F.5); the
//! note picker has no line number and follows `internal/tui/tui.go`, whose
//! `editorArgv` appends only the path (`tui_test.go:1252-1288`).

use std::io;
use std::process::{Command, ExitStatus, Stdio};

use ratatui::backend::Backend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

use super::Msg;

/// Runs the editor while the picker's terminal is suspended.
///
/// Returns the [`Msg::EditorFinished`] to feed back into
/// [`App::update`](super::App::update). The call blocks until the editor
/// exits; the picker performs no terminal I/O of its own until it returns.
/// A blank or missing `$EDITOR` never reaches this function — the picker
/// reports it through the status bar before emitting any command.
#[must_use = "feed the returned message back into App::update"]
pub fn exec_editor<B: Backend<Error = io::Error>>(
    terminal: &mut Terminal<B>,
    argv: &[String],
) -> Msg {
    exec_editor_with(terminal, argv, suspend_terminal, resume_terminal)
}

/// [`exec_editor`] with the terminal enter/leave operations injected, so tests
/// can drive the whole flow without touching the real terminal.
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
        (Err(error), _) | (Ok(_), Err(error)) => Some(error),
        (Ok(_), Ok(())) => None,
    };
    finished_message(argv, error)
}

/// Releases the terminal for the editor: raw mode off and the alternate screen
/// left (Go bubbletea `ReleaseTerminal`). A failure aborts the editor run, so
/// the editor never inherits a raw terminal.
fn suspend_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)
}

/// Takes the terminal back after the editor exits: alternate screen on, raw
/// mode restored, the back buffer reset, and the cursor hidden for the
/// picker's own cursor-cell rendering (Go bubbletea `RestoreTerminal`).
fn resume_terminal<B: Backend<Error = io::Error>>(terminal: &mut Terminal<B>) -> io::Result<()> {
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    terminal.hide_cursor()
}

/// Builds the editor process with stdio inherited, so it reads and writes the
/// same terminal the picker uses (Go `editorCommand`).
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

/// Runs the editor to completion (Go `exec.Cmd.Run`) and turns a non-zero exit
/// into an error whose text matches Go's `exec.ExitError` (`exit status 1`),
/// so the status bar message stays familiar.
fn run_command(argv: &[String]) -> io::Result<ExitStatus> {
    let status = command_for(argv)?.status()?;
    if status.success() {
        Ok(status)
    } else {
        Err(io::Error::other(exit_error_message(status)))
    }
}

/// Go `exec.ExitError.Error()` for the common cases: `exit status <code>` for
/// a normal non-zero exit, `signal: <n>` for a signal without an exit code.
fn exit_error_message(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => signal_description(status),
    }
}

/// Signal fallback for `exit_error_message`; Unix reports the signal number
/// where Go reports a name (`signal: killed`), a documented divergence.
#[cfg(unix)]
fn signal_description(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;

    status
        .signal()
        .map_or_else(|| status.to_string(), |signal| format!("signal: {signal}"))
}

/// Signal fallback for `exit_error_message`; non-Unix platforms only have the
/// `ExitStatus` display form.
#[cfg(not(unix))]
fn signal_description(status: ExitStatus) -> String {
    status.to_string()
}

/// Builds the finish message: the path is the argv tail placed there by
/// [`editor_argv`](super::editor_argv).
fn finished_message(argv: &[String], error: Option<io::Error>) -> Msg {
    Msg::EditorFinished {
        path: argv.last().cloned().unwrap_or_default(),
        error: error.map(|error| error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use mdfu_core::model::Document;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;

    use super::*;
    use crate::notes::{App, Cmd, Config, ReloadFn};

    fn doc(path: &str, title: &str) -> Document {
        Document {
            path: path.to_owned(),
            title: title.to_owned(),
            body: format!("body of {title}"),
            ..Document::default()
        }
    }

    /// Query-independent filter that orders documents by title, so a reload
    /// that renames a document visibly re-ranks the visible list.
    fn title_filter(_query: &str, items: &[Document]) -> Vec<crate::notes::FilterHit> {
        let mut indices: Vec<usize> = (0..items.len()).collect();
        indices.sort_by_key(|index| items[*index].title.clone());
        indices
            .into_iter()
            .map(|index| crate::notes::FilterHit {
                index,
                matched_indexes: Vec::new(),
            })
            .collect()
    }

    fn picker(items: Vec<Document>, config: Config) -> App {
        App::new(items, config, Box::new(title_filter))
    }

    fn rune_key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn down_key() -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
    }

    fn test_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).expect("TestBackend terminal")
    }

    /// Drives `e` on the focused document and returns the editor argv the App
    /// emitted.
    fn press_e(app: &mut App) -> Vec<String> {
        let cmd = app
            .update(rune_key('e'))
            .expect("e returns the editor command");
        let Cmd::ExecEditor(argv) = cmd else {
            panic!("expected Cmd::ExecEditor");
        };
        argv
    }

    #[test]
    fn command_for_builds_program_and_args() {
        let argv = vec!["vim".to_owned(), "--wait".to_owned(), "a.md".to_owned()];
        let command = command_for(&argv).expect("argv builds a command");
        assert_eq!(command.get_program().to_string_lossy(), "vim");
        let rest: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(rest, ["--wait", "a.md"]);

        let error = command_for(&[]).expect_err("an empty argv is the blank-$EDITOR error");
        assert_eq!(error.to_string(), "$EDITOR is not set");
    }

    #[cfg(unix)]
    #[test]
    fn run_command_reports_exit_like_go() {
        let status = run_command(&["true".to_owned()]).expect("true exits 0");
        assert!(status.success());

        let error = run_command(&["false".to_owned()]).expect_err("false exits 1");
        assert_eq!(error.to_string(), "exit status 1");

        let error = run_command(&["mdfu-missing-editor-binary".to_owned()])
            .expect_err("a missing program is a spawn error");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);

        let error = run_command(&[]).expect_err("an empty argv errors");
        assert_eq!(error.to_string(), "$EDITOR is not set");
    }

    #[test]
    fn driver_runs_editor_and_reload_replaces_and_reranks() {
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let reload: ReloadFn = Box::new(move |path: &str| {
            counter.set(counter.get() + 1);
            Ok(Document {
                path: path.to_owned(),
                title: "aaa".to_owned(),
                ..Document::default()
            })
        });
        let mut app = picker(
            vec![
                doc("a.md", "alpha"),
                doc("b.md", "beta"),
                doc("c.md", "gamma"),
            ],
            Config {
                editor: Some("true".to_owned()),
                reload: Some(reload),
                ..Config::default()
            },
        );
        let _ = app.update(down_key());
        let argv = press_e(&mut app);
        assert_eq!(argv, ["true", "b.md"]);

        let mut terminal = test_terminal();
        let msg = exec_editor_with(&mut terminal, &argv, || Ok(()), |_| Ok(()));
        let _ = app.update(msg);

        assert_eq!(calls.get(), 1, "the injected reload runs once");
        assert_eq!(app.status(), "");
        assert_eq!(
            app.items()[1].title,
            "aaa",
            "the reload replaces the document"
        );
        assert_eq!(
            app.filtered(),
            [1, 0, 2],
            "the replacement re-ranks the visible list"
        );
        assert_eq!(
            app.cursor_index(),
            0,
            "the cursor follows the edited document"
        );
    }

    #[test]
    fn driver_editor_failure_keeps_document_and_sets_status() {
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let reload: ReloadFn = Box::new(move |path: &str| {
            counter.set(counter.get() + 1);
            Ok(Document {
                path: path.to_owned(),
                title: "must not load".to_owned(),
                ..Document::default()
            })
        });
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config {
                editor: Some("false".to_owned()),
                reload: Some(reload),
                ..Config::default()
            },
        );
        let _ = app.update(down_key());
        let argv = press_e(&mut app);
        assert_eq!(argv, ["false", "b.md"]);

        let mut terminal = test_terminal();
        let msg = exec_editor_with(&mut terminal, &argv, || Ok(()), |_| Ok(()));
        let _ = app.update(msg);

        assert_eq!(calls.get(), 0, "a failed editor aborts before the reload");
        assert_eq!(app.status(), "exit status 1");
        assert_eq!(app.items()[1].title, "beta", "the old document is kept");
        assert_eq!(app.filtered(), [0, 1]);
    }

    #[test]
    fn driver_reload_error_keeps_old_doc_and_sets_status() {
        let reload: ReloadFn = Box::new(|_| Err("parse: b.md: boom".to_owned()));
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config {
                editor: Some("true".to_owned()),
                reload: Some(reload),
                ..Config::default()
            },
        );
        let _ = app.update(down_key());
        let argv = press_e(&mut app);

        let mut terminal = test_terminal();
        let msg = exec_editor_with(&mut terminal, &argv, || Ok(()), |_| Ok(()));
        let _ = app.update(msg);

        assert_eq!(app.status(), "parse: b.md: boom");
        assert_eq!(app.items()[1].title, "beta", "the old document is kept");
        assert_eq!(app.filtered_docs()[1].title, "beta");
    }

    #[test]
    fn driver_suspend_failure_skips_the_editor() {
        let mut terminal = test_terminal();
        // `false` would report `exit status 1` if it ever ran.
        let argv = vec!["false".to_owned(), "b.md".to_owned()];
        let msg = exec_editor_with(
            &mut terminal,
            &argv,
            || Err(io::Error::other("no tty")),
            |_| Ok(()),
        );
        assert_eq!(
            msg,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: Some("no tty".to_owned()),
            }
        );
    }

    #[test]
    fn driver_resume_failure_is_reported() {
        let mut terminal = test_terminal();
        let argv = vec!["true".to_owned(), "b.md".to_owned()];
        let msg = exec_editor_with(
            &mut terminal,
            &argv,
            || Ok(()),
            |_| Err(io::Error::other("resume failed")),
        );
        assert_eq!(
            msg,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: Some("resume failed".to_owned()),
            }
        );
    }

    #[test]
    fn driver_empty_argv_errors_without_spawning() {
        let mut terminal = test_terminal();
        let msg = exec_editor_with(&mut terminal, &[], || Ok(()), |_| Ok(()));
        assert_eq!(
            msg,
            Msg::EditorFinished {
                path: String::new(),
                error: Some("$EDITOR is not set".to_owned()),
            }
        );
    }
}
