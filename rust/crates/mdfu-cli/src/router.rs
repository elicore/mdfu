//! The `mdfu task` / `mdfu tasks` pre-parser: a port of `maybeRouteTask` from
//! `cmd/mdfu/task.go` (lines 133-319).
//!
//! Every task flag (`--path`, `--all`, `--blocked`, `--json`, `--sort`,
//! `--tag`, `--priority`, `--prefix`) is unknown to the root clap parser and
//! would otherwise abort with a usage error, so the router must claim the
//! invocation first. It scans the raw, program-name-less arguments by hand —
//! never with clap, never after clap — and then either dispatches into
//! [`mdfu_core::task::cli`], hands a D2 fall-through query back to the root
//! CLI, or steps aside for the root parse.
//!
//! Scanner semantics (exactly Go):
//!
//! * a bare `--` before the candidate bypasses the router entirely, so
//!   `mdfu -- task` searches for `task`;
//! * before the candidate only `--config <path>` / `--config=<path>` is
//!   accepted (remembered as the theme path);
//! * value-taking root flags (`--root`, `--limit`, `--format`) skip their
//!   value so `mdfu --root <v> task list` still finds its candidate;
//! * any other root flag before a `task`/`tasks` candidate is a usage error
//!   (E2, exit 2);
//! * a non-`task`/`tasks` candidate or `--config default` falls through to the
//!   root CLI unchanged.
//!
//! After the candidate the router hands the remaining arguments to the task
//! registry: empty -> `list`, `--` -> explicit dispatch, a known subcommand ->
//! dispatch, an ID-shaped token -> `view`, and anything else -> the D2 fuzzy
//! fall-through. `tasks` is `task list` on a non-TTY and the interactive tasks
//! UI on a TTY.
//!
//! Divergence: arguments are converted with `OsString::to_string_lossy` so a
//! non-UTF-8 argument is scanned lossily; Go scans raw bytes. UTF-8 argv is
//! unaffected.

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};

use mdfu_core::task::cli::{self as task_cli, Env};
use mdfu_core::task::id;
use mdfu_core::task::render::Terminal;

/// The reserved `--config` value that prints the builtin config; the router
/// must step aside so the root CLI can handle it.
const DEFAULT_CONFIG_VALUE: &str = "default";

/// What the pre-parser decided the raw arguments mean.
///
/// A pure value: [`decide`] performs no I/O, so every row of the Go router
/// decision table is unit-testable without a terminal or a filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Decision {
    /// Not a task invocation; the root clap parse owns it.
    FallThrough,
    /// A root flag preceded the candidate: E2 usage error.
    Usage {
        /// Bare flag name (no dashes) for the E2 message.
        flag: String,
    },
    /// `mdfu task ...`: dispatch `args` through the task registry.
    Task {
        /// Arguments handed to [`mdfu_core::task::cli::dispatch`].
        args: Vec<String>,
    },
    /// `mdfu tasks ...` without a TTY: `task list` with the same arguments.
    TasksList {
        /// Arguments handed to `task list`.
        args: Vec<String>,
    },
    /// `mdfu tasks ...` on a TTY: the interactive tasks UI.
    TasksTui {
        /// Selection arguments for the UI.
        args: Vec<String>,
        /// Theme path remembered from `--config`.
        config: String,
    },
    /// D2 fall-through: filter mode with this query.
    Filter {
        /// The joined, trimmed positional query.
        query: String,
    },
}

/// The pre-parser's result for the caller.
///
/// `Handled` means the router owned the invocation and its code is the process
/// exit code; `Filter` is the D2 fall-through handed back so the root CLI runs
/// the same filter pipeline as a bare query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// The router handled the invocation; exit with this code.
    Handled(i32),
    /// D2 fall-through: run filter mode with this query.
    Filter(String),
}

/// The TTY `tasks` UI hook (Go's `runTasksTUI` package variable), injected so
/// tests can record the call without a terminal.
type TasksTuiHook<'a> = &'a mut dyn FnMut(&mut dyn Write, &mut dyn Write, &[String], &str) -> i32;

/// The injected host handles of one router invocation.
struct Host<'a> {
    /// Standard output sink.
    stdout: &'a mut dyn Write,
    /// Standard error sink.
    stderr: &'a mut dyn Write,
    /// Standard input source for the `ids` prefix prompt.
    stdin: &'a mut dyn BufRead,
    /// Working directory the task scope resolves from.
    cwd: String,
    /// Whether stdout is an interactive terminal.
    is_terminal: bool,
    /// Environment lookup (`EDITOR`, `MDTASK_PATH`, ...).
    get_env: &'a dyn Fn(&str) -> Option<String>,
}

impl Host<'_> {
    /// Builds the task [`Env`] and routes `argv` through the task registry.
    fn dispatch_task(&mut self, argv: &[String]) -> i32 {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let mut env = Env {
            stdout: &mut *self.stdout,
            stderr: &mut *self.stderr,
            stdin: Some(&mut *self.stdin),
            terminal: Terminal {
                is_tty: self.is_terminal,
                no_color,
            },
            cwd: self.cwd.clone(),
            now: now_unix,
            get_env: self.get_env,
        };
        task_cli::dispatch(&mut env, argv).code
    }
}

/// Runs the pre-parser over the raw, program-name-less `args`.
///
/// Returns `None` when the invocation does not belong to the task surface
/// (`maybeRouteTask` returning `false`); otherwise the outcome the caller must
/// act on — an exit code or the D2 filter query. Must run BEFORE clap: the
/// task flags are unknown to the root parser.
#[must_use = "the router outcome must be acted on before the root parse"]
pub fn maybe_route_task(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Option<RouteOutcome> {
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let cwd = std::env::current_dir().map_or_else(
        |_| ".".to_string(),
        |path| path.to_string_lossy().into_owned(),
    );
    let get_env = |key: &str| std::env::var(key).ok();
    let mut stdin = io::stdin().lock();
    let mut host = Host {
        stdout,
        stderr,
        stdin: &mut stdin,
        cwd,
        is_terminal: io::stdout().is_terminal(),
        get_env: &get_env,
    };
    let mut hook =
        |stdout: &mut dyn Write, stderr: &mut dyn Write, args: &[String], config: &str| {
            run_tasks_tui(stdout, stderr, args, config)
        };
    route(&args, &mut host, &mut hook)
}

/// Executes the [`decide`] outcome against the injected host.
fn route(
    args: &[String],
    host: &mut Host<'_>,
    tasks_tui: TasksTuiHook<'_>,
) -> Option<RouteOutcome> {
    match decide(args, host.is_terminal) {
        Decision::FallThrough => None,
        Decision::Usage { flag } => {
            let _ = writeln!(
                host.stderr,
                "mdfu task: --{flag} must come after the task subcommand"
            );
            Some(RouteOutcome::Handled(2))
        }
        Decision::Task { args } => Some(RouteOutcome::Handled(host.dispatch_task(&args))),
        Decision::TasksList { args } => {
            let mut list_argv = Vec::with_capacity(args.len() + 1);
            list_argv.push("list".to_string());
            list_argv.extend(args);
            Some(RouteOutcome::Handled(host.dispatch_task(&list_argv)))
        }
        Decision::TasksTui { args, config } => Some(RouteOutcome::Handled(tasks_tui(
            &mut *host.stdout,
            &mut *host.stderr,
            &args,
            &config,
        ))),
        Decision::Filter { query } => Some(RouteOutcome::Filter(query)),
    }
}

/// Default TTY `tasks` entry point (Go's `runTasksTUI` default).
///
/// Todo 32 replaces this stub with the interactive tasks UI: resolve the same
/// selection flags as `list`, load the checkboxes, and hand the rows to
/// `mdfu-tui`. Until then the TTY branch reports that it is not wired, like
/// the filter/TUI stubs in `main.rs`.
fn run_tasks_tui(
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    args: &[String],
    config: &str,
) -> i32 {
    let _ = (stdout, args, config);
    let _ = writeln!(
        stderr,
        "mdfu tasks: interactive mode is not implemented yet (todo 32)"
    );
    1
}

/// Scans `args` and returns the routing decision without side effects.
fn decide(args: &[String], is_terminal: bool) -> Decision {
    let mut theme_path = String::new();
    let mut pending_flag = String::new();
    let mut candidate: Option<usize> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return Decision::FallThrough;
        }
        if arg.is_empty() || arg == "-" || !arg.starts_with('-') {
            candidate = Some(index);
            break;
        }
        let (name, inline_value) = split_flag_token(arg);
        if name == "config" {
            if let Some(value) = inline_value {
                value.clone_into(&mut theme_path);
            } else if index + 1 < args.len() {
                index += 1;
                theme_path.clone_from(&args[index]);
            }
            index += 1;
            continue;
        }
        if pending_flag.is_empty() {
            name.clone_into(&mut pending_flag);
        }
        if inline_value.is_none() && root_flag_takes_value(name) && index + 1 < args.len() {
            index += 1;
        }
        index += 1;
    }

    let Some(index) = candidate else {
        return Decision::FallThrough;
    };
    let candidate = &args[index];
    if candidate != "task" && candidate != "tasks" {
        return Decision::FallThrough;
    }
    if theme_path == DEFAULT_CONFIG_VALUE {
        return Decision::FallThrough;
    }
    if !pending_flag.is_empty() {
        return Decision::Usage { flag: pending_flag };
    }

    let (rest, theme_path) = strip_config_flag(&args[index + 1..], theme_path);
    if candidate == "tasks" {
        return if is_terminal {
            Decision::TasksTui {
                args: rest,
                config: theme_path,
            }
        } else {
            Decision::TasksList { args: rest }
        };
    }
    route_task(rest)
}

/// Implements the `mdfu task` dispatch table over the post-candidate args.
fn route_task(rest: Vec<String>) -> Decision {
    if rest.is_empty() {
        return Decision::Task {
            args: vec!["list".to_string()],
        };
    }
    if rest[0] == "--" {
        return Decision::Task {
            args: rest[1..].to_vec(),
        };
    }
    if is_task_subcommand(&rest[0]) {
        return Decision::Task { args: rest };
    }
    if task_id_shaped(&rest[0]) {
        let mut args = Vec::with_capacity(rest.len() + 1);
        args.push("view".to_string());
        args.extend(rest);
        return Decision::Task { args };
    }
    // D2 fall-through: an unknown token is a fuzzy search with the leading
    // `task` token consumed, so `mdfu task retention` searches for
    // "retention" and `mdfu task task` searches for "task".
    Decision::Filter {
        query: rest.join(" ").trim().to_string(),
    }
}

/// Reports whether `name` is a registered task subcommand or alias.
fn is_task_subcommand(name: &str) -> bool {
    task_cli::subcommands()
        .iter()
        .any(|subcommand| subcommand.name == name || subcommand.aliases.contains(&name))
}

/// Reports whether `token` is ID-shaped (`^[A-Z][A-Z0-9]*-[0-9]+$` or bare
/// digits), mirroring the task registry's private predicate.
fn task_id_shaped(token: &str) -> bool {
    id::valid_task_id(token)
        || (!token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Splits a flag token into its bare name and its inline value
/// (`--flag=value`), trimming every leading dash.
fn split_flag_token(token: &str) -> (&str, Option<&str>) {
    let name = token.trim_start_matches('-');
    match name.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (name, None),
    }
}

/// Reports whether a root flag consumes the following token. It mirrors the
/// value-typed flags of the root flagset; `--config` is handled separately.
fn root_flag_takes_value(name: &str) -> bool {
    matches!(name, "root" | "limit" | "format")
}

/// Removes every `--config <path>` / `--config=<path>` from the task
/// arguments and returns the theme path they named. The task subcommands do
/// not use a theme, so `--config` must never reach them as an unknown flag.
fn strip_config_flag(args: &[String], mut theme_path: String) -> (Vec<String>, String) {
    let mut out = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--config" {
            if index + 1 < args.len() {
                theme_path.clone_from(&args[index + 1]);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--config=") {
            value.clone_into(&mut theme_path);
        } else {
            out.push(arg.clone());
        }
        index += 1;
    }
    (out, theme_path)
}

/// Current Unix time in whole seconds, mirroring Go's `time.Now` for
/// [`Env::now`].
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Two tasks: one open (visible), one done (hidden unless `--all`).
    const ROUTER_VAULT: &str = concat!(
        "- [ ] EXMPL-001 Fix the thing\t\t#launch !high @status:doing\n",
        "- [x] EXMPL-002 Ship it\t\t#launch\n",
    );

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn decide_of(args: &[&str], is_terminal: bool) -> Decision {
        decide(&strings(args), is_terminal)
    }

    fn task(args: &[&str]) -> Decision {
        Decision::Task {
            args: strings(args),
        }
    }

    fn filter(query: &str) -> Decision {
        Decision::Filter {
            query: query.to_string(),
        }
    }

    fn usage(flag: &str) -> Decision {
        Decision::Usage {
            flag: flag.to_string(),
        }
    }

    #[test]
    fn router_task_alone_lists() {
        assert_eq!(decide_of(&["task"], false), task(&["list"]));
    }

    #[test]
    fn router_explicit_dispatch_marker_reaches_registry() {
        assert_eq!(decide_of(&["task", "--"], false), task(&[]));
        assert_eq!(
            decide_of(&["task", "--", "list", "--path", "d"], false),
            task(&["list", "--path", "d"])
        );
        assert_eq!(decide_of(&["task", "--", "bogus"], false), task(&["bogus"]));
    }

    #[test]
    fn router_known_subcommand_and_alias_dispatch() {
        assert_eq!(decide_of(&["task", "list"], false), task(&["list"]));
        assert_eq!(
            decide_of(&["task", "show", "EXMPL-001"], false),
            task(&["show", "EXMPL-001"])
        );
        assert_eq!(
            decide_of(&["task", "install-skills", "out"], false),
            task(&["install-skills", "out"])
        );
    }

    #[test]
    fn router_id_shaped_token_routes_to_view() {
        assert_eq!(
            decide_of(&["task", "EXMPL-001", "--path", "d"], false),
            task(&["view", "EXMPL-001", "--path", "d"])
        );
        assert_eq!(decide_of(&["task", "1"], false), task(&["view", "1"]));
        assert_eq!(
            decide_of(&["task", "PRJ-42"], false),
            task(&["view", "PRJ-42"])
        );
    }

    #[test]
    fn router_unknown_token_falls_through_to_filter() {
        assert_eq!(
            decide_of(&["task", "retention"], false),
            filter("retention")
        );
        assert_eq!(decide_of(&["task", "task"], false), filter("task"));
        assert_eq!(
            decide_of(&["task", "retention", "cohort"], false),
            filter("retention cohort")
        );
        // Lowercase prefixes are not ID-shaped (`^[A-Z][A-Z0-9]*-[0-9]+$`).
        assert_eq!(decide_of(&["task", "exmpl-1"], false), filter("exmpl-1"));
    }

    #[test]
    fn router_empty_args_fall_through() {
        assert_eq!(decide_of(&[], false), Decision::FallThrough);
    }

    #[test]
    fn router_non_task_candidate_falls_through_even_with_pending_flag() {
        assert_eq!(decide_of(&["retention"], false), Decision::FallThrough);
        assert_eq!(
            decide_of(&["--root", "x", "retention"], false),
            Decision::FallThrough
        );
    }

    #[test]
    fn router_double_dash_bypasses_before_candidate() {
        assert_eq!(decide_of(&["--", "task"], false), Decision::FallThrough);
        assert_eq!(
            decide_of(&["--config", "theme.yaml", "--", "task"], false),
            Decision::FallThrough
        );
        // `-` is a positional candidate, not a task command.
        assert_eq!(decide_of(&["-"], false), Decision::FallThrough);
    }

    #[test]
    fn router_root_value_flags_skip_their_value_then_are_e2() {
        // The value skip is what lets the scanner reach the `task` candidate;
        // the candidate is then still preceded by a root flag (E2).
        assert_eq!(
            decide_of(&["--root", "x", "task", "list"], false),
            usage("root")
        );
        assert_eq!(decide_of(&["--limit", "5", "task"], false), usage("limit"));
        assert_eq!(
            decide_of(&["--format", "json", "tasks", "x"], false),
            usage("format")
        );
        assert_eq!(decide_of(&["-root", "x", "task"], false), usage("root"));
    }

    #[test]
    fn router_other_root_flags_before_candidate_are_e2() {
        assert_eq!(
            decide_of(&["--hidden", "task", "list"], false),
            usage("hidden")
        );
        assert_eq!(
            decide_of(&["--no-ignore", "task"], false),
            usage("no-ignore")
        );
        // The first non-config flag wins, even when a value flag follows.
        assert_eq!(
            decide_of(&["--hidden", "--root", "x", "task"], false),
            usage("hidden")
        );
    }

    #[test]
    fn router_config_before_candidate_is_remembered() {
        assert_eq!(
            decide_of(&["--config", "theme.yaml", "tasks", "--all"], true),
            Decision::TasksTui {
                args: strings(&["--all"]),
                config: "theme.yaml".to_string(),
            }
        );
        assert_eq!(
            decide_of(&["--config=theme.yaml", "tasks", "--all"], false),
            Decision::TasksList {
                args: strings(&["--all"]),
            }
        );
        assert_eq!(
            decide_of(&["--config", "theme.yaml", "task", "list"], false),
            task(&["list"])
        );
    }

    #[test]
    fn router_config_default_falls_through() {
        assert_eq!(
            decide_of(&["--config", "default", "task", "list"], false),
            Decision::FallThrough
        );
        assert_eq!(
            decide_of(&["--config=default", "tasks"], true),
            Decision::FallThrough
        );
    }

    #[test]
    fn router_config_after_candidate_is_stripped_and_remembered() {
        assert_eq!(
            decide_of(
                &[
                    "tasks",
                    "--all",
                    "--config",
                    "theme.yaml",
                    "--path",
                    "vault"
                ],
                true
            ),
            Decision::TasksTui {
                args: strings(&["--all", "--path", "vault"]),
                config: "theme.yaml".to_string(),
            }
        );
        assert_eq!(
            decide_of(
                &["task", "list", "--config=theme.yaml", "--path", "d"],
                false
            ),
            task(&["list", "--path", "d"])
        );
        // A trailing value-less `--config` is dropped silently.
        assert_eq!(
            decide_of(&["task", "list", "--config"], false),
            task(&["list"])
        );
    }

    #[test]
    fn router_tasks_split_on_terminal() {
        assert_eq!(
            decide_of(&["tasks"], false),
            Decision::TasksList { args: vec![] }
        );
        assert_eq!(
            decide_of(&["tasks"], true),
            Decision::TasksTui {
                args: vec![],
                config: String::new(),
            }
        );
        assert_eq!(
            decide_of(&["tasks", "--all", "--blocked"], false),
            Decision::TasksList {
                args: strings(&["--all", "--blocked"]),
            }
        );
    }

    #[test]
    fn router_config_without_value_consumes_the_candidate() {
        // `--config task` reads "task" as the theme path, so nothing is left.
        assert_eq!(
            decide_of(&["--config", "task"], false),
            Decision::FallThrough
        );
    }

    /// One executor run: the outcome plus everything the injected host saw.
    struct Exec {
        outcome: Option<RouteOutcome>,
        stdout: String,
        stderr: String,
        tui_calls: Vec<(Vec<String>, String)>,
    }

    fn no_env(_key: &str) -> Option<String> {
        None
    }

    /// Runs `route` against injected buffers, cwd, and environment. The TTY
    /// hook records its arguments and returns 7, mirroring the Go hook test.
    fn exec(args: &[&str], is_terminal: bool, cwd: &str) -> Exec {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut stdin: &[u8] = b"";
        let mut tui_calls = Vec::new();
        let outcome = {
            let mut hook = |_stdout: &mut dyn Write,
                            _stderr: &mut dyn Write,
                            args: &[String],
                            config: &str|
             -> i32 {
                tui_calls.push((args.to_vec(), config.to_string()));
                7
            };
            let mut host = Host {
                stdout: &mut stdout,
                stderr: &mut stderr,
                stdin: &mut stdin,
                cwd: cwd.to_string(),
                is_terminal,
                get_env: &no_env,
            };
            route(&strings(args), &mut host, &mut hook)
        };
        Exec {
            outcome,
            stdout: String::from_utf8(stdout).expect("utf8 stdout"),
            stderr: String::from_utf8(stderr).expect("utf8 stderr"),
            tui_calls,
        }
    }

    /// Minimal temp dir (the crate carries no `tempfile` dev-dependency).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("mdfu-router-{tag}-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn router_task_list_dispatches_through_the_registry() {
        let dir = TempDir::new("list");
        fs::write(dir.path().join("tasks.md"), ROUTER_VAULT).expect("write vault");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(&["task", "list", "--path", cwd.as_str()], false, &cwd);
        assert_eq!(result.outcome, Some(RouteOutcome::Handled(0)));
        assert!(
            result.stdout.contains("EXMPL-001"),
            "stdout: {}",
            result.stdout
        );
        assert!(
            !result.stdout.contains("EXMPL-002"),
            "done task leaked: {}",
            result.stdout
        );
    }

    #[test]
    fn router_task_alone_lists_the_cwd_scope() {
        let dir = TempDir::new("alone");
        fs::write(dir.path().join("tasks.md"), ROUTER_VAULT).expect("write vault");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(&["task"], false, &cwd);
        assert_eq!(result.outcome, Some(RouteOutcome::Handled(0)));
        assert!(
            result.stdout.contains("EXMPL-001"),
            "stdout: {}",
            result.stdout
        );
    }

    #[test]
    fn router_tasks_non_tty_is_byte_identical_to_task_list() {
        let dir = TempDir::new("parity");
        fs::write(dir.path().join("tasks.md"), ROUTER_VAULT).expect("write vault");
        let cwd = dir.path().to_string_lossy().into_owned();

        let tasks = exec(&["tasks", "--all", "--path", cwd.as_str()], false, &cwd);
        let list = exec(
            &["task", "list", "--all", "--path", cwd.as_str()],
            false,
            &cwd,
        );
        assert_eq!(tasks.outcome, list.outcome);
        assert_eq!(tasks.stdout, list.stdout);
        assert_eq!(tasks.stderr, list.stderr);
    }

    #[test]
    fn router_usage_error_writes_e2_and_exits_2() {
        let dir = TempDir::new("usage");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(&["--hidden", "task", "list"], false, &cwd);
        assert_eq!(result.outcome, Some(RouteOutcome::Handled(2)));
        assert_eq!(result.stdout, "");
        assert_eq!(
            result.stderr,
            "mdfu task: --hidden must come after the task subcommand\n"
        );
    }

    #[test]
    fn router_tty_tasks_calls_the_ui_hook_with_args_and_config() {
        let dir = TempDir::new("tui");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(
            &[
                "tasks",
                "--all",
                "--config",
                "theme.yaml",
                "--path",
                "vault",
            ],
            true,
            &cwd,
        );
        assert_eq!(result.outcome, Some(RouteOutcome::Handled(7)));
        assert_eq!(result.stdout, "");
        assert_eq!(
            result.tui_calls,
            vec![(
                strings(&["--all", "--path", "vault"]),
                "theme.yaml".to_string()
            )]
        );
    }

    #[test]
    fn router_filter_fallthrough_returns_the_query() {
        let dir = TempDir::new("filter");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(&["task", "retention"], false, &cwd);
        assert_eq!(
            result.outcome,
            Some(RouteOutcome::Filter("retention".to_string()))
        );
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    #[test]
    fn router_double_dash_before_candidate_returns_none() {
        let dir = TempDir::new("bypass");
        let cwd = dir.path().to_string_lossy().into_owned();

        let result = exec(&["--", "task"], false, &cwd);
        assert_eq!(result.outcome, None);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }
}
