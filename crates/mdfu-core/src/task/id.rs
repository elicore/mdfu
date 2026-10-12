//! Task ID grammar, resolution, and all-or-nothing assignment planning.
//!
//! Port of `internal/task/id.go` and the planning half of
//! `internal/task/ids.go`. Every function here is pure: [`plan`] returns the
//! rewrites it would apply and never touches the filesystem. The command layer
//! that reads files, verifies the source lines are still current, and writes
//! headers lands in a follow-up submodule.
//!
//! IDs stay strings end to end. The numeric part is parsed only for ordering
//! and duplicate detection, so a leading zero in an existing ID (`EXMPL-007`)
//! survives every output verbatim. New numeric parts start at `GlobalMax + 1`
//! and are zero-padded to `max(3, digits(GlobalMax))`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::grammar::{parse_header, render_header};
use super::Task;

/// A resolution or planning failure. The display strings are the frozen
/// `SPEC.md` §J messages (E9-E13 and E16) and must not be reworded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// E9: the input is neither a full ID nor a bare numeric part.
    InvalidTaskId(String),
    /// E10: no task carries the requested ID or numeric part.
    NotFound(String),
    /// E11: a bare numeric part matches tasks with different prefixes.
    AmbiguousNumeric {
        /// The bare numeric input as written.
        input: String,
        /// The matching full IDs, sorted.
        ids: Vec<String>,
    },
    /// E12: a full ID appears on more than one task.
    DuplicateId(String),
    /// E13: a prefix fails the `^[A-Z][A-Z0-9]*$` grammar.
    InvalidPrefix(String),
    /// E16: an assignment has no valid prefix from any source.
    NoPrefix {
        /// File of the assignment line.
        file: String,
        /// 1-based line number in `file`.
        line: usize,
        /// The raw source line awaiting an ID.
        raw_line: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTaskId(input) => write!(f, "invalid task ID '{input}'"),
            Self::NotFound(id) => write!(f, "task '{id}' not found"),
            Self::AmbiguousNumeric { input, ids } => {
                write!(f, "ambiguous numeric ID '{input}' matches: {}", ids.join(", "))
            }
            Self::DuplicateId(id) => {
                write!(f, "task '{id}' appears multiple times; expected exactly one match")
            }
            Self::InvalidPrefix(prefix) => write!(f, "invalid prefix '{prefix}'"),
            Self::NoPrefix {
                file,
                line,
                raw_line,
            } => write!(
                f,
                "{file}:{line}: no prefix found for task \"{raw_line}\" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Reports whether `s` is a well-formed ID prefix (`^[A-Z][A-Z0-9]*$`).
#[must_use]
pub fn valid_prefix(s: &str) -> bool {
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

/// Reports whether `s` is a well-formed `PREFIX-<n>` identifier
/// (`^[A-Z][A-Z0-9]*-[0-9]+$`).
#[must_use]
pub fn valid_task_id(s: &str) -> bool {
    let Some((prefix, digits)) = s.split_once('-') else {
        return false;
    };
    valid_prefix(prefix) && !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Returns the decimal numeric part of an identifier.
///
/// Like Go's `strconv.Atoi`, a numeric part beyond [`i64::MAX`] is an overflow
/// and returns `None`. Leading zeros parse away here but are never used to
/// rewrite an existing ID.
#[must_use]
pub fn numeric_part(id: &str) -> Option<i64> {
    if !valid_task_id(id) {
        return None;
    }
    let (_, digits) = id.split_once('-')?;
    digits.parse::<i64>().ok()
}

/// Reports whether `input` is a bare numeric part (`^[0-9]+$`).
fn is_bare_numeric(input: &str) -> bool {
    !input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit())
}

/// Maps a full identifier or a bare numeric part to exactly one task.
///
/// Bare numeric inputs match on [`numeric_part`], so `7` and `007` both find
/// `EXMPL-007`; the task's own spelling is never rewritten here.
///
/// # Errors
///
/// Returns [`Error::InvalidTaskId`] (E9) for malformed input,
/// [`Error::NotFound`] (E10) when nothing matches,
/// [`Error::AmbiguousNumeric`] (E11) when a bare numeric part matches several
/// prefixes, and [`Error::DuplicateId`] (E12) when a full ID appears more than
/// once.
#[must_use = "the resolved task or the error must be handled"]
pub fn resolve<'a>(input: &str, tasks: &'a [Task]) -> Result<&'a Task, Error> {
    let bare = is_bare_numeric(input);
    if !valid_task_id(input) && !bare {
        return Err(Error::InvalidTaskId(input.to_string()));
    }

    let matches: Vec<&Task> = if bare {
        // Go ignores the Atoi error and compares against the clamped value, so
        // a huge digit string matches a task with the largest numeric part.
        let want = input.parse::<i64>().unwrap_or(i64::MAX);
        tasks
            .iter()
            .filter(|task| numeric_part(&task.id) == Some(want))
            .collect()
    } else {
        tasks.iter().filter(|task| task.id == input).collect()
    };

    match matches.len() {
        0 => Err(Error::NotFound(input.to_string())),
        1 => Ok(matches[0]),
        _ if bare => {
            let mut ids: Vec<String> = matches.iter().map(|task| task.id.clone()).collect();
            ids.sort();
            Err(Error::AmbiguousNumeric {
                input: input.to_string(),
                ids,
            })
        }
        _ => Err(Error::DuplicateId(input.to_string())),
    }
}

/// Returns the largest numeric part across all identified tasks, or 0 when
/// there is none. Seed and unidentified lines have no numeric part.
#[must_use]
pub fn global_max(tasks: &[Task]) -> i64 {
    tasks
        .iter()
        .filter_map(|task| numeric_part(&task.id))
        .max()
        .unwrap_or(0)
}

/// Returns the zero-padding width for new numeric parts: `max(3, digits)`,
/// with a negative `global_max` clamped to zero first.
#[must_use]
pub fn pad_width(global_max: i64) -> usize {
    let value = global_max.max(0);
    value.to_string().len().max(3)
}

/// Returns the prefix carried by the most tasks; ties break toward the first
/// prefix seen. Returns an empty string when there is no identified task.
#[must_use]
pub fn most_frequent_prefix(tasks: &[Task]) -> String {
    let prefixes: Vec<String> = tasks
        .iter()
        .filter_map(|task| task_prefix(&task.id).map(str::to_string))
        .collect();
    most_frequent(&prefixes)
}

/// Extracts the prefix from a seed header of the form `- [ ] PRJ- Title`.
///
/// Returns `None` for identified lines (`- [ ] PRJ-001 Title`), plain
/// unidentified lines, and lines that are not headers at all.
#[must_use]
pub fn seed_prefix(line: &str) -> Option<String> {
    parse_header(line)
        .filter(|task| task.seeded)
        .map(|task| task.seed_prefix)
}

/// Returns an error when `s` is not a valid prefix.
///
/// # Errors
///
/// Returns [`Error::InvalidPrefix`] (E13) when `s` fails [`valid_prefix`].
#[must_use = "the validation result must be handled"]
pub fn validate_prefix(s: &str) -> Result<(), Error> {
    if valid_prefix(s) {
        Ok(())
    } else {
        Err(Error::InvalidPrefix(s.to_string()))
    }
}

/// Maps a numeric part to the sorted identifiers that share it.
///
/// A part is included when it is used by more than one task or by tasks with
/// different prefixes. Tasks without a valid ID are ignored.
#[must_use]
pub fn duplicate_numeric_parts(tasks: &[Task]) -> BTreeMap<i64, Vec<String>> {
    let mut by_number: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    let mut prefixes: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for task in tasks {
        let Some(number) = numeric_part(&task.id) else {
            continue;
        };
        by_number.entry(number).or_default().push(task.id.clone());
        if let Some(prefix) = task_prefix(&task.id) {
            prefixes
                .entry(number)
                .or_default()
                .insert(prefix.to_string());
        }
    }

    let mut out = BTreeMap::new();
    for (number, mut ids) in by_number {
        if ids.len() > 1 || prefixes.get(&number).is_some_and(|set| set.len() > 1) {
            ids.sort();
            out.insert(number, ids);
        }
    }
    out
}

/// One unidentified or seed line awaiting an ID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assignment {
    /// Slash-separated path relative to the invocation directory.
    pub file: String,
    /// 1-based line number in `file`.
    pub line: usize,
    /// The raw source line, exactly as written.
    pub raw_line: String,
    /// Caller-supplied fallback prefix (`--prefix`, uppercased by the caller).
    pub prefix: String,
    /// Prefix extracted from a seed line in the same file.
    pub seed: String,
    /// Existing prefixes in the same file, for the most-frequent rule.
    pub file_prefixes: Vec<String>,
}

/// One header rewrite produced by [`plan`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlannedWrite {
    /// Slash-separated path relative to the invocation directory.
    pub file: String,
    /// 1-based line number in `file`.
    pub line: usize,
    /// The rewritten header line carrying the new ID.
    pub header: String,
}

/// Assigns sequential identifiers to every assignment.
///
/// Planning is all-or-nothing: every assignment's prefix is resolved and
/// validated before a single write is planned, so an unresolvable assignment
/// yields `Err` and no writes at all. Numbers start at `global_max + 1` and
/// are zero-padded to [`pad_width`]. This function never touches the
/// filesystem.
///
/// # Errors
///
/// Returns [`Error::InvalidPrefix`] (E13) for an invalid candidate prefix and
/// [`Error::NoPrefix`] (E16) when an assignment has no prefix from any source.
#[must_use = "the planned writes or the error must be handled"]
pub fn plan(assigns: &[Assignment], global_max: i64) -> Result<Vec<PlannedWrite>, Error> {
    // Pass 1: resolve and validate every prefix before any ID is numbered.
    let prefixes = assigns
        .iter()
        .map(resolve_prefix)
        .collect::<Result<Vec<String>, Error>>()?;

    let width = pad_width(global_max);
    let mut writes = Vec::with_capacity(assigns.len());
    let mut number = global_max;
    for (assignment, prefix) in assigns.iter().zip(prefixes) {
        // Go `int` arithmetic wraps on overflow; mirror it instead of panicking.
        number = number.wrapping_add(1);
        let id = format!("{prefix}-{number:0width$}");
        writes.push(PlannedWrite {
            file: assignment.file.clone(),
            line: assignment.line,
            header: header_with_id(&assignment.raw_line, &id),
        });
    }
    Ok(writes)
}

/// Picks the prefix for one assignment: the most frequent existing prefix in
/// the file, then the seed prefix, then the caller-supplied prefix.
fn resolve_prefix(assignment: &Assignment) -> Result<String, Error> {
    let candidates = [
        most_frequent(&assignment.file_prefixes),
        assignment.seed.clone(),
        assignment.prefix.clone(),
    ];
    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        validate_prefix(&candidate)?;
        return Ok(candidate);
    }
    Err(Error::NoPrefix {
        file: assignment.file.clone(),
        line: assignment.line,
        raw_line: assignment.raw_line.clone(),
    })
}

/// Rewrites a source header line to carry `id`, leaving every other byte
/// (title, metadata, separator, checkbox) untouched. A line that is not a
/// strict header is returned verbatim.
fn header_with_id(raw: &str, id: &str) -> String {
    match parse_header(raw) {
        Some(mut task) => {
            task.id = id.to_string();
            task.seeded = false;
            task.seed_prefix.clear();
            render_header(&task)
        }
        None => raw.to_string(),
    }
}

/// Returns the prefix of a full identifier.
fn task_prefix(id: &str) -> Option<&str> {
    if !valid_task_id(id) {
        return None;
    }
    id.split_once('-').map(|(prefix, _)| prefix)
}

/// Returns the most common non-empty string, preferring first appearance on
/// ties.
fn most_frequent(values: &[String]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    for value in values {
        if value.is_empty() {
            continue;
        }
        if !counts.contains_key(value.as_str()) {
            order.push(value);
        }
        *counts.entry(value.as_str()).or_insert(0) += 1;
    }

    let mut best = "";
    let mut best_count = 0;
    for value in order {
        let count = counts[value];
        if count > best_count {
            best_count = count;
            best = value;
        }
    }
    best.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        Task {
            id: id.to_string(),
            ..Task::default()
        }
    }

    fn assignment(file: &str, line: usize, raw_line: &str) -> Assignment {
        Assignment {
            file: file.to_string(),
            line,
            raw_line: raw_line.to_string(),
            ..Assignment::default()
        }
    }

    #[test]
    fn resolve_full_id_bare_number_and_errors() {
        let tasks = vec![
            Task {
                id: "EXMPL-1".into(),
                title: "one".into(),
                ..Task::default()
            },
            Task {
                id: "EXMPL-2".into(),
                title: "two".into(),
                ..Task::default()
            },
            Task {
                id: "A-1".into(),
                title: "a-one".into(),
                ..Task::default()
            },
        ];
        assert_eq!(resolve("EXMPL-2", &tasks).unwrap().title, "two");
        assert_eq!(resolve("2", &tasks).unwrap().id, "EXMPL-2");

        let cases = [
            ("not an id", tasks.clone(), "invalid task ID 'not an id'"),
            ("exmpl-1", tasks.clone(), "invalid task ID 'exmpl-1'"),
            ("NOPE-1", tasks.clone(), "task 'NOPE-1' not found"),
            ("9", tasks, "task '9' not found"),
            (
                "1",
                vec![task("B-1"), task("A-1")],
                "ambiguous numeric ID '1' matches: A-1, B-1",
            ),
            (
                "A-1",
                vec![task("A-1"), task("A-1")],
                "task 'A-1' appears multiple times; expected exactly one match",
            ),
        ];
        for (input, tasks, want) in cases {
            let err = resolve(input, &tasks).unwrap_err();
            assert_eq!(err.to_string(), want, "resolve({input:?})");
        }
    }

    #[test]
    fn leading_zeros_are_preserved() {
        assert_eq!(numeric_part("EXMPL-007"), Some(7));
        assert_eq!(numeric_part("A1-000"), Some(0));

        let tasks = vec![task("EXMPL-007")];
        assert_eq!(resolve("EXMPL-007", &tasks).unwrap().id, "EXMPL-007");
        assert_eq!(resolve("7", &tasks).unwrap().id, "EXMPL-007");
        assert_eq!(resolve("007", &tasks).unwrap().id, "EXMPL-007");
    }

    #[test]
    fn bare_numeric_overflow_clamps_like_go_atoi() {
        let tasks = vec![task("A-9223372036854775807")];
        let got = resolve("99999999999999999999", &tasks).unwrap();
        assert_eq!(got.id, "A-9223372036854775807");
    }

    #[test]
    fn global_max_matches_go_table() {
        assert_eq!(global_max(&[]), 0);
        assert_eq!(
            global_max(&[Task {
                title: "no id".into(),
                ..Task::default()
            }]),
            0
        );
        assert_eq!(global_max(&[task("EXMPL-007"), task("OTHER-3")]), 7);
        assert_eq!(global_max(&[task("EXMPL-2"), task("EXMPL-40")]), 40);
        // Numeric part beyond i64 overflows Atoi, so it is ignored like Go.
        assert_eq!(global_max(&[task("A-9223372036854775808")]), 0);
    }

    #[test]
    fn pad_width_matches_go_table() {
        let cases = [
            (0, 3),
            (7, 3),
            (42, 3),
            (999, 3),
            (1000, 4),
            (12345, 5),
            (-5, 3),
            (i64::MIN, 3),
            (i64::MAX, 19),
        ];
        for (input, want) in cases {
            assert_eq!(pad_width(input), want, "pad_width({input})");
        }
    }

    #[test]
    fn prefix_grammar_matches_go_table() {
        for s in ["A", "EXMPL", "A1", "ABC123"] {
            assert!(valid_prefix(s), "{s:?}");
        }
        for s in ["", "a", "1A", "A-B", "A B", "Ä"] {
            assert!(!valid_prefix(s), "{s:?}");
        }
        for s in ["A-1", "EXMPL-42", "A1-007"] {
            assert!(valid_task_id(s), "{s:?}");
        }
        for s in ["A", "A-", "-1", "a-1", "A-1-2", "A-x"] {
            assert!(!valid_task_id(s), "{s:?}");
        }
    }

    #[test]
    fn numeric_part_matches_go_table() {
        assert_eq!(numeric_part("EXMPL-007"), Some(7));
        assert_eq!(numeric_part("A-1"), Some(1));
        assert_eq!(numeric_part("A-9223372036854775807"), Some(i64::MAX));
        assert_eq!(numeric_part("A-9223372036854775808"), None);
        assert_eq!(numeric_part("not-an-id"), None);
    }

    #[test]
    fn most_frequent_prefix_matches_go_table() {
        let tasks = vec![task("PRJ-1"), task("OTHER-2"), task("PRJ-3"), task("SEED")];
        assert_eq!(most_frequent_prefix(&tasks), "PRJ");
        assert_eq!(most_frequent_prefix(&[]), "");
        // Tie broken by first appearance.
        assert_eq!(most_frequent_prefix(&[task("BBB-1"), task("AAA-2")]), "BBB");
    }

    #[test]
    fn seed_prefix_matches_go_table() {
        assert_eq!(seed_prefix("- [ ] PRJ- Task title").as_deref(), Some("PRJ"));
        assert_eq!(seed_prefix("- [ ] PRJ-001 Real task"), None);
        assert_eq!(seed_prefix("- [ ] No ID here"), None);
        assert_eq!(seed_prefix("not a header"), None);
        assert_eq!(seed_prefix("- [ ] SEED- A seed\r").as_deref(), Some("SEED"));
    }

    #[test]
    fn validate_prefix_matches_go_table() {
        assert!(validate_prefix("PRJ").is_ok());
        assert_eq!(
            validate_prefix("bad").unwrap_err().to_string(),
            "invalid prefix 'bad'"
        );
    }

    #[test]
    fn plan_assigns_sequential_padded_ids() {
        let assigns = vec![
            Assignment {
                file_prefixes: vec!["PRJ".into()],
                ..assignment("a.md", 3, "- [ ] First task")
            },
            Assignment {
                seed: "SEED".into(),
                ..assignment("a.md", 5, "- [ ] SEED- Second task")
            },
            Assignment {
                prefix: "NEW".into(),
                ..assignment("b.md", 2, "- [ ] Third task")
            },
        ];
        let writes = plan(&assigns, 7).unwrap();
        let want = vec![
            PlannedWrite {
                file: "a.md".into(),
                line: 3,
                header: "- [ ] PRJ-008 First task".into(),
            },
            PlannedWrite {
                file: "a.md".into(),
                line: 5,
                header: "- [ ] SEED-009 Second task".into(),
            },
            PlannedWrite {
                file: "b.md".into(),
                line: 2,
                header: "- [ ] NEW-010 Third task".into(),
            },
        ];
        assert_eq!(writes, want);
    }

    #[test]
    fn plan_is_all_or_nothing() {
        let assigns = vec![
            Assignment {
                file_prefixes: vec!["PRJ".into()],
                ..assignment("a.md", 3, "- [ ] First task")
            },
            assignment("b.md", 2, "- [ ] No source here"),
        ];
        let result = plan(&assigns, 0);
        let want = "b.md:2: no prefix found for task \"- [ ] No source here\" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ";
        assert_eq!(result.unwrap_err().to_string(), want);
    }

    #[test]
    fn plan_rejects_invalid_explicit_prefix() {
        let assigns = vec![Assignment {
            prefix: "bad".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        }];
        assert_eq!(
            plan(&assigns, 0).unwrap_err().to_string(),
            "invalid prefix 'bad'"
        );
    }

    #[test]
    fn plan_padding_follows_global_max() {
        let assigns = vec![Assignment {
            prefix: "PRJ".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        }];
        let writes = plan(&assigns, 999).unwrap();
        assert_eq!(writes[0].header, "- [ ] PRJ-1000 Task");
    }

    #[test]
    fn resolve_prefix_precedence_file_seed_then_provided() {
        let all = Assignment {
            file_prefixes: vec!["A".into()],
            seed: "SEED".into(),
            prefix: "NEW".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(resolve_prefix(&all).unwrap(), "A");

        let seed_over_provided = Assignment {
            seed: "SEED".into(),
            prefix: "NEW".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(resolve_prefix(&seed_over_provided).unwrap(), "SEED");

        let provided = Assignment {
            prefix: "NEW".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(resolve_prefix(&provided).unwrap(), "NEW");
    }

    #[test]
    fn resolve_prefix_uses_most_frequent_file_prefix() {
        let tie = Assignment {
            file_prefixes: vec!["A".into(), "A".into(), "B".into()],
            seed: "SEED".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(resolve_prefix(&tie).unwrap(), "A");

        let empty_first = Assignment {
            file_prefixes: vec![String::new()],
            seed: "SEED".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(resolve_prefix(&empty_first).unwrap(), "SEED");
    }

    #[test]
    fn resolve_prefix_reports_no_prefix() {
        let none = assignment("a.md", 1, "- [ ] Task");
        let want = "a.md:1: no prefix found for task \"- [ ] Task\" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ";
        assert_eq!(resolve_prefix(&none).unwrap_err().to_string(), want);
    }

    #[test]
    fn resolve_prefix_rejects_invalid_candidates() {
        let bad_file = Assignment {
            file_prefixes: vec!["bad".into()],
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(
            resolve_prefix(&bad_file).unwrap_err().to_string(),
            "invalid prefix 'bad'"
        );

        let bad_seed = Assignment {
            seed: "9X".into(),
            ..assignment("a.md", 1, "- [ ] Task")
        };
        assert_eq!(
            resolve_prefix(&bad_seed).unwrap_err().to_string(),
            "invalid prefix '9X'"
        );
    }

    #[test]
    fn header_with_id_rewrites_strict_headers() {
        assert_eq!(
            header_with_id("- [ ] OLD-1 Title", "NEW-2"),
            "- [ ] NEW-2 Title"
        );
        assert_eq!(
            header_with_id("- [ ] SEED- A seed", "SEED-001"),
            "- [ ] SEED-001 A seed"
        );
        assert_eq!(
            header_with_id("- [ ] Task\t\t#tag !high", "PRJ-042"),
            "- [ ] PRJ-042 Task\t\t#tag !high"
        );
        assert_eq!(header_with_id("- [x] Done", "D-1"), "- [x] D-1 Done");
        assert_eq!(
            header_with_id("- [ ] Task\r", "PRJ-001"),
            "- [ ] PRJ-001 Task"
        );
        assert_eq!(header_with_id("not a header", "PRJ-1"), "not a header");
    }

    #[test]
    fn duplicate_numeric_parts_matches_go_table() {
        let distinct = duplicate_numeric_parts(&[task("A-1"), task("B-1")]);
        assert_eq!(
            distinct,
            BTreeMap::from([(1, vec!["A-1".to_string(), "B-1".to_string()])])
        );

        let duplicated = duplicate_numeric_parts(&[task("A-1"), task("A-1")]);
        assert_eq!(
            duplicated,
            BTreeMap::from([(1, vec!["A-1".to_string(), "A-1".to_string()])])
        );

        assert!(duplicate_numeric_parts(&[task("A-1"), task("B-2")]).is_empty());

        let unidentified = Task {
            title: "none".into(),
            ..Task::default()
        };
        assert!(duplicate_numeric_parts(&[unidentified, task("A-1")]).is_empty());
    }
}
