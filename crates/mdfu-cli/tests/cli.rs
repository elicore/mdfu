//! `assert_cmd` integration coverage for the root CLI acceptance matrix:
//! `--version`, `--config default`, invalid `--format`, unknown flag, and
//! `--help`, plus the theme-load error path. The `pipeline_` tests cover filter
//! mode end to end against the shared fixture vault and throwaway vaults.
//!
//! The `exit_`, `format_`, `router_`, `tasks_`, and `limit_` tests extend the
//! matrix over `PARITY.md`:
//!
//! * the root and task 0/1/2 exit-code rows (E1 through E19 where reachable);
//! * the `paths`, `json`, and `vimgrep` output shapes, including Go's JSON
//!   HTML escaping for note-filter JSON versus the task JSON encoder's
//!   `SetEscapeHTML(false)` literal `<`, `>`, `&`;
//! * the D2 fall-through (`mdfu task retention` searches), the `--` bypass,
//!   E2 for a root flag before the candidate, and non-TTY `mdfu tasks` output
//!   byte-identical to `mdfu task list`;
//! * the `--limit` defaults (filter 20, TUI 50).
//!
//! Not reachable through the binary and therefore covered elsewhere: the
//! filter-mode JSON-formatting error (serde cannot fail on these values) and
//! the interactive-TUI 0/1 rows (needs a real terminal; see the `root_` unit
//! tests and the todo 32 TUI harness). E5/E16/E17 need injected IO or
//! concurrent mutation, so the sibling `mdfu-core` tests own them.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use predicates::prelude::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Two tasks: one open (visible), one done (hidden unless `--all`).
const TASK_VAULT: &str = "- [ ] EXMPL-001 Fix the thing\t\t#launch !high @status:doing\n\
                          - [x] EXMPL-002 Ship it\t\t#launch\n";

/// The repository fixture vault (`testdata/`), searched by the end-to-end
/// filter tests.
const TESTDATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");

fn mdfu() -> Command {
    Command::cargo_bin("mdfu").unwrap()
}

/// Throwaway Markdown vault removed on drop; `tempfile` is not a dev-dep and
/// the two tests that need one only require a few files.
struct TempVault {
    dir: PathBuf,
}

impl TempVault {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mdfu-cli-pipeline-{}-{tag}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.dir.join(name), content).unwrap();
    }

    fn root(&self) -> &str {
        self.dir.to_str().unwrap()
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Runs the built binary in `dir` and returns the raw captured output, so
/// byte-level comparisons (non-TTY `tasks` vs `task list`) stay exact.
fn run_in(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut command = mdfu();
    command.current_dir(dir).args(args);
    command.output().unwrap()
}

#[test]
fn root_version_prints_mdfu_and_package_version() {
    mdfu()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("mdfu {VERSION}\n"))
        .stderr("");
}

#[test]
fn root_config_default_prints_builtin_yaml() {
    mdfu()
        .args(["--config", "default"])
        .assert()
        .success()
        .stdout(mdfu_core::theme::DEFAULT_YAML)
        .stderr("");
}

#[test]
fn root_format_bogus_exits_2_with_documented_message() {
    mdfu()
        .args(["--format", "bogus"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "invalid --format \"bogus\": must be paths|json|vimgrep",
        ));
}

#[test]
fn root_unknown_flag_exits_2() {
    mdfu()
        .arg("--definitely-not-a-flag")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("--definitely-not-a-flag"));
}

#[test]
fn root_help_exits_0() {
    mdfu()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: mdfu"));
}

#[test]
fn root_theme_load_error_exits_2() {
    mdfu()
        .args(["--config", "/nonexistent/mdfu-root-test/config.yaml"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("mdfu: theme: read "));
}

#[test]
fn pipeline_filter_kumquat_zebra_on_testdata() {
    mdfu()
        .args(["--root", TESTDATA, "kumquat zebra"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.lines().count() == 1 && out.trim_end().ends_with("bad-yaml.md")
        }))
        .stderr("");
}

#[test]
fn pipeline_filter_no_match_exits_1_without_output() {
    mdfu()
        .args(["--root", TESTDATA, "zzzznotathing"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("");
}

#[test]
fn pipeline_filter_limit_caps_results() {
    let vault = TempVault::new("limit");
    for name in ["a.md", "b.md", "c.md"] {
        vault.write(name, "# Note\n\nlighthouse beacon shines here\n");
    }
    mdfu()
        .args(["--root", vault.root(), "lighthouse", "--limit", "2"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| out.lines().count() == 2))
        .stderr("");
}

#[test]
fn pipeline_filter_json_reports_snippet_and_score() {
    mdfu()
        .args(["--root", TESTDATA, "--format", "json", "kumquat zebra"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"snippet\":")
                .and(predicate::str::contains("kumquat"))
                .and(predicate::str::contains("\"score\": 0.0")),
        )
        .stderr("");
}

#[test]
fn pipeline_filter_vimgrep_reports_path_line_column() {
    mdfu()
        .args(["--root", TESTDATA, "--format", "vimgrep", "kumquat zebra"])
        .assert()
        .success()
        .stdout(predicate::str::contains(":1:1:"))
        .stderr("");
}

#[test]
fn pipeline_filter_excludes_archived_unless_flag() {
    let vault = TempVault::new("archived");
    vault.write("active.md", "# Active\n\nlighthouse beacon\n");
    vault.write(
        "done.md",
        "---\nstatus: archived\n---\n\n# Done\n\nlighthouse beacon\n",
    );

    mdfu()
        .args(["--root", vault.root(), "lighthouse"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.contains("active.md") && !out.contains("done.md")
        }))
        .stderr("");

    mdfu()
        .args(["--root", vault.root(), "--archived", "lighthouse"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.contains("active.md") && out.contains("done.md")
        }))
        .stderr("");
}

#[test]
fn exit_root_short_help_exits_0() {
    mdfu()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: mdfu"))
        .stderr("");
}

#[test]
fn exit_root_flag_missing_value_exits_2() {
    mdfu()
        .arg("--root")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("a value is required"));
}

#[test]
fn exit_filter_scan_error_exits_1_with_stderr_only() {
    mdfu()
        .args(["--root", "/nonexistent/mdfu-task-31", "anything"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::starts_with("mdfu: ")
                .and(predicate::str::contains("/nonexistent/mdfu-task-31")),
        );
}

#[test]
fn exit_filter_query_parse_error_exits_1_with_stderr_only() {
    mdfu()
        .args(["--root", TESTDATA, "created:notadate"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::starts_with("mdfu: ").and(predicate::str::contains("invalid date")),
        );
}

#[test]
fn exit_task_list_success_ignores_unrecognised_sort() {
    let vault = TempVault::new("sort");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .args(["task", "list", "--sort", "bogus", "--path", vault.root()])
        .assert()
        .success()
        .stdout(predicate::str::contains("EXMPL-001"))
        .stderr("");
}

#[test]
fn exit_task_validate_warnings_only_exits_0() {
    let vault = TempVault::new("warn");
    vault.write("a.md", "- [ ] AAA-001 First\n- [ ] BBB-001 Second\n");
    mdfu()
        .args(["task", "validate", "--path", vault.root()])
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains(
            "warning: duplicate numeric part 1 across prefixes: AAA-001, BBB-001",
        ));
}

#[test]
fn exit_e1_unknown_command_exits_1_and_prints_help() {
    mdfu()
        .args(["task", "--", "bogus"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::starts_with("mdfu task: unknown command 'bogus'\n")
                .and(predicate::str::contains("Usage: mdfu task <command>")),
        );
}

#[test]
fn exit_e2_root_flag_before_task_exits_2() {
    mdfu()
        .args(["--root", "/tmp", "task", "list"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("mdfu task: --root must come after the task subcommand\n");
}

#[test]
fn exit_e3_tasks_unexpected_argument_exits_2() {
    let vault = TempVault::new("e3");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .args(["tasks", "bogus", "--path", vault.root()])
        .assert()
        .code(2)
        .stdout("")
        .stderr("mdfu tasks: unexpected argument 'bogus'\n");
}

#[test]
fn exit_e4_missing_path_exits_1() {
    mdfu()
        .args(["task", "list", "--path", "/nonexistent/mdfu-task-31"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            "mdfu task: --path '/nonexistent/mdfu-task-31' does not exist or is not a \
             file/directory\n",
        );
}

#[test]
fn exit_e6_invalid_config_json_exits_1() {
    let vault = TempVault::new("e6");
    vault.write("tasks.md", TASK_VAULT);
    vault.write(".mdfurc", "{not json");

    let mut command = mdfu();
    command.current_dir(vault.path()).args(["task", "list"]);
    command.assert().code(1).stdout("").stderr(
        predicate::str::contains("Invalid JSON in").and(predicate::str::contains(".mdfurc")),
    );
}

#[test]
fn exit_e8_editor_not_set_exits_1() {
    let vault = TempVault::new("e8");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .env_remove("EDITOR")
        .args(["task", "open", "EXMPL-001", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: $EDITOR is not set\n");
}

#[test]
fn exit_e9_invalid_task_id_exits_1() {
    let vault = TempVault::new("e9");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .args(["task", "view", "notanid", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: invalid task ID 'notanid'\n");
}

#[test]
fn exit_e10_task_not_found_exits_1() {
    let vault = TempVault::new("e10");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .args(["task", "view", "ZZZ-999", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: task 'ZZZ-999' not found\n");
}

#[test]
fn exit_e11_ambiguous_numeric_id_exits_1() {
    let vault = TempVault::new("e11");
    vault.write("a.md", "- [ ] AAA-001 First\n- [ ] BBB-001 Second\n");
    mdfu()
        .args(["task", "view", "1", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: ambiguous numeric ID '1' matches: AAA-001, BBB-001\n");
}

#[test]
fn exit_e12_duplicate_id_exits_1() {
    let vault = TempVault::new("e12");
    vault.write("d.md", "- [ ] DUP-001 One\n- [ ] DUP-001 Two\n");
    mdfu()
        .args(["task", "view", "DUP-001", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: task 'DUP-001' appears multiple times; expected exactly one match\n");
}

#[test]
fn exit_e13_invalid_prefix_exits_1() {
    let vault = TempVault::new("e13");
    vault.write("ids.md", "- [ ] Plain task without an id\n");
    mdfu()
        .args(["task", "ids", "--prefix", "1bad", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: invalid prefix '1BAD'\n");
}

#[test]
fn exit_e14_e15_set_missing_arguments_exit_1() {
    let vault = TempVault::new("e14");
    vault.write("tasks.md", TASK_VAULT);

    mdfu()
        .args(["task", "set", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: no task IDs provided\n");

    mdfu()
        .args(["task", "set", "EXMPL-001", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: no metadata tokens provided\n");
}

#[test]
fn exit_e18_archive_open_task_exits_1() {
    let vault = TempVault::new("e18");
    vault.write("tasks.md", TASK_VAULT);
    mdfu()
        .args(["task", "archive", "EXMPL-001", "--path", vault.root()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: task 'EXMPL-001' is not done\n");
}

#[test]
fn exit_e19_archive_outside_base_exits_1() {
    let vault = TempVault::new("e19");
    vault.write("done.md", "- [x] ARC-001 Done thing\n");
    vault.write(".mdfurc", "{\"archivePath\":\"../outside-archive.md\"}");

    let mut command = mdfu();
    command.current_dir(vault.path()).args(["task", "archive"]);
    command
        .assert()
        .code(1)
        .stdout("")
        .stderr("mdfu task: archive path '../outside-archive.md' is outside the base directory\n");
}

#[test]
fn exit_task_usage_errors_exit_2() {
    let vault = TempVault::new("usage2");
    vault.write("tasks.md", TASK_VAULT);

    mdfu()
        .args(["task", "list", "--bogus", "--path", vault.root()])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::starts_with(
            "flag provided but not defined: -bogus",
        ));

    mdfu()
        .args(["task", "view", "--path", vault.root()])
        .assert()
        .code(2)
        .stdout("")
        .stderr("mdfu task: view requires exactly one task ID\n");
}

/// Vault with a single HTML-title note whose body holds the query term.
fn html_vault(tag: &str) -> TempVault {
    let vault = TempVault::new(tag);
    vault.write(
        "escape.md",
        "---\ntitle: \"<b>&</b>\"\n---\n\nneedle text here\n",
    );
    vault
}

#[test]
fn format_paths_emits_one_absolute_path_per_line() {
    let vault = html_vault("paths");
    let path = vault.path().join("escape.md");
    let path = path.to_string_lossy();

    mdfu()
        .args(["--root", vault.root(), "--format", "paths", "needle"])
        .assert()
        .success()
        .stdout(format!("{path}\n"))
        .stderr("");
}

#[test]
fn format_vimgrep_emits_path_line_column_title() {
    let vault = html_vault("vimgrep");
    let path = vault.path().join("escape.md");
    let path = path.to_string_lossy();

    mdfu()
        .args(["--root", vault.root(), "--format", "vimgrep", "needle"])
        .assert()
        .success()
        .stdout(format!("{path}:1:1:<b>&</b>\n"))
        .stderr("");
}

#[test]
fn format_json_shape_matches_go_field_order_and_escapes_html() {
    let vault = html_vault("json");
    let path = vault.path().join("escape.md");
    let path = path.to_string_lossy();

    // Go `encoding/json` HTML escaping (`\u003c`, `\u003e`, `\u0026`) over the
    // two-space-indented, `path,score,title,type,snippet`-ordered document;
    // no trailing newline.
    let expected = format!(
        "[\n  {{\n    \"path\": \"{path}\",\n    \"score\": 0.0,\n    \"title\": \
         \"\\u003cb\\u003e\\u0026\\u003c/b\\u003e\",\n    \"type\": \"\",\n    \"snippet\": \
         \"needle text here\"\n  }}\n]"
    );

    mdfu()
        .args(["--root", vault.root(), "--format", "json", "needle"])
        .assert()
        .success()
        .stdout(expected)
        .stderr("");
}

#[test]
fn format_task_json_keeps_html_literal() {
    let vault = TempVault::new("taskjson");
    vault.write("tasks.md", "- [ ] ESC-001 <b>&</b> mine\n");

    mdfu()
        .args(["task", "list", "--json", "--path", vault.root()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"id\": \"ESC-001\"")
                .and(predicate::str::contains("\"title\": \"<b>&</b> mine\""))
                .and(predicate::str::contains("\\u003c").not()),
        )
        .stderr("");
}

#[test]
fn router_d2_unknown_token_falls_through_to_filter() {
    let vault = TempVault::new("d2");
    vault.write(
        "retention.md",
        "# Retention\n\nCohort retention and task tracking notes.\n",
    );

    let mut command = mdfu();
    command
        .current_dir(vault.path())
        .args(["task", "retention"]);
    command
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.lines().any(|line| line.ends_with("retention.md"))
        }))
        .stderr(predicate::str::contains("unknown command").not());
}

#[test]
fn router_double_dash_bypasses_task_routing() {
    let vault = TempVault::new("bypass");
    vault.write(
        "retention.md",
        "# Retention\n\nCohort retention and task tracking notes.\n",
    );

    let mut command = mdfu();
    command.current_dir(vault.path()).args(["--", "task"]);
    command
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.lines().any(|line| line.ends_with("retention.md"))
        }))
        .stderr("");
}

#[test]
fn tasks_non_tty_output_equals_task_list_byte_for_byte() {
    let vault = TempVault::new("parity");
    vault.write("tasks.md", TASK_VAULT);

    for extra in [&[][..], &["--all"][..]] {
        let mut tasks_args = vec!["tasks", "--path", vault.root()];
        tasks_args.extend_from_slice(extra);
        let mut list_args = vec!["task", "list", "--path", vault.root()];
        list_args.extend_from_slice(extra);

        let tasks = run_in(vault.path(), &tasks_args);
        let list = run_in(vault.path(), &list_args);

        assert_eq!(tasks.status.code(), list.status.code(), "exit codes differ");
        assert_eq!(tasks.stdout, list.stdout, "stdout differs");
        assert_eq!(tasks.stderr, list.stderr, "stderr differs");

        let stdout = String::from_utf8(tasks.stdout.clone()).unwrap();
        assert!(
            stdout.contains("EXMPL-001"),
            "vacuous parity without a listed task: {stdout}"
        );
        assert!(
            !stdout.contains("\u{1b}[?1049h"),
            "non-TTY tasks must not enter the alternate screen: {stdout}"
        );
    }
}

#[test]
fn limit_filter_default_is_20_and_overrides_win() {
    let vault = TempVault::new("limit");
    for index in 1..=25 {
        vault.write(
            &format!("note-{index:02}.md"),
            "# Note\n\nbanana smoothie recipe\n",
        );
    }

    mdfu()
        .args(["--root", vault.root(), "banana"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| out.lines().count() == 20))
        .stderr("");

    mdfu()
        .args(["--root", vault.root(), "--limit", "0", "banana"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| out.lines().count() == 25))
        .stderr("");

    mdfu()
        .args(["--root", vault.root(), "banana", "--limit", "3"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| out.lines().count() == 3))
        .stderr("");
}

#[test]
fn limit_tui_default_is_fifty_at_the_constant() {
    // The interactive TUI cannot run under a piped stdout, so the TUI default
    // is pinned at its definition; the `root_tui_limit_defaults` unit test
    // exercises the `tui_limit` accessor that reads it.
    let source = include_str!("../src/main.rs");
    let compact: String = source.split_whitespace().collect();
    assert!(compact.contains("constDEFAULT_TUI_LIMIT:i64=50;"));
    assert!(compact.contains("constDEFAULT_FILTER_LIMIT:i64=20;"));
}
