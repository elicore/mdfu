//! Architectural guard tests, the Rust port of the Go `internal/tui` guards
//! (`tasks_run_test.go` `grepTasksSources`) and `internal/task`
//! (`install_skills_test.go` foreign-skill check).
//!
//! Both invariants run as pure `#[test]` functions, so they execute on every
//! `cargo test` (including CI) with no external grep/shell step:
//!
//! 1. **No note-picker filter leaks into the task UI.** Every `.rs` file under
//!    `src/tasks/` is enumerated from the filesystem at test time and scanned
//!    for the note picker's filter type and global-filter vocabulary. The scan
//!    has no hardcoded file list and no exclusions, so a new `tasks/*.rs` file
//!    is covered automatically.
//! 2. **Assets name no foreign skill.** The three embedded `SKILL.md` files must
//!    not mention the foreign `mdtask`-family names.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// Substrings that must never appear in `src/tasks/`: the note picker's module
/// path plus the global-filter vocabulary the Go guard forbids (`SetFilter` /
/// `globalFilter`, here also in `snake_case` form).
const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "crate::notes",
    "notes::",
    "GlobalFilter",
    "globalFilter",
    "global_filter",
    "SetFilter",
    "setFilter",
    "set_filter",
];

/// Identifier tokens that must never appear in `src/tasks/`. Matched with word
/// boundaries so the task UI's own `TaskFilterFn` is not a false positive.
const FORBIDDEN_WORDS: &[&str] = &["FilterFn"];

/// Embedded `SKILL.md` assets that must ship free of foreign skill names.
const SKILL_NAMES: &[&str] = &["mdfu-task", "mdfu-task-add", "mdfu-task-do"];

/// Foreign skill names forbidden inside the embedded assets.
const FOREIGN_SKILL_NAMES: &[&str] = &["mdtask", "mdtask-add", "mdtask-do"];

fn tasks_sources_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tasks")
}

fn skills_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../mdfu-core/assets/skills")
}

/// Scans one source file and returns `file:line: text` entries for every line
/// that references a forbidden token. An empty result means the file is clean.
fn scan_source(name: &str, content: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if let Some(token) = first_forbidden(line) {
            hits.push(format!(
                "{name}:{}: {} (forbidden: {token})",
                index + 1,
                line.trim()
            ));
        }
    }
    hits
}

/// Returns the first forbidden token present on `line`, if any.
fn first_forbidden(line: &str) -> Option<&'static str> {
    FORBIDDEN_SUBSTRINGS
        .iter()
        .copied()
        .find(|token| line.contains(*token))
        .or_else(|| {
            FORBIDDEN_WORDS
                .iter()
                .copied()
                .find(|word| contains_word(line, word))
        })
}

/// Whole-identifier match (`\bword\b`, ASCII word chars): `FilterFn` is a hit
/// but `TaskFilterFn` is not.
fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut search_from = 0;
    while let Some(offset) = haystack[search_from..].find(word) {
        let start = search_from + offset;
        let end = start + word.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        search_from = start + 1;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Reads every `.rs` file under `src/tasks/` at runtime. No file is excluded.
fn read_task_sources() -> Vec<(String, String)> {
    let dir = tasks_sources_dir();
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|error| panic!("read_dir {}: {error}", dir.display()));
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("dir entry: {error}"))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .unwrap_or_else(|| panic!("no file name: {}", path.display()))
                .to_string_lossy()
                .into_owned();
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (name, content)
        })
        .collect()
}

#[test]
fn guards_task_sources_reference_no_note_picker_filter() {
    let sources = read_task_sources();
    assert!(!sources.is_empty(), "the guard read no tasks sources");
    let total_bytes: usize = sources.iter().map(|(_, content)| content.len()).sum();
    assert!(total_bytes > 0, "the guard read zero source bytes");

    let hits: Vec<String> = sources
        .iter()
        .flat_map(|(name, content)| scan_source(name, content))
        .collect();
    assert!(
        hits.is_empty(),
        "tasks sources reference the note picker's filter/global:\n{}",
        hits.join("\n")
    );

    // Non-vacuous sanity: the task UI's own hook really is present, so a clean
    // result means the scan looked at real content, not an empty directory.
    let sanity = sources
        .iter()
        .any(|(_, content)| content.contains("TaskFilterFn"));
    assert!(
        sanity,
        "sanity token TaskFilterFn missing; the forbidden scan is vacuous"
    );
}

#[test]
fn guards_scan_covers_every_tasks_source() {
    let dir = tasks_sources_dir();
    let mut expected: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read_dir {}: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("dir entry: {error}"))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    expected.sort();

    let scanned: Vec<String> = read_task_sources()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        scanned, expected,
        "the guard must scan every tasks source (no exclusions)"
    );
}

#[test]
fn guards_scanner_flags_injected_note_picker_filter() {
    let injected = "use crate::notes::FilterFn as GlobalFilter;";
    let hits = scan_source("mutant.rs", injected);
    assert!(
        !hits.is_empty(),
        "scanner failed to flag an injected note-picker filter reference"
    );
    assert!(
        hits[0].contains("mutant.rs:1:"),
        "hit must carry file+line: {:?}",
        hits[0]
    );
}

#[test]
fn guards_scanner_ignores_task_ui_own_filter_type() {
    let clean = "pub type TaskFilterFn = Box<dyn Fn(&str, &[Task]) -> Vec<usize>>;";
    assert!(
        scan_source("clean.rs", clean).is_empty(),
        "TaskFilterFn must not trip the note-picker guard"
    );
}

#[test]
fn guards_skill_assets_name_no_foreign_skill() {
    let dir = skills_dir();
    for name in SKILL_NAMES {
        let path = dir.join(name).join("SKILL.md");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(!content.is_empty(), "{name}/SKILL.md is empty");
        for foreign in FOREIGN_SKILL_NAMES {
            assert!(
                !content.contains(foreign),
                "{name}/SKILL.md names the foreign skill {foreign}"
            );
        }
    }
}
