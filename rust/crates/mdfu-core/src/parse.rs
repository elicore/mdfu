//! Frontmatter splitting and normalization.
//!
//! Rust port of `internal/parse/parse.go`. Parsing never fails hard: malformed
//! frontmatter is recorded on `Document::parse_error` while the body (and any
//! H1/filename-derived title) stays searchable. [`parse_content`] is
//! infallible; only [`parse_file`] can fail, and only on I/O errors.

#![allow(clippy::module_name_repetitions)]

use std::collections::HashSet;
use std::fmt::{self, Write as _};
use std::fs;
use std::io;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Number, Value};

use crate::model::{Attestation, Document, FormatKind, Source};

/// Splits leading `---` frontmatter from the body.
///
/// Only a document that starts with an opening `---` line is treated as having
/// frontmatter; without a closing `---` line the whole content is the body and
/// `has_frontmatter` is false. A leading UTF-8 BOM is stripped first, exactly
/// like Go `SplitFrontmatter`.
#[must_use]
pub fn split_frontmatter(content: &str) -> (String, String, bool) {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(rest) = content.strip_prefix("---") else {
        return (String::new(), content.to_owned(), false);
    };
    let newline_after_dashes = matches!(rest.as_bytes().first(), Some(b'\n' | b'\r'));
    if !newline_after_dashes {
        return (String::new(), content.to_owned(), false);
    }
    let rest = rest.trim_start_matches(['\r', '\n']);
    let lines: Vec<&str> = rest.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            let frontmatter = lines[..index].join("\n");
            let body = lines[index + 1..].join("\n");
            return (frontmatter, body, true);
        }
    }
    (String::new(), content.to_owned(), false)
}

/// Reads the file at `path` and parses it via [`parse_content`].
///
/// # Errors
///
/// Propagates any I/O error from reading `path` (mirrors Go `ParseFile`). The
/// bytes are decoded lossily, so invalid UTF-8 never loses the body.
#[must_use = "parse_file returns a Result that should be handled"]
pub fn parse_file(path: &str) -> io::Result<Document> {
    let bytes = fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes);
    Ok(parse_content(path, &content))
}

/// Parses markdown content into a normalized [`Document`].
///
/// The frontmatter is YAML-decoded into a generic map; a YAML error never fails
/// hard — it is recorded on `Document::parse_error` and the returned document
/// still carries the body (and any filename/H1-derived title) so it stays
/// searchable.
#[must_use]
pub fn parse_content(path: &str, content: &str) -> Document {
    let mut document = Document {
        path: path.to_owned(),
        ..Document::default()
    };
    let (frontmatter, body, has_frontmatter) = split_frontmatter(content);
    document.body = body;
    if has_frontmatter {
        match parse_yaml(&frontmatter) {
            Ok(raw) => document.raw = raw,
            Err(error) => document.parse_error = Some(error),
        }
    }
    normalize(&mut document, has_frontmatter);
    let _ = document.rebuild_search_blob();
    document
}

/// Fills the normalized fields of `document` from its raw frontmatter map.
///
/// Order of operations matches Go `normalize`, including the title fallback
/// chain frontmatter title -> H1 -> filename base.
fn normalize(document: &mut Document, has_frontmatter: bool) {
    let raw = std::mem::take(&mut document.raw);

    if let Some(value) = lookup(&raw, "type") {
        as_string(value).trim().clone_into(&mut document.doc_type);
    }
    let frontmatter_title = lookup(&raw, "title")
        .map(|value| as_string(value).trim().to_owned())
        .filter(|title| !title.is_empty());
    document.title = if let Some(title) = frontmatter_title {
        title
    } else {
        let heading = extract_h1(&document.body);
        if heading.is_empty() {
            filename_base(&document.path)
        } else {
            heading
        }
    };
    if let Some(value) = lookup(&raw, "description") {
        as_string(value)
            .trim()
            .clone_into(&mut document.description);
    }
    if let Some(value) = lookup(&raw, "resource") {
        as_string(value).trim().clone_into(&mut document.resource);
    }
    if let Some(value) = lookup(&raw, "tags") {
        document.tags = parse_string_list(value);
    }
    if let Some(value) = lookup(&raw, "status") {
        document.status = as_string(value).trim().to_lowercase();
    }
    if let Some(value) = lookup(&raw, "organized") {
        if let Some(flag) = as_bool(value) {
            document.organized = Some(flag);
        }
    }
    if let Some(value) = lookup(&raw, "archived") {
        if let Some(flag) = as_bool(value) {
            document.archived = Some(flag);
        }
    }
    if let Some(value) = lookup(&raw, "belongs_to").or_else(|| lookup(&raw, "belongs-to")) {
        document.belongs_to = parse_string_list(value);
    }
    if let Some(value) = lookup(&raw, "related_to").or_else(|| lookup(&raw, "related-to")) {
        document.related_to = parse_string_list(value);
    }
    if let Some(value) = lookup(&raw, "sources") {
        document.sources = parse_sources(value);
    }
    if let Some(value) = lookup(&raw, "generated") {
        document.generated = parse_attestation(value);
    }
    if let Some(value) = lookup(&raw, "verified") {
        document.verified = parse_attestation(value);
    }

    document.created_at = first_date(&raw, &["created", "date", "timestamp", "generated"]);
    document.updated_at = first_date(
        &raw,
        &[
            "generated",
            "timestamp",
            "last_modified",
            "updated",
            "modified",
        ],
    );

    document.role = role_for_path(&document.path).to_owned();
    document.format = detect_format(has_frontmatter, &document.doc_type, &raw);
    document.raw = raw;
}

/// Parses the frontmatter YAML into a string-keyed map.
///
/// Mirrors Go `yaml.Unmarshal` into `map[string]any`: duplicate keys are
/// rejected, merge keys (`<<`) are applied, and an empty/comment-only document
/// yields an empty map rather than an error.
fn parse_yaml(frontmatter: &str) -> Result<Map<String, Value>, String> {
    if is_blank_or_comment(frontmatter) {
        return Ok(Map::new());
    }
    let parsed =
        yaml_serde::from_str::<StrictValue>(frontmatter).map_err(|error| error.to_string())?;
    match parsed.0 {
        Value::Null => Ok(Map::new()),
        Value::Object(object) => Ok(object),
        other => Err(format!(
            "cannot unmarshal {} into a frontmatter map",
            value_type_name(&other)
        )),
    }
}

/// Reports whether the frontmatter is only blank lines and YAML comments.
fn is_blank_or_comment(frontmatter: &str) -> bool {
    frontmatter.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

/// Human-readable kind for a non-mapping YAML root, used in parse errors.
fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "sequence",
        Value::Object(_) => "mapping",
    }
}

/// Finds a key in a frontmatter map case-insensitively.
///
/// Iteration is deterministic (sorted) where Go map iteration is not; only
/// keys that differ *solely* by case resolve differently, and Go itself leaves
/// that ordering unspecified.
fn lookup<'a>(raw: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    raw.iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

/// Returns the first parseable date found under `keys`, in order.
///
/// The key `generated` resolves to its nested `at` value (i.e.
/// `generated.at`), matching Go `firstDate`.
fn first_date(raw: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = lookup(raw, key) else {
            continue;
        };
        if key.eq_ignore_ascii_case("generated") {
            if let Value::Object(attestation) = value {
                if let Some(at) = lookup(attestation, "at") {
                    if let Some(date) = parse_date_value(at) {
                        return Some(date);
                    }
                }
                continue;
            }
        }
        if let Some(date) = parse_date_value(value) {
            return Some(date);
        }
    }
    None
}

/// Classifies the document family from `type` and Portent lifecycle keys.
fn detect_format(has_frontmatter: bool, doc_type: &str, raw: &Map<String, Value>) -> FormatKind {
    if !has_frontmatter {
        return FormatKind::None;
    }
    if !doc_type.is_empty() {
        return if is_portent_type(doc_type) {
            FormatKind::Portent
        } else {
            FormatKind::Okf
        };
    }
    if lookup(raw, "organized").is_some() || lookup(raw, "archived").is_some() {
        return FormatKind::Portent;
    }
    FormatKind::Generic
}

/// Reports whether `doc_type` (normalized) identifies a Portent/Tolaria doc.
fn is_portent_type(doc_type: &str) -> bool {
    matches!(
        doc_type.trim().to_lowercase().as_str(),
        "project" | "operation" | "responsibility" | "task" | "event" | "note" | "topic" | "person"
    )
}

/// Returns `index` for `index.md` basenames, `log` for `log.md` basenames
/// (case-insensitive), and `""` otherwise.
fn role_for_path(path: &str) -> &'static str {
    let base = filepath_base(path).to_lowercase();
    match base.as_str() {
        "index.md" => "index",
        "log.md" => "log",
        _ => "",
    }
}

/// `filepath.Base` equivalent: trailing separators are dropped.
fn filepath_base(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "." } else { "/" };
    }
    match trimmed.rfind('/') {
        Some(index) => &trimmed[index + 1..],
        None => trimmed,
    }
}

/// Returns the file base name without extension.
fn filename_base(path: &str) -> String {
    let base = filepath_base(path);
    match base.rfind('.') {
        Some(index) => base[..index].to_owned(),
        None => base.to_owned(),
    }
}

/// Renders a frontmatter scalar as a string, mirroring Go `asString`.
fn as_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number_to_string(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let rendered: Vec<String> = items.iter().map(as_string).collect();
            format!("[{}]", rendered.join(" "))
        }
        Value::Object(map) => {
            let rendered: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{key}:{}", as_string(value)))
                .collect();
            format!("map[{}]", rendered.join(" "))
        }
    }
}

/// Formats a JSON number the way Go `strconv.FormatFloat(t, 'f', -1, 64)` does
/// for `float64` frontmatter scalars.
fn number_to_string(number: &Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    match number.as_f64() {
        Some(float) => format_float(float),
        None => number.to_string(),
    }
}

/// Shortest decimal (`'f'`) rendering of a float, matching Go `%v` spellings
/// for the special values (`NaN`, `+Inf`, `-Inf`).
fn format_float(value: f64) -> String {
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
    format!("{value}")
}

/// Coerces a frontmatter value to a bool, mirroring Go `asBool`.
///
/// `yes`/`no`/`y`/`n` are deliberately *not* accepted: Go `yaml.v3` only
/// resolves YAML 1.1 booleans for typed bool targets, so `map[string]any`
/// holds them as strings and `strconv.ParseBool` rejects them.
#[allow(clippy::float_cmp)] // exact zero check mirrors Go `t != 0`
fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) => parse_go_bool(text.trim().to_lowercase().as_str()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Some(int != 0)
            } else if number.as_u64().is_some() {
                None
            } else {
                number.as_f64().map(|float| float != 0.0)
            }
        }
        _ => None,
    }
}

/// The string forms accepted by Go `strconv.ParseBool`.
fn parse_go_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "true" => Some(true),
        "0" | "f" | "false" => Some(false),
        _ => None,
    }
}

/// Removes surrounding `[[ ]]` markers and whitespace.
fn strip_wikilink(text: &str) -> String {
    let trimmed = text.trim();
    let stripped = if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
        &trimmed[2..trimmed.len() - 2]
    } else {
        trimmed
    };
    stripped.trim().to_owned()
}

/// Accepts a list, a single string, or a comma-separated string and returns
/// cleaned items with `[[ ]]` markers stripped.
fn parse_string_list(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match value {
        Value::Null => {}
        Value::String(text) => append_split_items(text, &mut out),
        Value::Array(items) => {
            for item in items {
                if let Value::String(text) = item {
                    append_split_items(text, &mut out);
                } else {
                    let cleaned = as_string(item).trim().to_owned();
                    if !cleaned.is_empty() {
                        out.push(strip_wikilink(&cleaned));
                    }
                }
            }
        }
        other => {
            let cleaned = as_string(other).trim().to_owned();
            if !cleaned.is_empty() {
                out.push(strip_wikilink(&cleaned));
            }
        }
    }
    out
}

/// Splits one string element on commas and strips wikilinks from each part.
fn append_split_items(text: &str, out: &mut Vec<String>) {
    for part in text.split(',') {
        let cleaned = strip_wikilink(part);
        if !cleaned.is_empty() {
            out.push(cleaned);
        }
    }
}

/// Normalizes a `sources` list into [`Source`] entries.
fn parse_sources(value: &Value) -> Vec<Source> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Value::Object(map) = item else {
            continue;
        };
        let mut source = Source::default();
        if let Some(field) = lookup(map, "id") {
            as_string(field).trim().clone_into(&mut source.id);
        }
        if let Some(field) = lookup(map, "resource") {
            as_string(field).trim().clone_into(&mut source.resource);
        }
        if let Some(field) = lookup(map, "title") {
            as_string(field).trim().clone_into(&mut source.title);
        }
        if let Some(field) = lookup(map, "author") {
            as_string(field).trim().clone_into(&mut source.author);
        }
        out.push(source);
    }
    out
}

/// Normalizes a `{by, at}` map into an [`Attestation`]; non-maps yield `None`.
fn parse_attestation(value: &Value) -> Option<Attestation> {
    let Value::Object(map) = value else {
        return None;
    };
    let mut attestation = Attestation::default();
    if let Some(field) = lookup(map, "by") {
        as_string(field).trim().clone_into(&mut attestation.by);
    }
    if let Some(field) = lookup(map, "at") {
        attestation.at = parse_date_value(field);
    }
    Some(attestation)
}

/// Parses a frontmatter date value into an RFC 3339 string.
///
/// Strings are tried against the Go `dateLayouts` list; integers are treated as
/// unix timestamps (float values truncate, exactly like Go `int64(t)`). Values
/// Go would hold as `uint64` (above `i64::MAX`) are rejected like Go's
/// `parseDate` default branch.
#[allow(clippy::cast_possible_truncation)] // Go `int64(float64)` truncates too
fn parse_date_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => parse_date_text(text.trim()),
        Value::Number(number) => {
            if let Some(seconds) = number.as_i64() {
                Some(format_unix(seconds))
            } else if number.as_u64().is_some() {
                None
            } else {
                number.as_f64().map(|seconds| format_unix(seconds as i64))
            }
        }
        _ => None,
    }
}

/// Tries the known date layouts in Go order: RFC 3339, `YYYY-MM-DD`,
/// `YYYY-MM`, and renders the first match as RFC 3339.
fn parse_date_text(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    parse_rfc3339(text)
        .or_else(|| parse_full_date(text))
        .or_else(|| parse_year_month(text))
        .map(DateTime::format_rfc3339)
}

/// A civil date-time with a fixed UTC offset, rendered as RFC 3339.
#[derive(Debug, Clone, Copy)]
struct DateTime {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    offset_minutes: i32,
}

impl DateTime {
    /// Midnight UTC on the given date.
    fn utc(year: i64, month: u32, day: u32) -> Self {
        Self {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            offset_minutes: 0,
        }
    }

    /// Renders the instant the way Go `time.Time.Format(time.RFC3339)` does.
    fn format_rfc3339(self) -> String {
        let mut rendered = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.offset_minutes == 0 {
            rendered.push('Z');
        } else {
            let offset = i64::from(self.offset_minutes);
            let sign = if offset < 0 { '-' } else { '+' };
            let absolute = offset.unsigned_abs();
            let _ = write!(rendered, "{sign}{:02}:{:02}", absolute / 60, absolute % 60);
        }
        rendered
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` leniently like Go
/// `time.Parse(time.RFC3339, s)` (1-2 digit fields, 4-digit year).
fn parse_rfc3339(text: &str) -> Option<DateTime> {
    let mut index = 0;
    let (year, next) = take_year(text, index)?;
    index = expect(text, next, '-')?;
    let (month, next) = take_digits(text, index, 2)?;
    index = expect(text, next, '-')?;
    let (day, next) = take_digits(text, index, 2)?;
    index = expect(text, next, 'T')?;
    let (hour, next) = take_digits(text, index, 2)?;
    index = expect(text, next, ':')?;
    let (minute, next) = take_digits(text, index, 2)?;
    index = expect(text, next, ':')?;
    let (second, next) = take_digits(text, index, 2)?;
    let bytes = text.as_bytes();
    if bytes.get(next) == Some(&b'.') {
        index = next + 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == fraction_start {
            return None;
        }
    } else {
        index = next;
    }
    let (offset_minutes, next) = parse_zone(text, index)?;
    if next != text.len() {
        return None;
    }
    validate(year, month, day, hour, minute, second, offset_minutes)?;
    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        offset_minutes,
    })
}

/// Parses a date-only `YYYY-MM-DD` value (Go layout `2006-01-02`).
fn parse_full_date(text: &str) -> Option<DateTime> {
    let mut index = 0;
    let (year, next) = take_year(text, index)?;
    index = expect(text, next, '-')?;
    let (month, next) = take_digits(text, index, 2)?;
    index = expect(text, next, '-')?;
    let (day, next) = take_digits(text, index, 2)?;
    if next != text.len() || validate(year, month, day, 0, 0, 0, 0).is_none() {
        return None;
    }
    Some(DateTime::utc(year, month, day))
}

/// Parses a month-only `YYYY-MM` value (Go layout `2006-01`).
fn parse_year_month(text: &str) -> Option<DateTime> {
    let (year, next) = take_year(text, 0)?;
    let dash = expect(text, next, '-')?;
    let (month, next) = take_digits(text, dash, 2)?;
    if next != text.len() || month == 0 || month > 12 {
        return None;
    }
    Some(DateTime::utc(year, month, 1))
}

/// Parses a zone suffix: `Z` or `±HH:MM`. Returns the offset in minutes.
fn parse_zone(text: &str, index: usize) -> Option<(i32, usize)> {
    match text.as_bytes().get(index) {
        Some(b'Z') => Some((0, index + 1)),
        Some(b'+' | b'-') => {
            let sign: i32 = if text.as_bytes().get(index) == Some(&b'-') {
                -1
            } else {
                1
            };
            let (hours, next) = take_digits(text, index + 1, 2)?;
            let colon = expect(text, next, ':')?;
            let (minutes, next) = take_digits(text, colon, 2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = i32::try_from(hours * 60 + minutes).ok()?;
            Some((sign * offset, next))
        }
        _ => None,
    }
}

/// Reads exactly four ASCII digits as a year.
fn take_year(text: &str, index: usize) -> Option<(i64, usize)> {
    let end = index.checked_add(4)?;
    let digits = text.get(index..end)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|year| (year, end))
}

/// Reads 1..=`max` ASCII digits as an unsigned number.
fn take_digits(text: &str, index: usize, max: usize) -> Option<(u32, usize)> {
    let bytes = text.as_bytes();
    let mut end = index;
    while end < bytes.len() && end - index < max && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == index {
        return None;
    }
    text.get(index..end)?
        .parse::<u32>()
        .ok()
        .map(|value| (value, end))
}

/// Consumes exactly `expected` at `index`, returning the next index.
fn expect(text: &str, index: usize, expected: char) -> Option<usize> {
    let rest = text.get(index..)?;
    if rest.starts_with(expected) {
        Some(index + expected.len_utf8())
    } else {
        None
    }
}

/// Validates civil field ranges; `None` marks an unparseable value.
fn validate(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    offset_minutes: i32,
) -> Option<()> {
    if month == 0 || month > 12 {
        return None;
    }
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    if offset_minutes.abs() > 23 * 60 + 59 {
        return None;
    }
    Some(())
}

/// Days in `month` for `year` (proleptic Gregorian).
fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        _ => 28,
    }
}

/// Proleptic Gregorian leap-year rule.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Renders unix `seconds` as an RFC 3339 UTC string, mirroring
/// `time.Unix(seconds, 0).UTC()`.
fn format_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = u32::try_from(time_of_day / 3_600).unwrap_or_default();
    let minute = u32::try_from((time_of_day % 3_600) / 60).unwrap_or_default();
    let second = u32::try_from(time_of_day % 60).unwrap_or_default();
    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        offset_minutes: 0,
    }
    .format_rfc3339()
}

/// Converts days since the unix epoch to a civil date (Howard Hinnant's
/// `civil_from_days` algorithm, valid for the full `i64` day range).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    if month <= 2 {
        year += 1;
    }
    (
        year,
        u32::try_from(month).unwrap_or_default(),
        u32::try_from(day).unwrap_or_default(),
    )
}

/// Returns the text of the first `# ` heading in the body.
fn extract_h1(body: &str) -> String {
    for line in body.split('\n') {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return title.to_owned();
            }
        }
    }
    String::new()
}

/// A JSON value decoded from YAML with Go `yaml.v3` semantics.
///
/// `yaml_serde` alone silently overwrites duplicate mapping keys, does not apply
/// merge keys, and (via `serde_json::Value`) would accept non-string keys; this
/// wrapper rejects duplicates and resolves `<<` merges so `map[string]any`
/// decoding matches Go.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

/// Builds [`StrictValue`]s while enforcing unique mapping keys.
struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any YAML value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    #[allow(clippy::cast_precision_loss)] // Go falls back to float64 too
    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::from(value as f64)))
    }

    #[allow(clippy::cast_precision_loss)] // Go falls back to float64 too
    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::from(value as f64)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<StrictValue>()? {
            items.push(item.0);
        }
        Ok(StrictValue(Value::Array(items)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut entries: Vec<(String, Value)> = Vec::new();
        let mut merges: Vec<Value> = Vec::new();
        let mut known_keys: HashSet<String> = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value::<StrictValue>()?.0;
            if !known_keys.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate mapping key {key:?}")));
            }
            if key == "<<" {
                merges.push(value);
            } else {
                entries.push((key, value));
            }
        }
        for merge in &merges {
            merge_source(merge, &mut entries, &mut known_keys).map_err(de::Error::custom)?;
        }
        let mut object = Map::new();
        for (key, value) in entries {
            let _ = object.insert(key, value);
        }
        Ok(StrictValue(Value::Object(object)))
    }
}

/// Applies one `<<` merge value: a mapping, or a sequence of mappings.
///
/// Explicit keys win and earlier merge sources win over later ones, matching
/// Go `(*decoder).merge`.
fn merge_source(
    value: &Value,
    entries: &mut Vec<(String, Value)>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if seen.insert(key.clone()) {
                    entries.push((key.clone(), value.clone()));
                }
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                merge_source(item, entries, seen)?;
            }
            Ok(())
        }
        _ => Err("map merge requires a map or sequence of maps as the value".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_DIR: &str = "../../../testdata";

    fn parse_fixture(name: &str) -> Document {
        let path = format!("{FIXTURE_DIR}/{name}");
        parse_file(&path).unwrap()
    }

    fn fixture_expectations() -> Vec<FixtureExpectation> {
        vec![
            FixtureExpectation {
                file: "okf-v02-metric.md",
                title: "Monthly Active Users",
                doc_type: "Metric",
                tags: &["growth", "kpi", "monthly"],
                created_at: Some("2024-05-01T00:00:00Z"),
                updated_at: Some("2024-06-01T10:00:00Z"),
                status: "verified",
                format: FormatKind::Okf,
                parse_error: false,
            },
            FixtureExpectation {
                file: "okf-v01-legacy.md",
                title: "Legacy Retention Claim",
                doc_type: "Claim",
                tags: &["retention", "cohort"],
                created_at: Some("2023-11-15T00:00:00Z"),
                updated_at: Some("2023-11-15T00:00:00Z"),
                status: "draft",
                format: FormatKind::Okf,
                parse_error: false,
            },
            FixtureExpectation {
                file: "portent-task.md",
                title: "Ship mdfu MVP",
                doc_type: "Task",
                tags: &["tolaria", "launch", "Project Atlas"],
                created_at: Some("2026-01-01T00:00:00Z"),
                updated_at: None,
                status: "draft",
                format: FormatKind::Portent,
                parse_error: false,
            },
            FixtureExpectation {
                file: "generic.md",
                title: "Generic Note",
                doc_type: "",
                tags: &["misc"],
                created_at: None,
                updated_at: None,
                status: "",
                format: FormatKind::Generic,
                parse_error: false,
            },
            FixtureExpectation {
                file: "nofrontmatter.md",
                title: "Lone Note",
                doc_type: "",
                tags: &[],
                created_at: None,
                updated_at: None,
                status: "",
                format: FormatKind::None,
                parse_error: false,
            },
            FixtureExpectation {
                file: "bad-yaml.md",
                title: "Recovered Title",
                doc_type: "",
                tags: &[],
                created_at: None,
                updated_at: None,
                status: "",
                format: FormatKind::Generic,
                parse_error: true,
            },
            FixtureExpectation {
                file: "okf-json-metadata.md",
                title: "Hermes Ingest Latency",
                doc_type: "Metric",
                tags: &["hermes", "ingest"],
                created_at: Some("2026-09-01T00:00:00Z"),
                updated_at: Some("2026-09-01T10:05:00Z"),
                status: "verified",
                format: FormatKind::Okf,
                parse_error: false,
            },
        ]
    }

    struct FixtureExpectation {
        file: &'static str,
        title: &'static str,
        doc_type: &'static str,
        tags: &'static [&'static str],
        created_at: Option<&'static str>,
        updated_at: Option<&'static str>,
        status: &'static str,
        format: FormatKind,
        parse_error: bool,
    }

    #[test]
    fn fixtures_parse_to_expected_fields() {
        for expected in fixture_expectations() {
            let doc = parse_fixture(expected.file);
            let file = expected.file;
            assert_eq!(doc.title, expected.title, "{file}: title");
            assert_eq!(doc.doc_type, expected.doc_type, "{file}: doc_type");
            let tags: Vec<&str> = doc.tags.iter().map(String::as_str).collect();
            assert_eq!(tags, expected.tags, "{file}: tags");
            assert_eq!(
                doc.created_at.as_deref(),
                expected.created_at,
                "{file}: created_at"
            );
            assert_eq!(
                doc.updated_at.as_deref(),
                expected.updated_at,
                "{file}: updated_at"
            );
            assert_eq!(doc.status, expected.status, "{file}: status");
            assert_eq!(doc.format, expected.format, "{file}: format");
            assert_eq!(
                doc.parse_error.is_some(),
                expected.parse_error,
                "{file}: parse_error"
            );
            assert!(!doc.search_blob.is_empty(), "{file}: search blob");
        }
    }

    #[test]
    fn fixture_search_blobs_contain_searchable_terms() {
        let cases: [(&str, &[&str]); 5] = [
            (
                "okf-v02-metric.md",
                &[
                    "Monthly Active Users",
                    "monthly active users of the platform",
                    "growth",
                    "data-pipeline",
                    "alice",
                    "snowflake://analytics/mau",
                    "12% month over month",
                ],
            ),
            (
                "okf-v01-legacy.md",
                &["Legacy Retention Claim", "Citations", "retention"],
            ),
            (
                "portent-task.md",
                &["Ship mdfu MVP", "Project Atlas", "Launch Plan"],
            ),
            (
                "generic.md",
                &[
                    "Generic Note",
                    "plain note without a type",
                    "bob",
                    "42",
                    "compost rotation",
                ],
            ),
            ("nofrontmatter.md", &["Lone Note", "honeycrisp apples"]),
        ];
        for (file, needles) in cases {
            let doc = parse_fixture(file);
            for needle in needles {
                assert!(
                    doc.search_blob.contains(needle),
                    "{file}: blob missing {needle:?}\n{}",
                    doc.search_blob
                );
            }
        }
    }

    #[test]
    fn broken_yaml_stays_searchable() {
        let doc = parse_fixture("bad-yaml.md");
        assert!(
            doc.parse_error.is_some(),
            "parse_error = {:?}",
            doc.parse_error
        );
        assert_eq!(doc.title, "Recovered Title");
        assert_eq!(doc.format, FormatKind::Generic);
        assert!(doc.search_blob.contains("kumquat zebra"));
        assert_eq!(
            doc.body,
            "\n# Recovered Title\n\nBody still searchable: kumquat zebra xylophone.\n"
        );
    }

    #[test]
    fn okf_v02_attestations_and_sources() {
        let doc = parse_fixture("okf-v02-metric.md");
        let generated = doc.generated.as_ref().expect("generated");
        assert_eq!(generated.by, "data-pipeline");
        assert_eq!(generated.at.as_deref(), Some("2024-06-01T10:00:00Z"));
        let verified = doc.verified.as_ref().expect("verified");
        assert_eq!(verified.by, "alice");
        assert_eq!(verified.at.as_deref(), Some("2024-06-02T12:30:00Z"));
        assert_eq!(doc.sources.len(), 2);
        assert_eq!(doc.sources[0].id, "warehouse");
        assert_eq!(doc.sources[0].author, "data-team");
        assert_eq!(doc.sources[0].resource, "snowflake://analytics/mau");
        assert_eq!(doc.sources[1].resource, "https://dash.example.com/mau");
        assert_eq!(
            doc.description,
            "Tracks monthly active users of the platform."
        );
        assert!(!doc.is_archived());
    }

    #[test]
    fn portent_relations_and_flags() {
        let doc = parse_fixture("portent-task.md");
        assert_eq!(doc.belongs_to, ["Project Atlas"]);
        assert_eq!(doc.related_to, ["Launch Plan", "Retro Notes"]);
        assert_eq!(doc.organized, Some(true));
        assert_eq!(doc.archived, Some(false));
        assert!(!doc.is_archived());
    }

    #[test]
    fn split_frontmatter_matches_go_table() {
        let cases: [(&str, &str, &str, &str, bool); 8] = [
            (
                "basic",
                "---\ntitle: x\n---\nbody\n",
                "title: x",
                "body\n",
                true,
            ),
            ("no fm", "# Title\n\nbody\n", "", "# Title\n\nbody\n", false),
            (
                "unclosed",
                "---\ntitle: x\nbody\n",
                "",
                "---\ntitle: x\nbody\n",
                false,
            ),
            (
                "not leading",
                "\n---\ntitle: x\n---\nbody\n",
                "",
                "\n---\ntitle: x\n---\nbody\n",
                false,
            ),
            (
                "no newline after dashes",
                "---title: x\n---\n",
                "",
                "---title: x\n---\n",
                false,
            ),
            ("empty fm", "---\n---\nbody\n", "", "body\n", true),
            (
                "hr in body untouched",
                "hello\n\n---\n\nworld\n",
                "",
                "hello\n\n---\n\nworld\n",
                false,
            ),
            (
                "bom stripped",
                "\u{feff}---\ntitle: x\n---\nbody\n",
                "title: x",
                "body\n",
                true,
            ),
        ];
        for (name, content, want_frontmatter, want_body, want_has) in cases {
            let (frontmatter, body, has_frontmatter) = split_frontmatter(content);
            assert_eq!(
                (frontmatter.as_str(), body.as_str(), has_frontmatter),
                (want_frontmatter, want_body, want_has),
                "{name}"
            );
        }
    }

    #[test]
    fn title_falls_back_h1_then_filename() {
        let doc = parse_content("notes/my-idea.md", "no heading here\n");
        assert_eq!(doc.title, "my-idea");
        assert_eq!(doc.format, FormatKind::None);

        let doc = parse_content("notes/only-heading.md", "# Real Heading\n\nbody\n");
        assert_eq!(doc.title, "Real Heading");

        let doc = parse_content(
            "notes/fm-wins.md",
            "---\ntitle: From Frontmatter\n---\n# Ignored\n",
        );
        assert_eq!(doc.title, "From Frontmatter");
    }

    #[test]
    fn role_comes_from_basename_case_insensitively() {
        let cases = [
            ("vault/index.md", "index"),
            ("vault/INDEX.MD", "index"),
            ("notes/log.md", "log"),
            ("notes/note.md", ""),
            ("notes/index.md/", "index"),
        ];
        for (path, want) in cases {
            let doc = parse_content(path, "# Hey\n\nbody\n");
            assert_eq!(doc.role, want, "{path}");
        }
    }

    #[test]
    fn portent_tags_split_and_strip_wikilinks() {
        let doc = parse_content(
            "task.md",
            "---\ntype: Task\ntitle: T\ntags: \"a, b, [[Project X]]\"\n---\n\n# T\n\nbody\n",
        );
        assert_eq!(doc.tags, ["a", "b", "Project X"]);
        assert_eq!(doc.format, FormatKind::Portent);
    }

    #[test]
    fn wikilink_relations_strip_brackets() {
        let doc = parse_content(
            "task.md",
            "---\ntype: Task\ntitle: Ship mdfu MVP\nbelongs_to: \"[[Project Atlas]]\"\nrelated_to:\n  - \"[[Launch Plan]]\"\n  - Retro Notes\n---\n\nbody\n",
        );
        assert_eq!(doc.belongs_to, ["Project Atlas"]);
        assert_eq!(doc.related_to, ["Launch Plan", "Retro Notes"]);
    }

    #[test]
    fn legacy_timestamp_populates_both_dates() {
        let doc = parse_fixture("okf-v01-legacy.md");
        assert_eq!(doc.created_at.as_deref(), Some("2023-11-15T00:00:00Z"));
        assert_eq!(doc.updated_at.as_deref(), Some("2023-11-15T00:00:00Z"));
    }

    #[test]
    fn first_date_prefers_earlier_keys() {
        let doc = parse_content(
            "d.md",
            "---\nupdated: 2024-01-02\nmodified: 2024-03-04\ngenerated:\n  at: \"2024-05-06T07:08:09Z\"\n---\nbody\n",
        );
        assert_eq!(doc.created_at.as_deref(), Some("2024-05-06T07:08:09Z"));
        assert_eq!(doc.updated_at.as_deref(), Some("2024-05-06T07:08:09Z"));

        let doc = parse_content(
            "d.md",
            "---\ndate: 2024-02-02\ntimestamp: 2024-01-01\n---\nbody\n",
        );
        assert_eq!(doc.created_at.as_deref(), Some("2024-02-02T00:00:00Z"));
        assert_eq!(doc.updated_at.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn date_layouts_and_unix_timestamps() {
        let cases: [(&str, &str); 8] = [
            ("\"2024-06-01T10:00:00Z\"", "2024-06-01T10:00:00Z"),
            ("\"2024-06-01T10:00:00+02:00\"", "2024-06-01T10:00:00+02:00"),
            ("\"2024-06-01T10:00:00.5Z\"", "2024-06-01T10:00:00Z"),
            ("\"2024-06-01\"", "2024-06-01T00:00:00Z"),
            ("2024-05-01", "2024-05-01T00:00:00Z"),
            ("2026-01", "2026-01-01T00:00:00Z"),
            ("1700000000", "2023-11-14T22:13:20Z"),
            ("1700000000.9", "2023-11-14T22:13:20Z"),
        ];
        for (input, want) in cases {
            let content = format!("---\ncreated: {input}\n---\nbody\n");
            let doc = parse_content("d.md", &content);
            assert_eq!(doc.created_at.as_deref(), Some(want), "created: {input}");
        }
    }

    #[test]
    fn unparseable_and_out_of_range_dates_stay_none() {
        let cases = [
            "\"not a date\"",
            "\"2024-13-01\"",
            "\"2024-02-30\"",
            "17000000000000000000",
            "\"2024-06-01 10:00:00\"",
        ];
        for input in cases {
            let content = format!("---\ncreated: {input}\n---\nbody\n");
            let doc = parse_content("d.md", &content);
            assert_eq!(doc.created_at, None, "created: {input}");
        }
    }

    #[test]
    fn date_value_helpers_cover_offsets_and_epochs() {
        assert_eq!(
            parse_date_text("1970-01-01"),
            Some("1970-01-01T00:00:00Z".to_owned())
        );
        assert_eq!(
            parse_date_text("1969-12-31T23:59:59Z"),
            Some("1969-12-31T23:59:59Z".to_owned())
        );
        assert_eq!(
            parse_date_text("1969-12-31"),
            Some("1969-12-31T00:00:00Z".to_owned())
        );
        assert_eq!(
            parse_date_text("2024-02-29T00:00:00-05:30"),
            Some("2024-02-29T00:00:00-05:30".to_owned())
        );
        assert_eq!(format_unix(-1), "1969-12-31T23:59:59Z");
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn duplicate_keys_set_parse_error() {
        let doc = parse_content(
            "dup.md",
            "---\ntitle: First\ntitle: Second\n---\n\n# Heading\n\nbody\n",
        );
        assert!(
            doc.parse_error.is_some(),
            "parse_error = {:?}",
            doc.parse_error
        );
        assert_eq!(doc.title, "Heading");
        assert_eq!(doc.format, FormatKind::Generic);
        assert!(doc.raw.is_empty());
    }

    #[test]
    fn yaml_yes_no_are_strings_for_generic_maps() {
        let doc = parse_content(
            "task.md",
            "---\ntype: Note\norganized: yes\narchived: no\n---\n\nbody\n",
        );
        assert_eq!(doc.organized, None);
        assert_eq!(doc.archived, None);
        assert_eq!(
            doc.raw.get("organized"),
            Some(&Value::String("yes".to_owned()))
        );
        assert_eq!(
            doc.raw.get("archived"),
            Some(&Value::String("no".to_owned()))
        );
        assert_eq!(doc.format, FormatKind::Portent);
    }

    #[test]
    fn bool_coercions_match_go_parse_bool() {
        let cases: [(&str, Option<bool>); 7] = [
            ("organized: true", Some(true)),
            ("organized: false", Some(false)),
            ("organized: \"true\"", Some(true)),
            ("organized: \"TRUE\"", Some(true)),
            ("organized: 1", Some(true)),
            ("organized: 0", Some(false)),
            ("organized: maybe", None),
        ];
        for (line, want) in cases {
            let content = format!("---\n{line}\n---\nbody\n");
            let doc = parse_content("b.md", &content);
            assert_eq!(doc.organized, want, "{line}");
        }
    }

    #[test]
    fn anchors_and_aliases_resolve() {
        let doc = parse_content("a.md", "---\ntype: &kind Task\ntitle: *kind\n---\n\nbody\n");
        assert_eq!(doc.doc_type, "Task");
        assert_eq!(doc.title, "Task");
        assert_eq!(doc.format, FormatKind::Portent);
    }

    #[test]
    fn merge_keys_apply_with_explicit_keys_winning() {
        let doc = parse_content(
            "m.md",
            "---\ndefaults: &defaults\n  type: Task\n  status: Done\nitem:\n  <<: *defaults\n  status: Overridden\n---\n\nbody\n",
        );
        let item = doc.raw.get("item").expect("item").as_object().expect("map");
        assert_eq!(item.get("type"), Some(&Value::String("Task".to_owned())));
        assert_eq!(
            item.get("status"),
            Some(&Value::String("Overridden".to_owned()))
        );
        assert!(!item.contains_key("<<"));
    }

    #[test]
    fn merge_sequence_first_source_wins() {
        let doc = parse_content(
            "m.md",
            "---\na: &a {type: Task, status: Done}\nb: &b {status: Blocked, tags: x}\nitem:\n  <<: [*a, *b]\n---\n\nbody\n",
        );
        let item = doc.raw.get("item").expect("item").as_object().expect("map");
        assert_eq!(item.get("status"), Some(&Value::String("Done".to_owned())));
        assert_eq!(item.get("tags"), Some(&Value::String("x".to_owned())));
    }

    #[test]
    fn empty_and_comment_only_frontmatter_are_generic() {
        for content in [
            "---\n---\nbody\n",
            "---\n# only a comment\n---\nbody\n",
            "---\n\n\n---\nbody\n",
        ] {
            let doc = parse_content("e.md", content);
            assert_eq!(doc.parse_error, None, "{content:?}");
            assert!(doc.raw.is_empty(), "{content:?}");
            assert_eq!(doc.format, FormatKind::Generic, "{content:?}");
        }
    }

    #[test]
    fn non_mapping_yaml_roots_set_parse_error() {
        for content in ["---\n- a\n- b\n---\nbody\n", "---\n42\n---\nbody\n"] {
            let doc = parse_content("n.md", content);
            assert!(doc.parse_error.is_some(), "{content:?}");
        }
    }

    #[test]
    fn format_detection_covers_portent_lifecycle_keys() {
        let doc = parse_content("p.md", "---\norganized: true\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Portent);

        let doc = parse_content("p.md", "---\narchived: false\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Portent);

        let doc = parse_content("p.md", "---\ntype: Note\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Portent);

        let doc = parse_content("p.md", "---\ntype: Weird\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Okf);

        let doc = parse_content("p.md", "---\ntype: \" task \"\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Portent);

        let doc = parse_content("p.md", "---\ntype: Claim\n---\nbody\n");
        assert_eq!(doc.format, FormatKind::Okf);
    }

    #[test]
    fn keys_are_matched_case_insensitively() {
        let doc = parse_content(
            "c.md",
            "---\nTYPE: Task\nTitle: Case Test\nTAGS: \"a, [[B]]\"\nSTATUS: Draft\n---\n\nbody\n",
        );
        assert_eq!(doc.doc_type, "Task");
        assert_eq!(doc.title, "Case Test");
        assert_eq!(doc.tags, ["a", "B"]);
        assert_eq!(doc.status, "draft");
    }

    #[test]
    fn list_values_accept_sequences_mixed_scalars_and_wikilinks() {
        let doc = parse_content(
            "l.md",
            "---\ntype: Task\ntags:\n  - plain\n  - \"[[Linked]]\"\n  - 42\nrelated_to: \"[[One]], [[Two]]\"\n---\nbody\n",
        );
        assert_eq!(doc.tags, ["plain", "Linked", "42"]);
        assert_eq!(doc.related_to, ["One", "Two"]);
    }

    #[test]
    fn sources_and_attestations_reject_wrong_shapes() {
        let doc = parse_content(
            "s.md",
            "---\nsources: not-a-list\ngenerated: not-a-map\nverified: null\n---\nbody\n",
        );
        assert!(doc.sources.is_empty());
        assert_eq!(doc.generated, None);
        assert_eq!(doc.verified, None);
    }

    #[test]
    fn parse_file_reads_bytes_lossily_and_errors_on_missing() {
        let missing = parse_file("does/not/exist.md");
        assert!(missing.is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bytes.md");
        fs::write(&path, b"---\ntitle: Bytes\n---\nbody \xff\xfe tail\n").unwrap();
        let doc = parse_file(&path.to_string_lossy()).unwrap();
        assert_eq!(doc.title, "Bytes");
        assert!(doc.body.contains("body \u{fffd}\u{fffd} tail"));
        assert!(doc.search_blob.contains("tail"));
    }
}
