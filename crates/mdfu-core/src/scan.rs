//! Directory scanning: find Markdown files under a root.
//!
//! Port of the Go `internal/scan` package with one deliberate divergence
//! (PLAN T6, todo 38): gitignore-respecting is **effective** here and it is
//! the default. `.gitignore` and `.ignore` files (including those in parent
//! directories) are consulted, so gitignored files are skipped unless
//! [`Options::respect_gitignore`] is `false` (`--no-ignore`). The Go
//! reference accepts `RespectGitignore` and ignores it
//! (`internal/scan/scan.go:31`), so it always returns gitignored files. Every
//! other discovery rule (hidden, `.git`, symlinks, sort, limit) matches Go.

use std::path::{Component, Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

/// Controls [`walk_markdown`] behavior. Mirrors Go `scan.Options`, plus the
/// now-effective `respect_gitignore` (todo 38).
#[derive(Debug, Clone)]
pub struct Options {
    /// Root directory to walk; empty defaults to `.` (Go parity).
    pub root: String,
    /// Include hidden files and directories (names starting with `.`).
    /// `.git` is always skipped regardless of this flag.
    pub include_hidden: bool,
    /// Honor `.gitignore` and `.ignore` files, including parent directories.
    /// Defaults to `true` (the CLI default); `--no-ignore` sets `false` and
    /// restores the Go reference's include-everything behavior.
    pub respect_gitignore: bool,
    /// Maximum number of results after sorting. `0` or negative = unlimited.
    pub limit: i64,
}

impl Default for Options {
    /// The CLI defaults: current directory, visible files only,
    /// gitignore-respecting, unlimited.
    fn default() -> Self {
        Self {
            root: String::new(),
            include_hidden: false,
            respect_gitignore: true,
            limit: 0,
        }
    }
}

/// Walks `opts.root` and returns sorted paths of `*.md` files
/// (case-insensitive extension match).
///
/// Behavior (parity with Go `scan.WalkMarkdown` except for gitignore):
/// - `.git` directories are always skipped.
/// - Hidden files/dirs (names starting with `.`) are skipped unless
///   [`Options::include_hidden`]; a hidden root itself is still walked.
/// - Symlinks (file and directory) are skipped and never followed.
/// - Gitignored files (`.gitignore`/`.ignore`, including parent-directory
///   files) are skipped unless [`Options::respect_gitignore`] is `false`; the
///   Go reference honors no ignore files at all.
/// - Results are sorted byte-wise and truncated to [`Options::limit`].
///
/// # Errors
///
/// Returns the first walk error encountered, mirroring Go returning the
/// `filepath.WalkDir` error (for example, a missing or unreadable root).
#[must_use = "returns the discovered Markdown paths or the first walk error"]
pub fn walk_markdown(opts: &Options) -> Result<Vec<String>, ignore::Error> {
    let root = if opts.root.is_empty() {
        "."
    } else {
        opts.root.as_str()
    };
    let root_path = PathBuf::from(root);

    let filter_root = root_path.clone();
    let include_hidden = opts.include_hidden;
    let filter = move |entry: &DirEntry| -> bool {
        if entry.path_is_symlink() {
            return false;
        }
        let Some(file_type) = entry.file_type() else {
            return false;
        };
        if !file_type.is_dir() {
            return true;
        }
        let name = entry.file_name().to_string_lossy();
        if name == ".git" {
            return false;
        }
        // Do not skip the root itself if the caller passed a hidden dir
        // (e.g. root=".hidden"): only skip subdirs below root.
        if !include_hidden && name.starts_with('.') && entry.path() != filter_root {
            return false;
        }
        true
    };

    let respect = opts.respect_gitignore;
    let walker = WalkBuilder::new(root)
        // Hidden handling is ours so the Go rules (root exemption, `.git`)
        // apply exactly.
        .hidden(false)
        // The one deliberate divergence: ignore files are honored by default;
        // `--no-ignore` turns each source off. `.gitignore` stays effective
        // outside a git work tree (`require_git(false)`), and the global git
        // config / `.git/info/exclude` stay out of scope.
        .git_ignore(respect)
        .git_global(false)
        .git_exclude(false)
        .ignore(respect)
        .parents(respect)
        .require_git(false)
        .follow_links(false)
        .filter_entry(filter)
        .build();

    let mut out = Vec::new();
    for entry in walker {
        let entry = entry?;
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        if !name.to_lowercase().ends_with(".md") {
            continue;
        }
        out.push(go_style_path(&root_path, entry.path()));
    }

    out.sort();
    if opts.limit > 0 {
        let cap = usize::try_from(opts.limit).unwrap_or(usize::MAX);
        out.truncate(cap);
    }
    Ok(out)
}

/// Renders a walker path the way Go's `filepath.WalkDir` does: the root is
/// kept as passed and `./` components are cleaned away (the `ignore` walker
/// yields e.g. `./note.md` for root `.`, Go yields `note.md`).
fn go_style_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    clean_cur_dir(&root.join(rel))
        .to_string_lossy()
        .into_owned()
}

/// Drops [`Component::CurDir`] segments (`./a` -> `a`); other components,
/// including a leading `..`, are preserved like `filepath.Clean`.
fn clean_cur_dir(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{go_style_path, walk_markdown, Options};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn vault(files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, contents) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, contents).expect("write");
        }
        dir
    }

    fn relative(base: &Path, paths: &[String]) -> Vec<String> {
        paths
            .iter()
            .map(|p| {
                Path::new(p)
                    .strip_prefix(base)
                    .expect("path under root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn walk(dir: &TempDir, include_hidden: bool) -> Vec<String> {
        walk_markdown(&Options {
            root: dir.path().to_string_lossy().into_owned(),
            include_hidden,
            ..Options::default()
        })
        .expect("walk")
    }

    #[test]
    fn hidden_excluded_by_default_and_included_with_option() {
        let dir = vault(&[
            ("visible.md", "# V\n"),
            (".secret.md", "# S\n"),
            (".hiddendir/in.md", "# H\n"),
        ]);
        assert_eq!(relative(dir.path(), &walk(&dir, false)), ["visible.md"]);
        assert_eq!(
            relative(dir.path(), &walk(&dir, true)),
            [".hiddendir/in.md", ".secret.md", "visible.md"]
        );
    }

    #[test]
    fn git_directory_always_skipped_even_with_hidden() {
        let dir = vault(&[("note.md", "# N\n"), (".git/evil.md", "# E\n")]);
        for include_hidden in [false, true] {
            let got = walk(&dir, include_hidden);
            assert_eq!(
                relative(dir.path(), &got),
                ["note.md"],
                "include_hidden={include_hidden}"
            );
        }
    }

    #[test]
    fn non_markdown_skipped_and_extension_case_insensitive() {
        let dir = vault(&[
            ("note.md", "# N\n"),
            ("SHOUT.MD", "# S\n"),
            ("other.txt", "x"),
            ("not.markdown", "x"),
            ("noext", "x"),
        ]);
        assert_eq!(
            relative(dir.path(), &walk(&dir, false)),
            ["SHOUT.MD", "note.md"]
        );
    }

    #[test]
    fn results_sorted_and_limit_applied_after_sort() {
        let dir = vault(&[
            ("b.md", ""),
            ("a.md", ""),
            ("c.md", ""),
            ("sub/z.md", ""),
            ("sub/a.md", ""),
        ]);
        let root = dir.path().to_string_lossy().into_owned();
        let all_sorted = ["a.md", "b.md", "c.md", "sub/a.md", "sub/z.md"];

        assert_eq!(relative(dir.path(), &walk(&dir, false)), all_sorted);

        let limited = walk_markdown(&Options {
            root: root.clone(),
            limit: 3,
            ..Options::default()
        })
        .expect("walk");
        assert_eq!(relative(dir.path(), &limited), ["a.md", "b.md", "c.md"]);

        let unlimited = walk_markdown(&Options {
            root,
            limit: -5,
            ..Options::default()
        })
        .expect("walk");
        assert_eq!(relative(dir.path(), &unlimited), all_sorted);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        use std::os::unix::fs::symlink;
        let dir = vault(&[("notes/visible.md", "# V\n"), ("copy-source.md", "# C\n")]);
        symlink(dir.path().join("notes"), dir.path().join("alias")).expect("symlink dir");
        symlink(
            dir.path().join("copy-source.md"),
            dir.path().join("copy.md"),
        )
        .expect("symlink file");
        assert_eq!(
            relative(dir.path(), &walk(&dir, true)),
            ["copy-source.md", "notes/visible.md"]
        );
    }

    #[test]
    fn hidden_root_directory_is_still_walked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join(".vault");
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("inside.md"), "# I\n").expect("write");
        let got = walk_markdown(&Options {
            root: root.to_string_lossy().into_owned(),
            ..Options::default()
        })
        .expect("walk");
        assert_eq!(relative(&root, &got), ["inside.md"]);
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        let result = walk_markdown(&Options {
            root: missing.to_string_lossy().into_owned(),
            ..Options::default()
        });
        assert!(result.is_err());
    }

    #[test]
    fn go_style_path_cleans_curdir_components() {
        assert_eq!(
            go_style_path(Path::new("."), Path::new("./note.md")),
            "note.md"
        );
        assert_eq!(
            go_style_path(Path::new("vault"), Path::new("vault/note.md")),
            "vault/note.md"
        );
    }
}

/// `.gitignore`/`.ignore` behavior (todo 38, PLAN T6): ignored files are
/// skipped by default and returned with `respect_gitignore: false`
/// (`--no-ignore`).
///
/// Module name is significant: `cargo test -p mdfu-core scan::gitignore`
/// selects exactly these cases.
#[cfg(test)]
mod gitignore {
    use super::{walk_markdown, Options};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn vault(files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, contents) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, contents).expect("write");
        }
        dir
    }

    fn relative(base: &Path, paths: &[String]) -> Vec<String> {
        paths
            .iter()
            .map(|p| {
                Path::new(p)
                    .strip_prefix(base)
                    .expect("path under root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn walk(root: &Path, respect_gitignore: bool) -> Vec<String> {
        walk_markdown(&Options {
            root: root.to_string_lossy().into_owned(),
            respect_gitignore,
            ..Options::default()
        })
        .expect("walk")
    }

    fn ignored_vault() -> TempDir {
        vault(&[
            (".gitignore", "ignored.md\ndist/\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("visible.md", "# V\n"),
            ("ignored.md", "# I\n"),
            ("dist/bundle.md", "# B\n"),
        ])
    }

    #[test]
    fn gitignored_files_excluded_by_default() {
        let dir = ignored_vault();
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), true)),
            ["visible.md"]
        );
    }

    #[test]
    fn gitignored_files_included_with_no_ignore() {
        let dir = ignored_vault();
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), false)),
            ["dist/bundle.md", "ignored.md", "visible.md"]
        );
    }

    #[test]
    fn gitignore_applies_outside_a_git_repository() {
        let dir = vault(&[
            (".gitignore", "ignored.md\n"),
            ("ignored.md", "# I\n"),
            ("kept.md", "# K\n"),
        ]);
        assert_eq!(relative(dir.path(), &walk(dir.path(), true)), ["kept.md"]);
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), false)),
            ["ignored.md", "kept.md"]
        );
    }

    #[test]
    fn dot_ignore_files_respected_by_default_and_disabled() {
        let dir = vault(&[
            (".ignore", "via-ignore.md\n"),
            ("via-ignore.md", "# V\n"),
            ("kept.md", "# K\n"),
        ]);
        assert_eq!(relative(dir.path(), &walk(dir.path(), true)), ["kept.md"]);
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), false)),
            ["kept.md", "via-ignore.md"]
        );
    }

    #[test]
    fn nested_gitignore_applies_within_its_directory() {
        let dir = vault(&[
            ("sub/.gitignore", "b.md\n"),
            ("sub/a.md", "# A\n"),
            ("sub/b.md", "# B\n"),
            ("c.md", "# C\n"),
        ]);
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), true)),
            ["c.md", "sub/a.md"]
        );
        assert_eq!(
            relative(dir.path(), &walk(dir.path(), false)),
            ["c.md", "sub/a.md", "sub/b.md"]
        );
    }

    #[test]
    fn parent_directory_gitignore_respected_and_disabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parent = dir.path().join("parent");
        let child = parent.join("child");
        let sub = child.join("sub");
        fs::create_dir_all(&sub).expect("mkdir");
        fs::write(parent.join(".gitignore"), "child/sub/\n").expect("write");
        fs::write(child.join("a.md"), "# A\n").expect("write");
        fs::write(sub.join("b.md"), "# B\n").expect("write");
        assert_eq!(relative(&child, &walk(&child, true)), ["a.md"]);
        assert_eq!(relative(&child, &walk(&child, false)), ["a.md", "sub/b.md"]);
    }
}
