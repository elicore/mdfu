//! Header grammar: parsing checkbox lines into [`Task`] values and rendering
//! them back.
//!
//! Port of `internal/task/format.go` and the grammar half of
//! `internal/task/checkbox.go`. [`parse_header`] is the frozen strict grammar
//! from `SPEC.md` §A (column 0, `-` bullet, lowercase `x`); [`parse_checkbox`]
//! is the broadened grammar the task browser uses (any indent, `-`/`*`/`+`,
//! either case). Both share one core parser behind a strictness flag, so the
//! strict output stays byte-identical while the broad output is a superset.
//!
//! The Go token regexes use ASCII character classes; the `regex` crate widens
//! `\w`/`\d`/`\s` to Unicode, so the patterns below spell the ASCII classes out
//! (`[0-9A-Za-z_]`, `[0-9]`, `[^\t\n\f\r ]`) to keep byte-level parity.
//!
//! Bodies are opaque: this module never looks past the header line, so a
//! task-shaped line inside a body is never re-parsed here.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use super::Task;

/// Strict header grammar (`SPEC.md` §A): column 0, `-` bullet, `[ ]`/`[x]`.
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^- \[([ x])\] ((?:[A-Z][A-Z0-9]*-[0-9]+ )?)(.*)$").expect("valid header regex")
});

/// Broadened checkbox grammar: optional indent, `-`/`*`/`+`, either case.
static CHECKBOX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)([-*+]) \[([ xX])\] (.*)$").expect("valid checkbox regex")
});

/// Seed grammar: `PREFIX- ` with no numeric part.
static SEED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z][A-Z0-9]*)- (.*)$").expect("valid seed regex"));

/// ID prefix: `PREFIX-<n> ` immediately after the checkbox.
static ID_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z][A-Z0-9]*-[0-9]+ )").expect("valid id prefix regex"));

/// Tag token: `#` then a letter then word characters, underscores, or dashes.
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#([A-Za-z][0-9A-Za-z_-]*)$").expect("valid tag regex"));

/// Priority token: `!` then a letter then word characters or underscores.
static PRIORITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^!([A-Za-z][0-9A-Za-z_]*)$").expect("valid priority regex"));

/// Property token: `@key:value` with a non-whitespace value.
static PROPERTY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@([0-9A-Za-z_-]+):([^\t\n\f\r ]+)$").expect("valid property regex")
});

/// Which checkbox grammar a parse applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strictness {
    /// Frozen `SPEC.md` grammar: column 0, `-` bullet, lowercase `x` only.
    Strict,
    /// Broadened grammar used by the task browser.
    Broad,
}

/// Parses one strict header line (`SPEC.md` §A).
///
/// A single trailing carriage return is stripped first, so CRLF input parses.
/// The line must start at column 0 with `- [ ]` or `- [x]`, optionally
/// followed by a `PREFIX-<n> ` ID; the remainder is split into title and
/// metadata. An uppercase `X`, leading indentation, or any line that is not a
/// header returns `None`.
#[must_use = "a line that is not a strict header parses to `None`"]
pub fn parse_header(line: &str) -> Option<Task> {
    parse_with(line, Strictness::Strict)
}

/// Parses one broadened checkbox line.
///
/// A superset of [`parse_header`]: any indentation, any of the `-`/`*`/`+`
/// bullets, and `[ ]`/`[x]`/`[X]`. The returned task has [`Task::broad`] set.
/// The checkbox must still be followed by a space, so `- [ ]x` is not an item.
#[must_use = "a line that is not a checkbox parses to `None`"]
pub fn parse_checkbox(line: &str) -> Option<Task> {
    parse_with(line, Strictness::Broad)
}

/// Flips a broadened checkbox between unchecked and checked.
///
/// The indent, bullet, and trailing text are preserved verbatim. Returns
/// `None` when `line` is not a checkbox item, so `- [ ]` (no trailing space)
/// does not flip.
#[must_use = "a line that is not a checkbox flips to `None`"]
pub fn flip_checkbox_any(line: &str) -> Option<String> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let caps = CHECKBOX_RE.captures(line)?;
    let indent = caps.get(1)?.as_str();
    let bullet = caps.get(2)?.as_str();
    let status = *caps.get(3)?.as_str().as_bytes().first()?;
    let rest = caps.get(4)?.as_str();
    let flip = if matches!(status, b'x' | b'X') {
        ' '
    } else {
        'x'
    };
    Some(format!("{indent}{bullet} [{flip}] {rest}"))
}

/// One parser behind both grammars; `strictness` picks the opening regex.
fn parse_with(line: &str, strictness: Strictness) -> Option<Task> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let (status, rest, id) = match strictness {
        Strictness::Strict => {
            let caps = HEADER_RE.captures(line)?;
            let status = *caps.get(1)?.as_str().as_bytes().first()?;
            let id = caps
                .get(2)
                .map(|capture| capture.as_str().trim())
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            (status, caps.get(3)?.as_str(), id)
        }
        Strictness::Broad => {
            let caps = CHECKBOX_RE.captures(line)?;
            let status = *caps.get(3)?.as_str().as_bytes().first()?;
            let raw_rest = caps.get(4)?.as_str();
            let (rest, id) = match ID_PREFIX_RE.find(raw_rest) {
                Some(capture) => {
                    let matched = capture.as_str();
                    (&raw_rest[matched.len()..], Some(matched.trim().to_string()))
                }
                None => (raw_rest, None),
            };
            (status, rest, id)
        }
    };

    let mut task = Task {
        status,
        header_raw: line.to_string(),
        broad: strictness == Strictness::Broad,
        checked: matches!(status, b'x' | b'X'),
        ..Task::default()
    };

    let mut rest = rest;
    if let Some(id) = id {
        task.id = id;
    } else if let Some(caps) = SEED_RE.captures(rest) {
        task.seeded = true;
        task.seed_prefix = caps.get(1)?.as_str().to_string();
        rest = caps.get(2)?.as_str();
    } else {
        task.unidentified = true;
    }

    let (title, meta, sep) = split_meta_sep(rest);
    task.title = title;
    task.meta_sep = sep;
    let (tags, priority, properties, property_order) = parse_tokens(&meta);
    task.tags = tags;
    task.priority = priority;
    task.properties = properties;
    task.property_order = property_order;
    Some(task)
}

/// Splits a header remainder into title, metadata, and the exact separator.
///
/// A literal two-tab sequence wins (split at the first occurrence); otherwise
/// a trailing run of metadata tokens is peeled from the right, stopping at the
/// first non-token word — which is why `#123` stays in a title. The returned
/// separator is the verbatim whitespace between title and metadata, so
/// [`render_header`] can reproduce the source line.
#[must_use]
pub fn split_meta_sep(rest: &str) -> (String, String, String) {
    if let Some(idx) = rest.find("\t\t") {
        let title = rest[..idx].trim_end_matches([' ', '\t']);
        let meta = rest[idx + 2..].trim_start_matches([' ', '\t']);
        let sep = &rest[title.len()..rest.len() - meta.len()];
        return (title.to_string(), meta.to_string(), sep.to_string());
    }

    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let mut first_meta = fields.len();
    while first_meta > 0 && is_meta_token(fields[first_meta - 1]) {
        first_meta -= 1;
    }
    if first_meta == fields.len() {
        return (
            rest.trim_end_matches([' ', '\t']).to_string(),
            String::new(),
            String::new(),
        );
    }

    let meta = fields[first_meta..].join(" ");
    let start = field_start(rest, first_meta);
    let title = rest[..start].trim_end_matches([' ', '\t']);
    let sep = &rest[title.len()..start];
    (title.to_string(), meta, sep.to_string())
}

/// Extracts the metadata tokens from `meta`.
///
/// Returns tags (without `#`), the priority (without `!`, keeping out-of-set
/// values for the validator), properties, and property keys in first-seen
/// order. A word matching no token form is ignored here; reporting it is a
/// validate concern.
#[must_use]
pub fn parse_tokens(meta: &str) -> (Vec<String>, String, BTreeMap<String, String>, Vec<String>) {
    let mut tags = Vec::new();
    let mut priority = String::new();
    let mut properties = BTreeMap::new();
    let mut property_order = Vec::new();

    for word in meta.split_whitespace() {
        if let Some(caps) = TAG_RE.captures(word) {
            tags.push(caps[1].to_string());
        } else if let Some(caps) = PRIORITY_RE.captures(word) {
            priority = caps[1].to_string();
        } else if let Some(caps) = PROPERTY_RE.captures(word) {
            let key = caps[1].to_string();
            let value = caps[2].to_string();
            if !properties.contains_key(&key) {
                property_order.push(key.clone());
            }
            properties.insert(key, value);
        }
    }

    (tags, priority, properties, property_order)
}

/// Reports whether `word` is a tag, priority, or property token.
#[must_use]
pub fn is_meta_token(word: &str) -> bool {
    TAG_RE.is_match(word) || PRIORITY_RE.is_match(word) || PROPERTY_RE.is_match(word)
}

/// Reports whether `word` is a tag token (`#` then a letter then word
/// characters).
#[must_use]
pub fn is_tag_token(word: &str) -> bool {
    TAG_RE.is_match(word)
}

/// Reports whether `word` is a priority token (`!` then a letter then word
/// characters).
#[must_use]
pub fn is_priority_token(word: &str) -> bool {
    PRIORITY_RE.is_match(word)
}

/// Returns the key of a property token (`@key:value`), or `None` when `word`
/// is not one.
#[must_use]
pub fn property_key_of(word: &str) -> Option<String> {
    PROPERTY_RE.captures(word).map(|caps| caps[1].to_string())
}

/// Splits a strict header line into the Go `headerRe` capture groups:
/// `(checkbox status, ID prefix with its trailing space, remainder)`.
///
/// The line is matched verbatim (no trailing `\r` is stripped first, unlike
/// [`parse_header`]), because the `set` and `validate` command layers rewrite
/// or report the raw source text.
#[must_use]
pub fn header_parts(line: &str) -> Option<(char, String, String)> {
    let caps = HEADER_RE.captures(line)?;
    let status = caps.get(1)?.as_str().chars().next()?;
    let id_part = caps
        .get(2)
        .map_or(String::new(), |m| m.as_str().to_string());
    let rest = caps
        .get(3)
        .map_or(String::new(), |m| m.as_str().to_string());
    Some((status, id_part, rest))
}

/// Extracts the raw metadata portion of a header line for `validate`.
///
/// Mirrors Go `validateHeaderMeta`: a seed prefix is dropped, then the split
/// keeps unknown words the way [`split_meta_sep`] does. Unlike
/// [`parse_header`], the line is not `\r`-stripped first, so a CRLF remainder
/// reaches the split exactly as Go sees it.
#[must_use]
pub fn header_meta(line: &str) -> String {
    let Some((_, id_part, mut rest)) = header_parts(line) else {
        return String::new();
    };
    if id_part.is_empty() {
        if let Some(seed) = SEED_RE.captures(&rest) {
            rest = seed
                .get(2)
                .map_or(String::new(), |m| m.as_str().to_string());
        }
    }
    let (_, meta, _) = split_meta_sep(&rest);
    meta
}

/// Serializes a task back to a header line.
///
/// For a task straight from [`parse_header`] and unmodified, this is
/// byte-exact. Metadata renders tags first, then priority, then properties; a
/// task with metadata but no recorded separator uses the canonical two-tab
/// separator. The output is always the strict `- [` shape (so a broad `[X]`
/// task renders as `[x]`); broad lines round-trip through
/// [`flip_checkbox_any`] instead.
#[must_use]
pub fn render_header(task: &Task) -> String {
    let mut out = String::from("- [");
    out.push(if task.checked { 'x' } else { ' ' });
    out.push_str("] ");
    if !task.id.is_empty() {
        out.push_str(&task.id);
        out.push(' ');
    } else if task.seeded && !task.seed_prefix.is_empty() {
        out.push_str(&task.seed_prefix);
        out.push_str("- ");
    }
    out.push_str(&task.title);

    let meta = render_meta(task);
    if !meta.is_empty() {
        let sep = if task.meta_sep.is_empty() {
            "\t\t"
        } else {
            task.meta_sep.as_str()
        };
        out.push_str(sep);
        out.push_str(&meta);
    }
    out
}

/// Rebuilds the metadata portion of a header: tags, then priority, then
/// properties in first-seen order.
#[must_use]
pub fn render_meta(task: &Task) -> String {
    let mut tokens: Vec<String> =
        Vec::with_capacity(task.tags.len() + 1 + task.property_order.len());
    for tag in &task.tags {
        tokens.push(format!("#{tag}"));
    }
    if !task.priority.is_empty() {
        tokens.push(format!("!{}", task.priority));
    }
    for key in &task.property_order {
        if let Some(value) = task.properties.get(key) {
            tokens.push(format!("@{key}:{value}"));
        }
    }
    tokens.join(" ")
}

/// Returns the byte offset at which the `n`-th (0-based) whitespace-delimited
/// field of `s` begins, matching Go's `strings.Fields` boundaries.
fn field_start(s: &str, n: usize) -> usize {
    let mut field = 0;
    let mut in_field = false;
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            in_field = false;
            continue;
        }
        if !in_field {
            if field == n {
                return i;
            }
            field += 1;
            in_field = true;
        }
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/tasks");

    type TokenCase<'a> = (
        &'a str,
        &'a [&'a str],
        &'a str,
        &'a [(&'a str, &'a str)],
        &'a [&'a str],
    );

    type CheckboxCase<'a> = (
        &'a str,
        bool,
        &'a str,
        &'a str,
        bool,
        bool,
        bool,
        &'a [&'a str],
    );

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn fixture_lines(name: &str) -> Vec<String> {
        let path = format!("{FIXTURE_DIR}/{name}");
        let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("read {path}: {err}"));
        String::from_utf8_lossy(&bytes)
            .split('\n')
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn parse_header_identified_with_metadata() {
        let line = "- [ ] BASIC-1 Draft the launch announcement\t\t#tag !high @status:doing";
        let got = parse_header(line).expect("strict header");
        assert_eq!(
            got,
            Task {
                id: "BASIC-1".into(),
                status: b' ',
                title: "Draft the launch announcement".into(),
                tags: vec!["tag".into()],
                priority: "high".into(),
                property_order: vec!["status".into()],
                properties: props(&[("status", "doing")]),
                meta_sep: "\t\t".into(),
                header_raw: line.into(),
                ..Task::default()
            }
        );
    }

    #[test]
    fn parse_header_checked_lowercase_x() {
        let line = "- [x] BASIC-3 Archive the beta feedback thread";
        let got = parse_header(line).expect("strict header");
        assert_eq!(got.id, "BASIC-3");
        assert_eq!(got.status, b'x');
        assert!(got.checked);
        assert_eq!(got.title, "Archive the beta feedback thread");
        assert_eq!(got.header_raw, line);
    }

    #[test]
    fn parse_header_seed_and_unidentified() {
        let seed = parse_header("- [ ] SEED- A seed line").expect("seed header");
        assert!(seed.seeded);
        assert_eq!(seed.seed_prefix, "SEED");
        assert_eq!(seed.title, "A seed line");
        assert!(seed.id.is_empty() && !seed.unidentified);

        let plain = parse_header("- [ ] No ID here").expect("unidentified header");
        assert!(plain.unidentified);
        assert_eq!(plain.title, "No ID here");
        assert!(plain.tags.is_empty() && plain.priority.is_empty());
        assert!(plain.properties.is_empty());
    }

    #[test]
    fn parse_header_strips_one_carriage_return() {
        let got = parse_header("- [ ] EDGE-1 Preserve the raw file\r").expect("CRLF header");
        assert_eq!(got.id, "EDGE-1");
        assert_eq!(got.title, "Preserve the raw file");
        assert_eq!(got.header_raw, "- [ ] EDGE-1 Preserve the raw file");
    }

    #[test]
    fn parse_header_rejects_hostile_lines() {
        for line in [
            "- [X] A-1 Title",
            "  - [ ] A-1 Title",
            "- Just a bullet point",
            "> - [ ] Quoted pseudo-task",
            "- [ ]x",
            "- [ ] A-1 Title\n",
        ] {
            assert!(parse_header(line).is_none(), "{line:?} must not parse");
        }
    }

    #[test]
    fn hash_number_stays_in_title() {
        let got = parse_header("- [ ] META-1 Fix issue #123").expect("header");
        assert_eq!(got.title, "Fix issue #123");
        assert!(got.tags.is_empty());
        assert_eq!(render_header(&got), "- [ ] META-1 Fix issue #123");
    }

    #[test]
    fn split_meta_sep_matches_go_table() {
        let cases = [
            (
                "Draft the launch announcement\t\t#tag !high @status:doing",
                "Draft the launch announcement",
                "#tag !high @status:doing",
                "\t\t",
            ),
            (
                "Header has three tabs\t\t\t#edge",
                "Header has three tabs",
                "#edge",
                "\t\t\t",
            ),
            (
                "Review the migration draft #docs",
                "Review the migration draft",
                "#docs",
                " ",
            ),
            ("Fix issue #123", "Fix issue #123", "", ""),
            (
                "Gather screenshots for the changelog",
                "Gather screenshots for the changelog",
                "",
                "",
            ),
            ("Take the long way home", "Take the long way home", "", ""),
            ("Title #a #b", "Title", "#a #b", " "),
            ("", "", "", ""),
            ("#a #b", "", "#a #b", ""),
            ("Title  \t #tag", "Title", "#tag", "  \t "),
            ("Title\t#tag", "Title", "#tag", "\t"),
        ];
        for (rest, want_title, want_meta, want_sep) in cases {
            let (title, meta, sep) = split_meta_sep(rest);
            assert_eq!(
                (title.as_str(), meta.as_str(), sep.as_str()),
                (want_title, want_meta, want_sep),
                "rest {rest:?}"
            );
        }
    }

    #[test]
    fn parse_tokens_matches_go_table() {
        let cases: &[TokenCase<'_>] = &[
            (
                "#tag !high @status:doing",
                &["tag"],
                "high",
                &[("status", "doing")],
                &["status"],
            ),
            (
                "#a #b !crit @k:v @k2:v2",
                &["a", "b"],
                "crit",
                &[("k", "v"), ("k2", "v2")],
                &["k", "k2"],
            ),
            ("#123", &[], "", &[], &[]),
            ("@bare", &[], "", &[], &[]),
            ("!urgent", &[], "urgent", &[], &[]),
            ("", &[], "", &[], &[]),
            (
                "#same @k:first word @k:second",
                &["same"],
                "",
                &[("k", "second")],
                &["k"],
            ),
        ];
        for (meta, want_tags, want_priority, want_props, want_order) in cases {
            let (tags, priority, properties, order) = parse_tokens(meta);
            assert_eq!(tags, *want_tags, "tags for {meta:?}");
            assert_eq!(priority, *want_priority, "priority for {meta:?}");
            assert_eq!(properties, props(want_props), "properties for {meta:?}");
            assert_eq!(order, *want_order, "order for {meta:?}");
        }
    }

    #[test]
    fn is_meta_token_classifies_by_shape() {
        for word in ["#tag", "#a-b", "!high", "@k:v"] {
            assert!(is_meta_token(word), "{word:?}");
        }
        for word in ["#123", "#_x", "@bare", "!1bad", "plain", ""] {
            assert!(!is_meta_token(word), "{word:?}");
        }
    }

    #[test]
    fn render_header_matches_go_cases() {
        assert_eq!(
            render_header(&Task {
                status: b' ',
                id: "A-1".into(),
                title: "Title".into(),
                tags: vec!["tag".into()],
                ..Task::default()
            }),
            "- [ ] A-1 Title\t\t#tag"
        );
        assert_eq!(
            render_header(&Task {
                status: b' ',
                title: "No ID here".into(),
                unidentified: true,
                ..Task::default()
            }),
            "- [ ] No ID here"
        );
        assert_eq!(
            render_header(&Task {
                status: b' ',
                title: "A seed line".into(),
                seeded: true,
                seed_prefix: "SEED".into(),
                ..Task::default()
            }),
            "- [ ] SEED- A seed line"
        );
        assert_eq!(
            render_header(&Task {
                status: b'x',
                checked: true,
                id: "A-1".into(),
                title: "Done".into(),
                ..Task::default()
            }),
            "- [x] A-1 Done"
        );
    }

    #[test]
    fn render_meta_orders_tags_priority_properties() {
        let filled = Task {
            tags: vec!["a".into(), "b".into()],
            priority: "crit".into(),
            property_order: vec!["k".into(), "k2".into()],
            properties: props(&[("k", "v"), ("k2", "v2")]),
            ..Task::default()
        };
        assert_eq!(render_meta(&filled), "#a #b !crit @k:v @k2:v2");
        assert_eq!(render_meta(&Task::default()), "");

        let missing = Task {
            property_order: vec!["gone".into()],
            ..Task::default()
        };
        assert_eq!(render_meta(&missing), "");
    }

    #[test]
    fn round_trip_header_is_byte_exact() {
        let lines = [
            "- [ ] BASIC-1 Draft the launch announcement\t\t#tag !high @status:doing",
            "- [ ] META-2 Review the migration draft #docs",
            "- [ ] META-1 Fix issue #123",
            "- [ ] BASIC-2 Gather screenshots for the changelog",
            "- [ ] No ID here",
            "- [ ] SEED- A seed line",
            "- [ ] EDGE-3 Header has three tabs\t\t\t#edge",
            "- [x] BASIC-3 Archive the beta feedback thread",
            "- [ ] PRI-4 Reread the incident report",
            "- [ ] Title #a #b",
            "- [ ] A-1 Title  #tag",
        ];
        for line in lines {
            let parsed =
                parse_header(line).unwrap_or_else(|| panic!("{line:?} should parse as a header"));
            assert_eq!(render_header(&parsed), line, "round trip for {line:?}");
        }
    }

    #[test]
    fn parse_checkbox_matches_go_table() {
        let cases: &[CheckboxCase<'_>] = &[
            (
                "- [ ] buy milk",
                true,
                "",
                "buy milk",
                false,
                false,
                true,
                &[],
            ),
            (
                "  * [x] nested done #home",
                true,
                "",
                "nested done",
                true,
                false,
                true,
                &["home"],
            ),
            ("+ [X] upper", true, "", "upper", true, false, true, &[]),
            (
                "\t- [ ] tabbed",
                true,
                "",
                "tabbed",
                false,
                false,
                true,
                &[],
            ),
            (
                "- [ ] PRJ-42 do it",
                true,
                "PRJ-42",
                "do it",
                false,
                false,
                false,
                &[],
            ),
            (
                "* [x] PRJ-007 done #a !high @k:v",
                true,
                "PRJ-007",
                "done",
                true,
                false,
                false,
                &["a"],
            ),
            (
                "  + [ ] PRJ- placeholder",
                true,
                "",
                "placeholder",
                false,
                true,
                false,
                &[],
            ),
            ("just text", false, "", "", false, false, false, &[]),
            ("- [ ]x", false, "", "", false, false, false, &[]),
            ("> - [ ] quoted", false, "", "", false, false, false, &[]),
            ("1. [ ] item", false, "", "", false, false, false, &[]),
            (
                "- [X] upper first",
                true,
                "",
                "upper first",
                true,
                false,
                true,
                &[],
            ),
        ];
        for (line, ok, want_id, want_title, checked, seeded, unidentified, want_tags) in cases {
            let got = parse_checkbox(line);
            assert_eq!(got.is_some(), *ok, "{line:?} ok");
            let Some(got) = got else { continue };
            assert!(got.broad, "{line:?} broad");
            assert_eq!(got.id, *want_id, "{line:?} id");
            assert_eq!(got.title, *want_title, "{line:?} title");
            assert_eq!(got.checked, *checked, "{line:?} checked");
            assert_eq!(got.seeded, *seeded, "{line:?} seeded");
            assert_eq!(got.unidentified, *unidentified, "{line:?} unidentified");
            assert_eq!(got.tags.as_slice(), *want_tags, "{line:?} tags");
            assert_eq!(got.header_raw, *line, "{line:?} header_raw");
        }
    }

    #[test]
    fn parse_checkbox_is_a_superset_of_parse_header() {
        let line = "- [ ] BASIC-1 Draft it\t\t#tag !high @status:doing";
        let strict = parse_header(line).expect("strict header");
        let mut broad = parse_checkbox(line).expect("broad checkbox");
        assert!(broad.broad && !strict.broad);
        broad.broad = false;
        assert_eq!(strict, broad);
    }

    #[test]
    fn uppercase_x_rejected_strict_but_broad() {
        assert!(parse_header("- [X] A-1 Title").is_none());
        let broad = parse_checkbox("- [X] A-1 Title").expect("broad grammar accepts X");
        assert_eq!(broad.status, b'X');
        assert!(broad.checked);
        assert_eq!(broad.id, "A-1");
        assert_eq!(broad.title, "Title");
    }

    #[test]
    fn checkbox_requires_space_after_bracket() {
        assert!(parse_header("- [ ]x").is_none());
        assert!(parse_checkbox("- [ ]x").is_none());
        assert!(flip_checkbox_any("- [ ]x").is_none());
    }

    #[test]
    fn flip_checkbox_any_matches_go_table() {
        let cases = [
            ("- [ ] a", Some("- [x] a")),
            ("  * [x] b", Some("  * [ ] b")),
            ("+ [X] c", Some("+ [ ] c")),
            (
                "\t- [ ] PRJ-1 title #t !high",
                Some("\t- [x] PRJ-1 title #t !high"),
            ),
            ("- [ ]", None),
            ("not a checkbox", None),
        ];
        for (line, want) in cases {
            assert_eq!(flip_checkbox_any(line).as_deref(), want, "{line:?}");
        }
    }

    #[test]
    fn broad_uppercase_x_renders_strict_shape() {
        let parsed = parse_checkbox("+ [X] upper").expect("broad checkbox");
        assert_eq!(render_header(&parsed), "- [x] upper");
    }

    #[test]
    fn spec_fixture_headers_round_trip() {
        let fixtures = [
            "spec-basic.md",
            "spec-metadata.md",
            "spec-fenced.md",
            "spec-blockers.md",
            "spec-unidentified.md",
            "spec-priority.md",
            "spec-headings.md",
            "spec-prose.md",
            "spec-edge.md",
        ];
        let mut checked = 0;
        for name in fixtures {
            for line in fixture_lines(name) {
                if let Some(parsed) = parse_header(&line) {
                    let literal = line.strip_suffix('\r').unwrap_or(&line);
                    assert_eq!(render_header(&parsed), literal, "{name}: {literal:?}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 31, "strict headers across the spec fixtures");
    }

    #[test]
    fn spec_fixture_headers_match_go_tables() {
        // `spec-fenced.md` is counted at line level here: fence masking is the
        // body port's job, not the grammar's.
        let fixtures: &[(&str, usize, &[&str])] = &[
            ("spec-basic.md", 3, &["BASIC-1", "BASIC-2", "BASIC-3"]),
            ("spec-metadata.md", 3, &["META-1", "META-2", "META-3"]),
            (
                "spec-fenced.md",
                4,
                &["FENCED-1", "FENCED-2", "FENCED-3", "FENCE-9"],
            ),
            ("spec-blockers.md", 4, &["BLK-1", "BLK-2", "BLK-3", "BLK-4"]),
            ("spec-unidentified.md", 3, &[]),
            ("spec-priority.md", 4, &["PRI-1", "PRI-2", "PRI-3", "PRI-4"]),
            (
                "spec-headings.md",
                4,
                &["HEAD-1", "HEAD-2", "HEAD-3", "HEAD-4"],
            ),
            ("spec-prose.md", 3, &["PROSE-1", "PROSE-2", "PROSE-3"]),
            ("spec-edge.md", 3, &["EDGE-1", "EDGE-2", "EDGE-3"]),
        ];
        for (name, want_count, want_ids) in fixtures {
            let parsed: Vec<Task> = fixture_lines(name)
                .iter()
                .filter_map(|line| parse_header(line))
                .collect();
            assert_eq!(parsed.len(), *want_count, "{name} header count");
            let ids: Vec<&str> = parsed
                .iter()
                .filter(|task| !task.id.is_empty())
                .map(|task| task.id.as_str())
                .collect();
            assert_eq!(ids.as_slice(), *want_ids, "{name} ids");
        }
    }

    #[test]
    fn spec_priority_fixture_values() {
        let priorities: Vec<String> = fixture_lines("spec-priority.md")
            .iter()
            .filter_map(|line| parse_header(line))
            .map(|task| task.priority)
            .collect();
        assert_eq!(priorities, ["crit", "high", "low", ""]);
    }

    #[test]
    fn spec_unidentified_fixture_seed_and_plain() {
        let mut seeds = 0;
        let mut plain = 0;
        for line in fixture_lines("spec-unidentified.md") {
            if let Some(parsed) = parse_header(&line) {
                if parsed.seeded {
                    assert_eq!(parsed.seed_prefix, "SEED");
                    seeds += 1;
                } else if parsed.unidentified {
                    plain += 1;
                }
            }
        }
        assert_eq!((seeds, plain), (1, 2));
    }

    #[test]
    fn spec_metadata_fixture_keeps_hash_number_in_title() {
        for line in fixture_lines("spec-metadata.md") {
            let Some(parsed) = parse_header(&line) else {
                continue;
            };
            match parsed.id.as_str() {
                "META-1" => {
                    assert_eq!(parsed.title, "Fix issue #123");
                    assert!(parsed.tags.is_empty());
                }
                "META-2" => {
                    assert_eq!(parsed.title, "Review the migration draft");
                    assert_eq!(parsed.tags, ["docs"]);
                }
                _ => {}
            }
        }
    }
}
