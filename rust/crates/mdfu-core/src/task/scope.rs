//! Task scope resolution and loading: [`Scope`], `LoadScope`, and `LoadAll`.
//!
//! Port of `internal/task/scope.go`, the `LoadAll` half of
//! `internal/task/discover.go`, and `resolveScope`/`extractPathFlag` from
//! `internal/task/commands.go`. The scope assembly lives here rather than in
//! [`super::cli`] so the command layer only routes and formats, and rather
//! than in [`super::discover`] because it depends on body parsing and archive
//! exclusion, not just the walker.
//!
//! Core never reads the process environment: `MDTASK_PATH` and
//! `MDTASK_EXCLUDE_DIRS` arrive through the injected `get_env` lookup.

use std::fmt;
use std::fs;
use std::io;

use super::body::{block_range, collect_body, fence_mask};
use super::config::{self, go_clean, join_go, Config, ConfigError, DEFAULT_ARCHIVE_PATH};
use super::discover::{self, DiscoverOptions, WalkError};
use super::grammar::parse_header;
use super::{Task, Unidentified};

/// The resolved execution context shared by every subcommand.
///
/// Mirrors the Go `Scope`: the base directory (or the single `--path` file),
/// the loaded configuration, and the invocation directory used for
/// cwd-relative rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    /// Resolved base directory of the scope (SPEC.md §C precedence).
    pub base: String,
    /// Loaded task configuration; defaults when no rc file exists.
    pub config: Config,
    /// Working directory the scope was resolved from.
    pub cwd: String,
    /// Whether `--path` named a single file, used as the whole scope.
    pub is_file: bool,
    /// The single file named by `--path` when [`Scope::is_file`] is true.
    pub file: String,
}

/// A scope-resolution or scope-loading failure.
#[derive(Debug)]
pub enum ScopeError {
    /// The task configuration is malformed or unreadable (E5/E6).
    Config(ConfigError),
    /// `--path` names a missing path (E4).
    PathNotFound(String),
    /// A scope file could not be read: `cannot read <path>: <err>` (E5).
    Read {
        /// File that could not be read.
        path: String,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A single-file read failure surfaced without the `cannot read` wrapper.
    Io(io::Error),
    /// The base directory could not be walked.
    Walk(WalkError),
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => err.fmt(f),
            Self::PathNotFound(path) => {
                write!(
                    f,
                    "--path '{path}' does not exist or is not a file/directory"
                )
            }
            Self::Read { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Io(err) => err.fmt(f),
            Self::Walk(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ScopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Read { source, .. } => Some(source),
            Self::Io(err) => Some(err),
            Self::Walk(err) => Some(err),
            Self::PathNotFound(_) => None,
        }
    }
}

impl From<ConfigError> for ScopeError {
    fn from(err: ConfigError) -> Self {
        Self::Config(err)
    }
}

/// Extracts the global `--path` flag (both `--path X` and `--path=X`) from
/// `args` and returns its value plus the remaining args in order.
#[must_use]
pub fn extract_path_flag(args: &[String]) -> (String, Vec<String>) {
    const PREFIX: &str = "--path=";
    let mut path_flag = String::new();
    let mut rest = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--path" {
            if index + 1 < args.len() {
                path_flag.clone_from(&args[index + 1]);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix(PREFIX) {
            path_flag = value.to_string();
        } else {
            rest.push(arg.clone());
        }
        index += 1;
    }
    (path_flag, rest)
}

/// Resolves the task scope from `args`: strips the global `--path` flag, loads
/// the task configuration starting at `cwd`, applies the SPEC.md §C base-path
/// precedence, and returns the remaining (non-global) args.
///
/// # Errors
///
/// Returns [`ScopeError::Config`] for a malformed/unreadable rc file and
/// [`ScopeError::PathNotFound`] (E4) when `--path` names a missing path.
#[must_use = "the resolved scope and remaining args must be handled"]
pub fn resolve_scope(
    cwd: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
    args: &[String],
) -> Result<(Scope, Vec<String>), ScopeError> {
    let (path_flag, rest) = extract_path_flag(args);
    let config = config::load(cwd)?;
    let config_path = config::find_config_path(cwd);
    let config_dir = config_path.as_deref().map_or(String::new(), go_dir);
    let env_path = get_env("MDTASK_PATH").unwrap_or_default();
    let mut base = config::resolve_base_path(&path_flag, &env_path, &config.path, &config_dir);
    if config_path.is_none() && base == "." {
        base = config::base_dir(cwd);
    }

    let mut scope = Scope {
        base,
        config,
        cwd: cwd.to_string(),
        is_file: false,
        file: String::new(),
    };
    if !path_flag.is_empty() {
        let Ok(metadata) = fs::metadata(&path_flag) else {
            return Err(ScopeError::PathNotFound(path_flag));
        };
        if !metadata.is_dir() {
            scope.is_file = true;
            scope.file = path_flag;
        }
    }
    Ok((scope, rest))
}

/// Loads every identified task and every ID-less (seed or unidentified) line
/// in scope (Go `LoadScope`).
///
/// A single-file scope is read directly; otherwise [`load_all`] performs the
/// walk.
///
/// # Errors
///
/// Returns [`ScopeError::Read`] when the single file cannot be read and
/// [`ScopeError::Walk`]/[`ScopeError::Read`] from [`load_all`] otherwise.
#[must_use = "the loaded tasks must be handled"]
pub fn load_scope(
    scope: &Scope,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<Task>, Vec<Unidentified>), ScopeError> {
    if !scope.is_file {
        return load_all(&scope.base, &scope.config, &scope.cwd, get_env);
    }
    let data = fs::read(&scope.file).map_err(|source| ScopeError::Read {
        path: scope.file.clone(),
        source,
    })?;
    Ok(parse_lines(&data, &scope.file, &scope.cwd))
}

/// Loads the identified tasks and ID-less lines of a resolved scope for the
/// mutating commands (Go `scopeTaskSet`).
///
/// # Errors
///
/// Returns the read error of a single `--path` file or the walk/read errors of
/// [`load_all`].
#[must_use = "the loaded tasks must be handled"]
pub fn scope_task_set(
    scope: &Scope,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<Task>, Vec<Unidentified>), ScopeError> {
    if scope.is_file {
        let data = fs::read(&scope.file).map_err(ScopeError::Io)?;
        return Ok(parse_lines(&data, &scope.file, &scope.cwd));
    }
    load_all(&scope.base, &scope.config, &scope.cwd, get_env)
}

/// Discovers, filters, and parses every task under `base` (Go `LoadAll`).
///
/// The resolved archive file is excluded from discovery, include globs apply
/// first, then exclude globs, and `File` is the slash-separated path relative
/// to `cwd`.
///
/// # Errors
///
/// Returns [`ScopeError::Walk`] when `base` cannot be walked and
/// [`ScopeError::Read`] when a discovered file cannot be read.
#[must_use = "the discovered tasks must be handled"]
pub fn load_all(
    base: &str,
    cfg: &Config,
    cwd: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<Task>, Vec<Unidentified>), ScopeError> {
    let mut options = DiscoverOptions {
        follow_symlinks: true,
        exclude_dirs: discover::exclude_dirs_from_env(get_env),
        warn: None,
    };
    let paths = discover::walk(base, &mut options).map_err(ScopeError::Walk)?;
    let paths = discover::filter(&paths, base, &cfg.include, &cfg.exclude);
    let archive = resolve_archive(base, &cfg.archive_path);

    let mut tasks = Vec::new();
    let mut unidentified = Vec::new();
    for path in &paths {
        if !archive.is_empty() && same_path(path, &archive) {
            continue;
        }
        let data = fs::read(path).map_err(|source| ScopeError::Read {
            path: path.clone(),
            source,
        })?;
        let (file_tasks, file_unidentified) = parse_lines(&data, path, cwd);
        tasks.extend(file_tasks);
        unidentified.extend(file_unidentified);
    }
    Ok((tasks, unidentified))
}

/// Parses one file's fence-unmasked headers into identified tasks and ID-less
/// lines, filling `File` (relative to `cwd`) and `Line`.
fn parse_lines(data: &[u8], path: &str, cwd: &str) -> (Vec<Task>, Vec<Unidentified>) {
    let text = String::from_utf8_lossy(data);
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let mask = fence_mask(&lines);
    let rel = rel_to_cwd(cwd, path);

    let mut tasks = Vec::new();
    let mut unidentified = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        let Some(mut task) = parse_header(raw) else {
            continue;
        };
        if task.id.is_empty() {
            unidentified.push(Unidentified {
                file: rel.clone(),
                line: index + 1,
                raw: raw.strip_suffix('\r').unwrap_or(raw).to_string(),
            });
            continue;
        }
        if let Some(end) = block_range(&lines, index) {
            let (body, indent) = collect_body(&lines, index, end);
            task.body = body;
            task.body_indent = indent;
        }
        task.file.clone_from(&rel);
        task.line = index + 1;
        tasks.push(task);
    }
    (tasks, unidentified)
}

/// Resolves the archive destination against `base`, defaulting it when empty
/// (Go `resolveArchive`).
fn resolve_archive(base: &str, archive_path: &str) -> String {
    let archive_path = if archive_path.is_empty() {
        DEFAULT_ARCHIVE_PATH
    } else {
        archive_path
    };
    if std::path::Path::new(archive_path).is_absolute() {
        go_clean(archive_path)
    } else {
        go_clean(&join_go(base, archive_path))
    }
}

/// Reports whether two paths name the same location for archive exclusion,
/// comparing cleaned absolute paths (Go `samePath`).
fn same_path(a: &str, b: &str) -> bool {
    match (std::path::absolute(a), std::path::absolute(b)) {
        (Ok(abs_a), Ok(abs_b)) => {
            go_clean(&abs_a.to_string_lossy()) == go_clean(&abs_b.to_string_lossy())
        }
        _ => go_clean(a) == go_clean(b),
    }
}

/// Returns `path` relative to `cwd` with forward slashes, falling back to
/// `path` itself when it cannot be expressed relatively (Go `relToCwd`).
fn rel_to_cwd(cwd: &str, path: &str) -> String {
    if !cwd.is_empty() {
        if let Some(rel) = discover::go_rel(cwd, path) {
            return rel;
        }
    }
    path.to_string()
}

/// Returns the directory component with Go `filepath.Dir` semantics.
pub(crate) fn go_dir(path: &str) -> String {
    let clean = go_clean(path);
    match clean.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => clean[..index].to_string(),
        None => ".".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata/tasks");

    fn no_env(_key: &str) -> Option<String> {
        None
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn write(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, content).expect("write");
    }

    #[test]
    fn extract_path_flag_removes_both_spellings() {
        let args = strings(&["a", "--path", "x", "b", "--path=y", "--path"]);
        let (flag, rest) = extract_path_flag(&args);
        assert_eq!(flag, "y");
        assert_eq!(rest, strings(&["a", "b"]));
    }

    #[test]
    fn resolve_scope_sets_file_scope_and_rest() {
        let dir = TempDir::new().expect("tempdir");
        let cwd = dir.path().to_string_lossy().into_owned();
        let file = dir.path().join("tasks.md");
        fs::write(&file, "- [ ] A-1 T\n").expect("write");
        let file_str = file.to_string_lossy().into_owned();

        let (scope, rest) = resolve_scope(&cwd, &no_env, &strings(&["--path", &file_str, "extra"]))
            .expect("resolve file");
        assert!(scope.is_file);
        assert_eq!(scope.file, file_str);
        assert_eq!(rest, strings(&["extra"]));

        let flag = format!("--path={cwd}");
        let (scope, rest) = resolve_scope(&cwd, &no_env, &strings(&[&flag])).expect("resolve dir");
        assert!(!scope.is_file);
        assert_eq!(scope.base, cwd);
        assert!(rest.is_empty());
    }

    #[test]
    fn resolve_scope_missing_path_is_e4() {
        let dir = TempDir::new().expect("tempdir");
        let cwd = dir.path().to_string_lossy().into_owned();
        let missing = dir.path().join("nope").to_string_lossy().into_owned();

        let err = resolve_scope(&cwd, &no_env, &strings(&["--path", &missing]))
            .expect_err("missing path");
        assert_eq!(
            err.to_string(),
            format!("--path '{missing}' does not exist or is not a file/directory")
        );
    }

    #[test]
    fn resolve_scope_uses_injected_mdtask_path() {
        let dir = TempDir::new().expect("tempdir");
        let env_dir = TempDir::new().expect("tempdir");
        let cwd = dir.path().to_string_lossy().into_owned();
        let env_path = env_dir.path().to_string_lossy().into_owned();
        let get_env = |key: &str| (key == "MDTASK_PATH").then(|| env_path.clone());

        let (scope, rest) = resolve_scope(&cwd, &get_env, &[]).expect("resolve");
        assert_eq!(scope.base, env_path);
        assert!(!scope.is_file);
        assert!(rest.is_empty());
    }

    #[test]
    fn load_scope_single_file_parses_fixture_with_body() {
        let fixture = Path::new(FIXTURE_DIR).join("spec-basic.md");
        let fixture_str = fixture.to_string_lossy().into_owned();
        let scope = Scope {
            base: String::new(),
            config: Config::default(),
            cwd: FIXTURE_DIR.to_string(),
            is_file: true,
            file: fixture_str,
        };

        let (tasks, unidentified) = load_scope(&scope, &no_env).expect("load");
        let ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids, ["BASIC-1", "BASIC-2", "BASIC-3"]);
        assert!(unidentified.is_empty());

        let first = &tasks[0];
        assert_eq!(first.file, "spec-basic.md");
        assert_eq!(first.line, 8);
        assert_eq!(
            first.body,
            "Write the first pass of the announcement, then circulate it to the\n\
             team for review before the freeze."
        );
        assert_eq!(first.body_indent, "  ");
    }

    #[test]
    fn load_all_filters_and_excludes_archive() {
        let dir = TempDir::new().expect("tempdir");
        write(dir.path(), "a.md", "- [ ] A-1 One\n");
        write(dir.path(), "sub/b.md", "- [ ] B-1 Two\n");
        write(dir.path(), "_archive.md", "- [ ] ARCH-1 Old\n");
        write(dir.path(), "skip/c.md", "- [ ] C-1 Skip\n");
        let base = dir.path().to_string_lossy().into_owned();
        let cfg = Config {
            archive_path: DEFAULT_ARCHIVE_PATH.to_string(),
            exclude: strings(&["skip/**"]),
            ..Config::default()
        };

        let (tasks, unidentified) = load_all(&base, &cfg, &base, &no_env).expect("load all");
        let ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids, ["A-1", "B-1"]);
        assert!(unidentified.is_empty());
        assert_eq!(tasks[0].file, "a.md");
        assert_eq!(tasks[1].file, "sub/b.md");
    }
}
