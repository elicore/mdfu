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
//! 3. **Every watched `pub` field keeps a non-test reader.** The `pub` fields
//!    of both `Config` structs, the task `KeyMap`, and `Theme` are scanned so
//!    a parsed/stored setting that changes nothing (unwired `--no-hyperlinks`,
//!    dead `tab` focus, unread `highlight_sgr`) fails the build instead of
//!    shipping as false parity.

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

/// (struct file, struct name, holder prefix) triples whose `pub` fields must
/// each keep a holder-qualified, non-test reader. The holder (`config.`,
/// `keys.`, `theme.`) also matches `self.`-qualified receivers, so both call
/// styles count.
const WATCHED_STRUCTS: &[(&str, &str, &str)] = &[
    ("src/notes/app.rs", "Config", "config."),
    ("src/tasks/model.rs", "Config", "config."),
    ("src/tasks/model.rs", "KeyMap", "keys."),
    ("../mdfu-core/src/theme.rs", "Theme", "theme."),
];

/// (struct, field) pairs knowingly without a production reader, each recorded
/// in `PARITY.md` (todo 6 reconciliation). Wiring one requires dropping
/// it here; adding one requires a matching PARITY.md line.
const ALLOWLIST_UNREAD: &[(&str, &str)] =
    &[("Theme", "preview_header"), ("Theme", "markdown_style")];

fn crate_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn cli_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../mdfu-cli/src")
}

/// Recursively reads every `.rs` file under `dir`, returning
/// (`display_path`, `content`) with display paths relative to the crate dir.
fn read_rs_recursive(dir: &Path, base: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("read_dir {}: {error}", dir.display()));
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("dir entry: {error}"))
                .path()
        })
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            out.extend(read_rs_recursive(&path, base));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let name = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            out.push((name, content));
        }
    }
    out
}

/// First line index holding `#[cfg(test)]`, if any. Everything from there on
/// counts as test code (each scanned file keeps a single trailing test
/// module, matching the repo layout).
fn test_start_line(content: &str) -> Option<usize> {
    content
        .lines()
        .position(|line| line.contains("#[cfg(test)]"))
}

/// `pub` field names of `pub struct {struct_name} { ... }`. Stops at the
/// closing brace; the watched structs carry no brace-bearing field types.
fn struct_pub_fields(struct_name: &str, content: &str) -> Vec<String> {
    let header = format!("pub struct {struct_name} {{");
    let mut fields = Vec::new();
    let mut in_struct = false;
    let mut depth = 0i32;
    for line in content.lines() {
        if !in_struct {
            if line.trim() == header {
                in_struct = true;
                depth = 1;
            }
            continue;
        }
        depth += i32::try_from(line.chars().filter(|ch| *ch == '{').count()).unwrap_or(i32::MAX);
        depth -= i32::try_from(line.chars().filter(|ch| *ch == '}').count()).unwrap_or(i32::MAX);
        if depth <= 0 {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub ") {
            if let Some(name) = rest.split([' ', ':']).next() {
                if !name.is_empty() && line.contains(':') {
                    fields.push(name.to_owned());
                }
            }
        }
    }
    fields
}

/// Code portion of a line: strips `//` comments (doc comments included) while
/// leaving `https://` URLs intact.
fn code_line(line: &str) -> &str {
    match line.find("//") {
        Some(pos) if pos == 0 || line.as_bytes()[pos - 1].is_ascii_whitespace() => &line[..pos],
        _ => line,
    }
}

/// Whether `line` reads `holder.field` (e.g. `config.limit`, also matching
/// `self.config` receivers through the substring). Assignments (`=`, not
/// `==`) do not count as reads.
fn has_holder_read(holder: &str, field: &str, line: &str) -> bool {
    let code = code_line(line);
    let needle = format!("{holder}{field}");
    let mut from = 0;
    while let Some(pos) = code[from..].find(needle.as_str()) {
        let start = from + pos;
        let end = start + needle.len();
        let after_ok = end == code.len() || !is_ident_byte(code.as_bytes()[end]);
        if after_ok {
            let rest = code[end..].trim_start();
            if !rest.starts_with('=') || rest.starts_with("==") {
                return true;
            }
        }
        from = start + 1;
    }
    false
}

/// Whether any non-test line of `content` (outside `skip_line`, the field's
/// own definition) reads `holder.field`.
fn file_has_reader(holder: &str, field: &str, content: &str, skip_line: Option<usize>) -> bool {
    let test_from = test_start_line(content);
    content
        .lines()
        .enumerate()
        .filter(|(index, _)| Some(*index) != skip_line)
        .filter(|(index, _)| test_from.is_none_or(|first| *index < first))
        .any(|(_, line)| has_holder_read(holder, field, line))
}

#[test]
fn guards_watched_struct_fields_keep_non_test_readers() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = read_rs_recursive(&crate_src_dir(), crate_dir);
    files.extend(read_rs_recursive(&cli_src_dir(), crate_dir));
    assert!(!files.is_empty(), "the guard read no sources");

    let mut violations: Vec<String> = Vec::new();
    let mut checked = 0;
    for (rel, struct_name, holder) in WATCHED_STRUCTS {
        let path = crate_dir.join(rel);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let fields = struct_pub_fields(struct_name, &content);
        assert!(
            !fields.is_empty(),
            "{rel} struct {struct_name} parsed zero fields; the field parser broke"
        );
        for field in fields {
            checked += 1;
            let read = files
                .iter()
                .any(|(_, content)| file_has_reader(holder, &field, content, None));
            let allowed = ALLOWLIST_UNREAD
                .iter()
                .any(|(name, allowed)| *name == *struct_name && *allowed == field);
            if allowed {
                assert!(
                    !read,
                    "{struct_name}.{field} gained a production reader; drop it from ALLOWLIST_UNREAD"
                );
            } else if !read {
                violations.push(format!(
                    "{struct_name}.{field} has no non-test `{holder}{field}` reader ({rel})"
                ));
            }
        }
    }
    assert!(
        checked > 0,
        "the guard checked zero fields; the struct scan is vacuous"
    );
    assert!(
        violations.is_empty(),
        "watched pub fields without a production reader (wire them or record them in PARITY.md):\n{}",
        violations.join("\n")
    );
}

#[test]
fn guards_field_reader_flags_unread_field() {
    let def = "pub struct Config {\n    pub ghost_flag: bool,\n    pub used_flag: bool,\n}\n";
    assert_eq!(
        struct_pub_fields("Config", def),
        vec!["ghost_flag".to_owned(), "used_flag".to_owned()]
    );
    let usage = "fn new(config: Config) -> bool {\n    config.used_flag\n}\n";
    assert!(
        file_has_reader("config.", "used_flag", usage, None),
        "a real read must count"
    );
    assert!(
        !file_has_reader("config.", "ghost_flag", usage, None),
        "an unread field must be flagged"
    );
    assert!(
        !file_has_reader("config.", "used_flag", "    used_flag: true,\n", None),
        "a struct-literal write is not a read"
    );
    assert!(
        !has_holder_read("config.", "used_flag", "    // config.used_flag default"),
        "comments are not reads"
    );
    assert!(
        !has_holder_read("theme.", "dim", "    theme.dim = other;\n"),
        "assignments are not reads"
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
