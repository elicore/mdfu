//! Output formatting for filter mode: paths, JSON, vimgrep, and snippets.
//!
//! Ports `internal/output/output.go`. Byte-level parity notes:
//!
//! - [`format_paths`] and [`format_vimgrep`] emit one line per result,
//!   including a trailing newline after the final line.
//! - [`format_json`] uses two-space indentation, Go field order (`path`,
//!   `score`, `title`, `type`, `snippet`), and Go `encoding/json` HTML
//!   escaping: `<`, `>`, and `&` become `\u003c`, `\u003e`, and `\u0026`,
//!   applied as a post-serialization pass over the JSON string.

use serde::{Deserialize, Serialize};

/// Display unit for filter mode.
///
/// Intentionally decoupled from scan, query, and rank: callers map ranked
/// documents to `Result`s before formatting. The serde field order matches the
/// Go struct's JSON tag order (`path`, `score`, `title`, `type`, `snippet`);
/// `doc_type` serializes as `"type"`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Result {
    /// Document file path.
    pub path: String,
    /// Ranking score. The Go CLI currently always emits `0`.
    pub score: f64,
    /// Document title.
    pub title: String,
    /// Document type, serialized as `"type"` for compatibility.
    #[serde(rename = "type")]
    pub doc_type: String,
    /// Single-line body excerpt (see [`snippet`]).
    pub snippet: String,
}

/// Renders one path per line (fzf-compatible), with a newline after each path.
#[must_use]
pub fn format_paths(results: &[Result]) -> String {
    let mut out = String::new();
    for result in results {
        out.push_str(&result.path);
        out.push('\n');
    }
    out
}

/// Renders results as indented JSON (two spaces).
///
/// The JSON string is post-processed exactly like Go `encoding/json` escapes
/// HTML characters: `<`, `>`, and `&` become `\u003c`, `\u003e`, and
/// `\u0026`. Keys and values are never restructured, only escaped.
///
/// # Errors
///
/// Returns the underlying [`serde_json::Error`] if the results cannot be
/// serialized.
#[must_use = "the serialized JSON must be inspected"]
pub fn format_json(results: &[Result]) -> std::result::Result<String, serde_json::Error> {
    let json = serde_json::to_string_pretty(results)?;
    Ok(escape_go_html(&json))
}

/// Applies Go `encoding/json` HTML escaping to a serialized JSON document.
///
/// These three bytes can only occur inside JSON string literals, so a plain
/// character pass over the whole document is safe and cannot alter keys.
fn escape_go_html(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            _ => out.push(ch),
        }
    }
    out
}

/// Renders `path:1:1:title` lines for editor quickfix lists.
#[must_use]
pub fn format_vimgrep(results: &[Result]) -> String {
    let mut out = String::new();
    for result in results {
        out.push_str(&result.path);
        out.push_str(":1:1:");
        out.push_str(&result.title);
        out.push('\n');
    }
    out
}

/// Returns a window of at most `width` runes from `body` containing the
/// earliest case-insensitive occurrence of any of `terms`.
///
/// Whitespace is first collapsed to single spaces. The window is trimmed with
/// `...` ellipsis markers on each truncated side. If no term hits (or `terms`
/// is empty), the leading prefix is returned trimmed with `...`. A
/// non-positive `width` defaults to 120, mirroring Go.
#[must_use]
pub fn snippet(body: &str, terms: &[String], width: i32) -> String {
    let width = if width <= 0 {
        120
    } else {
        usize::try_from(width).unwrap_or(120)
    };
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return String::new();
    }
    let runes: Vec<char> = collapsed.chars().collect();
    if runes.len() <= width {
        return collapsed;
    }

    let lower = collapsed.to_lowercase();
    let mut best: Option<usize> = None;
    for term in terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        if let Some(index) = lower.find(&term.to_lowercase()) {
            if best.is_none_or(|current| index < current) {
                best = Some(index);
            }
        }
    }

    let Some(best) = best else {
        let prefix: String = runes[..width].iter().collect();
        return format!("{prefix}...");
    };

    let best_rune = lower[..best].chars().count();
    let mut start = best_rune.saturating_sub(width / 4);
    let mut end = start + width;
    if end > runes.len() {
        end = runes.len();
        start = end - width;
    }
    let mut out: String = runes[start..end].iter().collect();
    if start > 0 {
        out.insert_str(0, "...");
    }
    if end < runes.len() {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_json, format_paths, format_vimgrep, snippet, Result};

    fn sample(path: &str, title: &str) -> Result {
        Result {
            path: path.to_owned(),
            title: title.to_owned(),
            ..Result::default()
        }
    }

    fn full_sample(
        path: &str,
        score: f64,
        title: &str,
        doc_type: &str,
        snippet_text: &str,
    ) -> Result {
        Result {
            path: path.to_owned(),
            score,
            title: title.to_owned(),
            doc_type: doc_type.to_owned(),
            snippet: snippet_text.to_owned(),
        }
    }

    fn terms(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    #[test]
    fn format_paths_writes_one_line_per_result_including_trailing_newline() {
        let results = [sample("a.md", "A"), sample("sub/b.md", "B")];
        assert_eq!(format_paths(&results), "a.md\nsub/b.md\n");
        assert_eq!(format_paths(&[]), "");
    }

    #[test]
    fn format_json_is_indented_and_round_trips() {
        let results = [
            full_sample("a.md", 1.5, "A", "Note", "hello"),
            full_sample("b.md", 0.5, "B", "Log", "world"),
        ];
        let out = format_json(&results).expect("serialize results");
        assert!(out.contains('\n'), "expected indented JSON: {out}");
        let back: Vec<Result> = serde_json::from_str(&out).expect("round-trip");
        assert_eq!(back.as_slice(), results.as_slice());
    }

    #[test]
    fn format_json_empty_slice_is_empty_array() {
        assert_eq!(format_json(&[]).expect("serialize empty"), "[]");
    }

    #[test]
    fn format_json_preserves_go_key_order() {
        let results = [sample("a.md", "t")];
        let out = format_json(&results).expect("serialize one result");
        let keys = [
            "\"path\"",
            "\"score\"",
            "\"title\"",
            "\"type\"",
            "\"snippet\"",
        ];
        let mut previous = 0;
        for key in keys {
            let index = out.find(key).expect("key present");
            assert!(index >= previous, "key {key} out of order in: {out}");
            previous = index;
        }
    }

    #[test]
    fn format_json_escapes_html_like_go() {
        let results = [full_sample(
            "a.md",
            1.0,
            "<b>&</b>",
            "Note",
            "x < y & z > w",
        )];
        let out = format_json(&results).expect("serialize escaped result");
        let back: Vec<Result> = serde_json::from_str(&out).expect("escaped JSON still parses");
        assert_eq!(back[0].title, "<b>&</b>");
        assert!(
            out.contains(r"\u003cb\u003e\u0026\u003c/b\u003e"),
            "expected Go-style \\uXXXX escapes, got: {out}"
        );
        assert!(!out.contains('<') && !out.contains('>') && !out.contains('&'));
    }

    #[test]
    fn format_vimgrep_writes_path_line_column_title() {
        let results = [sample("a.md", "Hello A"), sample("sub/b.md", "Hello B")];
        assert_eq!(
            format_vimgrep(&results),
            "a.md:1:1:Hello A\nsub/b.md:1:1:Hello B\n"
        );
        assert_eq!(format_vimgrep(&[]), "");
    }

    #[test]
    fn format_vimgrep_does_not_escape_html() {
        let results = [sample("a.md", "<b>&</b>")];
        assert_eq!(format_vimgrep(&results), "a.md:1:1:<b>&</b>\n");
    }

    #[test]
    fn snippet_centers_window_on_first_hit_with_ellipses() {
        let body = "This is a long document body that mentions Golang somewhere in the middle of all this text for testing.";
        let got = snippet(body, &terms(&["golang"]), 30);
        assert!(got.to_lowercase().contains("golang"), "got {got:?}");
        assert!(
            got.starts_with("..."),
            "expected leading ellipsis, got {got:?}"
        );
        assert!(
            got.ends_with("..."),
            "expected trailing ellipsis, got {got:?}"
        );
        assert!(got.chars().count() <= 30 + 2 * 3, "got {got:?}");
    }

    #[test]
    fn snippet_picks_earliest_term_regardless_of_term_order() {
        let body = "aaaaaaaaaa golang bbbbbbbbbb rust cccccccccc dddddddddd";
        let got = snippet(body, &terms(&["rust", "golang"]), 20);
        assert!(got.contains("golang"), "got {got:?}");
        assert!(!got.contains("rust"), "got {got:?}");
    }

    #[test]
    fn snippet_miss_returns_trimmed_prefix() {
        let body = "abcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(snippet(body, &terms(&["zzz"]), 10), "abcdefghij...");
    }

    #[test]
    fn snippet_empty_terms_returns_trimmed_prefix() {
        let body = "abcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(snippet(body, &[], 10), "abcdefghij...");
    }

    #[test]
    fn snippet_short_and_empty_bodies_pass_through() {
        assert_eq!(snippet("hi", &terms(&["hi"]), 10), "hi");
        assert_eq!(snippet("", &terms(&["hi"]), 10), "");
    }

    #[test]
    fn snippet_collapses_whitespace_to_single_spaces() {
        assert_eq!(snippet("  hello\n\tworld  ", &[], 120), "hello world");
    }

    #[test]
    fn snippet_non_positive_width_defaults_to_120() {
        let body = "x".repeat(200);
        let prefix = "x".repeat(120);
        let expected = format!("{prefix}...");
        assert_eq!(snippet(&body, &[], 0), expected);
        assert_eq!(snippet(&body, &[], -5), expected);
        assert_eq!(snippet(&body, &[], 120), expected);
    }

    #[test]
    fn snippet_counts_width_in_runes_not_bytes() {
        let body = "é".repeat(20);
        let prefix = "é".repeat(10);
        assert_eq!(snippet(&body, &[], 10), format!("{prefix}..."));
        assert_eq!(snippet(&body, &[], 10).chars().count(), 13);
    }
}
