//! Task body extraction and fence masking.
//!
//! Port of `internal/task/body.go` plus the broadened range from
//! `internal/task/checkbox.go` (`blockRangeAny`). A body is the maximal run of
//! indented lines after a header, stored dedented by the minimum indent of its
//! non-empty lines. Bodies are opaque: a body-shaped header line stays body
//! text and is never re-parsed as a task.

use super::grammar::{parse_checkbox, parse_header};

/// Reports, for every line, whether it lies inside a fenced code block.
///
/// The opening fence is a line with at most three leading spaces followed by
/// three or more backticks or tildes; the closing fence is a line with at most
/// three leading spaces and at least as many of the same character. An unclosed
/// fence masks through end of file. The mask gates both header detection and
/// body extraction.
#[must_use]
pub fn fence_mask(lines: &[String]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut open_char = 0_u8;
    let mut open_len = 0_usize;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if open_char == 0 {
            if let Some((ch, n)) = opening_fence(line) {
                open_char = ch;
                open_len = n;
                mask[i] = true;
            }
            continue;
        }
        mask[i] = true;
        if let Some(n) = closing_fence(line, open_char) {
            if n >= open_len {
                open_char = 0;
                open_len = 0;
            }
        }
    }
    mask
}

/// Recognizes an opening code fence and returns its character and run length.
fn opening_fence(line: &str) -> Option<(u8, usize)> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    if rest.starts_with("```") {
        Some((b'`', run_length(rest, b'`')))
    } else if rest.starts_with("~~~") {
        Some((b'~', run_length(rest, b'~')))
    } else {
        None
    }
}

/// Recognizes a closing fence for `ch` and returns its run length.
fn closing_fence(line: &str, ch: u8) -> Option<usize> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    if rest.as_bytes().first() != Some(&ch) {
        return None;
    }
    let run = run_length(rest, ch);
    if run < 3 {
        return None;
    }
    Some(run)
}

/// Counts the leading run of spaces in `line` (fence indentation only counts
/// spaces, never tabs).
fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|&byte| byte == b' ').count()
}

/// Counts the leading run of `ch` in `s`.
fn run_length(s: &str, ch: u8) -> usize {
    s.bytes().take_while(|&byte| byte == ch).count()
}

/// Returns the end of the strict task block that starts at the header line
/// index `start`: the index one past its last body line.
///
/// Returns `None` when `start` is out of range, fence-masked, or not a strict
/// header. A trailing run of blank lines is included only when an indented
/// body line follows it.
#[must_use]
pub fn block_range(lines: &[String], start: usize) -> Option<usize> {
    if start >= lines.len() || fence_mask(lines)[start] {
        return None;
    }
    parse_header(lines[start].as_str())?;

    let mut last_indented: Option<usize> = None;
    for (i, raw) in lines.iter().enumerate().skip(start + 1) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if blank(line) {
            continue;
        }
        if indented(line) {
            last_indented = Some(i);
            continue;
        }
        break;
    }
    Some(last_indented.map_or(start + 1, |i| i + 1))
}

/// Returns the end of the broadened task block that starts at the checkbox
/// line index `start`.
///
/// This is [`block_range`] for the broadened view: it ends the block at the
/// next checkbox line at any indent, so a nested checklist item is its own
/// item rather than body text of its parent. Non-checkbox indented lines
/// remain body, and fenced code is masked exactly as in [`block_range`].
#[must_use]
pub fn block_range_any(lines: &[String], start: usize) -> Option<usize> {
    if start >= lines.len() {
        return None;
    }
    let mask = fence_mask(lines);
    if mask[start] {
        return None;
    }
    parse_checkbox(lines[start].as_str())?;

    let mut last_indented: Option<usize> = None;
    for (i, raw) in lines.iter().enumerate().skip(start + 1) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if blank(line) {
            continue;
        }
        if !mask[i] && parse_checkbox(line).is_some() {
            break;
        }
        if indented(line) {
            last_indented = Some(i);
            continue;
        }
        break;
    }
    Some(last_indented.map_or(start + 1, |i| i + 1))
}

/// Dedents the body lines in `lines[start + 1..end]` by the minimum indent of
/// the non-blank lines and returns the dedented text and that indent.
///
/// Returns two empty strings when the range is invalid (`end` not past
/// `start + 1`), out of bounds, or contains no non-blank line. Blank lines
/// become empty output lines, and no trailing newline is appended.
#[must_use]
pub fn collect_body(lines: &[String], start: usize, end: usize) -> (String, String) {
    if start >= lines.len() || end > lines.len() || end <= start + 1 {
        return (String::new(), String::new());
    }
    let body = &lines[start + 1..end];

    let mut indent = String::new();
    let mut found = false;
    for raw in body {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if blank(line) {
            continue;
        }
        let whitespace = leading_whitespace(line);
        if !found || whitespace.len() < indent.len() {
            indent.clear();
            indent.push_str(whitespace);
            found = true;
        }
    }
    if !found {
        return (String::new(), String::new());
    }

    let dedented: Vec<&str> = body
        .iter()
        .map(|raw| {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if blank(line) {
                return "";
            }
            if let Some(stripped) = line.strip_prefix(indent.as_str()) {
                stripped
            } else {
                trim_leading_whitespace(line, indent.len())
            }
        })
        .collect();
    (dedented.join("\n"), indent)
}

/// Reports whether a line contains only spaces and tabs.
fn blank(line: &str) -> bool {
    line.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

/// Reports whether a non-blank line starts with an indent character.
fn indented(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// Returns the leading run of spaces and tabs in `line`.
fn leading_whitespace(line: &str) -> &str {
    let len = line
        .bytes()
        .take_while(|&byte| matches!(byte, b' ' | b'\t'))
        .count();
    &line[..len]
}

/// Removes at most `n` leading spaces or tabs from `line`.
fn trim_leading_whitespace(line: &str, n: usize) -> &str {
    let mut i = 0;
    while i < line.len() && i < n && matches!(line.as_bytes()[i], b' ' | b'\t') {
        i += 1;
    }
    &line[i..]
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata/tasks");

    fn lines_of(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|line| (*line).to_string()).collect()
    }

    fn fixture_lines(name: &str) -> Vec<String> {
        let path = format!("{FIXTURE_DIR}/{name}");
        let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("read {path}: {err}"));
        String::from_utf8_lossy(&bytes)
            .split('\n')
            .map(str::to_string)
            .collect()
    }

    struct FenceCase {
        name: &'static str,
        lines: &'static [&'static str],
        want: &'static [bool],
        hidden: usize,
    }

    #[test]
    fn fence_mask_matches_go_table() {
        let cases: &[FenceCase] = &[
            FenceCase {
                name: "backtick fence",
                lines: &[
                    "# Title",
                    "```",
                    "- [ ] A-1 Not a task",
                    "```",
                    "- [ ] A-2 Real",
                ],
                want: &[false, true, true, true, false],
                hidden: 1,
            },
            FenceCase {
                name: "tilde fence",
                lines: &["~~~", "text", "~~~", "- [ ] A-1 Real"],
                want: &[true, true, true, false],
                hidden: 0,
            },
            FenceCase {
                name: "unclosed fence masks to end of file",
                lines: &["```", "- [ ] A-1 Hidden", "- [ ] A-2 Hidden"],
                want: &[true, true, true],
                hidden: 2,
            },
            FenceCase {
                name: "up to three leading spaces opens",
                lines: &["   ```", "x", "   ```"],
                want: &[true, true, true],
                hidden: 0,
            },
            FenceCase {
                name: "four leading spaces is not a fence",
                lines: &["    ```", "x"],
                want: &[false, false],
                hidden: 0,
            },
            FenceCase {
                name: "closing run must be at least as long",
                lines: &["````", "x", "```", "y", "````"],
                want: &[true, true, true, true, true],
                hidden: 0,
            },
            FenceCase {
                name: "shorter opposite fence does not close",
                lines: &["```", "~~~", "```"],
                want: &[true, true, true],
                hidden: 0,
            },
            FenceCase {
                name: "info string on opening fence",
                lines: &["```text", "- [ ] A-1", "```", "after"],
                want: &[true, true, true, false],
                hidden: 1,
            },
            FenceCase {
                name: "carriage returns are stripped before detection",
                lines: &["```\r", "- [ ] A-1\r", "```\r", "- [ ] A-2"],
                want: &[true, true, true, false],
                hidden: 1,
            },
        ];
        for case in cases {
            let name = case.name;
            let lines = lines_of(case.lines);
            let mask = fence_mask(&lines);
            assert_eq!(mask, case.want, "{name}: mask");
            let hidden = lines
                .iter()
                .zip(mask.iter())
                .filter(|(line, masked)| **masked && parse_header(line.as_str()).is_some())
                .count();
            assert_eq!(hidden, case.hidden, "{name}: masked header-shaped lines");
        }
    }

    #[test]
    fn spec_fenced_fixture_has_no_tasks_inside_fences() {
        let lines = fixture_lines("spec-fenced.md");
        let mask = fence_mask(&lines);

        let masked_headers = lines
            .iter()
            .zip(mask.iter())
            .filter(|(line, masked)| **masked && parse_header(line.as_str()).is_some())
            .count();
        assert_eq!(masked_headers, 3, "FENCED-1, FENCED-2, FENCED-3 are hidden");

        let tasks: Vec<String> = lines
            .iter()
            .zip(mask.iter())
            .filter(|(_, masked)| !**masked)
            .filter_map(|(line, _)| parse_header(line.as_str()))
            .map(|task| task.id)
            .collect();
        assert_eq!(
            tasks,
            ["FENCE-9"],
            "only the task after both fences remains"
        );

        let start = lines
            .iter()
            .position(|line| line.starts_with("- [ ] FENCE-9"))
            .expect("FENCE-9 header line");
        let end = block_range(&lines, start).expect("strict block for FENCE-9");
        let (body, indent) = collect_body(&lines, start, end);
        assert_eq!(indent, "  ");
        assert_eq!(
            body,
            "It sits after both fences and must be the single block we see."
        );
    }

    #[test]
    fn unclosed_fence_masks_through_end_of_file() {
        let lines = lines_of(&["```", "- [ ] A-1 Hidden", "- [ ] A-2 Hidden"]);
        assert_eq!(fence_mask(&lines), [true, true, true]);
        assert_eq!(block_range(&lines, 1), None);
        assert_eq!(block_range_any(&lines, 1), None);
    }

    struct BodyCase {
        name: &'static str,
        lines: &'static [&'static str],
        start: usize,
        want_end: Option<usize>,
        want_body: &'static str,
        want_indent: &'static str,
    }

    #[test]
    fn block_range_matches_go_table() {
        let cases: &[BodyCase] = &[
            BodyCase {
                name: "two line body",
                lines: &["- [ ] A-1 Title", "  one", "  two", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(3),
                want_body: "one\ntwo",
                want_indent: "  ",
            },
            BodyCase {
                name: "blank line inside body",
                lines: &["- [ ] A-1 Title", "  one", "", "  two", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(4),
                want_body: "one\n\ntwo",
                want_indent: "  ",
            },
            BodyCase {
                name: "whitespace-only line is a blank line",
                lines: &["- [ ] A-1 Title", "  one", "   ", "  two"],
                start: 0,
                want_end: Some(4),
                want_body: "one\n\ntwo",
                want_indent: "  ",
            },
            BodyCase {
                name: "trailing blank before non-indented is not body",
                lines: &["- [ ] A-1 Title", "  one", "", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(2),
                want_body: "one",
                want_indent: "  ",
            },
            BodyCase {
                name: "no body",
                lines: &["- [ ] A-1 Title", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(1),
                want_body: "",
                want_indent: "",
            },
            BodyCase {
                name: "deep indent dedents to the minimum",
                lines: &["- [ ] A-1 Title", "    four", "  two", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(3),
                want_body: "  four\ntwo",
                want_indent: "  ",
            },
            BodyCase {
                name: "mixed tab and space indents count bytes",
                lines: &["- [ ] A-1 Title", "\tone", "  two", "- [ ] A-2 Next"],
                start: 0,
                want_end: Some(3),
                want_body: "one\n two",
                want_indent: "\t",
            },
        ];
        for case in cases {
            let name = case.name;
            let lines = lines_of(case.lines);
            let end = block_range(&lines, case.start);
            assert_eq!(end, case.want_end, "{name}: block range");
            let Some(end) = end else { continue };
            let (body, indent) = collect_body(&lines, case.start, end);
            assert_eq!(body, case.want_body, "{name}: body");
            assert_eq!(indent, case.want_indent, "{name}: indent");
        }
    }

    #[test]
    fn block_range_handles_carriage_returns_and_rejections() {
        let crlf = lines_of(&[
            "- [ ] A-1 Title\r",
            "  one\r",
            "  two\r",
            "- [ ] A-2 Next\r",
        ]);
        assert_eq!(block_range(&crlf, 0), Some(3));
        let (body, indent) = collect_body(&crlf, 0, 3);
        assert_eq!(body, "one\ntwo");
        assert_eq!(indent, "  ");

        let rejects: &[(&str, &[&str], usize)] = &[
            ("out of range", &["- [ ] A-1 Title"], 5),
            ("not a header", &["plain", "  body"], 0),
            (
                "fence masked start",
                &["```", "- [ ] A-1 Title", "  body", "```"],
                1,
            ),
        ];
        for (name, raw, start) in rejects {
            let lines = lines_of(raw);
            assert_eq!(block_range(&lines, *start), None, "{name}");
        }
    }

    #[test]
    fn collect_body_guards_match_go() {
        let lines = lines_of(&["- [ ] A-1", "  body"]);
        let empty = (String::new(), String::new());
        assert_eq!(collect_body(&lines, 0, 1), empty, "end not past start + 1");
        assert_eq!(collect_body(&lines, 0, 3), empty, "end out of bounds");
        assert_eq!(collect_body(&lines, 5, 6), empty, "start out of bounds");

        let blanks = lines_of(&["- [ ] A-1", "  ", "\t"]);
        assert_eq!(collect_body(&blanks, 0, 2), empty, "no non-blank line");
    }

    #[test]
    fn collect_body_appends_no_trailing_newline() {
        let eof_body = lines_of(&["- [ ] A-1 Title", "  one", "  two"]);
        let end = block_range(&eof_body, 0).expect("header at 0");
        let (body, _) = collect_body(&eof_body, 0, end);
        assert_eq!(body, "one\ntwo");
        assert!(!body.ends_with('\n'), "no trailing newline is added");

        // A trailing newline in the file becomes a final empty element, which
        // is not part of the block.
        let trailing = lines_of(&["- [ ] A-1 Title", "  one", ""]);
        let end = block_range(&trailing, 0).expect("header at 0");
        assert_eq!(end, 2);
        assert_eq!(collect_body(&trailing, 0, end).0, "one");

        let bare = lines_of(&["- [ ] A-1 Title"]);
        let end = block_range(&bare, 0).expect("header at 0");
        assert_eq!(end, 1);
        assert_eq!(collect_body(&bare, 0, end), (String::new(), String::new()));
    }

    #[test]
    fn dedent_then_reindent_round_trips_the_block() {
        let original = [
            "    four spaces",
            "  two spaces",
            "",
            "    deeper",
            "  trailing",
        ];
        let mut lines: Vec<String> = vec!["- [ ] RT-1 Round trip".to_string()];
        lines.extend(original.iter().map(|line| (*line).to_string()));
        lines.push("- [ ] RT-2 Next".to_string());

        let end = block_range(&lines, 0).expect("header at 0");
        assert_eq!(end, 6);
        let (body, indent) = collect_body(&lines, 0, end);
        assert_eq!(indent, "  ");
        assert_eq!(body, "  four spaces\ntwo spaces\n\n  deeper\ntrailing");

        let reindented: Vec<String> = body
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{indent}{line}")
                }
            })
            .collect();
        assert_eq!(reindented, original.as_slice());
    }

    #[test]
    fn indented_header_shaped_line_stays_body() {
        let lines = lines_of(&[
            "- [ ] A-1 Parent",
            "  - [ ] A-9 Not a task",
            "    deeper",
            "- [ ] A-2 Next",
        ]);
        let end = block_range(&lines, 0).expect("strict header at 0");
        assert_eq!(end, 3, "the indented header-shaped line is body");
        let (body, indent) = collect_body(&lines, 0, end);
        assert_eq!(body, "- [ ] A-9 Not a task\n  deeper");
        assert_eq!(indent, "  ");
        assert!(
            parse_header(&lines[1]).is_none(),
            "the body line is never re-parsed as a strict task"
        );
    }

    #[test]
    fn broad_range_ends_at_nested_checkbox() {
        let lines = lines_of(&[
            "- [ ] parent",
            "  parent note",
            "  - [ ] child",
            "    child note",
            "- [ ] sibling",
        ]);

        let parent_end = block_range_any(&lines, 0).expect("parent checkbox");
        assert_eq!(
            parent_end, 2,
            "a nested checkbox must not be absorbed as the parent's body"
        );
        assert_eq!(collect_body(&lines, 0, parent_end).0, "parent note");

        let child_end = block_range_any(&lines, 2).expect("child checkbox");
        assert_eq!(child_end, 4);
        let (body, indent) = collect_body(&lines, 2, child_end);
        assert_eq!(body, "child note");
        assert_eq!(indent, "    ");

        let sibling_end = block_range_any(&lines, 4).expect("sibling checkbox");
        assert_eq!(sibling_end, 5);
        assert_eq!(
            collect_body(&lines, 4, sibling_end),
            (String::new(), String::new())
        );
    }

    #[test]
    fn broad_range_keeps_non_checkbox_indented_lines() {
        let lines = lines_of(&["- [ ] parent", "  prose", "  more prose", "- [ ] next"]);
        let end = block_range_any(&lines, 0).expect("parent checkbox");
        assert_eq!(end, 3);
        assert_eq!(collect_body(&lines, 0, end).0, "prose\nmore prose");
    }

    #[test]
    fn broad_range_ignores_masked_checkboxes() {
        let lines = lines_of(&[
            "- [ ] real",
            "  ```",
            "  - [ ] fake",
            "  ```",
            "  trailing note",
            "- [ ] next",
        ]);
        let end = block_range_any(&lines, 0).expect("real checkbox");
        assert_eq!(end, 5, "a fenced checkbox is body, not a new item");
        let (body, indent) = collect_body(&lines, 0, end);
        assert_eq!(indent, "  ");
        assert_eq!(body, "```\n- [ ] fake\n```\ntrailing note");
    }

    #[test]
    fn broad_range_rejects_masked_start() {
        let lines = lines_of(&["```", "- [ ] hidden", "```"]);
        assert_eq!(block_range_any(&lines, 1), None);
    }

    #[test]
    fn broad_range_accepts_any_bullet_and_indent() {
        let lines = lines_of(&["\t* [ ] star", "\t\tbody"]);
        let end = block_range_any(&lines, 0).expect("broad checkbox");
        assert_eq!(end, 2);
        assert_eq!(collect_body(&lines, 0, end).0, "body");
    }
}
