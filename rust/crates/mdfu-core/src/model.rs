//! Unified normalized document contract shared by every `mdfu` track.
//!
//! Rust port of `internal/model/document.go`. Field names, serde/JSON names,
//! and the `SearchBlob` join order are byte-compatible with the Go original:
//! the Go struct carries no JSON tags, so JSON field names are the `PascalCase`
//! Go field names.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Identifies which frontmatter family a [`Document`] was parsed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatKind {
    /// OKF v0.1/v0.2 frontmatter.
    Okf,
    /// Portent/Tolaria frontmatter.
    Portent,
    /// Frontmatter without a recognizable family.
    Generic,
    /// No frontmatter at all.
    #[default]
    None,
}

impl FormatKind {
    /// Returns the wire string, matching the Go `FormatKind` constants.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Okf => "okf",
            Self::Portent => "portent",
            Self::Generic => "generic",
            Self::None => "none",
        }
    }
}

/// Records who generated or verified a document and when.
///
/// `at` is an RFC 3339 timestamp string: the Rust port has no `chrono`
/// dependency, and the Go `*time.Time` JSON form is an RFC 3339 string.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Attestation {
    /// Who attested.
    #[serde(rename = "By", default)]
    pub by: String,
    /// When, as an RFC 3339 timestamp.
    #[serde(rename = "At", default)]
    pub at: Option<String>,
}

/// Records a provenance entry for a document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Source {
    /// Source identifier.
    #[serde(rename = "ID", default)]
    pub id: String,
    /// Source resource (URL or path).
    #[serde(rename = "Resource", default)]
    pub resource: String,
    /// Source title.
    #[serde(rename = "Title", default)]
    pub title: String,
    /// Source author.
    #[serde(rename = "Author", default)]
    pub author: String,
}

/// The unified normalized model for a single markdown file.
///
/// Contract from `internal/model/document.go`; keep field names, JSON names,
/// and semantics in sync across both implementations.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Document {
    /// Path of the markdown file, as provided by the caller.
    #[serde(rename = "Path", default)]
    pub path: String,
    /// Normalized title.
    #[serde(rename = "Title", default)]
    pub title: String,
    /// Frontmatter `type` value.
    #[serde(rename = "DocType", default)]
    pub doc_type: String,
    /// Normalized description.
    #[serde(rename = "Description", default)]
    pub description: String,
    /// Frontmatter `resource` value.
    #[serde(rename = "Resource", default)]
    pub resource: String,
    /// `""` | `index` | `log`, derived from the file basename.
    #[serde(rename = "Role", default)]
    pub role: String,
    /// Normalized tags.
    #[serde(rename = "Tags", default)]
    pub tags: Vec<String>,
    /// Markdown body with the frontmatter block removed.
    #[serde(rename = "Body", default)]
    pub body: String,
    /// Earliest known timestamp, RFC 3339.
    #[serde(rename = "CreatedAt", default)]
    pub created_at: Option<String>,
    /// Latest known timestamp, RFC 3339.
    #[serde(rename = "UpdatedAt", default)]
    pub updated_at: Option<String>,
    /// Normalized (lowercased) status.
    #[serde(rename = "Status", default)]
    pub status: String,
    /// Explicit `organized` flag when present.
    #[serde(rename = "Organized", default)]
    pub organized: Option<bool>,
    /// Explicit `archived` flag when present.
    #[serde(rename = "Archived", default)]
    pub archived: Option<bool>,
    /// `belongs_to` relation values.
    #[serde(rename = "BelongsTo", default)]
    pub belongs_to: Vec<String>,
    /// `related_to` relation values.
    #[serde(rename = "RelatedTo", default)]
    pub related_to: Vec<String>,
    /// Provenance entries.
    #[serde(rename = "Sources", default)]
    pub sources: Vec<Source>,
    /// Generation attestation.
    #[serde(rename = "Generated", default)]
    pub generated: Option<Attestation>,
    /// Verification attestation.
    #[serde(rename = "Verified", default)]
    pub verified: Option<Attestation>,
    /// Frontmatter family.
    #[serde(rename = "Format", default)]
    pub format: FormatKind,
    /// Raw frontmatter map (Go `map[string]any`).
    #[serde(rename = "Raw", default)]
    pub raw: Map<String, Value>,
    /// Precomputed searchable blob, see [`build_search_blob`].
    #[serde(rename = "SearchBlob", default)]
    pub search_blob: String,
    /// Frontmatter YAML parse error, if any (parsing never fails hard).
    #[serde(rename = "ParseError", default)]
    pub parse_error: Option<String>,
}

impl Document {
    /// Reports whether the document is archived: explicitly via the `archived`
    /// flag or implicitly via status `"archived"`.
    ///
    /// Mirrors Go `(*Document).IsArchived`; unlike Go there is no nil receiver
    /// to guard against.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.archived == Some(true) || self.status == "archived"
    }

    /// Recomputes [`Self::search_blob`] from the normalized fields and returns it.
    #[must_use]
    pub fn rebuild_search_blob(&mut self) -> &str {
        self.search_blob = build_search_blob(
            &self.title,
            &self.description,
            &self.tags,
            &self.raw,
            &self.body,
        );
        &self.search_blob
    }
}

/// Concatenates title + description + tags + flattened scalars of the raw
/// frontmatter + body into a single searchable blob.
///
/// Each part is trimmed; empty parts are skipped; parts are joined with `\n`
/// in exactly the Go order. The blob is stored as-is; lowercasing happens at
/// match time.
#[must_use]
pub fn build_search_blob(
    title: &str,
    description: &str,
    tags: &[String],
    raw: &Map<String, Value>,
    body: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim();
        if !s.is_empty() {
            parts.push(s.to_owned());
        }
    };
    push(title);
    push(description);
    for tag in tags {
        push(tag);
    }
    for scalar in flatten_scalars(raw) {
        push(&scalar);
    }
    push(body);
    parts.join("\n")
}

/// Walks a raw frontmatter map and returns every scalar leaf value as a
/// string, in deterministic (sorted-key) order.
///
/// Nested maps and lists are recursed into; keys themselves, `null` values,
/// blank strings, and empty containers produce nothing.
#[must_use]
pub fn flatten_scalars(raw: &Map<String, Value>) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    flatten_object(raw, &mut out);
    out
}

/// Flattens an object's values into `out`, visiting keys in sorted order.
fn flatten_object(map: &Map<String, Value>, out: &mut Vec<String>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_unstable();
    for key in keys {
        if let Some(value) = map.get(key) {
            flatten_value(value, out);
        }
    }
}

/// Flattens one frontmatter value, recursing into lists and nested maps.
fn flatten_value(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Null => {}
        Value::Bool(flag) => out.push(flag.to_string()),
        Value::Number(number) => out.push(stringify_number(number)),
        Value::String(text) => {
            if !text.trim().is_empty() {
                out.push(text.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                flatten_value(item, out);
            }
        }
        Value::Object(map) => flatten_object(map, out),
    }
}

/// Renders a JSON number the way Go `fmt.Sprintf("%v", ...)` does for the
/// numeric types YAML frontmatter can produce.
fn stringify_number(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    number
        .as_f64()
        .map_or_else(|| number.to_string(), go_float_string)
}

/// Formats an `f64` like Go's `fmt.Sprintf("%v", f)`: `strconv.FormatFloat`
/// with `'g'` and shortest precision, which uses plain decimal notation for
/// exponents in `[-4, 6)` and Go-style scientific notation otherwise.
#[allow(clippy::float_cmp)] // exact zero/sign checks are intentional here
fn go_float_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf"
        } else {
            "+Inf"
        }
        .to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent_text.parse().unwrap_or(0);
    if (-4..6).contains(&exponent) {
        format!("{value}")
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Builds a frontmatter map from a JSON object literal.
    fn frontmatter(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn flatten_scalars_sorts_keys_and_drops_nil_and_blank() {
        let raw = frontmatter(&json!({
            "zeta": "last",
            "blank": "   ",
            "nil": null,
            "num": 42,
            "flag": true,
            "list": ["b", "", "a", null, 3],
            "blanks": ["  ", "\t"],
            "empty_map": {},
            "blank_map": {"x": " "},
            "nested": {"b": "nb", "a": "na"},
            "alpha": "first",
        }));
        assert_eq!(
            flatten_scalars(&raw),
            ["first", "true", "b", "a", "3", "na", "nb", "42", "last"]
        );
    }

    #[test]
    fn flatten_scalars_empty_map_returns_nothing() {
        assert!(flatten_scalars(&Map::new()).is_empty());
    }

    #[test]
    fn nested_flatten_is_stable_across_runs() {
        let raw = frontmatter(&json!({
            "m": {"z": "2", "a": {"y": "1", "b": "0"}},
            "n": ["q", "p"],
        }));
        assert_eq!(flatten_scalars(&raw), flatten_scalars(&raw));
        assert_eq!(flatten_scalars(&raw), ["0", "1", "2", "q", "p"]);
    }

    #[test]
    fn scalars_stringify_numbers_and_bools_like_go() {
        let raw = frontmatter(&json!({
            "int": 42,
            "float": 3.5,
            "whole": 1.0,
            "big": 1_000_000.0,
            "tiny": 0.000_01,
            "yes": true,
            "no": false,
        }));
        assert_eq!(
            flatten_scalars(&raw),
            ["1e+06", "3.5", "42", "false", "1e-05", "1", "true"]
        );
    }

    #[test]
    fn build_search_blob_joins_parts_with_newlines_in_go_order() {
        let raw = frontmatter(&json!({"k": "v"}));
        let tags = vec![" a ".to_owned(), String::new(), "b".to_owned()];
        assert_eq!(
            build_search_blob("  Title  ", "Desc", &tags, &raw, "Body\nline"),
            "Title\nDesc\na\nb\nv\nBody\nline"
        );
    }

    #[test]
    fn build_search_blob_skips_empty_parts() {
        assert_eq!(
            build_search_blob("", "   ", &[], &Map::new(), ""),
            String::new()
        );
    }

    #[test]
    fn rebuild_search_blob_populates_from_normalized_fields() {
        let mut doc = Document {
            title: "Ship mdfu MVP".to_owned(),
            description: "Tracks monthly active users of the platform.".to_owned(),
            tags: vec!["growth".to_owned()],
            raw: frontmatter(&json!({
                "belongs_to": "[[Project Atlas]]",
                "related_to": ["[[Launch Plan]]", "Retro Notes"],
            })),
            body: "# Ship mdfu MVP\n\nbody".to_owned(),
            ..Document::default()
        };
        let blob = doc.rebuild_search_blob().to_owned();
        assert_eq!(blob, doc.search_blob);
        for needle in [
            "Ship mdfu MVP",
            "platform",
            "growth",
            "Project Atlas",
            "Launch Plan",
            "Retro Notes",
            "body",
        ] {
            assert!(blob.contains(needle), "blob missing {needle:?}: {blob}");
        }
    }

    #[test]
    fn is_archived_matches_go_semantics() {
        let cases: [(Option<bool>, &str, bool); 6] = [
            (None, "draft", false),
            (Some(true), "", true),
            (Some(false), "", false),
            (None, "archived", true),
            (Some(true), "draft", true),
            (None, "Archived", false),
        ];
        for (archived, status, want) in cases {
            let doc = Document {
                archived,
                status: status.to_owned(),
                ..Document::default()
            };
            assert_eq!(
                doc.is_archived(),
                want,
                "archived={archived:?} status={status:?}"
            );
        }
    }

    #[test]
    fn document_serializes_with_go_field_names() {
        let doc = Document {
            path: "p.md".to_owned(),
            format: FormatKind::Portent,
            raw: frontmatter(&json!({"a": 1})),
            ..Document::default()
        };
        let value = serde_json::to_value(&doc).unwrap();
        let obj = value.as_object().unwrap();
        let expected = [
            "Path",
            "Title",
            "DocType",
            "Description",
            "Resource",
            "Role",
            "Tags",
            "Body",
            "CreatedAt",
            "UpdatedAt",
            "Status",
            "Organized",
            "Archived",
            "BelongsTo",
            "RelatedTo",
            "Sources",
            "Generated",
            "Verified",
            "Format",
            "Raw",
            "SearchBlob",
            "ParseError",
        ];
        assert_eq!(obj.len(), expected.len());
        for key in expected {
            assert!(obj.contains_key(key), "missing JSON field {key}");
        }
        assert_eq!(obj["Format"], json!("portent"));
        assert_eq!(obj["ParseError"], Value::Null);
        assert_eq!(obj["CreatedAt"], Value::Null);
    }

    #[test]
    fn format_kind_wire_strings_match_go_constants() {
        for (kind, want) in [
            (FormatKind::Okf, "okf"),
            (FormatKind::Portent, "portent"),
            (FormatKind::Generic, "generic"),
            (FormatKind::None, "none"),
        ] {
            assert_eq!(kind.as_str(), want);
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(want));
        }
    }
}
