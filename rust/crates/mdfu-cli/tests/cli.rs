//! `assert_cmd` integration coverage for the root CLI acceptance matrix:
//! `--version`, `--config default`, invalid `--format`, unknown flag, and
//! `--help`, plus the theme-load error path. The `pipeline_` tests cover filter
//! mode end to end against the shared fixture vault and throwaway vaults.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use predicates::prelude::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The repository fixture vault (`testdata/`), searched by the end-to-end
/// filter tests.
const TESTDATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata");

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
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
