//! Byte-exact, atomic task-file editing.
//!
//! Port of `internal/task/edit.go`, `internal/task/write.go`, and the write
//! half of `internal/task/ids.go` (`writeAssignments`). File contents never
//! round-trip through `String`: the original bytes, the staged lines, and every
//! write stay in `Vec<u8>`, so a CRLF file, a file without a trailing newline,
//! and invalid UTF-8 bytes all survive verbatim. `String` appears only as a
//! read-only lossy view for grammar checks — Go's regexp engine decodes an
//! invalid byte to U+FFFD, which `from_utf8_lossy` reproduces — and in error
//! messages.
//!
//! The `bstr` crate is deliberately unnecessary here: the only separators are
//! the fixed byte strings `\n` and `\r\n`, so two tiny helpers suffice.
//!
//! Every write is atomic and all-or-nothing: the staged bytes go to a temporary
//! file in the target directory and are renamed over the original, preserving
//! the original file mode. A stale source is detected by re-reading the file and
//! comparing the recorded line byte-for-byte before anything is written
//! (SPEC.md §J, E17).

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::body::{block_range, block_range_any};
use super::config::{go_clean, join_go, Config, DEFAULT_ARCHIVE_PATH};
use super::grammar::parse_header;
use super::id::PlannedWrite;
use super::Task;

/// The LF line ending, as bytes.
const LF: &[u8] = b"\n";
/// The CRLF line ending, as bytes.
const CRLF: &[u8] = b"\r\n";
/// Mode used for a file that does not exist yet (`0644`).
const DEFAULT_MODE: u32 = 0o644;

/// Reports that a task did not sit at its recorded line when the file was
/// re-read.
///
/// `Display` is exactly the E17 string from SPEC.md §J.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleError {
    /// The task file that changed.
    pub file: String,
    /// The task ID (or the raw header for a broadened checkbox item).
    pub id: String,
}

impl fmt::Display for StaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "file changed, task '{}' not at expected line", self.id)
    }
}

impl std::error::Error for StaleError {}

/// A failure from a byte-exact file edit.
#[derive(Debug)]
pub enum EditError {
    /// Underlying I/O failure (read, write, or rename).
    Io(io::Error),
    /// E17: the task no longer sits at its recorded line (SPEC.md §J).
    Stale(StaleError),
    /// `'<path>' is a directory` (SPEC.md §F.5/§F.7).
    Directory(String),
    /// `cannot write to '<path>': permission denied` (SPEC.md §F.7).
    PermissionDenied(String),
    /// `cannot write to '<path>': <err>` for any other write failure.
    WriteFailed {
        /// The path that could not be written.
        path: String,
        /// The underlying failure.
        source: io::Error,
    },
    /// `line range [<start>,<end>) out of bounds for <len> lines`.
    LineRangeOutOfBounds {
        /// Inclusive staged-line start.
        start: usize,
        /// Exclusive staged-line end.
        end: usize,
        /// The current staged line count.
        len: usize,
    },
    /// `line <line> out of bounds for <len> lines`.
    LineOutOfBounds {
        /// The requested 1-based line.
        line: usize,
        /// The current staged line count.
        len: usize,
    },
    /// `line <line> is not a task header`.
    NotATaskHeader {
        /// The requested 1-based line.
        line: usize,
    },
    /// E19: `archive path '<path>' is outside the base directory`.
    ArchiveOutsideBase(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => err.fmt(f),
            Self::Stale(err) => err.fmt(f),
            Self::Directory(path) => write!(f, "'{path}' is a directory"),
            Self::PermissionDenied(path) => {
                write!(f, "cannot write to '{path}': permission denied")
            }
            Self::WriteFailed { path, source } => write!(f, "cannot write to '{path}': {source}"),
            Self::LineRangeOutOfBounds { start, end, len } => {
                write!(
                    f,
                    "line range [{start},{end}) out of bounds for {len} lines"
                )
            }
            Self::LineOutOfBounds { line, len } => {
                write!(f, "line {line} out of bounds for {len} lines")
            }
            Self::NotATaskHeader { line } => write!(f, "line {line} is not a task header"),
            Self::ArchiveOutsideBase(path) => {
                write!(f, "archive path '{path}' is outside the base directory")
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::WriteFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<io::Error> for EditError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// A byte-preserving, line-anchored view of one task file.
///
/// The original bytes, the detected line ending, whether the file ended with a
/// newline, and the staged line index are kept together; replacements are
/// staged in memory and only hit disk on [`FileEdit::commit`].
pub struct FileEdit {
    path: String,
    original: Vec<u8>,
    orig_lines: Vec<Vec<u8>>,
    lines: Vec<Vec<u8>>,
    eol: &'static [u8],
    trailing_newline: bool,
    dirty: bool,
}

/// Reads `path`, detects its line ending, and splits it into lines while
/// remembering the presence or absence of a trailing newline.
///
/// # Errors
///
/// Returns the underlying I/O error when `path` cannot be read.
#[must_use = "the opened edit must be used or dropped deliberately"]
pub fn open(path: &str) -> io::Result<FileEdit> {
    let data = fs::read(path)?;
    Ok(FileEdit::from_bytes(path, data))
}

impl FileEdit {
    /// Builds the in-memory state from raw bytes.
    fn from_bytes(path: &str, data: Vec<u8>) -> Self {
        let eol = detect_eol(&data);
        let (lines, trailing_newline) = split_lines(&data, eol);
        Self {
            path: path.to_string(),
            original: data,
            orig_lines: lines.clone(),
            lines,
            eol,
            trailing_newline,
            dirty: false,
        }
    }

    /// Resets the in-memory state from raw bytes after a successful write.
    fn reload(&mut self, data: Vec<u8>) {
        let path = self.path.clone();
        *self = Self::from_bytes(&path, data);
    }

    /// Returns the staged content, byte-exact to the original when nothing was
    /// staged.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        if !self.dirty {
            return self.original.clone();
        }
        let mut content = join_bytes(&self.lines, self.eol);
        if self.trailing_newline {
            content.extend_from_slice(self.eol);
        }
        content
    }

    /// Stages a replacement of the half-open line range `[start, end)` without
    /// writing. `start == end` inserts. Bounds are validated against the
    /// current staged lines.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::LineRangeOutOfBounds`] when the range is not
    /// contained in the staged lines.
    #[must_use = "the staging result must be handled before committing"]
    pub fn replace_lines(
        &mut self,
        start: usize,
        end: usize,
        lines: &[Vec<u8>],
    ) -> Result<(), EditError> {
        if end < start || end > self.lines.len() {
            return Err(EditError::LineRangeOutOfBounds {
                start,
                end,
                len: self.lines.len(),
            });
        }
        let mut staged = Vec::with_capacity(self.lines.len() - (end - start) + lines.len());
        staged.extend_from_slice(&self.lines[..start]);
        staged.extend_from_slice(lines);
        staged.extend_from_slice(&self.lines[end..]);
        self.lines = staged;
        self.dirty = self.lines != self.orig_lines;
        Ok(())
    }

    /// Writes the staged content to a temp file in the same directory and
    /// renames it over the path, preserving the original file mode. It is a
    /// no-op when nothing changed.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error from the temp file, the mode change,
    /// or the rename. The temp file is removed on failure.
    #[must_use = "the write result must be handled; a failed commit must not be ignored"]
    pub fn commit(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let data = self.bytes();
        let mode = fs::metadata(&self.path).map_or(DEFAULT_MODE, |metadata| file_mode(&metadata));
        atomic_write(&self.path, &data, mode)?;
        self.reload(data);
        Ok(())
    }

    /// Re-reads the file from disk and confirms the task is still at the
    /// recorded 1-based line with an unchanged header.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Stale`] (E17) when the line moved, no longer
    /// parses to `task_id`, or its bytes changed; I/O read failures surface as
    /// [`EditError::Io`].
    #[must_use = "the verification result must be handled"]
    pub fn verify(&self, task_id: &str, line: usize) -> Result<(), EditError> {
        let data = fs::read(&self.path)?;
        let (lines, _) = split_lines(&data, detect_eol(&data));
        let found = line.checked_sub(1).and_then(|index| lines.get(index));
        let parsed = found.and_then(|raw| parse_header(&String::from_utf8_lossy(raw)));
        if parsed.is_none_or(|task| task.id != task_id) {
            return Err(self.stale(task_id));
        }
        if let Some(index) = line.checked_sub(1) {
            if index < self.orig_lines.len() && lines.get(index) != self.orig_lines.get(index) {
                return Err(self.stale(task_id));
            }
        }
        Ok(())
    }

    /// Re-reads the file from disk and confirms `line` still holds exactly
    /// `raw`. Broadened checkbox items carry no ID, so they are matched by
    /// their raw header instead.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Stale`] (E17) when the line moved or changed; I/O
    /// read failures surface as [`EditError::Io`].
    #[must_use = "the verification result must be handled"]
    pub fn verify_line(&self, line: usize, raw: &[u8]) -> Result<(), EditError> {
        let data = fs::read(&self.path)?;
        let (lines, _) = split_lines(&data, detect_eol(&data));
        let expected = trim_cr(raw);
        let matches = line
            .checked_sub(1)
            .and_then(|index| lines.get(index))
            .is_some_and(|found| found.as_slice() == expected);
        if matches {
            Ok(())
        } else {
            Err(EditError::Stale(StaleError {
                file: self.path.clone(),
                id: String::from_utf8_lossy(raw).into_owned(),
            }))
        }
    }

    /// Confirms `task` still sits at its recorded line, picking the check that
    /// matches how it was parsed: exact raw header for broadened items,
    /// ID-based for strict tasks.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Stale`] (E17) when the task moved or changed; I/O
    /// read failures surface as [`EditError::Io`].
    #[must_use = "the verification result must be handled"]
    pub fn verify_task(&self, task: &Task) -> Result<(), EditError> {
        if task.broad {
            self.verify_line(task.line, task.header_raw.as_bytes())
        } else {
            self.verify(&task.id, task.line)
        }
    }

    /// Stages a replacement of the 1-based header line.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::LineOutOfBounds`] for line 0 and
    /// [`EditError::LineRangeOutOfBounds`] when the line is past the end.
    #[must_use = "the staging result must be handled before committing"]
    pub fn replace_header_line(&mut self, line: usize, header: &[u8]) -> Result<(), EditError> {
        let start = line.checked_sub(1).ok_or(EditError::LineOutOfBounds {
            line,
            len: self.lines.len(),
        })?;
        self.replace_lines(start, line, &[header.to_vec()])
    }

    /// Stages a replacement of the half-open line range `[start, end)` with
    /// `body_lines`.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::LineRangeOutOfBounds`] when the range is not
    /// contained in the staged lines.
    #[must_use = "the staging result must be handled before committing"]
    pub fn replace_body_range(
        &mut self,
        start: usize,
        end: usize,
        body_lines: &[Vec<u8>],
    ) -> Result<(), EditError> {
        self.replace_lines(start, end, body_lines)
    }

    /// Stages the checkbox of the 1-based header line flipped between
    /// unchecked and checked, leaving the rest of the line byte-identical. It
    /// accepts both strict headers and broadened checkbox items.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::LineOutOfBounds`] when the line is not staged and
    /// [`EditError::NotATaskHeader`] when the line is not a checkbox item.
    #[must_use = "the staging result must be handled before committing"]
    pub fn flip_checkbox(&mut self, line: usize) -> Result<(), EditError> {
        let index = line.checked_sub(1).ok_or(EditError::LineOutOfBounds {
            line,
            len: self.lines.len(),
        })?;
        let current = self.lines.get(index).ok_or(EditError::LineOutOfBounds {
            line,
            len: self.lines.len(),
        })?;
        let flipped = flip_checkbox_bytes(current).ok_or(EditError::NotATaskHeader { line })?;
        self.replace_lines(index, line, &[flipped])
    }

    /// Builds the E17 error for this file.
    fn stale(&self, id: &str) -> EditError {
        EditError::Stale(StaleError {
            file: self.path.clone(),
            id: id.to_string(),
        })
    }
}

/// Appends `block` to the end of `path`, creating parent directories as needed,
/// and preserves the existing line ending.
///
/// When the file already has content and lacks a trailing newline, one is
/// inserted before the block; the result always ends with a newline. A new
/// file uses `\n` (there is no existing ending to detect), exactly like the Go
/// original.
///
/// # Errors
///
/// Returns the underlying I/O error when the existing file cannot be read or
/// the result cannot be written atomically.
#[must_use = "the append result must be handled"]
pub fn append_block(path: &str, block: &[u8]) -> io::Result<()> {
    let block = trim_end_crlf(block);

    let mut mode = DEFAULT_MODE;
    if let Ok(metadata) = fs::metadata(path) {
        mode = file_mode(&metadata);
    }
    let existing = match fs::read(path) {
        Ok(data) => data,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err),
    };

    let eol: &[u8] = if existing.is_empty() {
        LF
    } else {
        detect_eol(&existing)
    };

    let mut out = Vec::with_capacity(existing.len() + block.len() + 2);
    if !existing.is_empty() {
        out.extend_from_slice(&existing);
        if !existing.ends_with(eol) {
            out.extend_from_slice(eol);
        }
    }
    if !block.is_empty() {
        let normalized = replace_crlf(block);
        for (index, line) in split_bytes(&normalized, LF).iter().enumerate() {
            if index > 0 {
                out.extend_from_slice(eol);
            }
            out.extend_from_slice(line);
        }
        out.extend_from_slice(eol);
    }
    atomic_write(path, &out, mode)
}

/// Applies planned header rewrites, one [`FileEdit`] per file, committing each
/// file in first-seen order.
///
/// This is the write half of `ids`: the caller verifies every assignment
/// against the source first, so this function trusts the planned line numbers.
///
/// # Errors
///
/// Returns the first staging or I/O failure. Files committed before the
/// failure keep their writes, replicating the Go validate-all-then-write
/// behavior.
#[must_use = "the write result must be handled"]
pub fn apply(writes: &[PlannedWrite]) -> Result<(), EditError> {
    let mut by_file: HashMap<&str, Vec<&PlannedWrite>> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for write in writes {
        if !by_file.contains_key(write.file.as_str()) {
            order.push(write.file.as_str());
        }
        by_file.entry(write.file.as_str()).or_default().push(write);
    }

    for file in order {
        let mut edit = open(file)?;
        if let Some(file_writes) = by_file.get(file) {
            for write in file_writes {
                edit.replace_header_line(write.line, write.header.as_bytes())?;
            }
        }
        edit.commit()?;
    }
    Ok(())
}

/// Relocates the whole block of `task` to the end of `target`.
///
/// Validate the target is not a directory, treat a move onto the same realpath
/// as a silent no-op, validate writability of both files, verify the source is
/// not stale, write the target first, then rewrite the source with the block
/// removed (SPEC.md §F.5).
///
/// # Errors
///
/// Returns [`EditError::Directory`] for a directory target,
/// [`EditError::PermissionDenied`] when either file lacks a write bit,
/// [`EditError::Stale`] (E17) when the source moved, and [`EditError::Io`] for
/// any read/write failure. The target is never written before the source is
/// verified.
#[must_use = "the move result must be handled"]
pub fn move_task(task: &Task, target: &str) -> Result<(), EditError> {
    let source = task.file.as_str();

    if let Ok(metadata) = fs::metadata(target) {
        if metadata.is_dir() {
            return Err(EditError::Directory(target.to_string()));
        }
    }
    if same_file_real(source, target) {
        return Ok(());
    }
    assert_writable(source)?;
    assert_writable(target)?;

    let mut edit = open(source)?;
    let start = task
        .line
        .checked_sub(1)
        .ok_or_else(|| edit.stale(&task.id))?;
    let header_matches = edit
        .lines
        .get(start)
        .is_some_and(|line| String::from_utf8_lossy(trim_cr(line)) == task.header_raw);
    if !header_matches {
        return Err(edit.stale(&task.id));
    }
    edit.verify_task(task)?;

    let end = task_block_end(task, &edit, start).ok_or_else(|| edit.stale(&task.id))?;
    let block = join_bytes(&edit.lines[start..end], LF);

    append_block(target, &block)?;
    edit.replace_lines(start, end, &[])?;
    edit.commit()?;
    Ok(())
}

/// Appends the blocks named by `targets` to `base_dir`'s resolved archive file
/// and removes them from their sources (SPEC.md §F.7).
///
/// Every source is verified before a single byte is written, the archive is
/// written before any source is rewritten, and per-file removals run in
/// descending line order. An empty target list is a no-op.
///
/// # Errors
///
/// Returns [`EditError::ArchiveOutsideBase`] (E19) for an escaping destination,
/// [`EditError::Directory`] for a directory destination,
/// [`EditError::Stale`] (E17) for any stale source, and
/// [`EditError::WriteFailed`]/[`EditError::PermissionDenied`] for a failed
/// write. No archive is created and no source is touched when a precondition
/// fails.
#[must_use = "the archive result must be handled"]
pub fn archive_tasks(targets: &[Task], base_dir: &str, cfg: &Config) -> Result<(), EditError> {
    if targets.is_empty() {
        return Ok(());
    }

    let archive_path = resolve_archive(base_dir, &cfg.archive_path);
    preflight_archive(base_dir, &archive_path)?;

    let (mut edits, order) = open_and_verify(targets)?;
    let block = collect_archive_blocks(targets, &edits);
    append_block(&archive_path, &block).map_err(|err| write_error(&archive_path, err))?;
    remove_archived(targets, &order, &mut edits)?;
    Ok(())
}

/// Opens one [`FileEdit`] per source file and verifies every target against
/// the fresh disk state before anything is written.
fn open_and_verify(
    targets: &[Task],
) -> Result<(HashMap<String, FileEdit>, Vec<String>), EditError> {
    let mut edits: HashMap<String, FileEdit> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for task in targets {
        if let Some(edit) = edits.get(&task.file) {
            edit.verify_task(task)?;
        } else {
            let edit = open(&task.file)?;
            edit.verify_task(task)?;
            order.push(task.file.clone());
            edits.insert(task.file.clone(), edit);
        }
    }
    Ok((edits, order))
}

/// Collects the raw block bytes of every target, joined with blank lines.
fn collect_archive_blocks(targets: &[Task], edits: &HashMap<String, FileEdit>) -> Vec<u8> {
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(targets.len());
    for task in targets {
        if let Some(edit) = edits.get(&task.file) {
            let (start, end) = archive_block_range(task, edit);
            blocks.push(join_bytes(&edit.lines[start..end], LF));
        }
    }
    let mut out = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.extend_from_slice(b"\n\n");
        }
        out.extend_from_slice(block);
    }
    out
}

/// Removes every archived block per file, in descending line order, and commits
/// each source. Earlier files stay committed when a later one fails.
fn remove_archived(
    targets: &[Task],
    order: &[String],
    edits: &mut HashMap<String, FileEdit>,
) -> Result<(), EditError> {
    let mut by_file: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in targets {
        by_file.entry(task.file.as_str()).or_default().push(task);
    }
    for file in order {
        let Some(mut file_tasks) = by_file.remove(file.as_str()) else {
            continue;
        };
        file_tasks.sort_by_key(|task| std::cmp::Reverse(task.line));
        let Some(edit) = edits.get_mut(file) else {
            continue;
        };
        for task in file_tasks {
            let (start, end) = archive_block_range(task, edit);
            edit.replace_lines(start, end, &[])?;
        }
        edit.commit().map_err(|err| write_error(file, err))?;
    }
    Ok(())
}

/// Computes the block end for the Go archive path: `!ok` falls back to a
/// single-line removal, and the result is clamped to the staged lines so a
/// defensive caller can never slice out of bounds.
fn archive_block_range(task: &Task, edit: &FileEdit) -> (usize, usize) {
    let start = task.line.saturating_sub(1).min(edit.lines.len());
    let end = match task_block_end(task, edit, start) {
        Some(end) => end,
        None => start + 1,
    };
    (start, end.min(edit.lines.len()))
}

/// Returns the end of `task`'s block at `start`, or `None` when the Go range
/// function would report `!ok` (out of range, fenced, not a header, or empty).
fn task_block_end(task: &Task, edit: &FileEdit, start: usize) -> Option<usize> {
    let view: Vec<String> = edit
        .lines
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect();
    let end = if task.broad {
        block_range_any(&view, start)
    } else {
        block_range(&view, start)
    }?;
    (end > start).then_some(end)
}

/// Resolves `archive_path` against `base`, defaulting it when empty
/// (Go `resolveArchive`).
fn resolve_archive(base: &str, archive_path: &str) -> String {
    let path = if archive_path.is_empty() {
        DEFAULT_ARCHIVE_PATH
    } else {
        archive_path
    };
    if Path::new(path).is_absolute() {
        go_clean(path)
    } else {
        join_go(base, path)
    }
}

/// Validates the archive destination before any write: it must stay inside
/// `base_dir` (E19), must not be a directory, and an existing file must open
/// for writing.
fn preflight_archive(base_dir: &str, archive_path: &str) -> Result<(), EditError> {
    if outside_base(base_dir, archive_path) {
        return Err(EditError::ArchiveOutsideBase(archive_path.to_string()));
    }

    let Ok(metadata) = fs::metadata(archive_path) else {
        return Ok(());
    };
    if metadata.is_dir() {
        return Err(EditError::Directory(archive_path.to_string()));
    }
    OpenOptions::new()
        .write(true)
        .open(archive_path)
        .map(|_| ())
        .map_err(|err| write_error(archive_path, err))
}

/// Reports whether `target` escapes `base` under Go `filepath.Rel` semantics:
/// mismatched absolute/relative forms or a target that does not sit under the
/// base's lexical prefix both yield a `..`-prefixed relative path.
fn outside_base(base: &str, target: &str) -> bool {
    let base = go_clean(base);
    let target = go_clean(target);
    if base.starts_with('/') != target.starts_with('/') {
        return true;
    }
    let base_parts = lexical_parts(&base);
    let target_parts = lexical_parts(&target);
    let common = base_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    common != base_parts.len()
}

/// Splits a cleaned path into its non-empty, non-`.` components.
fn lexical_parts(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// Recognizes a broadened checkbox and returns the line with its status byte
/// flipped, preserving every other byte (including invalid UTF-8).
fn flip_checkbox_bytes(line: &[u8]) -> Option<Vec<u8>> {
    let line = trim_cr(line);
    let mut index = 0;
    while index < line.len() && matches!(line[index], b' ' | b'\t') {
        index += 1;
    }
    if !matches!(line.get(index), Some(b'-' | b'*' | b'+')) {
        return None;
    }
    index += 1;
    if line.get(index) != Some(&b' ') || line.get(index + 1) != Some(&b'[') {
        return None;
    }
    let status_index = index + 2;
    let status = *line.get(status_index)?;
    if !matches!(status, b' ' | b'x' | b'X') {
        return None;
    }
    if line.get(status_index + 1) != Some(&b']') || line.get(status_index + 2) != Some(&b' ') {
        return None;
    }
    let mut flipped = line.to_vec();
    flipped[status_index] = if matches!(status, b'x' | b'X') {
        b' '
    } else {
        b'x'
    };
    Some(flipped)
}

/// Reports whether two paths name the same file, comparing realpaths when both
/// exist and cleaned absolute paths otherwise (Go `sameFileReal`).
fn same_file_real(left: &str, right: &str) -> bool {
    if let (Ok(left), Ok(right)) = (fs::canonicalize(left), fs::canonicalize(right)) {
        return go_clean(&left.to_string_lossy()) == go_clean(&right.to_string_lossy());
    }
    match (absolute_go(left), absolute_go(right)) {
        (Some(left), Some(right)) => left == right,
        _ => go_clean(left) == go_clean(right),
    }
}

/// Returns the cleaned absolute form of `path` (Go `filepath.Abs`), or `None`
/// when the working directory is unavailable.
fn absolute_go(path: &str) -> Option<String> {
    if Path::new(path).is_absolute() {
        return Some(go_clean(path));
    }
    std::env::current_dir()
        .ok()
        .map(|cwd| join_go(&cwd.to_string_lossy(), path))
}

/// Reports a permission-denied error when `path`, or the nearest existing
/// ancestor of a not-yet-created path, carries no write bit
/// (Go `assertWritablePath`).
fn assert_writable(path: &str) -> Result<(), EditError> {
    match fs::metadata(path) {
        Ok(metadata) => return check_writable(path, file_mode(&metadata)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(EditError::Io(err)),
    }

    let mut dir = dir_go(path);
    loop {
        if let Ok(metadata) = fs::metadata(&dir) {
            return check_writable(path, file_mode(&metadata));
        }
        let parent = dir_go(&dir);
        if parent == dir {
            return Ok(());
        }
        dir = parent;
    }
}

/// Returns the permission-denied error when `mode` has no write bit.
fn check_writable(path: &str, mode: u32) -> Result<(), EditError> {
    if mode & 0o222 == 0 {
        return Err(EditError::PermissionDenied(path.to_string()));
    }
    Ok(())
}

/// Returns the directory component with Go `filepath.Dir` semantics.
fn dir_go(path: &str) -> String {
    let clean = go_clean(path);
    match clean.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => clean[..index].to_string(),
        None => ".".to_string(),
    }
}

/// Maps a filesystem failure to the SPEC §F.7 archive message, using the exact
/// permission-denied spelling.
fn write_error(path: &str, err: io::Error) -> EditError {
    if err.kind() == io::ErrorKind::PermissionDenied {
        EditError::PermissionDenied(path.to_string())
    } else {
        EditError::WriteFailed {
            path: path.to_string(),
            source: err,
        }
    }
}

/// Writes `data` to a temp file in `path`'s directory and renames it over
/// `path`, preserving `mode`. The temp file is removed on any failure.
fn atomic_write(path: &str, data: &[u8], mode: u32) -> io::Result<()> {
    let parent = Path::new(path)
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let (mut temp, temp_path) = create_temp(parent)?;
    let result = write_temp(&mut temp, &temp_path, data, mode);
    drop(temp);
    if let Err(err) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    Ok(())
}

/// Writes the staged bytes and applies the target mode.
fn write_temp(temp: &mut File, temp_path: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    temp.write_all(data)?;
    set_file_mode(temp_path, mode)?;
    temp.flush()
}

/// Creates a uniquely named temp file in `dir`, retrying on name collisions.
fn create_temp(dir: &Path) -> io::Result<(File, PathBuf)> {
    for _ in 0..16 {
        let stamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => elapsed.as_nanos(),
            Err(_) => 0,
        };
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".mdfu-task-{:x}-{stamp:x}-{sequence:x}", std::process::id());
        let candidate = dir.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, candidate)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create a unique temporary file",
    ))
}

/// Sequence appended to temp file names to make them unique within a process.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Returns the permission bits of `metadata`.
#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o777
}

/// Non-Unix platforms have no Unix permission bits; report the default mode.
#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    DEFAULT_MODE
}

/// Applies Unix permission bits to `path`.
#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Non-Unix platforms have no Unix permission bits to apply.
#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Reports the line ending used by `data`, preferring CRLF when present.
fn detect_eol(data: &[u8]) -> &'static [u8] {
    if data.windows(2).any(|window| window == b"\r\n") {
        CRLF
    } else {
        LF
    }
}

/// Splits `data` on `eol` without retaining the endings and reports whether
/// `data` ended with `eol`.
fn split_lines(data: &[u8], eol: &[u8]) -> (Vec<Vec<u8>>, bool) {
    let (body, trailing) = match data.strip_suffix(eol) {
        Some(rest) => (rest, true),
        None => (data, false),
    };
    if body.is_empty() {
        return (Vec::new(), trailing);
    }
    (
        split_bytes(body, eol)
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect(),
        trailing,
    )
}

/// Splits `data` on `separator`, always returning at least one element.
fn split_bytes<'a>(data: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    if separator.is_empty() {
        return vec![data];
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index + separator.len() <= data.len() {
        if &data[index..index + separator.len()] == separator {
            out.push(&data[start..index]);
            index += separator.len();
            start = index;
        } else {
            index += 1;
        }
    }
    out.push(&data[start..]);
    out
}

/// Joins `lines` with `separator` (Go `strings.Join`).
fn join_bytes(lines: &[Vec<u8>], separator: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.extend_from_slice(separator);
        }
        out.extend_from_slice(line);
    }
    out
}

/// Strips a single trailing carriage return (Go `strings.TrimSuffix(s, "\r")`).
fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Strips every trailing `\r` and `\n` byte (Go `strings.TrimRight(s, "\r\n")`).
fn trim_end_crlf(mut data: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = data.split_last() {
        if matches!(last, b'\r' | b'\n') {
            data = rest;
        } else {
            break;
        }
    }
    data
}

/// Replaces every CRLF pair with a single LF (Go
/// `strings.ReplaceAll(s, "\r\n", "\n")`).
fn replace_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        if index + 1 < data.len() && data[index] == b'\r' && data[index + 1] == b'\n' {
            out.push(b'\n');
            index += 2;
        } else {
            out.push(data[index]);
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    /// Writes raw bytes to `path`.
    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("write file");
    }

    /// Reads raw bytes from `path`.
    fn read(path: &Path) -> Vec<u8> {
        fs::read(path).expect("read file")
    }

    /// Reads one binary fixture by file name.
    fn fixture(name: &str) -> Vec<u8> {
        read(Path::new(&format!("{FIXTURE_DIR}/{name}")))
    }

    /// Builds a task record pointing at `file`.
    fn task(file: &str, line: usize, header: &str, id: &str) -> Task {
        Task {
            file: file.to_string(),
            line,
            header_raw: header.to_string(),
            id: id.to_string(),
            ..Task::default()
        }
    }

    /// The archive configuration used across the tests.
    fn archive_config() -> Config {
        Config {
            archive_path: "_archive.md".to_string(),
            ..Config::default()
        }
    }

    /// Applies Unix permission bits to a test file.
    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
    }

    /// Reads the Unix permission bits of a test file.
    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        fs::metadata(path).expect("stat").permissions().mode() & 0o777
    }

    /// Returns `true` when permission bits are ignored (running privileged).
    #[cfg(unix)]
    fn permission_bits_ignored(dir: &Path) -> bool {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("probe"))
            .is_ok()
    }

    #[test]
    fn file_edit_round_trips_bytes_and_preserves_mode() {
        struct Case {
            name: &'static str,
            content: &'static [u8],
            line: usize,
            header: &'static [u8],
            want: &'static [u8],
        }
        let cases = [
            Case {
                name: "crlf file",
                content: b"- [ ] A-1 Old\r\n\t\t#x\r\n",
                line: 1,
                header: b"- [x] A-1 New",
                want: b"- [x] A-1 New\r\n\t\t#x\r\n",
            },
            Case {
                name: "no trailing newline",
                content: b"- [ ] A-1 Title",
                line: 1,
                header: b"- [x] A-1 Title",
                want: b"- [x] A-1 Title",
            },
            Case {
                name: "eof header without body",
                content: b"- [ ] A-1 Title\n",
                line: 1,
                header: b"- [x] A-1 Title",
                want: b"- [x] A-1 Title\n",
            },
            Case {
                name: "invalid utf-8 bytes",
                content: b"- [ ] A-1 \xff\xfe\n",
                line: 1,
                header: b"- [x] A-1 \xff\xfe",
                want: b"- [x] A-1 \xff\xfe\n",
            },
        ];

        for case in cases {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("tasks.md");
            write(&path, case.content);
            #[cfg(unix)]
            set_mode(&path, 0o640);

            let mut edit = open(path.to_str().expect("utf8 path")).expect("open");
            assert_eq!(
                edit.bytes(),
                case.content,
                "{}: byte-exact before the edit",
                case.name
            );
            edit.replace_header_line(case.line, case.header)
                .expect("replace");
            edit.commit().expect("commit");

            assert_eq!(read(&path), case.want, "{}: file bytes", case.name);
            #[cfg(unix)]
            assert_eq!(mode_of(&path), 0o640, "{}: mode preserved", case.name);
        }
    }

    #[test]
    fn crlf_fixture_round_trips_byte_for_byte() {
        let original = fixture("tasks-crlf.md");
        assert!(
            original.windows(2).any(|window| window == b"\r\n"),
            "fixture uses CRLF"
        );

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("crlf.md");
        write(&path, &original);

        let mut edit = open(path.to_str().expect("utf8 path")).expect("open");
        assert_eq!(edit.bytes(), original, "clean edit is byte-exact");
        edit.replace_header_line(2, b"- [x] CRLF-1 Preserve every byte")
            .expect("replace");
        edit.commit().expect("commit");

        let expected: &[u8] = b"# CRLF fixture\r\n- [x] CRLF-1 Preserve every byte\r\n  body line one\r\n- [x] CRLF-2 Done already\r\n";
        assert_eq!(read(&path), expected);
    }

    #[test]
    fn invalid_utf8_fixture_round_trips_byte_for_byte() {
        let original = fixture("tasks-invalid-utf8.md");
        assert!(original.contains(&0xff), "fixture has a raw 0xFF byte");

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("bytes.md");
        write(&path, &original);

        let mut edit = open(path.to_str().expect("utf8 path")).expect("open");
        assert_eq!(edit.bytes(), original, "clean edit is byte-exact");
        edit.flip_checkbox(1).expect("flip");
        edit.commit().expect("commit");

        let expected: &[u8] =
            b"- [x] BYTE-1 Title with a raw byte: \xff\n  body with raw \xfe\xff bytes\n";
        assert_eq!(read(&path), expected);
        assert!(read(&path).contains(&0xff), "0xFF survives the write");
    }

    #[test]
    fn verify_reports_stale_with_e17_and_writes_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("tasks.md");
        write(&path, b"- [ ] A-1 Title\n");

        let edit = open(path.to_str().expect("utf8 path")).expect("open");
        edit.verify("A-1", 1).expect("verify before mutation");

        let changed = b"# changed\n- [ ] A-1 Title\n";
        write(&path, changed);
        let err = edit.verify("A-1", 1).expect_err("stale after mutation");
        assert_eq!(
            err.to_string(),
            "file changed, task 'A-1' not at expected line"
        );
        assert!(matches!(err, EditError::Stale(StaleError { .. })));
        assert_eq!(read(&path), changed, "verification never writes");
    }

    #[test]
    fn verify_task_prefers_the_raw_header_for_broad_items() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("tasks.md");
        write(&path, b"* [ ] no id here\n");
        let path_str = path.to_str().expect("utf8 path");

        let edit = open(path_str).expect("open");
        let header = "* [ ] no id here";
        let broad = Task {
            broad: true,
            ..task(path_str, 1, header, "")
        };
        edit.verify_task(&broad).expect("broad header matches");

        write(&path, b"* [x] no id here\n");
        let err = edit.verify_task(&broad).expect_err("changed header");
        assert_eq!(
            err.to_string(),
            "file changed, task '* [ ] no id here' not at expected line"
        );
    }

    #[test]
    fn flip_checkbox_preserves_every_other_byte() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("tasks.md");
        write(&path, b"\t+ [X] star task \xff\n");
        let path_str = path.to_str().expect("utf8 path");

        let mut edit = open(path_str).expect("open");
        edit.flip_checkbox(1).expect("flip");
        edit.commit().expect("commit");
        assert_eq!(read(&path), b"\t+ [ ] star task \xff\n");

        let mut edit = open(path_str).expect("reopen");
        let err = edit.flip_checkbox(5).expect_err("line out of bounds");
        assert_eq!(err.to_string(), "line 5 out of bounds for 1 lines");

        write(&path, b"- [ ]nospace\n");
        let mut edit = open(path_str).expect("reopen");
        let err = edit.flip_checkbox(1).expect_err("not a checkbox");
        assert_eq!(err.to_string(), "line 1 is not a task header");
    }

    #[test]
    fn append_block_creates_parents_and_preserves_the_existing_eol() {
        let dir = TempDir::new().expect("tempdir");
        let nested = dir.path().join("nested").join("archive.md");
        let nested_str = nested.to_str().expect("utf8 path");

        append_block(nested_str, b"- [x] A-9 Done\r\n\tbody\r\n").expect("append");
        assert_eq!(
            read(&nested),
            b"- [x] A-9 Done\n\tbody\n",
            "a new file uses LF even for a CRLF block"
        );

        append_block(nested_str, b"- [x] A-10 Also\n").expect("append");
        append_block(nested_str, b"- [x] A-11 Last").expect("append");
        assert_eq!(
            read(&nested),
            b"- [x] A-9 Done\n\tbody\n- [x] A-10 Also\n- [x] A-11 Last\n"
        );
    }

    #[test]
    fn append_block_restores_crlf_and_adds_missing_separators() {
        let dir = TempDir::new().expect("tempdir");
        let crlf = dir.path().join("crlf.md");
        write(&crlf, b"- [x] A-1 Old\r\n");
        append_block(
            crlf.to_str().expect("utf8 path"),
            b"- [x] A-2 New\r\n\tbody\r\n",
        )
        .expect("append");
        assert_eq!(read(&crlf), b"- [x] A-1 Old\r\n- [x] A-2 New\r\n\tbody\r\n");

        let bare = dir.path().join("bare.md");
        write(&bare, b"- [x] A-1 Old");
        append_block(bare.to_str().expect("utf8 path"), b"- [x] A-2 New").expect("append");
        assert_eq!(read(&bare), b"- [x] A-1 Old\n- [x] A-2 New\n");
    }

    #[test]
    fn apply_rewrites_planned_headers_per_file() {
        let dir = TempDir::new().expect("tempdir");
        let first = dir.path().join("a.md");
        let second = dir.path().join("b.md");
        write(&first, b"- [ ] one\n- [ ] two\n");
        write(&second, b"- [ ] three\n");
        let first_str = first.to_str().expect("utf8 path").to_string();
        let second_str = second.to_str().expect("utf8 path").to_string();

        let writes = vec![
            PlannedWrite {
                file: first_str.clone(),
                line: 2,
                header: "- [ ] PRJ-002 two".to_string(),
            },
            PlannedWrite {
                file: first_str,
                line: 1,
                header: "- [ ] PRJ-001 one".to_string(),
            },
            PlannedWrite {
                file: second_str,
                line: 1,
                header: "- [ ] PRJ-003 three".to_string(),
            },
        ];
        apply(&writes).expect("apply");

        assert_eq!(read(&first), b"- [ ] PRJ-001 one\n- [ ] PRJ-002 two\n");
        assert_eq!(read(&second), b"- [ ] PRJ-003 three\n");
    }

    #[test]
    fn archive_appends_done_blocks_then_removes_them_descending() {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("a.md");
        write(
            &source,
            b"- [x] A-1 Done one\n  body of one\n- [ ] A-2 Open two\n- [x] A-3 Done three\n",
        );
        let source_str = source.to_str().expect("utf8 path");
        let targets = vec![
            task(source_str, 1, "- [x] A-1 Done one", "A-1"),
            task(source_str, 4, "- [x] A-3 Done three", "A-3"),
        ];

        archive_tasks(
            &targets,
            dir.path().to_str().expect("utf8 path"),
            &archive_config(),
        )
        .expect("archive");

        assert_eq!(read(&source), b"- [ ] A-2 Open two\n");
        assert_eq!(
            read(&dir.path().join("_archive.md")),
            b"- [x] A-1 Done one\n  body of one\n\n- [x] A-3 Done three\n"
        );
    }

    #[test]
    fn archive_validates_every_source_before_writing() {
        let dir = TempDir::new().expect("tempdir");
        let first = dir.path().join("a.md");
        let second = dir.path().join("b.md");
        write(&first, b"- [x] A-1 Done\n");
        write(&second, b"- [x] B-1 Done\n");
        let first_str = first.to_str().expect("utf8 path");
        let second_str = second.to_str().expect("utf8 path");
        let targets = vec![
            task(first_str, 1, "- [x] A-1 Done", "A-1"),
            task(second_str, 1, "- [x] B-1 Done", "B-1"),
        ];

        // b.md becomes stale between discovery and the archive call.
        write(&second, b"# moved\n- [x] B-1 Done\n");
        let err = archive_tasks(
            &targets,
            dir.path().to_str().expect("utf8 path"),
            &archive_config(),
        )
        .expect_err("stale source");

        assert!(matches!(err, EditError::Stale(StaleError { .. })));
        assert_eq!(read(&first), b"- [x] A-1 Done\n", "first source untouched");
        assert!(
            !dir.path().join("_archive.md").exists(),
            "no archive is written"
        );
    }

    #[test]
    fn archive_rejects_an_escaping_destination() {
        let dir = TempDir::new().expect("tempdir");
        let base = dir.path().join("work");
        fs::create_dir(&base).expect("mkdir");
        let source = base.join("a.md");
        write(&source, b"- [x] A-1 Done\n");
        let target = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [x] A-1 Done",
            "A-1",
        );
        let cfg = Config {
            archive_path: "../escaped.md".to_string(),
            ..Config::default()
        };

        let err = archive_tasks(&[target], base.to_str().expect("utf8 path"), &cfg)
            .expect_err("escaping destination");
        assert_eq!(
            err.to_string(),
            format!(
                "archive path '{}' is outside the base directory",
                base.parent().expect("parent").join("escaped.md").display()
            )
        );
        assert!(!dir.path().join("escaped.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn archive_is_written_before_a_source_rewrite_fails() {
        let dir = TempDir::new().expect("tempdir");
        let source_dir = dir.path().join("src");
        fs::create_dir(&source_dir).expect("mkdir");
        let source = source_dir.join("tasks.md");
        write(&source, b"- [x] A-1 Done\n");

        set_mode(&source_dir, 0o555);
        if permission_bits_ignored(&source_dir) {
            set_mode(&source_dir, 0o755);
            return;
        }

        let target = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [x] A-1 Done",
            "A-1",
        );
        let err = archive_tasks(
            &[target],
            dir.path().to_str().expect("utf8 path"),
            &archive_config(),
        )
        .expect_err("source rewrite fails");

        assert!(err.to_string().contains("cannot write to"), "got: {err}");
        assert_eq!(
            read(&dir.path().join("_archive.md")),
            b"- [x] A-1 Done\n",
            "the block is archived before the source rewrite"
        );
        assert_eq!(read(&source), b"- [x] A-1 Done\n", "source left untouched");
        set_mode(&source_dir, 0o755);
    }

    #[test]
    fn move_relocates_the_block_target_first() {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("a.md");
        let target = dir.path().join("b.md");
        write(
            &source,
            b"# Notes\n\n- [ ] A-1 First\n\n- [ ] A-2 Second\n  second body\n",
        );
        write(&target, b"# Target\n");

        let moving = task(
            source.to_str().expect("utf8 path"),
            5,
            "- [ ] A-2 Second",
            "A-2",
        );
        move_task(&moving, target.to_str().expect("utf8 path")).expect("move");

        assert_eq!(
            read(&target),
            b"# Target\n- [ ] A-2 Second\n  second body\n"
        );
        assert_eq!(read(&source), b"# Notes\n\n- [ ] A-1 First\n\n");
    }

    #[test]
    fn move_reports_a_stale_source_without_writing() {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("a.md");
        let target = dir.path().join("b.md");
        write(&source, b"- [ ] A-1 Title\n");
        write(&target, b"# Target\n");
        let moving = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [ ] A-1 Title",
            "A-1",
        );

        write(&source, b"# changed\n- [ ] A-1 Title\n");
        let err = move_task(&moving, target.to_str().expect("utf8 path")).expect_err("stale");

        assert_eq!(
            err.to_string(),
            "file changed, task 'A-1' not at expected line"
        );
        assert_eq!(read(&source), b"# changed\n- [ ] A-1 Title\n");
        assert_eq!(read(&target), b"# Target\n");
    }

    #[test]
    fn move_rejects_a_directory_target() {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("a.md");
        write(&source, b"- [ ] A-1 Title\n");
        let target_dir = dir.path().join("adir");
        fs::create_dir(&target_dir).expect("mkdir");
        let moving = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [ ] A-1 Title",
            "A-1",
        );

        let err = move_task(&moving, target_dir.to_str().expect("utf8 path"))
            .expect_err("directory target");
        assert!(matches!(err, EditError::Directory(_)));
        assert!(err.to_string().ends_with("' is a directory"));
    }

    #[cfg(unix)]
    #[test]
    fn move_is_a_no_op_for_the_same_realpath() {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("tasks.md");
        write(&source, b"- [ ] M-1 Stay\n");
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        let moving = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [ ] M-1 Stay",
            "M-1",
        );

        move_task(&moving, link.to_str().expect("utf8 path")).expect("no-op");
        assert_eq!(read(&source), b"- [ ] M-1 Stay\n");
        assert_eq!(read(&link), b"- [ ] M-1 Stay\n");
    }

    #[cfg(unix)]
    #[test]
    fn move_writes_the_target_before_failing_on_the_source() {
        let dir = TempDir::new().expect("tempdir");
        let source_dir = dir.path().join("src");
        fs::create_dir(&source_dir).expect("mkdir");
        let source = source_dir.join("tasks.md");
        let target = dir.path().join("target.md");
        write(&source, b"- [ ] M-1 Relocate\n  body\n- [ ] M-2 Stay\n");

        set_mode(&source_dir, 0o555);
        if permission_bits_ignored(&source_dir) {
            set_mode(&source_dir, 0o755);
            return;
        }

        let moving = task(
            source.to_str().expect("utf8 path"),
            1,
            "- [ ] M-1 Relocate",
            "M-1",
        );
        let err = move_task(&moving, target.to_str().expect("utf8 path"))
            .expect_err("source rewrite fails");

        assert!(matches!(err, EditError::Io(_)), "got: {err}");
        assert_eq!(
            read(&target),
            b"- [ ] M-1 Relocate\n  body\n",
            "target written first"
        );
        assert_eq!(
            read(&source),
            b"- [ ] M-1 Relocate\n  body\n- [ ] M-2 Stay\n",
            "source untouched"
        );
        set_mode(&source_dir, 0o755);
    }
}
