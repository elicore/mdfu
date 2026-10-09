//! Task configuration (`.mdtaskrc` / `.mdfurc`) and scope resolution.
//!
//! Port of the Go `internal/task/config.go`. The task configuration is JSON and
//! is read from `.mdtaskrc` or `.mdfurc`. The `.mdfurc` name is the **task**
//! configuration ONLY: it is entirely distinct from the YAML theme
//! configuration that `mdfu` reads from `$XDG_CONFIG_HOME/mdfu/config.yaml` or
//! receives through the `--config` flag. The two files never merge and share no
//! keys.
//!
//! Resolution walks upward from the start directory. At each directory level
//! `.mdtaskrc` wins over `.mdfurc`; the walk terminates at the filesystem root
//! (SPEC.md §C).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

/// Default task archive file name (SPEC.md §C).
pub const DEFAULT_ARCHIVE_PATH: &str = "_archive.md";

/// Task-scope configuration read from `.mdtaskrc` or `.mdfurc`.
///
/// Mirrors the Go `Config` struct; the zero value has an empty
/// [`Config::archive_path`], while [`load`] always fills the
/// [`DEFAULT_ARCHIVE_PATH`] default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Base directory, resolved relative to the config file's directory.
    pub path: String,
    /// Include globs; empty means every `.md` file.
    pub include: Vec<String>,
    /// Exclude globs, applied after include.
    pub exclude: Vec<String>,
    /// Prefixes whose tasks `validate` must not report.
    pub exclude_prefixes: Vec<String>,
    /// Archive file, relative to the base directory.
    pub archive_path: String,
}

/// Fatal configuration error, mirroring the Go `Load` error strings.
#[derive(Debug)]
pub enum ConfigError {
    /// Malformed JSON: `Invalid JSON in <path>: <message>` (SPEC.md §C, E6).
    InvalidJson {
        /// Config file path.
        path: String,
        /// Decoder message.
        message: String,
    },
    /// The rc path could not be read: `cannot read <path>: <err>`.
    CannotRead {
        /// Config file path.
        path: String,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// The rc path is a directory: `cannot read <path>: is a directory`.
    IsADirectory {
        /// Config file path.
        path: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { path, message } => {
                write!(f, "Invalid JSON in {path}: {message}")
            }
            Self::CannotRead { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::IsADirectory { path } => write!(f, "cannot read {path}: is a directory"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CannotRead { source, .. } => Some(source),
            Self::InvalidJson { .. } | Self::IsADirectory { .. } => None,
        }
    }
}

/// Walks upward from `start_dir` looking for a task configuration file.
///
/// At each directory level `.mdtaskrc` wins over `.mdfurc`; `.mdfurc` is used
/// only when the level has no `.mdtaskrc`. The walk ends at the filesystem
/// root. Returns the first candidate found, or `None`.
#[must_use]
pub fn find_config_path(start_dir: &str) -> Option<String> {
    let start = if start_dir.is_empty() { "." } else { start_dir };
    let mut dir = match std::path::absolute(start) {
        Ok(absolute) => go_clean(&absolute.to_string_lossy()),
        Err(_) => go_clean(start),
    };
    loop {
        let mdtaskrc = join_go(&dir, ".mdtaskrc");
        if file_exists(&mdtaskrc) {
            return Some(mdtaskrc);
        }
        let mdfurc = join_go(&dir, ".mdfurc");
        if file_exists(&mdfurc) {
            return Some(mdfurc);
        }
        let parent = Path::new(&dir).parent()?;
        let mut parent_str = parent.to_string_lossy().into_owned();
        if parent_str.is_empty() {
            parent_str = ".".to_string();
        }
        if parent_str == dir {
            return None;
        }
        dir = parent_str;
    }
}

/// Reads the task configuration discovered from `dir`.
///
/// When no configuration file exists it returns a defaults [`Config`] and no
/// error. Malformed JSON and an unreadable or directory rc path are fatal.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidJson`] for malformed JSON (SPEC.md §C, E6),
/// [`ConfigError::CannotRead`] when the rc path cannot be read, and
/// [`ConfigError::IsADirectory`] when the rc path is a directory.
#[must_use = "returns the loaded task configuration or the fatal read error"]
pub fn load(dir: &str) -> Result<Config, ConfigError> {
    let Some(path) = find_config_path(dir) else {
        return Ok(defaults());
    };

    let metadata = fs::metadata(&path).map_err(|source| ConfigError::CannotRead {
        path: path.clone(),
        source,
    })?;
    if metadata.is_dir() {
        return Err(ConfigError::IsADirectory { path });
    }
    let data = fs::read(&path).map_err(|source| ConfigError::CannotRead {
        path: path.clone(),
        source,
    })?;
    parse_config(&path, &data)
}

/// Applies the base-path precedence: an explicit flag, then the environment,
/// then the config path resolved relative to `cfg_dir`, then `.`.
///
/// The flag and environment spellings are returned untouched; a config path is
/// cleaned like Go `filepath.Clean`/`filepath.Join`.
#[must_use]
pub fn resolve_base_path(flag_path: &str, env_path: &str, cfg_path: &str, cfg_dir: &str) -> String {
    if !flag_path.is_empty() {
        return flag_path.to_string();
    }
    if !env_path.is_empty() {
        return env_path.to_string();
    }
    if !cfg_path.is_empty() {
        if Path::new(cfg_path).is_absolute() {
            return go_clean(cfg_path);
        }
        return join_go(cfg_dir, cfg_path);
    }
    ".".to_string()
}

/// Returns the nearest ancestor of `start_dir` containing a `.git` entry, or
/// `start_dir` when there is none.
#[must_use]
pub fn base_dir(start_dir: &str) -> String {
    let mut dir = start_dir.to_string();
    loop {
        if file_exists(&join_go(&dir, ".git")) {
            return dir;
        }
        let Some(parent) = Path::new(&dir).parent() else {
            break;
        };
        let mut parent_str = parent.to_string_lossy().into_owned();
        if parent_str.is_empty() {
            parent_str = ".".to_string();
        }
        if parent_str == dir {
            break;
        }
        dir = parent_str;
    }
    start_dir.to_string()
}

/// Returns the defaults config: every field empty except the archive path.
fn defaults() -> Config {
    Config {
        archive_path: DEFAULT_ARCHIVE_PATH.to_string(),
        ..Config::default()
    }
}

/// Parses config bytes with the defensive typing rules of SPEC.md §C.
fn parse_config(path: &str, data: &[u8]) -> Result<Config, ConfigError> {
    let value: Value = serde_json::from_slice(data).map_err(|err| ConfigError::InvalidJson {
        path: path.to_string(),
        message: err.to_string(),
    })?;
    // Go unmarshals into a map: a JSON `null` root yields a nil map and no
    // error, while any other non-object root is fatal.
    if value.is_null() {
        return Ok(defaults());
    }
    let Some(root) = value.as_object() else {
        return Err(ConfigError::InvalidJson {
            path: path.to_string(),
            message: format!(
                "cannot unmarshal {} into a task configuration object",
                json_kind(&value)
            ),
        });
    };

    let mut cfg = defaults();
    if let Some(decoded) = root.get("path").and_then(decode_string) {
        cfg.path = decoded;
    }
    if let Some(files) = root.get("files").and_then(Value::as_object) {
        if let Some(value) = files.get("include") {
            cfg.include = decode_string_array(value);
        }
        if let Some(value) = files.get("exclude") {
            cfg.exclude = decode_string_array(value);
        }
    }
    if let Some(value) = root.get("excludePrefixes") {
        cfg.exclude_prefixes = decode_string_array(value);
    }
    if let Some(decoded) = root.get("archivePath").and_then(decode_string) {
        cfg.archive_path = decoded;
    }
    Ok(cfg)
}

/// Decodes a JSON string, reporting `None` for any other type.
///
/// Go's `encoding/json` leaves a string target untouched for JSON `null`
/// without error, so `null` decodes as the empty string.
fn decode_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Null => Some(String::new()),
        Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Decodes a JSON array of strings, dropping wrongly-typed entries. Returns an
/// empty vector when the value is not an array.
fn decode_string_array(value: &Value) -> Vec<String> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(decode_string).collect()
}

/// Names a JSON value kind for the non-object root error message.
fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Reports whether `path` exists, including as a directory (Go `os.Stat`).
fn file_exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// Lexical path cleaning with Go `filepath.Clean` semantics on Unix: multiple
/// separators collapse, `.` elements vanish, and `..` elements pop the
/// preceding element (or survive only on relative paths).
pub(crate) fn go_clean(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let rooted = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for element in path.split('/') {
        match element {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|last| *last != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            _ => out.push(element),
        }
    }
    let mut cleaned = if rooted {
        String::from("/")
    } else {
        String::new()
    };
    cleaned.push_str(&out.join("/"));
    if cleaned.is_empty() {
        ".".to_string()
    } else {
        cleaned
    }
}

/// Joins a directory and a name with Go `filepath.Join` semantics (join then
/// clean).
pub(crate) fn join_go(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        return go_clean(name);
    }
    go_clean(&format!("{dir}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::{
        base_dir, find_config_path, load, resolve_base_path, Config, DEFAULT_ARCHIVE_PATH,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Writes `content` to `dir/name`, returning the written path.
    fn write_task_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).expect("write task file");
        path
    }

    fn joined(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn find_config_path_prefers_mdtaskrc_at_the_same_level() {
        let root = TempDir::new().expect("tempdir");
        let sub = root.path().join("a").join("b");
        fs::create_dir_all(&sub).expect("mkdir");
        write_task_file(root.path(), ".mdtaskrc", "{}");
        write_task_file(root.path(), ".mdfurc", "{}");

        let got = find_config_path(&sub.to_string_lossy());
        assert_eq!(got, Some(joined(root.path(), ".mdtaskrc")));
    }

    #[test]
    fn find_config_path_falls_back_to_mdfurc() {
        let root = TempDir::new().expect("tempdir");
        write_task_file(root.path(), ".mdfurc", "{}");

        let got = find_config_path(&root.path().to_string_lossy());
        assert_eq!(got, Some(joined(root.path(), ".mdfurc")));
    }

    #[test]
    fn find_config_path_nearer_mdfurc_beats_farther_mdtaskrc() {
        let root = TempDir::new().expect("tempdir");
        let sub = root.path().join("sub");
        fs::create_dir_all(&sub).expect("mkdir");
        write_task_file(root.path(), ".mdtaskrc", "{}");
        write_task_file(&sub, ".mdfurc", "{}");

        let got = find_config_path(&sub.to_string_lossy());
        assert_eq!(got, Some(joined(&sub, ".mdfurc")));
    }

    #[test]
    fn find_config_path_nearer_mdtaskrc_wins() {
        let root = TempDir::new().expect("tempdir");
        let sub = root.path().join("sub");
        fs::create_dir_all(&sub).expect("mkdir");
        write_task_file(root.path(), ".mdfurc", "{}");
        write_task_file(&sub, ".mdtaskrc", "{}");

        let got = find_config_path(&sub.to_string_lossy());
        assert_eq!(got, Some(joined(&sub, ".mdtaskrc")));
    }

    #[test]
    fn find_config_path_none_found() {
        let root = TempDir::new().expect("tempdir");
        assert_eq!(find_config_path(&root.path().to_string_lossy()), None);
    }

    #[test]
    fn load_full_schema() {
        let dir = TempDir::new().expect("tempdir");
        write_task_file(
            dir.path(),
            ".mdtaskrc",
            r#"{
  "path": "notes",
  "files": {"include": ["**/*.md"], "exclude": ["archive/**"]},
  "excludePrefixes": ["ARCH"],
  "archivePath": "_arch.md"
}"#,
        );

        let cfg = load(&dir.path().to_string_lossy()).expect("load");
        let want = Config {
            path: "notes".to_string(),
            include: vec!["**/*.md".to_string()],
            exclude: vec!["archive/**".to_string()],
            exclude_prefixes: vec!["ARCH".to_string()],
            archive_path: "_arch.md".to_string(),
        };
        assert_eq!(cfg, want);
    }

    #[test]
    fn load_defaults_when_no_config() {
        let dir = TempDir::new().expect("tempdir");
        let cfg = load(&dir.path().to_string_lossy()).expect("load");
        assert_eq!(cfg.archive_path, DEFAULT_ARCHIVE_PATH);
    }

    #[test]
    fn load_malformed_json_is_fatal_with_e6_prefix() {
        let dir = TempDir::new().expect("tempdir");
        write_task_file(dir.path(), ".mdtaskrc", "{");

        let err = load(&dir.path().to_string_lossy()).expect_err("malformed json");
        let message = err.to_string();
        assert!(
            message.starts_with("Invalid JSON in "),
            "message = {message:?}"
        );
        assert!(message.contains(": "), "message = {message:?}");
    }

    #[test]
    fn load_defensive_typing() {
        let dir = TempDir::new().expect("tempdir");
        write_task_file(
            dir.path(),
            ".mdtaskrc",
            r#"{
  "path": 42,
  "files": "nope",
  "excludePrefixes": ["ARCH", 7, true],
  "archivePath": 9,
  "unknownKey": {"x": 1}
}"#,
        );

        let cfg = load(&dir.path().to_string_lossy()).expect("load");
        assert_eq!(cfg.path, "");
        assert!(cfg.include.is_empty() && cfg.exclude.is_empty());
        assert_eq!(cfg.exclude_prefixes, vec!["ARCH".to_string()]);
        assert_eq!(cfg.archive_path, DEFAULT_ARCHIVE_PATH);
    }

    #[test]
    fn load_mdtaskrc_wins_over_mdfurc() {
        let dir = TempDir::new().expect("tempdir");
        write_task_file(dir.path(), ".mdtaskrc", r#"{"path":"from-task"}"#);
        write_task_file(dir.path(), ".mdfurc", r#"{"path":"from-mdfu"}"#);

        let cfg = load(&dir.path().to_string_lossy()).expect("load");
        assert_eq!(cfg.path, "from-task");
    }

    #[test]
    fn load_mdfurc_used_when_no_mdtaskrc() {
        let dir = TempDir::new().expect("tempdir");
        write_task_file(dir.path(), ".mdfurc", r#"{"path":"from-mdfu"}"#);

        let cfg = load(&dir.path().to_string_lossy()).expect("load");
        assert_eq!(cfg.path, "from-mdfu");
    }

    #[test]
    fn load_directory_rc_path_is_fatal() {
        let dir = TempDir::new().expect("tempdir");
        fs::create_dir(dir.path().join(".mdtaskrc")).expect("mkdir rc");

        let err = load(&dir.path().to_string_lossy()).expect_err("dir rc");
        assert!(
            err.to_string().starts_with("cannot read "),
            "message = {err}"
        );
    }

    #[test]
    fn resolve_base_path_matches_go_table() {
        let cases: &[(&str, &str, &str, &str, &str)] = &[
            ("/flag", "/env", "cfg", "/cfgdir", "/flag"),
            ("", "/env", "cfg", "/cfgdir", "/env"),
            ("", "", "notes", "/cfgdir", "/cfgdir/notes"),
            ("", "", "/abs/notes", "/cfgdir", "/abs/notes"),
            ("", "", "", "/cfgdir", "."),
        ];
        for &(flag, env, cfg, cfg_dir, want) in cases {
            assert_eq!(
                resolve_base_path(flag, env, cfg, cfg_dir),
                want,
                "resolve_base_path({flag:?}, {env:?}, {cfg:?}, {cfg_dir:?})"
            );
        }
    }

    #[test]
    fn base_dir_finds_nearest_git_root() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir(root.path().join(".git")).expect("mkdir .git");
        let deep = root.path().join("a").join("b");
        fs::create_dir_all(&deep).expect("mkdir deep");

        assert_eq!(
            base_dir(&deep.to_string_lossy()),
            root.path().to_string_lossy()
        );
    }

    #[test]
    fn base_dir_without_git_returns_start() {
        let plain = TempDir::new().expect("tempdir");
        assert_eq!(
            base_dir(&plain.path().to_string_lossy()),
            plain.path().to_string_lossy()
        );
    }
}
