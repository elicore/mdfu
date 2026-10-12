//! Hand-rolled `**` glob subset used by task include/exclude filters.
//!
//! Port of the Go `internal/task/glob.go` matcher. Third-party matchers were
//! evaluated and rejected, matching the Go decision:
//!
//! * `doublestar` was rejected in Go to keep the direct dependency set small;
//! * `globset` is already transitive through `ignore`, but its matcher violates
//!   SPEC.md §D rule 3 — it treats `?` as a single-character wildcard and
//!   `[...]` as a character class, while the task subset requires both to be
//!   literal. `globset` also gives `archive/**` its own zero-or-more-segments
//!   reading. The four rules below are therefore ported by hand.
//!
//! Exactly four rules apply (SPEC.md §D):
//!
//! 1. `**` matches zero or more path segments. `**/*.md` matches `a.md`,
//!    `x/a.md`, and `x/y/z.md`.
//! 2. `*` matches any run of characters other than the path separator `/`.
//!    `*.md` matches `a.md` but not `x/a.md`.
//! 3. `?` and `[...]` are literal characters; there is no single-character
//!    wildcard and no character class.
//! 4. A pattern ending in `/**` requires at least one following segment:
//!    `archive/**` matches `archive/a.md` but not `archive` itself.
//!
//! A pattern that contains no separator matches at any depth only through the
//! `**` rule; there is no implicit prefix matching.

/// Reports whether `name` matches `pattern` under the four SPEC.md §D rules.
///
/// Both arguments are slash-separated paths. Byte-wise comparison, mirroring
/// Go's byte-at-a-time matcher.
#[must_use]
pub fn matches(pattern: &str, name: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let seg: Vec<&str> = name.split('/').collect();
    // Rule 4: only a terminal `**` that follows at least one other segment is
    // forced to consume a segment. A bare `**` still matches zero or more.
    let force_trailing = pat.len() > 1 && pat[pat.len() - 1] == "**";
    match_segments(&pat, &seg, force_trailing)
}

/// Matches the remaining pattern segments against the remaining name segments.
/// A `**` segment consumes zero or more name segments via backtracking.
fn match_segments(pat: &[&str], name: &[&str], force_trailing: bool) -> bool {
    let Some((&first, rest_pat)) = pat.split_first() else {
        return name.is_empty();
    };
    if first == "**" {
        if rest_pat.is_empty() && force_trailing {
            return !name.is_empty();
        }
        return (0..=name.len()).any(|k| match_segments(rest_pat, &name[k..], force_trailing));
    }
    let Some((&first_name, rest_name)) = name.split_first() else {
        return false;
    };
    match_segment(first, first_name) && match_segments(rest_pat, rest_name, force_trailing)
}

/// Matches a single pattern segment against a single name segment.
///
/// The only wildcard is `*`, which matches any run of bytes; `?` and bracket
/// characters are literal.
fn match_segment(pattern: &str, name: &str) -> bool {
    let pat = pattern.as_bytes();
    let text = name.as_bytes();
    let mut pi = 0_usize;
    let mut si = 0_usize;
    let mut star: Option<usize> = None;
    let mut matched = 0_usize;
    while si < text.len() {
        if pi < pat.len() && pat[pi] != b'*' && pat[pi] == text[si] {
            pi += 1;
            si += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star = Some(pi);
            matched = si;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            matched += 1;
            si = matched;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::matches;

    /// The Go `TestGlobMatch` table, case-for-case.
    #[test]
    fn matches_go_glob_table() {
        let cases: &[(&str, &str, bool)] = &[
            ("**/*.md", "a.md", true),
            ("**/*.md", "x/a.md", true),
            ("**/*.md", "x/y/z.md", true),
            ("src/**/*.md", "src/a.md", true),
            ("src/**/*.md", "src/x/y/a.md", true),
            ("src/**/*.md", "a.md", false),
            ("*.md", "a.md", true),
            ("*.md", "x/a.md", false),
            ("a/*.md", "a/b.md", true),
            ("a/*.md", "a/b/c.md", false),
            ("*.md", ".md", true),
            ("a?.md", "a?.md", true),
            ("a?.md", "ab.md", false),
            ("[ab].md", "[ab].md", true),
            ("[ab].md", "a.md", false),
            ("archive/**", "archive", false),
            ("archive/**", "archive/a.md", true),
            ("archive/**", "archive/a/b.md", true),
            ("**", "a", true),
            ("**", "a/b/c", true),
            ("a.md", "a.md", true),
            ("a.md", "b.md", false),
            ("a.md", "x/a.md", false),
        ];
        for &(pattern, target, want) in cases {
            assert_eq!(
                matches(pattern, target),
                want,
                "matches({pattern:?}, {target:?})"
            );
        }
    }

    /// Rule 1 + rule 4 together: a bare `**` matches zero segments, so `**/*.md`
    /// matches a top-level file, while a trailing `/**` never matches its own
    /// prefix directory.
    #[test]
    fn doublestar_zero_or_more_and_trailing_rules() {
        assert!(matches("x/**/*.md", "x/a.md"));
        assert!(matches("**/**/*.md", "a.md"));
        assert!(matches("archive/**", "archive/sub/deep.md"));
        assert!(!matches("archive/**", "archive"));
        assert!(!matches("archive/**", "archived/a.md"));
    }

    /// Rule 2: `*` never crosses `/`, and a segment ending in `*` cannot reach
    /// into a subdirectory.
    #[test]
    fn asterisk_never_crosses_separator() {
        assert!(matches("a/*", "a/b"));
        assert!(!matches("a/*", "a/b/c"));
        assert!(!matches("*", "a/b"));
        assert!(matches("*-*.md", "one-two.md"));
    }

    /// Rule 3: `?` and `[...]` are literal, never wildcards or classes.
    #[test]
    fn question_and_brackets_are_literal() {
        assert!(matches("file?.md", "file?.md"));
        assert!(!matches("file?.md", "file1.md"));
        assert!(matches("file[1].md", "file[1].md"));
        assert!(!matches("file[1].md", "file1.md"));
    }
}
