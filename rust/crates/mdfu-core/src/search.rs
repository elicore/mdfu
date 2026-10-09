//! Search ranking: hard filters, bare-word scoring, and archived filtering.
//!
//! Port of `internal/search/search.go`. Hard filters ([`matches_doc`]) and the
//! bare-word ranking ([`rank`]) reproduce the Go implementation exactly,
//! including the `UpdatedAt`-then-`Path` tie-break. Bare words are matched by
//! case-insensitive substring over the whole search blob; the fuzzy fallback is
//! restricted to the title, so a multi-thousand-word body can never produce a
//! subsequence false positive.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::model::Document;
use crate::query::{parse_one_date, DateFilter, Instant, Query};

/// Title is exactly the bare word.
const EXACT_TITLE_BOOST: i32 = 200;
/// Bare word appears as a whole word in the title.
const TITLE_BOOST: i32 = 100;
/// Bare word appears inside a longer title word.
const PARTIAL_TITLE_BOOST: i32 = 50;

/// Fuzzy string matching, a faithful port of the
/// [`sahilm/fuzzy`](https://github.com/sahilm/fuzzy) matcher (`v0.1.3`, MIT).
///
/// Only the parts `mdfu` uses are ported: matching a pattern against candidate
/// strings and returning matches sorted by descending score. Match decisions
/// and integer scores are reproduced exactly, so ranking stays identical to
/// the Go implementation.
pub mod fuzzy {
    const FIRST_CHAR_MATCH_BONUS: i32 = 10;
    const MATCH_FOLLOWING_SEPARATOR_BONUS: i32 = 20;
    const CAMEL_CASE_MATCH_BONUS: i32 = 20;
    const ADJACENT_MATCH_BONUS: i32 = 5;
    const UNMATCHED_LEADING_CHAR_PENALTY: i32 = -5;
    const MAX_UNMATCHED_LEADING_CHAR_PENALTY: i32 = -15;
    const SEPARATORS: [char; 6] = ['/', '-', '_', ' ', '.', '\\'];

    /// A single fuzzy match.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Match {
        /// The matched string.
        pub text: String,
        /// Index of the matched string in the supplied slice.
        pub index: usize,
        /// Byte indexes of the matched characters.
        pub matched_indexes: Vec<usize>,
        /// Match quality score (higher is better).
        pub score: i32,
    }

    /// Finds `pattern` in `data`, returning matches in descending score order.
    #[must_use]
    pub fn find(pattern: &str, data: &[&str]) -> Vec<Match> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let runes: Vec<char> = pattern.chars().collect();
        let mut matches: Vec<Match> = Vec::new();
        for (index, &text) in data.iter().enumerate() {
            let clean = text.split('\0').next().unwrap_or(text);
            if let Some(found) = match_one(&runes, clean, index) {
                matches.push(found);
            }
        }
        matches.sort_by_key(|found| std::cmp::Reverse(found.score));
        matches
    }

    /// Reports whether `pattern` fuzzy-matches `text`.
    #[must_use]
    pub fn matches(pattern: &str, text: &str) -> bool {
        !find(pattern, &[text]).is_empty()
    }

    /// Scores one candidate string against the pre-split `pattern` runes.
    fn match_one(runes: &[char], text: &str, index: usize) -> Option<Match> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut matched_offsets: Vec<usize> = Vec::with_capacity(runes.len());
        let mut total_score: i32 = 0;
        let mut pattern_index = 0usize;
        let mut best_score: i32 = -1;
        let mut best_offset: Option<usize> = None;
        let mut adjacent_bonus: i32 = 0;
        let mut last_char: char = '\0';
        let mut last_offset: usize = 0;
        for (position, &(byte_offset, candidate)) in chars.iter().enumerate() {
            if equal_fold(candidate, runes[pattern_index]) {
                let mut score: i32 = 0;
                if byte_offset == 0 {
                    score += FIRST_CHAR_MATCH_BONUS;
                }
                if last_char.is_lowercase() && candidate.is_uppercase() {
                    score += CAMEL_CASE_MATCH_BONUS;
                }
                if byte_offset != 0 && is_separator(last_char) {
                    score += MATCH_FOLLOWING_SEPARATOR_BONUS;
                }
                if let Some(&last_match) = matched_offsets.last() {
                    let bonus = adjacent_char_bonus(last_offset, last_match, adjacent_bonus);
                    score += bonus;
                    adjacent_bonus += bonus;
                }
                if score > best_score {
                    best_score = score;
                    best_offset = Some(byte_offset);
                }
            }
            let next_pattern = if pattern_index < runes.len() - 1 {
                runes[pattern_index + 1]
            } else {
                '\0'
            };
            let next_candidate = chars.get(position + 1).map_or('\0', |&(_, value)| value);
            if equal_fold(next_pattern, next_candidate) || next_candidate == '\0' {
                if let Some(offset) = best_offset {
                    if matched_offsets.is_empty() {
                        let penalty = i32::try_from(offset).map_or(i32::MIN, |index| {
                            UNMATCHED_LEADING_CHAR_PENALTY.saturating_mul(index)
                        });
                        best_score += penalty.max(MAX_UNMATCHED_LEADING_CHAR_PENALTY);
                    }
                    total_score += best_score;
                    matched_offsets.push(offset);
                    best_score = -1;
                    pattern_index += 1;
                }
            }
            last_offset = byte_offset;
            last_char = candidate;
        }
        let penalty = i32::try_from(matched_offsets.len())
            .unwrap_or(i32::MAX)
            .saturating_sub(i32::try_from(chars.len()).unwrap_or(i32::MAX));
        total_score += penalty;
        if matched_offsets.len() == runes.len() {
            Some(Match {
                text: text.to_owned(),
                index,
                matched_indexes: matched_offsets,
                score: total_score,
            })
        } else {
            None
        }
    }

    /// Case-insensitive rune equality mirroring Go `strings.EqualFold`.
    ///
    /// ASCII is exact; non-ASCII uses a best-effort simple-fold representative
    /// (a single-character lowercase mapping), which matches the reference
    /// corpus and the common cases, though a few Unicode special folds
    /// (`ſ`/`s`, final sigma) are not reproduced.
    pub(crate) fn equal_fold(left: char, right: char) -> bool {
        if left == right {
            return true;
        }
        if left.is_ascii() && right.is_ascii() {
            return left.eq_ignore_ascii_case(&right);
        }
        simple_fold_representative(left) == simple_fold_representative(right)
    }

    fn simple_fold_representative(value: char) -> char {
        let mut lowered = value.to_lowercase();
        let first = lowered.next().unwrap_or(value);
        if lowered.next().is_none() {
            first
        } else {
            value
        }
    }

    fn adjacent_char_bonus(index: usize, last_match: usize, current_bonus: i32) -> i32 {
        if last_match == index {
            current_bonus * 2 + ADJACENT_MATCH_BONUS
        } else {
            0
        }
    }

    fn is_separator(value: char) -> bool {
        SEPARATORS.contains(&value)
    }
}

/// Reports whether `doc` satisfies every hard filter in `query`.
///
/// Bare words are not checked here; they only affect [`rank`] scoring. A `None`
/// query matches every document, mirroring Go's nil-query behavior.
#[must_use]
pub fn matches_doc(doc: &Document, query: Option<&Query>) -> bool {
    let Some(query) = query else {
        return true;
    };
    if !query.tags.is_empty() {
        let present = tag_set(&doc.tags);
        for want in &query.tags {
            if !present.contains(&want.trim().to_lowercase()) {
                return false;
            }
        }
    }
    if !query.not_tags.is_empty() {
        let present = tag_set(&doc.tags);
        for ban in &query.not_tags {
            if present.contains(&ban.trim().to_lowercase()) {
                return false;
            }
        }
    }
    if !query.doc_type.is_empty() && !str_equal_fold(doc.doc_type.trim(), query.doc_type.trim()) {
        return false;
    }
    if !query.title.is_empty() && !fuzzy_contains_fold(&query.title, &doc.title) {
        return false;
    }
    if !query.status.is_empty() {
        let doc_status = doc.status.trim();
        let status_filter = query.status.trim();
        let status_matches = str_equal_fold(doc_status, status_filter)
            || (str_equal_fold(status_filter, "archived") && doc.is_archived());
        if !status_matches {
            return false;
        }
    }
    if !query.path.is_empty() && !doc.path.to_lowercase().contains(&query.path.to_lowercase()) {
        return false;
    }
    for (key, value) in &query.generic {
        let Some(field) = lookup_field(doc, key) else {
            return false;
        };
        if !fuzzy_contains_fold(value, &field) {
            return false;
        }
    }
    if query
        .created
        .as_ref()
        .is_some_and(|filter| !date_filter_matches(doc, filter, true))
    {
        return false;
    }
    if query
        .updated
        .as_ref()
        .is_some_and(|filter| !date_filter_matches(doc, filter, false))
    {
        return false;
    }
    true
}

/// Filters `docs` through [`matches_doc`], scores each bare word, and sorts by
/// score descending, then most-recent `UpdatedAt`, then `Path` ascending.
///
/// With no non-empty bare words the filtered docs are sorted by `Path` only.
#[must_use]
pub fn rank<'a>(docs: &'a [Document], query: Option<&Query>) -> Vec<&'a Document> {
    let filtered: Vec<&Document> = docs.iter().filter(|doc| matches_doc(doc, query)).collect();
    let bare: Vec<&str> = query.map_or_else(Vec::new, |query| {
        query.bare.iter().map(String::as_str).collect()
    });
    if !bare.iter().any(|word| !word.trim().is_empty()) {
        let mut by_path = filtered;
        by_path.sort_by(|left, right| left.path.cmp(&right.path));
        return by_path;
    }
    let mut scored: Vec<(&Document, i32)> = Vec::with_capacity(filtered.len());
    for doc in filtered {
        if let Some(score) = score_doc(doc, &bare) {
            scored.push((doc, score));
        }
    }
    scored.sort_by(|(left_doc, left_score), (right_doc, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| compare_recent(left_doc, right_doc))
    });
    scored.into_iter().map(|(doc, _score)| doc).collect()
}

/// Returns `docs` with archived documents removed unless `include_archived`.
///
/// Neither the input slice nor the documents are mutated.
#[must_use]
pub fn filter_archived(docs: &[Document], include_archived: bool) -> Vec<&Document> {
    docs.iter()
        .filter(|doc| include_archived || !doc.is_archived())
        .collect()
}

/// Scores `doc` against every bare word. `None` means a bare word did not match
/// (AND semantics) and the document must be excluded.
fn score_doc(doc: &Document, bare: &[&str]) -> Option<i32> {
    let mut searchable = doc.search_blob.clone();
    if searchable.trim().is_empty() {
        format!("{} {}", doc.title, doc.body)
            .trim()
            .clone_into(&mut searchable);
    }
    let searchable = searchable.to_lowercase();
    let title_folded = doc.title.trim().to_lowercase();
    let mut total: i32 = 0;
    for word in bare {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let needle = word.to_lowercase();
        if searchable.contains(&needle) {
            total += i32::try_from(needle.chars().count()).unwrap_or(i32::MAX);
        } else {
            let matches = fuzzy::find(word, &[doc.title.as_str()]);
            let found = matches.first()?;
            total += found.score;
        }
        total += title_match_boost(&title_folded, &needle);
    }
    Some(total)
}

/// Grades how the lowercased bare word appears in the lowercased title.
///
/// The three title boosts are mutually exclusive: exact beats whole-word beats
/// a substring inside a longer word.
fn title_match_boost(title: &str, needle: &str) -> i32 {
    if needle.is_empty() || title.is_empty() {
        0
    } else if title == needle {
        EXACT_TITLE_BOOST
    } else if contains_word(title, needle) {
        TITLE_BOOST
    } else if title.contains(needle) {
        PARTIAL_TITLE_BOOST
    } else {
        0
    }
}

/// Reports whether `word` occurs in `text` delimited by non-word characters, so
/// `"butter"` matches `"Butter chicken"` but not `"Buttermilk"`.
fn contains_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let haystack = text.as_bytes();
    let needle = word.as_bytes();
    let mut offset = 0;
    while offset + needle.len() <= haystack.len() {
        let Some(relative) = find_bytes(&haystack[offset..], needle) else {
            return false;
        };
        let start = offset + relative;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_word_rune(previous_char(text, start));
        let after_ok = end >= text.len() || !is_word_rune(next_char(text, end));
        if before_ok && after_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

/// Case-insensitive substring or fuzzy match, mirroring Go `fuzzyContainsFold`.
fn fuzzy_contains_fold(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if text.is_empty() {
        return false;
    }
    if text.to_lowercase().contains(&pattern.to_lowercase()) {
        return true;
    }
    fuzzy::matches(pattern, text)
}

/// Applies a date filter to the document's created (`either = true`) or updated
/// timestamp, using the Go `date`/`before`/`after` either-field semantics.
fn date_filter_matches(doc: &Document, filter: &DateFilter, created: bool) -> bool {
    let created_at = doc.created_at.as_deref();
    let updated_at = doc.updated_at.as_deref();
    if is_either_field(&filter.field) {
        match_either(created_at, updated_at, filter)
    } else if created {
        match_date(created_at, filter)
    } else {
        match_date(updated_at, filter)
    }
}

fn is_either_field(field: &str) -> bool {
    matches!(field.to_lowercase().as_str(), "date" | "before" | "after")
}

/// Reports whether the timestamp satisfies `filter`. A missing timestamp never
/// matches a present filter.
fn match_date(timestamp: Option<&str>, filter: &DateFilter) -> bool {
    let Some(timestamp) = timestamp else {
        return false;
    };
    let Ok((instant, _)) = parse_one_date(timestamp) else {
        return false;
    };
    if let Some(from) = filter.from {
        if filter.from_incl {
            if instant < from {
                return false;
            }
        } else if instant <= from {
            return false;
        }
    }
    if let Some(to) = filter.to {
        if filter.to_incl {
            if instant > to {
                return false;
            }
        } else if instant >= to {
            return false;
        }
    }
    true
}

/// Reports whether either timestamp satisfies `filter`. Both-missing fails.
fn match_either(created_at: Option<&str>, updated_at: Option<&str>, filter: &DateFilter) -> bool {
    if created_at.is_none() && updated_at.is_none() {
        return false;
    }
    match_date(created_at, filter) || match_date(updated_at, filter)
}

/// Finds the stringified value for a generic key: raw frontmatter first
/// (case-insensitively, including normalized key forms), then normalized fields.
fn lookup_field(doc: &Document, key: &str) -> Option<String> {
    let target = key.trim().to_lowercase();
    if !doc.raw.is_empty() {
        for (raw_key, value) in &doc.raw {
            if str_equal_fold(raw_key, &target) || str_equal_fold(raw_key, key) {
                return Some(stringify_value(value));
            }
        }
        let wanted = normalize_key(&target);
        for (raw_key, value) in &doc.raw {
            if normalize_key(&raw_key.to_lowercase()) == wanted {
                return Some(stringify_value(value));
            }
        }
    }
    normalized_field(doc, &target)
}

fn normalize_key(key: &str) -> String {
    key.trim().to_lowercase().replace('_', "")
}

/// Maps generic keys onto normalized [`Document`] fields.
fn normalized_field(doc: &Document, lower_key: &str) -> Option<String> {
    match normalize_key(lower_key).as_str() {
        "type" | "doctype" => Some(doc.doc_type.clone()),
        "title" => Some(doc.title.clone()),
        "description" | "desc" => Some(doc.description.clone()),
        "status" => Some(doc.status.clone()),
        "resource" => Some(doc.resource.clone()),
        "role" => Some(doc.role.clone()),
        "path" => Some(doc.path.clone()),
        "tags" | "tag" => Some(doc.tags.join(" ")),
        "body" | "content" => Some(doc.body.clone()),
        "format" => Some(doc.format.as_str().to_owned()),
        "organized" => Some(
            doc.organized
                .map_or_else(|| "false".to_owned(), |flag| flag.to_string()),
        ),
        "archived" => Some(if doc.is_archived() { "true" } else { "false" }.to_owned()),
        "belongsto" | "belongs" | "relatedto" | "related" => {
            if normalize_key(lower_key).starts_with("belongs") {
                Some(doc.belongs_to.join(" "))
            } else {
                Some(doc.related_to.join(" "))
            }
        }
        "created" | "createdat" | "updated" | "updatedat" | "modified" | "lastmodified"
        | "date" | "timestamp" => Some(dates_string(doc)),
        _ => None,
    }
}

/// Joins `YYYY-MM-DD` and RFC 3339 forms of both timestamps, Go `%v` order.
fn dates_string(doc: &Document) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(created) = &doc.created_at {
        parts.push(date_part(created));
        parts.push(created.clone());
    }
    if let Some(updated) = &doc.updated_at {
        parts.push(date_part(updated));
        parts.push(updated.clone());
    }
    parts.join(" ")
}

fn date_part(rfc3339: &str) -> String {
    rfc3339.get(..10).unwrap_or(rfc3339).to_owned()
}

/// Flattens a raw frontmatter value for fuzzy matching, mirroring Go
/// `stringifyValue`.
fn stringify_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => stringify_number(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(stringify_value)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(map) => go_map_string(map),
    }
}

/// Renders a map the way Go `fmt.Sprintf("%v", ...)` does (`map[k:v ...]`).
fn go_map_string(map: &Map<String, Value>) -> String {
    let mut inner = String::new();
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            inner.push(' ');
        }
        inner.push_str(key);
        inner.push(':');
        inner.push_str(&stringify_value(value));
    }
    format!("map[{inner}]")
}

/// Renders a JSON number the way Go prints the numeric types YAML can produce.
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

/// Formats an `f64` like Go `fmt.Sprintf("%v", f)`: shortest `'g'`, plain
/// decimal for exponents in `[-4, 6)` and Go-style scientific notation beyond.
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
    let (mantissa, power_text) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = power_text.parse().unwrap_or(0);
    if (-4..6).contains(&exponent) {
        format!("{value}")
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
    }
}

fn tag_set(tags: &[String]) -> HashSet<String> {
    tags.iter().map(|tag| tag.trim().to_lowercase()).collect()
}

/// Case-insensitive string equality mirroring Go `strings.EqualFold`.
fn str_equal_fold(left: &str, right: &str) -> bool {
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();
    loop {
        match (left_chars.next(), right_chars.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) => {
                if !fuzzy::equal_fold(a, b) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Byte-level substring search, so an offset may advance past a rune boundary
/// exactly like Go's `strings.Index` on a byte string.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn previous_char(text: &str, index: usize) -> char {
    text.get(..index)
        .and_then(|slice| slice.chars().next_back())
        .unwrap_or('\0')
}

fn next_char(text: &str, index: usize) -> char {
    text.get(index..)
        .and_then(|slice| slice.chars().next())
        .unwrap_or('\0')
}

fn is_word_rune(value: char) -> bool {
    value.is_alphanumeric() || value == '_'
}

/// Orders two docs at equal score: most-recent `UpdatedAt` first (missing is
/// oldest), then `Path` ascending.
fn compare_recent(left: &Document, right: &Document) -> Ordering {
    match (
        parse_optional_instant(left.updated_at.as_deref()),
        parse_optional_instant(right.updated_at.as_deref()),
    ) {
        (Some(left_at), Some(right_at)) => {
            if left_at != right_at {
                return right_at.cmp(&left_at);
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }
    left.path.cmp(&right.path)
}

fn parse_optional_instant(value: Option<&str>) -> Option<Instant> {
    value.and_then(|text| parse_one_date(text).ok().map(|(instant, _)| instant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::parse;
    use serde_json::json;

    fn parsed(input: &str) -> Query {
        parse(input).unwrap()
    }

    /// Builds a document with the fields ranking cares about.
    fn doc(path: &str, title: &str, blob: &str) -> Document {
        Document {
            path: path.to_owned(),
            title: title.to_owned(),
            search_blob: blob.to_owned(),
            ..Document::default()
        }
    }

    fn raw(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn paths<'a>(docs: &[&'a Document]) -> Vec<&'a str> {
        docs.iter().map(|doc| doc.path.as_str()).collect()
    }

    // --- fuzzy matcher port ---

    #[test]
    fn fuzzy_scores_match_go_reference() {
        type FuzzyCase = (&'static str, &'static str, Option<(i32, &'static [usize])>);
        let cases: &[FuzzyCase] = &[
            ("mdu", "Ship mdfu MVP", Some((0, &[5, 6, 8]))),
            ("hlo", "Hello World", Some((2, &[0, 2, 4]))),
            ("tk", "The Black Knight", Some((16, &[0, 10]))),
            ("butter", "Buttermilk", Some((611, &[0, 1, 2, 3, 4, 5]))),
            ("xyz", "Hello World", None),
            ("kumquat", "Miss", None),
            ("al", "Alice", Some((12, &[0, 1]))),
            ("al", "Bob", None),
            ("al", "Allie", Some((12, &[0, 1]))),
            ("mnr", "game.cpp", None),
            ("mnr", "moduleNameResolver.ts", Some((32, &[0, 6, 10]))),
            ("mnr", "my name is_Ramsey", Some((36, &[0, 3, 11]))),
            ("a", "abc", Some((8, &[0]))),
            ("abc", "abc", Some((30, &[0, 1, 2]))),
            ("fb", "foo_bar", Some((25, &[0, 4]))),
            ("fb", "FooBar", Some((26, &[0, 3]))),
            ("fb", "foo bar", Some((25, &[0, 4]))),
            ("ss", "Super-Sonic", Some((21, &[0, 6]))),
            ("x", "x", Some((10, &[0]))),
            ("X", "x", Some((10, &[0]))),
            ("x", "X", Some((10, &[0]))),
            ("camel", "camelCaseWord", Some((202, &[0, 1, 2, 3, 4]))),
            ("ccw", "camelCaseWord", Some((40, &[0, 5, 9]))),
            ("tc", "The quick brown fox", Some((-7, &[0, 7]))),
            ("zzz", "The quick brown fox", None),
            ("", "anything", None),
            ("b", "aB", Some((14, &[1]))),
            ("path", "some/path/file", Some((60, &[5, 6, 7, 8]))),
        ];
        for (pattern, text, expected) in cases {
            let found = fuzzy::find(pattern, &[text]);
            match expected {
                None => assert!(
                    found.is_empty(),
                    "expected no match for {pattern:?} {text:?}"
                ),
                Some((score, indexes)) => {
                    assert_eq!(
                        found.len(),
                        1,
                        "expected one match for {pattern:?} {text:?}"
                    );
                    assert_eq!(found[0].score, *score, "score for {pattern:?} {text:?}");
                    assert_eq!(
                        found[0].matched_indexes, *indexes,
                        "indexes for {pattern:?} {text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn fuzzy_matches_helper_and_sort() {
        assert!(fuzzy::matches("hlo", "Hello World"));
        assert!(!fuzzy::matches("xyz", "Hello World"));
        assert!(!fuzzy::matches("", "anything"));
        // Multiple candidates sort by descending score, stable on ties.
        let found = fuzzy::find("al", &["Alice", "Bob", "Allie"]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "Alice");
        assert_eq!(found[1].text, "Allie");
        assert_eq!(found[0].index, 0);
        assert_eq!(found[1].index, 2);
    }

    #[test]
    fn fuzzy_stops_at_first_nul_like_go() {
        let found = fuzzy::find("abc", &["abc\0xyz"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].score, 30);
    }

    // --- contains_word / title boost ---

    #[test]
    fn contains_word_matches_go_table() {
        let cases = [
            ("butter", "butter", true),
            ("buttermilk", "butter", false),
            ("butternut squash", "butter", false),
            ("unsalted butter", "butter", true),
            ("butter-chicken", "butter", true),
            ("peanut butter", "butter", true),
            ("", "butter", false),
        ];
        for (text, word, want) in cases {
            assert_eq!(
                contains_word(text, word),
                want,
                "contains_word({text:?}, {word:?})"
            );
        }
    }

    #[test]
    fn title_boost_is_mutually_exclusive() {
        assert_eq!(title_match_boost("butter", "butter"), EXACT_TITLE_BOOST);
        assert_eq!(title_match_boost("butter chicken", "butter"), TITLE_BOOST);
        assert_eq!(
            title_match_boost("buttermilk", "butter"),
            PARTIAL_TITLE_BOOST
        );
        assert_eq!(title_match_boost("unrelated", "butter"), 0);
        assert_eq!(title_match_boost("", "butter"), 0);
        assert_eq!(title_match_boost("butter", ""), 0);
        assert_eq!(
            title_match_boost("butterchicken", "chicken"),
            PARTIAL_TITLE_BOOST
        );
        assert_eq!(title_match_boost("butter chicken", "chicken"), TITLE_BOOST);
    }

    // --- matches_doc hard filters ---

    #[test]
    fn matches_tags_required_and_negated() {
        let document = Document {
            path: "a.md".to_owned(),
            tags: vec!["Foo".to_owned(), "Bar".to_owned()],
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("tag:foo"))));
        assert!(matches_doc(&document, Some(&parsed("tags:foo,bar"))));
        assert!(!matches_doc(&document, Some(&parsed("tag:baz"))));
        assert!(!matches_doc(&document, Some(&parsed("tag:-foo"))));
        assert!(matches_doc(&document, Some(&parsed("tag:-baz"))));
    }

    #[test]
    fn matches_doctype_exact_fold() {
        let document = Document {
            path: "a.md".to_owned(),
            doc_type: "Note".to_owned(),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("type:note"))));
        assert!(!matches_doc(&document, Some(&parsed("type:metric"))));
    }

    #[test]
    fn matches_title_substring_or_fuzzy() {
        let document = Document {
            path: "a.md".to_owned(),
            title: "Hello World".to_owned(),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("title:hello"))));
        assert!(matches_doc(&document, Some(&parsed("title:hlo"))));
        assert!(!matches_doc(&document, Some(&parsed("title:xyz"))));
    }

    #[test]
    fn matches_status_and_archived() {
        let draft = Document {
            path: "a.md".to_owned(),
            status: "draft".to_owned(),
            ..Document::default()
        };
        assert!(matches_doc(&draft, Some(&parsed("status:draft"))));
        assert!(matches_doc(&draft, Some(&parsed("status:DRAFT"))));
        assert!(!matches_doc(&draft, Some(&parsed("status:published"))));
        assert!(!matches_doc(&draft, Some(&parsed("status:archived"))));

        let flagged = Document {
            path: "b.md".to_owned(),
            archived: Some(true),
            ..Document::default()
        };
        assert!(matches_doc(&flagged, Some(&parsed("status:archived"))));

        let by_status = Document {
            path: "c.md".to_owned(),
            status: "archived".to_owned(),
            ..Document::default()
        };
        assert!(matches_doc(&by_status, Some(&parsed("status:archived"))));
    }

    #[test]
    fn matches_path_substring_fold() {
        let document = Document {
            path: "notes/sub/dir.md".to_owned(),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("path:sub/"))));
        assert!(matches_doc(&document, Some(&parsed("path:SUB"))));
        assert!(!matches_doc(&document, Some(&parsed("path:other"))));
    }

    #[test]
    fn matches_generic_raw_keys() {
        let document = Document {
            path: "a.md".to_owned(),
            raw: raw(&json!({"author": "John Doe", "count": 42})),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("author:john"))));
        assert!(matches_doc(&document, Some(&parsed("AUTHOR:doe"))));
        assert!(matches_doc(&document, Some(&parsed("count:42"))));
        assert!(!matches_doc(&document, Some(&parsed("author:xyz"))));
        assert!(!matches_doc(&document, Some(&parsed("missing:value"))));
    }

    #[test]
    fn matches_generic_normalized_fields() {
        let document = Document {
            path: "a.md".to_owned(),
            doc_type: "Note".to_owned(),
            resource: "https://example.com".to_owned(),
            organized: Some(true),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("resource:example"))));
        assert!(matches_doc(&document, Some(&parsed("organized:true"))));
        assert!(!matches_doc(&document, Some(&parsed("organized:false"))));

        let archived = Document {
            path: "b.md".to_owned(),
            archived: Some(true),
            ..Document::default()
        };
        assert!(matches_doc(&archived, Some(&parsed("archived:true"))));
    }

    #[test]
    fn matches_dates_and_either_semantics() {
        let document = Document {
            path: "a.md".to_owned(),
            created_at: Some("2024-03-15T00:00:00Z".to_owned()),
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("created:2024-03-15"))));
        assert!(!matches_doc(&document, Some(&parsed("created:2024-01-01"))));
        assert!(matches_doc(
            &document,
            Some(&parsed("created:2024-01-01..2024-12-31"))
        ));
        assert!(matches_doc(
            &document,
            Some(&parsed("updated:>=2024-01-01"))
        ));
        assert!(!matches_doc(
            &document,
            Some(&parsed("updated:<2024-01-01"))
        ));
        assert!(matches_doc(&document, Some(&parsed("date:2024-06-01"))));
        assert!(matches_doc(&document, Some(&parsed("date:2024-03-15"))));
        assert!(!matches_doc(&document, Some(&parsed("date:2023-01-01"))));
    }

    #[test]
    fn matches_nil_dates_only_excluded_by_present_filter() {
        let nil_doc = Document {
            path: "b.md".to_owned(),
            ..Document::default()
        };
        assert!(!matches_doc(&nil_doc, Some(&parsed("created:2024-01-01"))));
        assert!(matches_doc(&nil_doc, Some(&parsed("hello"))));
        assert!(matches_doc(&nil_doc, Some(&parsed(""))));
        assert!(matches_doc(&nil_doc, None));
    }

    #[test]
    fn matches_rfc3339_instants() {
        let document = Document {
            path: "a.md".to_owned(),
            created_at: Some("2024-03-15T10:00:00Z".to_owned()),
            ..Document::default()
        };
        assert!(matches_doc(&document, Some(&parsed("created:2024-03-15"))));
        assert!(!matches_doc(&document, Some(&parsed("created:2024-03-16"))));
    }

    // --- normalized lookup helpers ---

    #[test]
    fn lookup_field_covers_normalized_and_relations() {
        let document = Document {
            path: "a.md".to_owned(),
            doc_type: "Note".to_owned(),
            title: "A".to_owned(),
            description: "desc".to_owned(),
            status: "draft".to_owned(),
            resource: "res".to_owned(),
            role: "log".to_owned(),
            tags: vec!["x".to_owned(), "y".to_owned()],
            body: "body text".to_owned(),
            created_at: Some("2024-03-15T00:00:00Z".to_owned()),
            updated_at: Some("2024-06-01T12:30:00Z".to_owned()),
            belongs_to: vec!["Project".to_owned()],
            related_to: vec!["Launch".to_owned()],
            ..Document::default()
        };
        assert_eq!(lookup_field(&document, "type").as_deref(), Some("Note"));
        assert_eq!(
            lookup_field(&document, "belongs_to").as_deref(),
            Some("Project")
        );
        assert_eq!(
            lookup_field(&document, "related_to").as_deref(),
            Some("Launch")
        );
        assert_eq!(lookup_field(&document, "tag").as_deref(), Some("x y"));
        assert_eq!(
            lookup_field(&document, "body").as_deref(),
            Some("body text")
        );
        assert_eq!(lookup_field(&document, "format").as_deref(), Some("none"));
        assert_eq!(
            lookup_field(&document, "created").as_deref(),
            Some("2024-03-15 2024-03-15T00:00:00Z 2024-06-01 2024-06-01T12:30:00Z")
        );
        assert_eq!(lookup_field(&document, "nope"), None);
    }

    // --- score_doc exact scores ---

    #[test]
    fn score_doc_matches_go_reference() {
        let cases = [
            (
                doc("butter.md", "Butter", "Butter\nunsalted butter"),
                "butter",
                Some(206),
            ),
            (
                doc("buttermilk.md", "Buttermilk", "Buttermilk\npancakes"),
                "butter",
                Some(56),
            ),
            (
                doc("butternut.md", "Butternut squash", "Butternut squash\nsoup"),
                "butter",
                Some(56),
            ),
            (
                doc(
                    "bodyonly.md",
                    "unrelated notes",
                    "unrelated notes\nfresh butter from the market",
                ),
                "butter",
                Some(6),
            ),
            (
                doc("metric_a.md", "metric overview", "metric data here"),
                "metric",
                Some(106),
            ),
            (
                doc("metric_b.md", "unrelated notes", "metric data here"),
                "metric",
                Some(6),
            ),
            (
                doc("ship.md", "Ship mdfu MVP", "Ship mdfu MVP\nbody one"),
                "mdu",
                Some(0),
            ),
            (
                doc("hit.md", "Hit", "Hit\nwe bought a kumquat at the market"),
                "kumquat",
                Some(7),
            ),
        ];
        for (document, query, expected) in cases {
            assert_eq!(
                score_doc(&document, &[query]),
                expected,
                "score_doc({}, {query:?})",
                document.path
            );
        }
    }

    #[test]
    fn score_doc_blob_fallback_and_and_semantics() {
        let fallback = Document {
            path: "a.md".to_owned(),
            title: "hello world".to_owned(),
            body: "some body text".to_owned(),
            ..Document::default()
        };
        assert_eq!(score_doc(&fallback, &["hello"]), Some(105));
        assert_eq!(score_doc(&fallback, &["hello", "world"]), Some(210));

        let butter = doc("butter.md", "Butter", "Butter\nunsalted butter");
        assert_eq!(score_doc(&butter, &["butter", "pancakes"]), None);
    }

    // --- rank behavior ---

    #[test]
    fn rank_title_boost_beats_body_only() {
        let blob = "metric data here".to_owned();
        let title_hit = doc("a.md", "metric overview", &blob);
        let body_only = doc("b.md", "unrelated notes", &blob);
        let docs = [body_only, title_hit];
        let ranked = rank(&docs, Some(&parsed("metric")));
        assert_eq!(paths(&ranked), ["a.md", "b.md"]);
    }

    #[test]
    fn rank_exact_word_beats_longer_word_and_recency() {
        let exact = Document {
            updated_at: Some("2024-01-01T00:00:00Z".to_owned()),
            ..doc("butter.md", "Butter", "Butter\nunsalted butter")
        };
        let milk = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("buttermilk.md", "Buttermilk", "Buttermilk\npancakes")
        };
        let nut = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("butternut.md", "Butternut squash", "Butternut squash\nsoup")
        };
        let docs = [milk, nut, exact];
        let ranked = rank(&docs, Some(&parsed("butter")));
        assert_eq!(
            paths(&ranked),
            ["butter.md", "buttermilk.md", "butternut.md"]
        );
    }

    #[test]
    fn rank_empty_bare_sorts_by_path() {
        let b = doc("b.md", "B", "");
        let a = doc("a.md", "A", "");
        let docs = [b, a];
        let ranked = rank(&docs, Some(&parsed("")));
        assert_eq!(paths(&ranked), ["a.md", "b.md"]);
    }

    #[test]
    fn rank_tie_break_updated_then_path() {
        let recent = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("a.md", "same", "same blob")
        };
        let old = Document {
            updated_at: Some("2024-01-01T00:00:00Z".to_owned()),
            ..doc("b.md", "same", "same blob")
        };
        let docs = [old, recent];
        let ranked = rank(&docs, Some(&parsed("same")));
        assert_eq!(paths(&ranked), ["a.md", "b.md"]);

        // Most-recent wins even when its path sorts later.
        let later_path = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("z.md", "same", "same blob")
        };
        let earlier_path = Document {
            updated_at: Some("2024-01-01T00:00:00Z".to_owned()),
            ..doc("a.md", "same", "same blob")
        };
        let docs = [earlier_path, later_path];
        let ranked = rank(&docs, Some(&parsed("same")));
        assert_eq!(paths(&ranked), ["z.md", "a.md"]);

        // A present UpdatedAt beats a missing one at equal score.
        let dated = Document {
            updated_at: Some("2024-01-01T00:00:00Z".to_owned()),
            ..doc("z.md", "same", "same blob")
        };
        let undated = doc("a.md", "same", "same blob");
        let docs = [undated, dated];
        let ranked = rank(&docs, Some(&parsed("same")));
        assert_eq!(paths(&ranked), ["z.md", "a.md"]);

        // No timestamps: path tie-break.
        let path_b = doc("b.md", "same", "same blob");
        let path_a = doc("a.md", "same", "same blob");
        let docs = [path_b, path_a];
        let ranked = rank(&docs, Some(&parsed("same")));
        assert_eq!(paths(&ranked), ["a.md", "b.md"]);
    }

    #[test]
    fn rank_hard_filters_and_blob_fallback() {
        let tagged_x = Document {
            path: "a.md".to_owned(),
            title: "hello".to_owned(),
            tags: vec!["x".to_owned()],
            search_blob: "hello".to_owned(),
            ..Document::default()
        };
        let tagged_y = Document {
            path: "b.md".to_owned(),
            title: "hello".to_owned(),
            tags: vec!["y".to_owned()],
            search_blob: "hello".to_owned(),
            ..Document::default()
        };
        let docs = [tagged_x, tagged_y];
        let ranked = rank(&docs, Some(&parsed("hello tag:x")));
        assert_eq!(paths(&ranked), ["a.md"]);

        let fallback = Document {
            path: "a.md".to_owned(),
            title: "hello world".to_owned(),
            body: "some body text".to_owned(),
            ..Document::default()
        };
        let docs = [fallback];
        let ranked = rank(&docs, Some(&parsed("hello")));
        assert_eq!(paths(&ranked), ["a.md"]);

        // A `None` query keeps every doc, sorted by path.
        let docs = [doc("b.md", "B", ""), doc("a.md", "A", "")];
        let ranked = rank(&docs, None);
        assert_eq!(paths(&ranked), ["a.md", "b.md"]);
    }

    #[test]
    fn rank_does_not_hide_archived() {
        let active = doc("a.md", "", "");
        let flagged = Document {
            path: "b.md".to_owned(),
            archived: Some(true),
            ..Document::default()
        };
        let by_status = Document {
            path: "c.md".to_owned(),
            status: "archived".to_owned(),
            ..Document::default()
        };
        let docs = [active, flagged, by_status];
        let ranked = rank(&docs, Some(&parsed("")));
        assert_eq!(paths(&ranked), ["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn filter_archived_hides_and_keeps() {
        let active = doc("a.md", "", "");
        let flagged = Document {
            path: "b.md".to_owned(),
            archived: Some(true),
            ..Document::default()
        };
        let by_status = Document {
            path: "c.md".to_owned(),
            status: "archived".to_owned(),
            ..Document::default()
        };
        let docs = [active, flagged, by_status];
        let hidden = filter_archived(&docs, false);
        assert_eq!(paths(&hidden), ["a.md"]);
        assert_eq!(
            paths(&filter_archived(&docs, true)),
            ["a.md", "b.md", "c.md"]
        );
    }

    // --- regression: no subsequence fuzzy over the body ---

    #[test]
    fn bare_word_is_not_fuzzy_matched_against_body() {
        let mut filler = String::new();
        for _ in 0..200 {
            filler.push_str("kangaroo under mountain quickly uses a telescope. ");
        }
        let hit = doc("hit.md", "Hit", "Hit\nwe bought a kumquat at the market");
        let miss = Document {
            path: "miss.md".to_owned(),
            title: "Miss".to_owned(),
            search_blob: format!("Miss\n{filler}"),
            ..Document::default()
        };
        let docs = [miss, hit];
        let ranked = rank(&docs, Some(&parsed("kumquat")));
        assert_eq!(paths(&ranked), ["hit.md"]);
    }

    // --- differential ordering: Rust order == recorded Go order ---

    #[test]
    fn rank_order_matches_go_corpus() {
        // butter family: exact > whole/partial word > body-only.
        let butter = Document {
            updated_at: Some("2024-01-01T00:00:00Z".to_owned()),
            ..doc("butter.md", "Butter", "Butter\nunsalted butter")
        };
        let milk = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("buttermilk.md", "Buttermilk", "Buttermilk\npancakes")
        };
        let nut = Document {
            updated_at: Some("2024-06-01T00:00:00Z".to_owned()),
            ..doc("butternut.md", "Butternut squash", "Butternut squash\nsoup")
        };
        let body = Document {
            updated_at: Some("2024-06-02T00:00:00Z".to_owned()),
            ..doc(
                "bodyonly.md",
                "unrelated notes",
                "unrelated notes\nfresh butter from the market",
            )
        };
        let butter_docs = [milk, nut, body, butter];
        assert_eq!(
            paths(&rank(&butter_docs, Some(&parsed("butter")))),
            ["butter.md", "buttermilk.md", "butternut.md", "bodyonly.md"]
        );
        assert_eq!(
            paths(&rank(&butter_docs, Some(&parsed("butter pancakes")))),
            ["buttermilk.md"]
        );

        // title whole-word boost outranks a body-only substring.
        let metric_title = doc("metric_a.md", "metric overview", "metric data here");
        let metric_body = doc("metric_b.md", "unrelated notes", "metric data here");
        let metric_docs = [metric_body, metric_title];
        assert_eq!(
            paths(&rank(&metric_docs, Some(&parsed("metric")))),
            ["metric_a.md", "metric_b.md"]
        );

        // fuzzy title match only.
        let ship = doc("ship.md", "Ship mdfu MVP", "Ship mdfu MVP\nbody one");
        let garden = doc(
            "garden.md",
            "Unrelated gardening notes",
            "Unrelated gardening notes\nbody two",
        );
        assert_eq!(
            paths(&rank(&[garden, ship], Some(&parsed("mdu")))),
            ["ship.md"]
        );

        // empty bare => path sort.
        let path_b = doc("b.md", "B", "");
        let path_a = doc("a.md", "A", "");
        assert_eq!(
            paths(&rank(&[path_b, path_a], Some(&parsed("")))),
            ["a.md", "b.md"]
        );

        // hard tag filter.
        let tagged_x = Document {
            tags: vec!["x".to_owned()],
            ..doc("a.md", "hello", "hello")
        };
        let tagged_y = Document {
            tags: vec!["y".to_owned()],
            ..doc("b.md", "hello", "hello")
        };
        assert_eq!(
            paths(&rank(&[tagged_x, tagged_y], Some(&parsed("hello tag:x")))),
            ["a.md"]
        );
    }
}
