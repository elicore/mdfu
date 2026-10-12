//! `mdfu` command-line entry point.
//!
//! This is the root CLI: flag parsing (clap derive), mode dispatch, and the
//! exit-code contract (`0` success, `1` runtime error, `2` usage error) from
//! `internal/task/SPEC.md` §I. The Go reference is `cmd/mdfu/main.go`.
//!
//! The `mdfu task` / `mdfu tasks` pre-parser lives in the `router` module and
//! runs before clap on the raw arguments; it is deliberately not a clap
//! subcommand (todo 29).
//!
//! The filter pipeline and interactive picker wiring live in the `pipeline`
//! module; [`run_filter`] and [`run_tui`] below are thin adapters from the
//! parsed root flags into it.
//!
//! The theme is resolved only for the TUI path via [`theme::resolve`]
//! (explicit `--config` > `$MDFU_CONFIG` > discovered XDG file > builtin
//! defaults) and passed by reference into [`run_tui`]; there is deliberately
//! no global theme.

#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used))]

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Parser;
use mdfu_core::theme::{self, Theme};

mod pipeline;
mod router;
mod tasks_runner;

/// Version source for `--version`: the Cargo workspace version, which
/// release-please owns. (Go sets `main.version` through `-ldflags`.)
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Filter-mode result cap when `--limit` is not given; the interactive TUI
/// defaults to 50 instead.
const DEFAULT_FILTER_LIMIT: i64 = 20;

/// Default result cap for the interactive TUI.
const DEFAULT_TUI_LIMIT: i64 = 50;

// Five booleans mirror the Go root flags; the allow below is for clippy's
// struct_excessive_bools. Kept out of a doc comment so clap's long_about
// does not leak this rationale into `--help`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Parser)]
#[command(
    name = "mdfu",
    about = "Fuzzy finder for Markdown notes: interactive picker and pipeable filter.",
    color = clap::ColorChoice::Never
)]
struct Cli {
    /// Root directory to scan.
    #[arg(long, value_name = "DIR", default_value = ".")]
    root: PathBuf,

    /// Include hidden files and directories.
    #[arg(long)]
    hidden: bool,

    /// Disable gitignore respect (respect is on by default, unlike Go).
    #[arg(long = "no-ignore")]
    no_ignore: bool,

    /// Max number of results (filter mode defaults to 20; 0 or negative = unlimited).
    #[arg(long, value_name = "N")]
    limit: Option<i64>,

    /// Output format: paths|json|vimgrep.
    #[arg(long, value_name = "F", default_value = "paths")]
    format: String,

    /// Include archived documents.
    #[arg(long)]
    archived: bool,

    /// Render markdown links as label and URL instead of OSC 8 terminal hyperlinks.
    #[arg(long = "no-hyperlinks")]
    no_hyperlinks: bool,

    /// Path to a TUI theme YAML, or "default" to print the builtin default config.
    #[arg(long, value_name = "PATH")]
    config: Option<String>,

    /// Print version and exit.
    #[arg(long)]
    version: bool,

    /// Non-interactive filter query; any positional argument selects filter mode.
    #[arg(value_name = "QUERY")]
    query: Vec<String>,
}

fn main() {
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = run(
        std::env::args_os().skip(1),
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    std::process::exit(code);
}

/// Parses `args` (without the program name) and dispatches to one of three
/// modes: print the default config (`--config default`), the non-interactive
/// filter (a positional query), or the interactive TUI (no positional query).
/// Returns the process exit code.
#[must_use]
fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();

    // The task router inspects the raw arguments before clap sees them
    // (`cmd/mdfu/task.go:153-205`).
    if let Some(outcome) = router::maybe_route_task(&args, stdout, stderr) {
        return match outcome {
            router::RouteOutcome::Handled(code) => code,
            router::RouteOutcome::Filter(query) => run_filter_fallthrough(&query, stdout, stderr),
        };
    }

    let cli = match Cli::try_parse_from(
        std::iter::once(OsString::from("mdfu")).chain(args.iter().cloned()),
    ) {
        Ok(cli) => cli,
        Err(error) => return report_clap_error(&error, stdout, stderr),
    };

    if cli.version {
        let _ = writeln!(stdout, "mdfu {VERSION}");
        return 0;
    }

    if !matches!(cli.format.as_str(), "paths" | "json" | "vimgrep") {
        let _ = writeln!(
            stderr,
            "invalid --format {:?}: must be paths|json|vimgrep",
            cli.format
        );
        return 2;
    }

    if cli.config.as_deref() == Some("default") {
        let _ = write!(stdout, "{}", theme::default_yaml());
        return 0;
    }

    let query = cli.query.join(" ");
    let query = query.trim();
    if !query.is_empty() {
        return run_filter(&cli, query, filter_limit(cli.limit), stdout, stderr);
    }

    let theme_path = cli.config.as_deref().filter(|value| !value.is_empty());
    let theme = match theme::resolve(theme_path.map(Path::new), stderr) {
        Ok(theme) => theme,
        Err(error) => {
            let _ = writeln!(stderr, "mdfu: {error}");
            return 2;
        }
    };
    run_tui(&cli, tui_limit(cli.limit), &theme, stdout, stderr)
}

/// Runs the D2 fall-through query in filter mode with Go's fixed defaults:
/// `runFilter(stdout, stderr, ".", false, true, query, false,
/// defaultFilterLimit, "paths")` (`cmd/mdfu/task.go:224-225`).
fn run_filter_fallthrough(query: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let cli = Cli {
        root: PathBuf::from("."),
        hidden: false,
        no_ignore: false,
        limit: None,
        format: "paths".to_string(),
        archived: false,
        no_hyperlinks: false,
        config: None,
        version: false,
        query: vec![query.to_string()],
    };
    let limit = filter_limit(cli.limit);
    run_filter(&cli, query, limit, stdout, stderr)
}

/// Non-interactive filter mode (`mdfu <query>`): adapts the parsed root flags
/// to the shared pipeline in [`pipeline::run_filter`].
fn run_filter(
    cli: &Cli,
    query: &str,
    limit: i64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let root = cli.root.to_string_lossy();
    pipeline::run_filter(
        &pipeline_options(cli, &root, limit),
        query,
        &cli.format,
        stdout,
        stderr,
    )
}

/// Interactive TUI mode (no positional query): adapts the parsed root flags to
/// [`pipeline::run_tui`]. The theme is resolved by the caller and injected
/// here, never read from a global; `$EDITOR` is read from the process
/// environment so the picker crate stays env-agnostic.
fn run_tui(
    cli: &Cli,
    limit: i64,
    theme: &Theme,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let root = cli.root.to_string_lossy();
    let editor = std::env::var("EDITOR").ok();
    pipeline::run_tui(
        &pipeline_options(cli, &root, limit),
        theme,
        cli.no_hyperlinks,
        editor.as_deref(),
        stdout,
        stderr,
    )
}

/// Builds [`pipeline::Options`] from the parsed root flags and a resolved
/// result cap; the adapter's single mapping from `Cli` to pipeline input.
fn pipeline_options<'a>(cli: &Cli, root: &'a str, limit: i64) -> pipeline::Options<'a> {
    pipeline::Options {
        root,
        include_hidden: cli.hidden,
        respect_gitignore: !cli.no_ignore,
        include_archived: cli.archived,
        limit,
    }
}

/// Effective `--limit` for filter mode: an explicit value wins, otherwise the
/// filter default (20). Zero or negative means unlimited.
#[must_use]
fn filter_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_FILTER_LIMIT)
}

/// Effective `--limit` for the TUI: an explicit value wins, otherwise 50.
#[must_use]
fn tui_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_TUI_LIMIT)
}

/// Writes the clap parse outcome and maps it onto the root exit-code contract:
/// help/version displays go to stdout and exit 0, usage errors go to stderr
/// and exit 2.
fn report_clap_error(error: &clap::Error, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = if error.use_stderr() {
        write!(stderr, "{error}")
    } else {
        write!(stdout, "{error}")
    };
    error.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `run` with `args` (no program name) and captures both streams.
    fn run_args(args: &[&str]) -> (i32, String, String) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(args.iter().copied(), &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn root_version_prints_mdfu_and_package_version() {
        let (code, stdout, stderr) = run_args(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(stdout, format!("mdfu {VERSION}\n"));
        assert_eq!(stderr, "");
    }

    #[test]
    fn root_version_wins_over_invalid_format() {
        // Go checks --version before --format (cmd/mdfu/main.go:67-75).
        let (code, stdout, stderr) = run_args(&["--format", "bogus", "--version"]);
        assert_eq!(code, 0);
        assert_eq!(stdout, format!("mdfu {VERSION}\n"));
        assert_eq!(stderr, "");
    }

    #[test]
    fn root_config_default_prints_builtin_yaml() {
        let (code, stdout, stderr) = run_args(&["--config", "default"]);
        assert_eq!(code, 0);
        assert_eq!(stdout, theme::default_yaml());
        assert_eq!(stderr, "");
    }

    #[test]
    fn root_config_default_wins_over_query() {
        // Go checks --config default before joining the positional query.
        let (code, stdout, _stderr) = run_args(&["retention", "--config", "default"]);
        assert_eq!(code, 0);
        assert_eq!(stdout, theme::default_yaml());
    }

    #[test]
    fn root_invalid_format_exits_2_with_documented_message() {
        let (code, stdout, stderr) = run_args(&["--format", "bogus"]);
        assert_eq!(code, 2);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            "invalid --format \"bogus\": must be paths|json|vimgrep\n"
        );
    }

    #[test]
    fn root_unknown_flag_exits_2() {
        let (code, stdout, stderr) = run_args(&["--bogus"]);
        assert_eq!(code, 2);
        assert_eq!(stdout, "");
        assert!(stderr.contains("--bogus"), "stderr: {stderr}");
    }

    #[test]
    fn root_help_exits_0() {
        let (code, stdout, stderr) = run_args(&["--help"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("Usage: mdfu"), "stdout: {stdout}");
        assert_eq!(stderr, "");
    }

    #[test]
    fn root_theme_load_error_exits_2_with_mdfu_prefix() {
        let (code, stdout, stderr) = run_args(&["--config", "/nonexistent/mdfu-root/config.yaml"]);
        assert_eq!(code, 2);
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("mdfu: theme: read "), "stderr: {stderr}");
    }

    #[test]
    fn root_filter_limit_defaults() {
        assert_eq!(filter_limit(None), 20);
        assert_eq!(filter_limit(Some(5)), 5);
        assert_eq!(filter_limit(Some(0)), 0);
    }

    #[test]
    fn root_tui_limit_defaults() {
        assert_eq!(tui_limit(None), 50);
        assert_eq!(tui_limit(Some(7)), 7);
    }

    #[test]
    fn root_flags_may_follow_the_query() {
        // clap handles interspersed flags natively (PARITY: dropped
        // ReorderInterspersed Go shim).
        let (code, _stdout, stderr) = run_args(&["retention", "--limit", "3"]);
        assert_ne!(code, 2);
        assert!(!stderr.contains("unexpected argument"), "stderr: {stderr}");
    }
}
