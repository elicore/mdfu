//! `assert_cmd` integration coverage for the root CLI acceptance matrix:
//! `--version`, `--config default`, invalid `--format`, unknown flag, and
//! `--help`, plus the theme-load error path.

#![allow(clippy::panic, clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn mdfu() -> Command {
    Command::cargo_bin("mdfu").unwrap()
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
