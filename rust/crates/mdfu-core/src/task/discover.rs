//! Task-scope discovery: the `Walk` walker and the include/exclude `Filter`.
//!
//! Port of the Go `internal/task/discover.go` discovery portion (`LoadAll`
//! lands with the subcommand layer). The rules deliberately differ from
//! [`crate::scan`]: hidden files and directories are **included**, `.gitignore`
//! is **ignored**, symlinks are **followed** through a realpath visited-set,
//! and `node_modules`, `.git`, and `$MDTASK_EXCLUDE_DIRS` are excluded
//! (SPEC.md §E).

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::config::{go_clean, join_go};
use super::glob;

/// Walk-recursion cap, matching SPEC.md §E (`maxWalkDepth` in Go).
pub const MAX_WALK_DEPTH: usize = 64;

/// Controls [`walk`]. Mirrors the Go `DiscoverOptions`.
///
/// `warn` is the Rust spelling of Go's `Warn io.Writer`: the walker must be
/// able to emit the single-line skip warning without writing to the process
/// stderr, so tests can capture it. `None` discards the warning.
#[derive(Default)]
pub struct DiscoverOptions<'w> {
    /// Additional directory names to exclude, on top of `node_modules`/`.git`.
    pub exclude_dirs: Vec<String>,
    /// Follow symlinked directories and files.
    pub follow_symlinks: bool,
    /// Warning sink for unreadable directories; `None` discards warnings.
    pub warn: Option<&'w mut dyn Write>,
}

/// Error returned by [`walk`] when the base path is unusable.
#[derive(Debug)]
pub enum WalkError {
    /// The base path could not be stat'ed.
    Stat {
        /// Base path as passed to [`walk`].
        path: String,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// The base path exists but is not a directory.
    NotDirectory {
        /// Base path as passed to [`walk`].
        path: String,
    },
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stat { path, source } => write!(f, "cannot stat {path}: {source}"),
            Self::NotDirectory { path } => {
                write!(f, "cannot walk {path}: not a directory")
            }
        }
    }
}

impl std::error::Error for WalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Stat { source, .. } => Some(source),
            Self::NotDirectory { .. } => None,
        }
    }
}

/// Returns the sorted, realpath-deduplicated Markdown files under `base`.
///
/// It includes hidden files and directories, ignores `.gitignore` entirely,
/// always excludes `node_modules`, `.git`, and each directory named in
/// [`DiscoverOptions::exclude_dirs`], and accepts only names ending in `.md`
/// (case-insensitive). When [`DiscoverOptions::follow_symlinks`] is true
/// symlinks are followed through an `fs::canonicalize` visited-set, `ELOOP` is
/// tolerated, and recursion is capped at [`MAX_WALK_DEPTH`]. An unreadable
/// directory is skipped with exactly one line on
/// [`DiscoverOptions::warn`] and does not fail the walk.
///
/// # Errors
///
/// Returns [`WalkError::Stat`] when `base` cannot be stat'ed and
/// [`WalkError::NotDirectory`] when it is not a directory.
#[must_use = "returns the discovered Markdown paths or the base-directory error"]
pub fn walk<'a>(base: &str, opts: &'a mut DiscoverOptions<'a>) -> Result<Vec<String>, WalkError> {
    let metadata = fs::metadata(base).map_err(|source| WalkError::Stat {
        path: base.to_string(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(WalkError::NotDirectory {
            path: base.to_string(),
        });
    }

    let mut exclude: HashSet<String> =
        HashSet::from(["node_modules".to_string(), ".git".to_string()]);
    for dir in &opts.exclude_dirs {
        if !dir.is_empty() {
            exclude.insert(dir.clone());
        }
    }

    let mut walker = Walker {
        follow: opts.follow_symlinks,
        warn: opts.warn.as_deref_mut(),
        exclude,
        seen_dirs: HashSet::new(),
        seen_files: HashSet::new(),
        warned: HashSet::new(),
        out: Vec::new(),
    };
    walker.descend(base, 0);
    walker.out.sort();
    Ok(walker.out)
}

/// Carries the visited-sets and exclusions for one [`walk`] call.
struct Walker<'w> {
    follow: bool,
    warn: Option<&'w mut dyn Write>,
    exclude: HashSet<String>,
    seen_dirs: HashSet<PathBuf>,
    seen_files: HashSet<PathBuf>,
    warned: HashSet<String>,
    out: Vec<String>,
}

impl Walker<'_> {
    /// Descends into `dir`. The resolved directory realpath is marked so a
    /// symlink cycle is traversed at most once.
    fn descend(&mut self, dir: &str, depth: usize) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        let real = match fs::canonicalize(dir) {
            Ok(real) => real,
            Err(err) => {
                self.skip(dir, None, &err);
                return;
            }
        };
        if !self.seen_dirs.insert(real.clone()) {
            return;
        }
        let entries = match read_sorted(Path::new(dir)) {
            Ok(entries) => entries,
            Err(err) => {
                self.skip(dir, Some(&real), &err);
                return;
            }
        };
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = join_go(dir, &name);
            let file_type = entry.file_type().ok();

            if file_type.is_some_and(|kind| kind.is_dir()) {
                if self.exclude.contains(&name) {
                    continue;
                }
                self.descend(&path, depth + 1);
                continue;
            }
            // A symlink has `is_dir() == false` on the entry itself.
            if !self.follow && file_type.is_some_and(|kind| kind.is_symlink()) {
                continue;
            }

            let Ok(info) = fs::metadata(&path) else {
                // Broken symlink or ELOOP: tolerated, not a warning.
                continue;
            };
            if info.is_dir() {
                if self.exclude.contains(&name) {
                    continue;
                }
                self.descend(&path, depth + 1);
                continue;
            }
            if !is_markdown_name(&name) {
                continue;
            }
            let Ok(real_file) = fs::canonicalize(&path) else {
                continue;
            };
            if !self.seen_files.insert(real_file) {
                continue;
            }
            self.out.push(path);
        }
    }

    /// Emits the single-line unreadable-directory warning, at most once per
    /// unreadable directory.
    fn skip(&mut self, dir: &str, real: Option<&Path>, err: &io::Error) {
        let key = real.map_or_else(
            || dir.to_string(),
            |path| path.to_string_lossy().into_owned(),
        );
        if !self.warned.insert(key) {
            return;
        }
        if let Some(warn) = self.warn.as_deref_mut() {
            let _ = writeln!(warn, "mdfu task: skipping {dir}: {err}");
        }
    }
}

/// Reads one directory with the entry order of Go `os.ReadDir` (sorted by file
/// name), so the realpath dedup keeps the same first spelling as the Go walker.
fn read_sorted(dir: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

/// Reports whether `name` ends in `.md`, case-insensitively.
fn is_markdown_name(name: &str) -> bool {
    name.to_lowercase().ends_with(".md")
}

/// Applies include then exclude globs to each path's slash-separated
/// base-relative form and preserves input order. An empty include list means
/// every path (which [`walk`] has already restricted to `.md` files).
#[must_use]
pub fn filter(paths: &[String], base: &str, include: &[String], exclude: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let rel = relative_slash(base, path);
        if !include.is_empty() && !matches_any(include, &rel) {
            continue;
        }
        if !exclude.is_empty() && matches_any(exclude, &rel) {
            continue;
        }
        out.push(path.clone());
    }
    out
}

/// Reports whether `name` matches any of `globs`.
fn matches_any(globs: &[String], name: &str) -> bool {
    globs.iter().any(|pattern| glob::matches(pattern, name))
}

/// Returns `path` relative to `base` with forward slashes, falling back to
/// `path` itself when it cannot be expressed relatively (Go `filepath.Rel`
/// failure).
fn relative_slash(base: &str, path: &str) -> String {
    go_rel(base, path).unwrap_or_else(|| path.to_string())
}

/// Lexical relative path with Go `filepath.Rel` semantics on Unix. Returns
/// `None` when one path is absolute and the other relative, or when the base
/// would have to climb through its own `..` prefix.
pub(crate) fn go_rel(base: &str, target: &str) -> Option<String> {
    let base = go_clean(base);
    let target = go_clean(target);
    if base == target {
        return Some(".".to_string());
    }
    let base_absolute = base.starts_with('/');
    if base_absolute != target.starts_with('/') {
        return None;
    }
    let base_parts: Vec<&str> = base
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let target_parts: Vec<&str> = target
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let common = base_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    // Go errors when the first differing base element is `..`.
    if common == 0 && base_parts.first() == Some(&"..") {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    parts.extend(std::iter::repeat_n("..", base_parts.len() - common));
    parts.extend_from_slice(&target_parts[common..]);
    if parts.is_empty() {
        Some(".".to_string())
    } else {
        Some(parts.join("/"))
    }
}

/// Splits `$MDTASK_EXCLUDE_DIRS` on colons and commas.
///
/// `get_env` is injected so tests and the command layer can supply their own
/// environment lookup; core logic never reads the process environment.
#[must_use]
pub fn exclude_dirs_from_env<F>(get_env: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String>,
{
    let Some(raw) = get_env("MDTASK_EXCLUDE_DIRS") else {
        return Vec::new();
    };
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split([':', ','])
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{exclude_dirs_from_env, filter, walk, DiscoverOptions, WalkError};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::TempDir;

    /// Writes `content` to `dir/name`, creating parent directories.
    fn must_write(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, content).expect("write");
    }

    /// Returns the paths joined under `base` and sorted like [`walk`].
    fn sorted_paths(base: &Path, rels: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = rels
            .iter()
            .map(|rel| {
                base.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        out.sort();
        out
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn includes_hidden_and_excludes_git_and_node_modules() {
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "a.md", "x\n");
        must_write(base.path(), "A.MD", "x\n");
        must_write(base.path(), "README.txt", "x\n");
        must_write(base.path(), ".hidden/dir/h.md", "x\n");
        must_write(base.path(), ".git/g.md", "x\n");
        must_write(base.path(), "node_modules/n.md", "x\n");
        must_write(base.path(), "sub/c.md", "x\n");
        must_write(base.path(), "sub/deep/d.md", "x\n");
        must_write(base.path(), "keep/k.md", "x\n");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        let want = sorted_paths(
            base.path(),
            &[
                ".hidden/dir/h.md",
                "A.MD",
                "a.md",
                "keep/k.md",
                "sub/c.md",
                "sub/deep/d.md",
            ],
        );
        assert_eq!(got, want);
    }

    #[test]
    fn excludes_configured_directories() {
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "a.md", "x\n");
        must_write(base.path(), "skipme/s.md", "x\n");
        must_write(base.path(), "keep/k.md", "x\n");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                exclude_dirs: strings(&["skipme", "keep"]),
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        assert_eq!(got, sorted_paths(base.path(), &["a.md"]));
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks_when_disabled() {
        use std::os::unix::fs::symlink;
        let base = TempDir::new().expect("tempdir");
        let real = base.path().join("real");
        fs::create_dir_all(&real).expect("mkdir");
        must_write(base.path(), "real/r.md", "x\n");
        symlink(&real, base.path().join("link")).expect("symlink");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: false,
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        assert_eq!(got, sorted_paths(base.path(), &["real/r.md"]));
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinks_without_duplicating_realpaths() {
        use std::os::unix::fs::symlink;
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "real/r.md", "x\n");
        symlink(base.path().join("real"), base.path().join("alias")).expect("symlink");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        // The alias spelling is encountered first (ReadDir is lexical), so the
        // duplicate realpath is deduplicated keeping alias/r.md.
        assert_eq!(got, sorted_paths(base.path(), &["alias/r.md"]));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_completes_under_a_five_second_guard() {
        use std::os::unix::fs::symlink;
        let base = TempDir::new().expect("tempdir");
        let cycle = base.path().join("cycle");
        fs::create_dir_all(&cycle).expect("mkdir");
        must_write(base.path(), "cycle/x.md", "x\n");
        symlink(&cycle, cycle.join("link")).expect("symlink");

        let base_string = base.path().to_string_lossy().into_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = walk(
                &base_string,
                &mut DiscoverOptions {
                    follow_symlinks: true,
                    ..DiscoverOptions::default()
                },
            );
            let _ = sender.send(result);
        });
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(got)) => assert_eq!(got, sorted_paths(base.path(), &["cycle/x.md"])),
            Ok(Err(err)) => panic!("walk failed: {err}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("walk did not complete within 5s (symlink cycle?)")
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!("walk thread terminated unexpectedly")
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_warns_once_and_continues() {
        use std::os::unix::fs::PermissionsExt;
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "a.md", "x\n");
        let locked = base.path().join("locked");
        fs::create_dir_all(&locked).expect("mkdir");
        must_write(base.path(), "locked/secret.md", "x\n");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod");

        // Running as root: permission bits are not enforced.
        if fs::read_dir(&locked).is_ok() {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("chmod back");
            return;
        }

        let mut warn: Vec<u8> = Vec::new();
        let result = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                warn: Some(&mut warn),
                ..DiscoverOptions::default()
            },
        );
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("chmod back");

        let got = result.expect("walk");
        assert_eq!(got, sorted_paths(base.path(), &["a.md"]));
        let text = String::from_utf8(warn).expect("utf8 warning");
        let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 1, "warning = {text:?}");
        let prefix = format!("mdfu task: skipping {}: ", locked.display());
        assert!(lines[0].starts_with(&prefix), "warning = {text:?}");
    }

    #[cfg(unix)]
    #[test]
    fn broken_and_looping_symlinks_are_tolerated_without_warnings() {
        use std::os::unix::fs::symlink;
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "a.md", "x\n");
        symlink(base.path().join("missing"), base.path().join("broken.md")).expect("symlink");
        symlink(base.path().join("loop"), base.path().join("loop")).expect("symlink");

        let mut warn: Vec<u8> = Vec::new();
        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                warn: Some(&mut warn),
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        assert_eq!(got, sorted_paths(base.path(), &["a.md"]));
        assert!(
            warn.is_empty(),
            "warning = {:?}",
            String::from_utf8_lossy(&warn)
        );
    }

    #[test]
    fn gitignore_is_ignored_and_hidden_files_are_included() {
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), ".gitignore", "ignored.md\n");
        must_write(base.path(), "ignored.md", "x\n");
        must_write(base.path(), ".secret.md", "x\n");
        must_write(base.path(), "a.md", "x\n");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        assert_eq!(
            got,
            sorted_paths(base.path(), &[".secret.md", "a.md", "ignored.md"])
        );
    }

    #[test]
    fn walk_depth_is_capped_at_64() {
        let base = TempDir::new().expect("tempdir");
        let mut deep = base.path().to_path_buf();
        let mut rel = PathBuf::new();
        for _ in 0..64 {
            deep.push("d");
            rel.push("d");
        }
        fs::create_dir_all(&deep).expect("mkdir deep");
        fs::write(deep.join("deep.md"), "x\n").expect("write deep");
        let too_deep = deep.join("d");
        fs::create_dir_all(&too_deep).expect("mkdir too deep");
        fs::write(too_deep.join("too-deep.md"), "x\n").expect("write too deep");
        rel.push("deep.md");

        let got = walk(
            &base.path().to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        )
        .expect("walk");
        assert_eq!(
            got,
            sorted_paths(base.path(), &[rel.to_str().expect("utf8 rel")])
        );
    }

    #[test]
    fn missing_base_is_an_error() {
        let base = TempDir::new().expect("tempdir");
        let missing = base.path().join("nope");
        let result = walk(
            &missing.to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        );
        assert!(matches!(result, Err(WalkError::Stat { .. })));
    }

    #[test]
    fn file_base_is_not_a_directory() {
        let base = TempDir::new().expect("tempdir");
        must_write(base.path(), "a.md", "x\n");
        let file = base.path().join("a.md");
        let result = walk(
            &file.to_string_lossy(),
            &mut DiscoverOptions {
                follow_symlinks: true,
                ..DiscoverOptions::default()
            },
        );
        assert!(matches!(result, Err(WalkError::NotDirectory { .. })));
    }

    #[test]
    fn filter_matches_go_table() {
        let paths = strings(&["a.md", "x/a.md", "archive/z.md"]);
        let cases: &[(&[&str], &[&str], &[&str])] = &[
            (&[], &[], &["a.md", "x/a.md", "archive/z.md"]),
            (&["**/*.md"], &[], &["a.md", "x/a.md", "archive/z.md"]),
            (&["*.md"], &[], &["a.md"]),
            (&["**/*.md"], &["archive/**"], &["a.md", "x/a.md"]),
            (&["nope/*.md"], &[], &[]),
        ];
        for &(include, exclude, want) in cases {
            let got = filter(&paths, ".", &strings(include), &strings(exclude));
            assert_eq!(
                got,
                strings(want),
                "filter(include={include:?}, exclude={exclude:?})"
            );
        }
    }

    #[test]
    fn exclude_dirs_from_env_splits_and_ignores_empty() {
        let get =
            |key: &str| (key == "MDTASK_EXCLUDE_DIRS").then(|| "skipme:also,third".to_string());
        assert_eq!(
            exclude_dirs_from_env(get),
            strings(&["skipme", "also", "third"])
        );
        assert!(exclude_dirs_from_env(|_| Some(String::new())).is_empty());
        assert!(exclude_dirs_from_env(|_| None).is_empty());
        assert_eq!(
            exclude_dirs_from_env(|_| Some("a::b,,c".to_string())),
            strings(&["a", "b", "c"])
        );
    }
}
