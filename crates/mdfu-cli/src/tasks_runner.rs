//! Interactive tasks runner: the TTY `mdfu tasks` entry point.
//!
//! Port of `runTasksTUI` from `cmd/mdfu/task.go` of the frozen Go baseline
//! (`PARITY.md` records the SHA): resolve the same `--path`/`--all`/
//! `--blocked`/`--sort`/`--tag`/`--priority` surface as `task list`, load the
//! **broadened** checkbox scope (any bullet, indent, `[X]`, seeds, and
//! unidentified lines), select the rows, resolve the theme, and hand the
//! resulting [`App`] to the ratatui event loop. The non-TTY `tasks` branch
//! never reaches this module; the router keeps it structurally identical to
//! `mdfu task list`.
//!
//! The loop mirrors [`crate::pipeline::drive_picker`]: ratatui `try_init`, one
//! frame per event, `KeyEventKind::Release` ignored, [`Cmd::Quit`] ends the
//! loop, and [`Cmd::ExecEditor`] suspends the terminal through
//! [`mdfu_tui::tasks::exec_editor`] (the same suspend/run/resume driver as the
//! note picker) before feeding [`Msg::EditorFinished`] back so the scope
//! reloads. Quit and abort both exit `0`, like Go. Loading, flag, or theme
//! failures happen **before** the terminal is initialized, so their messages
//! never paint over an alternate screen.
//!
//! Filtering is injected with the task UI's own [`TaskFilterFn`]
//! ([`App::with_default_filter`] = Go `FilterTaskItems`); the note picker's
//! filter type and global are never referenced.
//!
//! Divergences (recorded in the task evidence): Go's `task.LoadCheckboxes`
//! reports paths relative to the scope cwd; the broadened loader in
//! `mdfu_tui::tasks` keeps the walker's paths as-is, so an absolute `--path`
//! scope feeds absolute file names into the actions. Task identity (ID or
//! file+line) and every rendered field are unaffected. Go's SIGINT/SIGTERM
//! force-restore handler is not ported; the crossterm ctrl-c key event aborts
//! and ratatui's panic hook restores the terminal, exactly like the note
//! picker's documented divergence.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use mdfu_core::task::render::{select_visible, sort_by_priority, ListOptions};
use mdfu_core::task::scope;
use mdfu_core::theme;
use mdfu_tui::tasks::{self, App, Cmd, Config, Msg};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::Terminal;

/// The TTY `mdfu tasks` entry point (Go `runTasksTUI`).
///
/// Returns the process exit code: `0` for a normal quit or an abort, `1` for a
/// scope/load/terminal/event failure, and `2` for a flag or theme usage error,
/// matching the Go SPEC `0/1/2` contract. `stdout` is part of the router hook
/// signature for symmetry with the Go function; the interactive path paints
/// the real terminal and writes nothing to it.
#[must_use = "the exit code belongs to the process"]
pub fn run_tasks_tui(
    _stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    args: &[String],
    config: &str,
) -> i32 {
    let cwd = std::env::current_dir().map_or_else(
        |_| ".".to_string(),
        |path| path.to_string_lossy().into_owned(),
    );
    let get_env = |key: &str| std::env::var(key).ok();
    let mut app = match prepare(&cwd, &get_env, args, config, stderr) {
        Ok(app) => app,
        Err(code) => return code,
    };

    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            write_error(stderr, &error.to_string());
            return 1;
        }
    };
    let outcome = drive_tasks(&mut terminal, &mut app, event::read, tasks::exec_editor);
    ratatui::restore();
    if let Err(message) = outcome {
        write_error(stderr, &message);
        return 1;
    }
    0
}

/// Resolves the scope, parses the TUI flag subset, loads the broadened
/// checkboxes, applies the tag/priority selection, resolves the theme, and
/// builds the task [`App`] — everything Go does before `tea.NewProgram`.
///
/// `Err(code)` is the process exit code for a scope (`1`), flag (`0` help /
/// `2` usage), load (`1`), or theme (`2`) failure, already reported on
/// `stderr`.
fn prepare(
    cwd: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
    args: &[String],
    theme_path: &str,
    stderr: &mut dyn Write,
) -> Result<App, i32> {
    let (scope, rest) = match scope::resolve_scope(cwd, get_env, args) {
        Ok(parts) => parts,
        Err(error) => {
            let _ = writeln!(stderr, "mdfu task: {error}");
            return Err(1);
        }
    };
    let parsed = parse_flags(stderr, "tasks", TASKS_FLAGS, &rest)?;

    let mut tags = parsed.list_flag("tag").to_vec();
    let mut priorities = parsed.list_flag("priority").to_vec();
    for arg in &parsed.positionals {
        if arg.starts_with('#') {
            tags.push(arg.clone());
        } else if arg.starts_with('!') {
            priorities.push(arg.clone());
        } else {
            let _ = writeln!(stderr, "mdfu tasks: unexpected argument '{arg}'");
            return Err(2);
        }
    }

    let tasks = match tasks::load_checkboxes(&scope.base, &scope.config) {
        Ok(tasks) => tasks,
        Err(error) => {
            let _ = writeln!(stderr, "mdfu task: {error}");
            return Err(1);
        }
    };

    // Go `SelectTasks`: the done/blocked defaults are disabled here because
    // the model owns visibility; the flags only feed the gates below.
    let options = ListOptions {
        all: true,
        blocked: true,
        tag_filter: tags,
        priority_filter: priorities,
        ..ListOptions::default()
    };
    let mut selected = select_visible(&tasks, &options);
    if parsed.text_flag("sort") == "priority" {
        sort_by_priority(&mut selected);
    }

    let explicit = (!theme_path.is_empty()).then(|| Path::new(theme_path));
    let theme = match theme::resolve(explicit, stderr) {
        Ok(theme) => theme,
        Err(error) => {
            write_error(stderr, &error.to_string());
            return Err(2);
        }
    };

    let config = Config {
        theme: Some(theme),
        show_blocked: parsed.bool_flag("blocked"),
        show_done: parsed.bool_flag("all"),
        base: scope.base.clone(),
        task_config: scope.config.clone(),
        editor: get_env("EDITOR"),
    };
    Ok(App::with_default_filter(selected, config))
}

/// Builds the picker and pumps its event loop until it quits.
///
/// `next_event` and `run_editor` are injected so tests can drive the whole
/// loop headlessly: a finished [`Cmd::ExecEditor`] becomes
/// [`Msg::EditorFinished`] through `run_editor` and is fed straight back into
/// [`App::update`] (the production runner passes
/// [`mdfu_tui::tasks::exec_editor`]). Returns an error string for a terminal
/// draw/size or event-read failure.
fn drive_tasks<B, R, E>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mut next_event: R,
    mut run_editor: E,
) -> Result<(), String>
where
    B: Backend,
    B::Error: std::fmt::Display,
    R: FnMut() -> io::Result<Event>,
    E: FnMut(&mut Terminal<B>, &[String]) -> Msg,
{
    let size = terminal.size().map_err(|error| error.to_string())?;
    let _ = app.update(Msg::Resize {
        width: size.width,
        height: size.height,
    });

    loop {
        terminal
            .draw(|frame| app.view(frame))
            .map_err(|error| error.to_string())?;
        match next_event().map_err(|error| error.to_string())? {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                let Some(command) = app.update(Msg::Key(key)) else {
                    continue;
                };
                match command {
                    Cmd::Quit => break,
                    Cmd::ExecEditor(argv) => {
                        let message = run_editor(terminal, &argv);
                        let _ = app.update(message);
                    }
                }
            }
            Event::Resize(width, height) => {
                let _ = app.update(Msg::Resize { width, height });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Writes one `mdfu: ...` failure line, matching Go's `fmt.Fprintln`.
fn write_error(stderr: &mut dyn Write, message: &str) {
    let _ = writeln!(stderr, "mdfu: {message}");
}

/// Kind of value a flag accepts (Go `flag` typing for `PrintDefaults`).
#[derive(Clone, Copy)]
enum FlagKind {
    /// A boolean flag; `-flag` alone means true.
    Bool,
    /// A single string value.
    Value,
    /// A repeatable string value.
    List,
}

/// One flag specification for [`parse_flags`].
#[derive(Clone, Copy)]
struct FlagDef {
    name: &'static str,
    kind: FlagKind,
    usage: &'static str,
}

/// The `mdfu tasks` flag set (Go `runTasksTUI`): the `list` flags minus
/// `--json`, which has no meaning for the browser.
const TASKS_FLAGS: &[FlagDef] = &[
    FlagDef {
        name: "all",
        kind: FlagKind::Bool,
        usage: "include done tasks",
    },
    FlagDef {
        name: "blocked",
        kind: FlagKind::Bool,
        usage: "include blocked tasks",
    },
    FlagDef {
        name: "sort",
        kind: FlagKind::Value,
        usage: "sort field",
    },
    FlagDef {
        name: "tag",
        kind: FlagKind::List,
        usage: "filter by tag (repeatable; AND)",
    },
    FlagDef {
        name: "priority",
        kind: FlagKind::List,
        usage: "filter by priority (repeatable; OR)",
    },
];

/// Values parsed for one `tasks` invocation.
#[derive(Default)]
struct ParsedFlags {
    bools: BTreeMap<&'static str, bool>,
    values: BTreeMap<&'static str, String>,
    lists: BTreeMap<&'static str, Vec<String>>,
    positionals: Vec<String>,
}

impl ParsedFlags {
    /// One boolean flag's value; absent means `false`.
    fn bool_flag(&self, name: &str) -> bool {
        self.bools.get(name).copied().unwrap_or(false)
    }

    /// One string flag's value; absent means the empty string.
    fn text_flag(&self, name: &str) -> &str {
        self.values.get(name).map_or("", String::as_str)
    }

    /// One repeatable flag's values in order; absent means empty.
    fn list_flag(&self, name: &str) -> &[String] {
        self.lists.get(name).map_or(&[], Vec::as_slice)
    }
}

/// Parses `args` with GNU-style interspersed flags (the observable behavior of
/// Go's `ReorderInterspersed` + `flag.Parse`).
///
/// Returns `Err(0)` for `-h`/`-help` and `Err(2)` for any parse error after
/// writing the Go-style diagnostic plus the usage block to `stderr`; this is
/// the same parser shape the frozen `task list` uses, scoped to the tasks flag
/// set (the core parser is not exported, and the runner must not change core).
fn parse_flags(
    stderr: &mut dyn Write,
    name: &str,
    defs: &[FlagDef],
    args: &[String],
) -> Result<ParsedFlags, i32> {
    let mut parsed = ParsedFlags::default();
    let mut index = 0;
    let mut positional_only = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if positional_only || arg.is_empty() || arg == "-" || !arg.starts_with('-') {
            parsed.positionals.push(arg.clone());
            continue;
        }
        if arg == "--" {
            positional_only = true;
            continue;
        }
        let body = arg.trim_start_matches('-');
        let (flag_name, inline_value) = match body.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_string())),
            None => (body, None),
        };
        let Some(def) = defs.iter().find(|def| def.name == flag_name) else {
            if flag_name == "h" || flag_name == "help" {
                write_usage(stderr, name, defs);
                return Err(0);
            }
            let _ = writeln!(stderr, "flag provided but not defined: -{flag_name}");
            write_usage(stderr, name, defs);
            return Err(2);
        };
        parse_one_flag(
            stderr,
            name,
            defs,
            def,
            inline_value,
            args,
            &mut index,
            &mut parsed,
        )?;
    }
    Ok(parsed)
}

/// Parses one recognized flag into `parsed`, consuming a following value when
/// needed.
#[allow(clippy::too_many_arguments)]
fn parse_one_flag(
    stderr: &mut dyn Write,
    name: &str,
    defs: &[FlagDef],
    def: &FlagDef,
    inline_value: Option<String>,
    args: &[String],
    index: &mut usize,
    parsed: &mut ParsedFlags,
) -> Result<(), i32> {
    match def.kind {
        FlagKind::Bool => {
            let value = if let Some(text) = inline_value {
                if let Some(parsed_bool) = parse_go_bool(&text) {
                    parsed_bool
                } else {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {text:?} for -{}: parse error",
                        def.name
                    );
                    write_usage(stderr, name, defs);
                    return Err(2);
                }
            } else {
                true
            };
            parsed.bools.insert(def.name, value);
        }
        FlagKind::Value | FlagKind::List => {
            let value = if let Some(text) = inline_value {
                text
            } else if *index < args.len() {
                let value = args[*index].clone();
                *index += 1;
                value
            } else {
                let _ = writeln!(stderr, "flag needs an argument: -{}", def.name);
                write_usage(stderr, name, defs);
                return Err(2);
            };
            if matches!(def.kind, FlagKind::List) {
                parsed.lists.entry(def.name).or_default().push(value);
            } else {
                parsed.values.insert(def.name, value);
            }
        }
    }
    Ok(())
}

/// Parses a Go `strconv.ParseBool` spelling.
fn parse_go_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// Writes the Go `flag.PrintDefaults` usage block, flags sorted by name with
/// the value-flag type suffix and indented usage.
fn write_usage(stderr: &mut dyn Write, name: &str, defs: &[FlagDef]) {
    let _ = writeln!(stderr, "Usage of {name}:");
    let mut sorted: Vec<&FlagDef> = defs.iter().collect();
    sorted.sort_by_key(|def| def.name);
    for def in sorted {
        let type_name = match def.kind {
            FlagKind::Bool => "",
            FlagKind::Value => " string",
            FlagKind::List => " value",
        };
        let _ = writeln!(stderr, "  -{}{type_name}", def.name);
        let _ = writeln!(stderr, "    \t{}", def.usage);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use mdfu_tui::tasks::Mode;

    /// Minimal temp dir (the crate carries no `tempfile` dev-dependency).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mdfu-tasks-runner-{tag}-{}-{id}",
                std::process::id()
            ));
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

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    /// A valid explicit theme: the builtin default YAML, so no XDG discovery
    /// or developer config can influence a test.
    fn theme_file(dir: &TempDir) -> String {
        let path = dir.path().join("theme.yaml");
        fs::write(&path, theme::default_yaml()).expect("write theme");
        path.to_string_lossy().into_owned()
    }

    /// Runs [`prepare`] against a vault and returns the app plus its stderr.
    fn build(
        cwd: &str,
        args: &[String],
        theme_path: &str,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> (App, String) {
        let mut stderr = Vec::new();
        let app = prepare(cwd, env, args, theme_path, &mut stderr).expect("prepare");
        (app, String::from_utf8(stderr).expect("utf8 stderr"))
    }

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn esc() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    }

    /// Flattens a `TestBackend` frame to text, one line per buffer row.
    fn buffer_text(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> String {
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                if let Some(cell) = buffer.cell((x, y)) {
                    out.push_str(cell.symbol());
                }
            }
            out.push('\n');
        }
        out
    }

    fn idle_editor(_terminal: &mut Terminal<TestBackend>, argv: &[String]) -> Msg {
        Msg::EditorFinished {
            error: Some(format!("unexpected editor call: {argv:?}")),
        }
    }

    #[test]
    fn tasks_runner_prepare_applies_selection_and_visibility_flags() {
        let vault = TempDir::new("prepare");
        vault.write(
            "tasks.md",
            concat!(
                "- [ ] PRJ-001 alpha\t\t#launch !high\n",
                "- [x] PRJ-002 bravo\t\t#launch\n",
                "- [ ] PRJ-003 charlie\t\t#other !low\n",
            ),
        );
        let base = vault.path_string();
        let theme = theme_file(&vault);

        // Defaults: every broadened checkbox loads, the model gates done rows.
        let (app, stderr) = build(&base, &strings(&["--path", &base]), &theme, &no_env);
        assert_eq!(app.tasks().len(), 3);
        assert!(!app.show_done());
        assert!(!app.show_blocked());
        assert_eq!(app.filtered().len(), 2, "done rows hidden by default");
        assert_eq!(app.base(), base);
        assert_eq!(stderr, "");

        // --all/--blocked feed the model gates; --tag and sort select rows.
        let (app, _) = build(
            &base,
            &strings(&[
                "--all",
                "--blocked",
                "--sort",
                "priority",
                "--path",
                &base,
                "#launch",
            ]),
            &theme,
            &no_env,
        );
        assert!(app.show_done());
        assert!(app.show_blocked());
        let ids: Vec<&str> = app.tasks().iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids, ["PRJ-001", "PRJ-002"], "tag AND + priority sort");

        // Positional !priority is an OR filter over the effective priority.
        let (app, _) = build(
            &base,
            &strings(&["--path", &base, "!high"]),
            &theme,
            &no_env,
        );
        let ids: Vec<&str> = app.tasks().iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids, ["PRJ-001"]);
    }

    #[test]
    fn tasks_runner_drive_keys_toggle_filter_and_quit() {
        let vault = TempDir::new("drive");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n- [ ] PRJ-002 bravo\n");
        let base = vault.path_string();
        let theme = theme_file(&vault);
        let (mut app, _) = build(&base, &strings(&["--path", &base]), &theme, &no_env);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let mut queue = vec![key('x'), key('b'), key('/'), key('b'), esc(), esc()].into_iter();
        let outcome = drive_tasks(
            &mut terminal,
            &mut app,
            || {
                queue
                    .next()
                    .ok_or_else(|| io::Error::other("events exhausted"))
            },
            idle_editor,
        );

        assert_eq!(outcome, Ok(()), "esc on an empty filter quits");
        assert_eq!(
            vault.read("tasks.md"),
            "- [x] PRJ-001 alpha\n- [ ] PRJ-002 bravo\n",
            "x wrote the checkbox through the engine"
        );
        assert!(app.is_aborted());
        assert!(app.show_blocked(), "b toggled the blocked gate");
        assert_eq!(app.mode(), Mode::Browse, "esc cleared the filter");
        let text = buffer_text(&terminal, 80, 24);
        assert!(text.contains("PRJ-002"), "the loop drew frames: {text}");
    }

    #[test]
    fn tasks_runner_drive_open_invokes_the_injected_editor_runner() {
        let vault = TempDir::new("editor");
        vault.write("tasks.md", "text\n- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();
        let theme = theme_file(&vault);
        let get_env = |key: &str| {
            if key == "EDITOR" {
                Some("vim --wait".to_owned())
            } else {
                None
            }
        };
        let (mut app, _) = build(&base, &strings(&["--path", &base]), &theme, &get_env);

        let file = vault.path().join("tasks.md").to_string_lossy().into_owned();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let mut queue = vec![key('o'), esc()].into_iter();
        let mut editor_calls: Vec<Vec<String>> = Vec::new();
        let outcome = drive_tasks(
            &mut terminal,
            &mut app,
            || {
                queue
                    .next()
                    .ok_or_else(|| io::Error::other("events exhausted"))
            },
            |_terminal, argv| {
                editor_calls.push(argv.to_vec());
                Msg::EditorFinished { error: None }
            },
        );

        assert_eq!(outcome, Ok(()));
        assert_eq!(
            editor_calls,
            vec![vec![
                "vim".to_owned(),
                "--wait".to_owned(),
                "+2".to_owned(),
                file
            ]],
            "Cmd::ExecEditor carries `$EDITOR +<line> <file>`"
        );
    }

    #[test]
    fn tasks_runner_error_paths_exit_before_terminal_init() {
        let vault = TempDir::new("errors");
        let base = vault.path_string();

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--bogus", "--path", &base]),
            "",
        );
        assert_eq!(code, 2);
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(
            err.contains("flag provided but not defined: -bogus"),
            "stderr: {err}"
        );
        assert!(err.contains("Usage of tasks:"), "stderr: {err}");

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--all=bogus", "--path", &base]),
            "",
        );
        assert_eq!(code, 2);
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(
            err.contains("invalid boolean value \"bogus\" for -all: parse error"),
            "stderr: {err}"
        );

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--path", &base, "-h"]),
            "",
        );
        assert_eq!(code, 0);
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(err.contains("Usage of tasks:"), "stderr: {err}");

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--path", "/nonexistent/mdfu-tasks-runner"]),
            "",
        );
        assert_eq!(code, 1);
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(err.starts_with("mdfu task: --path "), "stderr: {err}");

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--path", &base, "bogus"]),
            "",
        );
        assert_eq!(code, 2);
        assert_eq!(
            String::from_utf8(stderr).expect("utf8"),
            "mdfu tasks: unexpected argument 'bogus'\n"
        );
    }

    #[test]
    fn tasks_runner_theme_error_exits_2_with_mdfu_prefix() {
        let vault = TempDir::new("theme-error");
        vault.write("tasks.md", "- [ ] PRJ-001 alpha\n");
        let base = vault.path_string();

        let mut stderr = Vec::new();
        let code = run_tasks_tui(
            &mut Vec::new(),
            &mut stderr,
            &strings(&["--path", &base]),
            "/nonexistent/mdfu-tasks/theme.yaml",
        );
        assert_eq!(code, 2);
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(err.starts_with("mdfu: "), "stderr: {err}");
    }
}
