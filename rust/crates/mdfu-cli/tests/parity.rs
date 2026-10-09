//! Go-vs-Rust differential parity harness (plan task 32).
//!
//! This integration test runs the frozen-SHA Go binary and the Rust binary over
//! identical vaults and compares the observable non-TTY surface byte for byte:
//! `stdout`, `stderr`, the exit code, and the files the command rewrote.
//!
//! The Go binary is supplied by [`MDFU_PARITY_GO`]; `scripts/parity.sh` builds
//! it from the baseline SHA recorded in `rust/PARITY.md` and runs this test in
//! release mode. When the variable is unset (a plain `cargo test` without a Go
//! toolchain) every matrix test prints `PARITY-SKIP` and returns, so the normal
//! Rust CI stays green; `scripts/parity.sh` always sets it.
//!
//! # Covered surface
//!
//! * Note filter: `--format paths|json|vimgrep` over the repository `testdata/`
//!   fixture and a synthetic notes vault (bad YAML, tags, archived/hidden,
//!   dates, HTML-escaping characters, no-frontmatter fallback), including the
//!   `--archived`, `--hidden`, `--no-ignore`, and `--limit` flags and the
//!   0/1/2 exit-code rows reachable without a TTY.
//! * Task commands in non-TTY mode: `list` (flags included), `view`, `set`,
//!   `ids`, `archive`, `validate`, and `mdfu tasks`, over `testdata/tasks/` and
//!   a synthetic vault with `.mdfurc` exclude rules, `_archive.md`, seeds, and
//!   duplicate IDs.
//!
//! # Deliberate exclusions and normalizations
//!
//! TTY box bytes, SGR-exact output, and the interactive TUI are excluded and
//! recorded in `rust/PARITY.md` (asserted by
//! [`parity_docs_record_the_non_testable_surfaces`]). Exactly one normalization
//! is applied, to note-filter JSON stdout: Go `encoding/json` renders the
//! constant `0` score as `0` while `serde_json` renders `0.0`
//! ([`Normalization::NoteJsonScore`]).
//! [`parity_note_json_score_normalization_is_the_only_difference`] proves that
//! the raw outputs differ *only* on `"score"` lines.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Environment variable holding the path to the frozen-SHA Go binary.
const GO_BIN_ENV: &str = "MDFU_PARITY_GO";
/// Optional override for the Rust binary; defaults to the cargo-built `mdfu`.
const RUST_BIN_ENV: &str = "MDFU_PARITY_RUST";
/// Repository fixture vault (`testdata/`).
const TESTDATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata");
/// The parity contract document, checked for the exclusion/normalization list.
const PARITY_MD: &str = include_str!("../../../PARITY.md");
/// Go's rendering of the constant zero score (see module docs).
const GO_SCORE_ZERO: &[u8] = b"\"score\": 0,";
/// Rust's rendering of the same constant zero score.
const RUST_SCORE_ZERO: &[u8] = b"\"score\": 0.0,";

/// One vault source for the synthetic fixtures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Vault {
    /// A copy of the repository `testdata/` fixture (notes view).
    TestdataNotes,
    /// The handwritten synthetic notes vault below.
    SyntheticNotes,
    /// A copy of the repository `testdata/tasks/` fixture.
    TasksFixture,
    /// The handwritten synthetic tasks vault below.
    TasksSynthetic,
}

/// The only normalization this harness may apply, always cited in
/// `rust/PARITY.md`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Normalization {
    /// Byte-exact.
    None,
    /// Note-filter `--format json` stdout only: compare Go's `"score": 0,`
    /// as Rust's `"score": 0.0,`.
    NoteJsonScore,
}

/// One differential case: run `args` with `vault` as cwd under both binaries.
struct Case {
    name: &'static str,
    vault: Vault,
    args: &'static [&'static str],
    norm: Normalization,
}

/// Synthetic notes vault: exercises body/frontmatter search, hidden/`.git`
/// exclusion, archived hiding, tag and date filters, the HTML-escaping
/// characters Go's JSON encoder escapes, and both title fallbacks.
const SYNTHETIC_NOTES: &[(&str, &str)] = &[
    (
        "alpha.md",
        "---\ntitle: Alpha <b>bold</b> & \"clever\"\ntags: [growth, kpi]\n---\n\n# Alpha\n\nthe lighthouse beacon shines over the harbor with extra filler text to keep the snippet long enough\n",
    ),
    (
        "bad.md",
        "---\ntype: [unclosed\n  title: \"missing quote\ntags: [a, b\n---\n\n# Recovered Title\n\nBody still searchable: kumquat zebra xylophone.\n",
    ),
    (
        "archived.md",
        "---\ntitle: Old Note\narchived: true\n---\n\n# Old\n\nlighthouse archived content\n",
    ),
    (
        "by_status.md",
        "---\ntitle: By Status\nstatus: archived\n---\n\n# By Status\n\nlighthouse by-status content\n",
    ),
    (
        "deep/nested.md",
        "---\ntitle: Nested Note\ncreated: 2024-06-01\nupdated: 2024-06-02\ntags: [growth]\n---\n\n# Nested\n\ndeep lighthouse keeper notes\n",
    ),
    (".hidden.md", "# Hidden\n\nlighthouse hidden\n"),
    (".hiddendir/in.md", "# HiddenDir\n\nlighthouse hiddendir\n"),
    (".git/evil.md", "# Evil\n\nlighthouse git\n"),
    (
        "lone.md",
        "# Lone Note\n\nJust body, no frontmatter at all. Mentions honeycrisp apples.\n",
    ),
    (
        "nofname.md",
        "just plain text mentioning durian fruit with no heading at all\n",
    ),
    (
        "dates.md",
        "---\ntitle: Mid\ncreated: 2024-03-15\n---\n\n# Mid\n\nmid body\n",
    ),
];

/// Synthetic tasks vault: `.mdfurc` include/exclude rules, a seed task, a
/// duplicate ID for `validate`, and a pre-existing `_archive.md`.
const SYNTHETIC_TASKS: &[(&str, &str)] = &[
    (
        ".mdfurc",
        "{\"files\":{\"exclude\":[\"skip/**\"]},\"archivePath\":\"_archive.md\"}\n",
    ),
    (
        "main.md",
        "# Main\n\n- [ ] DUP-1 first\n- [ ] DUP-1 second\n- [ ] OK-1 fine\n- [x] OK-2 done\n- [ ] SEED- seed me\n",
    ),
    ("skip/hidden.md", "- [ ] SKIP-1 should be excluded\n"),
    ("_archive.md", "- [x] ARCH-1 old archived task\n"),
];

/// Note-filter cases over `testdata/` and the synthetic notes vault.
const NOTE_CASES: &[Case] = &[
    Case {
        name: "testdata paths kumquat",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "kumquat"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths kumquat zebra",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "kumquat zebra"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata json kumquat",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "kumquat"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata vimgrep kumquat",
        vault: Vault::TestdataNotes,
        args: &["--format", "vimgrep", "kumquat"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata json type:Metric",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "type:Metric"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata paths tag:retention",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "tag:retention"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths tags:growth,kpi",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "tags:growth,kpi"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths tag:-kpi",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "tag:-kpi"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata json status:verified",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "status:verified"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata json created range",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "created:2024-01-01..2024-12-31"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata paths limit 3",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "--limit", "3", "the"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata vimgrep limit 3",
        vault: Vault::TestdataNotes,
        args: &["--format", "vimgrep", "--limit", "3", "the"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata json limit 0",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "--limit", "0", "the"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata paths limit=-1",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "--limit=-1", "the"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths no match",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "zzznope"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata json no match",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "zzznope"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "testdata paths invalid date query",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "created:notadate"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata invalid format",
        vault: Vault::TestdataNotes,
        args: &["--format", "bogus", "x"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths hidden",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "--hidden", "kumquat"],
        norm: Normalization::None,
    },
    Case {
        name: "testdata paths no-ignore",
        vault: Vault::TestdataNotes,
        args: &["--format", "paths", "--no-ignore", "kumquat"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic paths lighthouse",
        vault: Vault::SyntheticNotes,
        args: &["--format", "paths", "lighthouse"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic json lighthouse",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "lighthouse"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic vimgrep lighthouse",
        vault: Vault::SyntheticNotes,
        args: &["--format", "vimgrep", "lighthouse"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic json archived",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "--archived", "lighthouse"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic paths archived",
        vault: Vault::SyntheticNotes,
        args: &["--format", "paths", "--archived", "lighthouse"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic json hidden",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "--hidden", "lighthouse"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic json hidden archived",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "--hidden", "--archived", "lighthouse"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic json kumquat zebra",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "kumquat zebra"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic paths tag:growth",
        vault: Vault::SyntheticNotes,
        args: &["--format", "paths", "tag:growth"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic json created range",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "created:2024-01-01..2024-12-31"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic json no-ignore",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "--no-ignore", "lighthouse"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic json html escaping",
        vault: Vault::SyntheticNotes,
        args: &["--format", "json", "bold"],
        norm: Normalization::NoteJsonScore,
    },
    Case {
        name: "synthetic vimgrep no-frontmatter fallback",
        vault: Vault::SyntheticNotes,
        args: &["--format", "vimgrep", "durian"],
        norm: Normalization::None,
    },
];

/// Task subcommand cases over `testdata/tasks/` and the synthetic tasks vault.
const TASK_CASES: &[Case] = &[
    Case {
        name: "fixture task list",
        vault: Vault::TasksFixture,
        args: &["task", "list"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --all",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--all"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --all --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--all", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --blocked --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--blocked", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --tag --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--all", "--tag", "release", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --priority --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--all", "--priority", "high", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list --sort --json",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--all", "--sort", "priority", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task view",
        vault: Vault::TasksFixture,
        args: &["task", "view", "BASIC-1"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task view --json",
        vault: Vault::TasksFixture,
        args: &["task", "view", "BASIC-1", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task view blocked --json",
        vault: Vault::TasksFixture,
        args: &["task", "view", "BLK-2", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task view missing",
        vault: Vault::TasksFixture,
        args: &["task", "view", "NOPE-1"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task validate",
        vault: Vault::TasksFixture,
        args: &["task", "validate"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture tasks non-TTY",
        vault: Vault::TasksFixture,
        args: &["tasks"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture tasks non-TTY --all --json",
        vault: Vault::TasksFixture,
        args: &["tasks", "--all", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task ids --prefix",
        vault: Vault::TasksFixture,
        args: &["task", "ids", "--prefix", "NEW"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task ids seed",
        vault: Vault::TasksFixture,
        args: &["task", "ids"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task set priority",
        vault: Vault::TasksFixture,
        args: &["task", "set", "BASIC-1", "!low"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task set status",
        vault: Vault::TasksFixture,
        args: &["task", "set", "BASIC-1", "@status:done"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task archive done",
        vault: Vault::TasksFixture,
        args: &["task", "archive", "BASIC-3"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task archive open",
        vault: Vault::TasksFixture,
        args: &["task", "archive", "BASIC-1"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task ids bad prefix",
        vault: Vault::TasksFixture,
        args: &["task", "ids", "--prefix", "1bad"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task set no ids",
        vault: Vault::TasksFixture,
        args: &["task", "set"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task unknown subcommand",
        vault: Vault::TasksFixture,
        args: &["task", "bogus"],
        norm: Normalization::None,
    },
    Case {
        name: "fixture task list missing path",
        vault: Vault::TasksFixture,
        args: &["task", "list", "--path", "nope.md"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task list --json",
        vault: Vault::TasksSynthetic,
        args: &["task", "list", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task list --all --json",
        vault: Vault::TasksSynthetic,
        args: &["task", "list", "--all", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task validate duplicates",
        vault: Vault::TasksSynthetic,
        args: &["task", "validate"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task archive done",
        vault: Vault::TasksSynthetic,
        args: &["task", "archive", "OK-2"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task ids seed",
        vault: Vault::TasksSynthetic,
        args: &["task", "ids"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task ids --prefix",
        vault: Vault::TasksSynthetic,
        args: &["task", "ids", "--prefix", "NEW"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task view --json",
        vault: Vault::TasksSynthetic,
        args: &["task", "view", "OK-1", "--json"],
        norm: Normalization::None,
    },
    Case {
        name: "synthetic task archive duplicate",
        vault: Vault::TasksSynthetic,
        args: &["task", "archive", "DUP-1"],
        norm: Normalization::None,
    },
];

/// One run outcome: captured streams, exit code, and the post-run vault tree.
struct Outcome {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: i32,
    tree: Vec<(String, Vec<u8>)>,
}

/// Throwaway directory removed on drop (`tempfile` is not a dependency).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mdfu-parity-{}-{tag}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Returns `(go_binary, rust_binary)` or `None` when the Go binary is not
/// configured (plain `cargo test` without the script).
fn binaries() -> Option<(PathBuf, PathBuf)> {
    let go = std::env::var_os(GO_BIN_ENV)?;
    let rust = std::env::var_os(RUST_BIN_ENV)
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_mdfu")), PathBuf::from);
    Some((PathBuf::from(go), rust))
}

/// Prints the skip notice once per test and returns `None`.
fn go_binaries_or_skip(test: &str) -> Option<(PathBuf, PathBuf)> {
    let found = binaries();
    if found.is_none() {
        eprintln!("PARITY-SKIP {test}: {GO_BIN_ENV} is not set; run scripts/parity.sh");
    }
    found
}

/// Writes `files` (slash-separated relative paths) under `root`.
fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
}

/// Recursively copies regular files (and directories) from `src` to `dst`.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    let mut entries: Vec<_> = std::fs::read_dir(src)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let to = dst.join(entry.file_name());
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            copy_tree(&entry.path(), &to);
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// Materializes the `kind` vault at `dest`.
fn prepare_vault(kind: Vault, dest: &Path) {
    match kind {
        Vault::TestdataNotes => copy_tree(Path::new(TESTDATA), dest),
        Vault::SyntheticNotes => write_files(dest, SYNTHETIC_NOTES),
        Vault::TasksFixture => copy_tree(&Path::new(TESTDATA).join("tasks"), dest),
        Vault::TasksSynthetic => write_files(dest, SYNTHETIC_TASKS),
    }
}

/// Snapshots every file under `root` as `(relative path, bytes)`, sorted.
fn snapshot_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            collect_files(root, &path, files);
        } else if ty.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, std::fs::read(&path).unwrap()));
        }
    }
}

/// Runs `bin` with `args` in `cwd`, returning `(stdout, stderr, exit code)`.
fn invoke(bin: &Path, cwd: &Path, args: &[&str]) -> (Vec<u8>, Vec<u8>, i32) {
    let output = Command::new(bin)
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", bin.display()));
    (
        output.stdout,
        output.stderr,
        output.status.code().unwrap_or(-1),
    )
}

/// Prepares a fresh copy of `kind` under `work` and runs one case.
fn run_case(bin: &Path, kind: Vault, args: &[&str], work: &Path) -> Outcome {
    let vault = work.join("vault");
    prepare_vault(kind, &vault);
    let (stdout, stderr, code) = invoke(bin, &vault, args);
    Outcome {
        stdout,
        stderr,
        code,
        tree: snapshot_tree(&vault),
    }
}

/// Applies the case's documented normalization to stdout only.
fn normalize(norm: Normalization, bytes: &[u8]) -> Vec<u8> {
    match norm {
        Normalization::None => bytes.to_vec(),
        Normalization::NoteJsonScore => replace_all(bytes, GO_SCORE_ZERO, RUST_SCORE_ZERO),
    }
}

/// Replaces every occurrence of `from` in `haystack` with `to` (byte-exact).
fn replace_all(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Human-readable first-difference window between two byte strings.
fn describe_bytes(left: &[u8], right: &[u8]) -> String {
    let shared = left.len().min(right.len());
    let mut idx = shared;
    for i in 0..shared {
        if left[i] != right[i] {
            idx = i;
            break;
        }
    }
    let window = |bytes: &[u8]| {
        let start = idx.saturating_sub(30);
        let end = (idx + 30).min(bytes.len());
        format!("{:?}", String::from_utf8_lossy(&bytes[start..end]))
    };
    format!(
        "first diff at byte {idx} (go {} B, rust {} B)\n    go:   {}\n    rust: {}",
        left.len(),
        right.len(),
        window(left),
        window(right)
    )
}

/// Human-readable changed/missing/extra file list between two snapshots.
fn describe_tree(go: &[(String, Vec<u8>)], rust: &[(String, Vec<u8>)]) -> String {
    let go_map: BTreeMap<&str, &[u8]> =
        go.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect();
    let rust_map: BTreeMap<&str, &[u8]> = rust
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let keys: BTreeSet<&str> = go_map.keys().chain(rust_map.keys()).copied().collect();
    let mut parts = Vec::new();
    for key in keys {
        match (go_map.get(key), rust_map.get(key)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => parts.push(format!("changed {key}: {}", describe_bytes(a, b))),
            (Some(_), None) => parts.push(format!("missing in rust: {key}")),
            (None, Some(_)) => parts.push(format!("extra in rust: {key}")),
            (None, None) => {}
        }
    }
    parts.join("; ")
}

/// Compares one case; returns a `PARITY-DIFF` report per diverging dimension.
fn compare(name: &str, norm: Normalization, go: &Outcome, rust: &Outcome) -> Vec<String> {
    let mut diffs = Vec::new();
    let go_stdout = normalize(norm, &go.stdout);
    let rust_stdout = normalize(norm, &rust.stdout);
    if go_stdout != rust_stdout {
        diffs.push(format!(
            "PARITY-DIFF {name}: stdout\n  {}",
            describe_bytes(&go_stdout, &rust_stdout)
        ));
    }
    if go.stderr != rust.stderr {
        diffs.push(format!(
            "PARITY-DIFF {name}: stderr\n  {}",
            describe_bytes(&go.stderr, &rust.stderr)
        ));
    }
    if go.code != rust.code {
        diffs.push(format!(
            "PARITY-DIFF {name}: exit code go={} rust={}",
            go.code, rust.code
        ));
    }
    if go.tree != rust.tree {
        diffs.push(format!(
            "PARITY-DIFF {name}: vault tree\n  {}",
            describe_tree(&go.tree, &rust.tree)
        ));
    }
    diffs
}

/// Runs every case and fails with one report per case that diverged.
fn assert_zero_diffs(test: &str, cases: &[Case]) {
    let Some((go_bin, rust_bin)) = go_binaries_or_skip(test) else {
        return;
    };
    let root = TempDir::new("matrix");
    let mut diffs = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let go_work = root.path().join(format!("go-{i}"));
        let rust_work = root.path().join(format!("rust-{i}"));
        std::fs::create_dir_all(&go_work).unwrap();
        std::fs::create_dir_all(&rust_work).unwrap();
        let go = run_case(&go_bin, case.vault, case.args, &go_work);
        let rust = run_case(&rust_bin, case.vault, case.args, &rust_work);
        diffs.extend(compare(case.name, case.norm, &go, &rust));
    }
    if !diffs.is_empty() {
        for diff in &diffs {
            eprintln!("{diff}");
        }
        panic!(
            "PARITY-DIFF {test}: {} of {} cases diverged",
            diffs.len(),
            cases.len()
        );
    }
    eprintln!("PARITY-OK {test}: {} cases byte-identical", cases.len());
}

#[test]
fn parity_note_filter_matrix_reports_zero_diffs() {
    assert_zero_diffs("note-filter", NOTE_CASES);
}

#[test]
fn parity_task_subcommands_report_zero_diffs() {
    assert_zero_diffs("task-subcommands", TASK_CASES);
}

/// The zero-score normalization is the *only* raw difference in note JSON.
#[test]
fn parity_note_json_score_normalization_is_the_only_difference() {
    let Some((go_bin, rust_bin)) = go_binaries_or_skip("score-normalization") else {
        return;
    };
    let case = Case {
        name: "score-normalization",
        vault: Vault::TestdataNotes,
        args: &["--format", "json", "--limit", "0", "the"],
        norm: Normalization::NoteJsonScore,
    };
    let root = TempDir::new("score");
    let go_work = root.path().join("go");
    let rust_work = root.path().join("rust");
    std::fs::create_dir_all(&go_work).unwrap();
    std::fs::create_dir_all(&rust_work).unwrap();
    let go = run_case(&go_bin, case.vault, case.args, &go_work);
    let rust = run_case(&rust_bin, case.vault, case.args, &rust_work);

    assert_eq!(go.code, 0);
    assert_eq!(rust.code, 0);
    assert_ne!(
        go.stdout, rust.stdout,
        "raw note JSON is byte-identical; the score normalization is unnecessary"
    );
    assert_eq!(
        normalize(case.norm, &go.stdout),
        normalize(case.norm, &rust.stdout)
    );

    let go_lines: Vec<&str> = std::str::from_utf8(&go.stdout).unwrap().lines().collect();
    let rust_lines: Vec<&str> = std::str::from_utf8(&rust.stdout).unwrap().lines().collect();
    assert_eq!(go_lines.len(), rust_lines.len());
    let differing: Vec<usize> = go_lines
        .iter()
        .zip(rust_lines.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!differing.is_empty());
    for i in differing {
        assert!(
            go_lines[i].contains("\"score\""),
            "non-score raw diff at line {i}: go={:?} rust={:?}",
            go_lines[i],
            rust_lines[i]
        );
    }
    assert!(
        PARITY_MD.contains("score"),
        "rust/PARITY.md must document the score normalization"
    );
    eprintln!("PARITY-OK score-normalization: raw diff confined to \"score\" lines");
}

/// The comparator itself detects a tamper in each compared dimension.
#[test]
fn parity_comparator_detects_each_divergence_dimension() {
    let base = Outcome {
        stdout: b"out\n".to_vec(),
        stderr: b"warn\n".to_vec(),
        code: 0,
        tree: vec![("a.md".to_string(), b"content\n".to_vec())],
    };
    let identical = Outcome {
        stdout: b"out\n".to_vec(),
        stderr: b"warn\n".to_vec(),
        code: 0,
        tree: vec![("a.md".to_string(), b"content\n".to_vec())],
    };
    assert!(compare("identical", Normalization::None, &base, &identical).is_empty());

    let tampered_stdout = Outcome {
        stdout: b"outX\n".to_vec(),
        ..clone_outcome(&base)
    };
    assert!(!compare("stdout", Normalization::None, &base, &tampered_stdout).is_empty());

    let tampered_stderr = Outcome {
        stderr: b"warnX\n".to_vec(),
        ..clone_outcome(&base)
    };
    assert!(!compare("stderr", Normalization::None, &base, &tampered_stderr).is_empty());

    let tampered_code = Outcome {
        code: 1,
        ..clone_outcome(&base)
    };
    assert!(!compare("code", Normalization::None, &base, &tampered_code).is_empty());

    let changed_tree = Outcome {
        tree: vec![("a.md".to_string(), b"changed\n".to_vec())],
        ..clone_outcome(&base)
    };
    assert!(!compare("tree", Normalization::None, &base, &changed_tree).is_empty());

    let missing_tree = Outcome {
        tree: Vec::new(),
        ..clone_outcome(&base)
    };
    assert!(!compare("tree-missing", Normalization::None, &base, &missing_tree).is_empty());

    // The normalization applies to stdout only: a stderr tamper still fails.
    let stderr_under_norm = Outcome {
        stderr: b"different\n".to_vec(),
        ..clone_outcome(&base)
    };
    assert!(!compare(
        "stderr-under-norm",
        Normalization::NoteJsonScore,
        &base,
        &stderr_under_norm
    )
    .is_empty());
    eprintln!("PARITY-OK comparator: all four dimensions detect a one-byte tamper");
}

fn clone_outcome(outcome: &Outcome) -> Outcome {
    Outcome {
        stdout: outcome.stdout.clone(),
        stderr: outcome.stderr.clone(),
        code: outcome.code,
        tree: outcome.tree.clone(),
    }
}

/// The missing-root scan error is a recorded divergence, not a silent one.
#[test]
fn parity_missing_root_is_a_documented_divergence() {
    let Some((go_bin, rust_bin)) = go_binaries_or_skip("missing-root") else {
        return;
    };
    let missing = std::env::temp_dir().join(format!("mdfu-parity-missing-{}", std::process::id()));
    let root = missing.to_str().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let (go_out, go_err, go_code) = invoke(&go_bin, &cwd, &["--root", root, "x"]);
    let (rs_out, rs_err, rs_code) = invoke(&rust_bin, &cwd, &["--root", root, "x"]);
    let go_err = String::from_utf8_lossy(&go_err).into_owned();
    let rs_err = String::from_utf8_lossy(&rs_err).into_owned();

    assert_eq!(go_code, 1);
    assert_eq!(rs_code, 1);
    assert!(go_out.is_empty());
    assert!(rs_out.is_empty());
    assert!(go_err.starts_with("mdfu: "), "go stderr: {go_err}");
    assert!(rs_err.starts_with("mdfu: "), "rust stderr: {rs_err}");
    assert!(go_err.contains("lstat"), "go stderr: {go_err}");
    assert!(rs_err.contains("IO error"), "rust stderr: {rs_err}");
    assert!(
        PARITY_MD.contains("lstat"),
        "rust/PARITY.md must record the missing-root stderr divergence"
    );
    eprintln!(
        "PARITY-OK missing-root: exit 1 and `mdfu: ` prefix match; text divergence documented"
    );
}

/// `rust/PARITY.md` lists the exact non-testable surfaces and the one
/// normalization; this guard fails when the document drifts from the harness.
#[test]
fn parity_docs_record_the_non_testable_surfaces() {
    for needle in [
        "Differential parity harness",
        "TTY box bytes",
        "SGR-exact output",
        "Interactive TUI",
        "Harness normalization",
    ] {
        assert!(
            PARITY_MD.contains(needle),
            "rust/PARITY.md is missing the required phrase {needle:?}"
        );
    }
    eprintln!("PARITY-OK docs: non-testable list and normalization recorded");
}
