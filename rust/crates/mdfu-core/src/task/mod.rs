//! Task engine types and grammar (`mdtask`-compatible).
//!
//! First slice of the `internal/task` port: [`Task`], [`Unidentified`], and
//! the header grammar in [`grammar`]. Body extraction, ID assignment,
//! discovery, rendering, editing, and the command layer land in follow-up
//! submodules (`body`, `id`, `discover`, `render`, `edit`, `cli`).

pub mod grammar;

use std::collections::BTreeMap;

/// A single parsed task header plus its body.
///
/// Mirrors the Go `Task` struct from `internal/task/format.go`.
///
/// Two fields exist beyond the frozen shape, both required for byte-exact
/// round-tripping:
///
/// * `meta_sep` records the exact whitespace that separated the title from the
///   metadata tokens in the source header. It is empty when the task has no
///   metadata; when metadata is present and `meta_sep` is empty,
///   [`grammar::render_header`] emits the canonical two-tab separator.
/// * `seed_prefix` records the bare prefix of a seed header (`PRJ` for
///   `- [ ] PRJ- Title`) so the line can be rendered back verbatim.
#[allow(clippy::struct_excessive_bools)] // the bool set is the frozen Go/SPEC shape
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Task {
    /// Full task ID (`PREFIX-<n>`); empty for seed and unidentified lines.
    pub id: String,
    /// Raw checkbox byte: `b' '` or `b'x'`, plus `b'X'` for broad parses.
    pub status: u8,
    /// Title text, with metadata stripped.
    pub title: String,
    /// Tags without their leading `#`, in source order.
    pub tags: Vec<String>,
    /// Priority without its leading `!`; empty when absent.
    pub priority: String,
    /// Dedented body text. Filled by the body port.
    pub body: String,
    /// Original block indent of the body. Filled by the body port.
    pub body_indent: String,
    /// Slash-separated path relative to the invocation directory.
    pub file: String,
    /// 1-based line number in `file`; 0 when unknown.
    pub line: usize,
    /// The exact header line, after a single trailing `\r` is stripped.
    pub header_raw: String,
    /// Checkbox state: `status` is `x` or `X`.
    pub checked: bool,
    /// A seed line: `PREFIX- ` with no numeric part.
    pub seeded: bool,
    /// A checkbox line that carries no ID candidate at all.
    pub unidentified: bool,
    /// Property keys in first-seen order.
    pub property_order: Vec<String>,
    /// Property values keyed by property name. Go's map is unordered, so
    /// deterministic iteration lives in `property_order`.
    pub properties: BTreeMap<String, String>,
    /// Exact title/metadata separator from the source; empty with no metadata.
    pub meta_sep: String,
    /// Bare seed prefix (`PRJ`) so a seed line renders back verbatim.
    pub seed_prefix: String,
    /// Set only by [`grammar::parse_checkbox`], the broadened grammar.
    pub broad: bool,
}

/// A checkbox-shaped line that carries no ID and no seed prefix.
///
/// Seed lines are reported separately through [`Task::seeded`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unidentified {
    /// Slash-separated path relative to the invocation directory.
    pub file: String,
    /// 1-based line number in `file`.
    pub line: usize,
    /// Raw source line, exactly as written.
    pub raw: String,
}
