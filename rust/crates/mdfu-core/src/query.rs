//! Single-box query grammar: bare words, `key:value` filters, and date ranges.
//!
//! Port of `internal/query/query.go`. Keys are case-insensitive, values keep
//! their case, and tokens are preserved in input order without deduplication.
//!
//! Dates are self-contained: [`Instant`] stores UTC nanoseconds since the Unix
//! epoch in an `i128`, with a hand-rolled civil-calendar conversion
//! (Howard Hinnant's `days_from_civil`/`civil_from_days`), so no external date
//! crate is needed. [`parse_one_date`] is public for downstream date matching.

use std::collections::BTreeMap;
use std::fmt;

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const NANOS_PER_DAY: i128 = 86_400 * NANOS_PER_SECOND;

/// Granularity of a parsed date expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Granularity {
    /// Full timestamp (`RFC3339` or `YYYY-MM-DDTHH:MM:SS`).
    Instant,
    /// `YYYY-MM-DD`; range bounds expand to the whole day.
    Day,
    /// `YYYY-MM`; range bounds expand to the whole month.
    Month,
}

/// An absolute UTC instant with nanosecond precision.
///
/// Ordering is by instant, matching `time.Time` comparison order in Go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    nanos: i128,
}

impl Instant {
    /// Builds an instant from nanoseconds since the Unix epoch (UTC).
    #[must_use]
    pub const fn from_unix_nanos(nanos: i128) -> Self {
        Self { nanos }
    }

    /// Nanoseconds since the Unix epoch (UTC).
    #[must_use]
    pub const fn unix_nanos(self) -> i128 {
        self.nanos
    }

    /// UTC civil date as `(year, month, day)`.
    #[must_use]
    pub fn civil(self) -> (i64, u32, u32) {
        let days = saturating_i64(self.nanos.div_euclid(NANOS_PER_DAY));
        civil_from_days(days)
    }
}

/// A date range constraint on a document date field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateFilter {
    /// Source field/operator: `created`, `updated`, `date`, `before`, `after`.
    pub field: String,
    /// Lower bound, if any.
    pub from: Option<Instant>,
    /// Upper bound, if any.
    pub to: Option<Instant>,
    /// Whether `from` itself satisfies the filter.
    pub from_incl: bool,
    /// Whether `to` itself satisfies the filter.
    pub to_incl: bool,
}

/// Parsed representation of a single-box search input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Bare (free-text) terms, in input order.
    pub bare: Vec<String>,
    /// Required tags (AND-combined downstream).
    pub tags: Vec<String>,
    /// Forbidden tags.
    pub not_tags: Vec<String>,
    /// `type:` filter value.
    pub doc_type: String,
    /// `title:` terms, joined by a single space.
    pub title: String,
    /// `status:` filter value.
    pub status: String,
    /// `path:` filter value.
    pub path: String,
    /// Unknown `key:value` filters, keyed by lowercased key.
    pub generic: BTreeMap<String, String>,
    /// `created:`/`date:`/`before:`/`after:` filter (`date`/`before`/`after`
    /// carry "either field" semantics via [`DateFilter::field`]).
    pub created: Option<DateFilter>,
    /// `updated:` filter.
    pub updated: Option<DateFilter>,
    /// The original input string.
    pub raw: String,
}

/// A single date operand that could not be recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateError {
    /// Unrecognized date text (after trimming and quote stripping).
    Unrecognized {
        /// The offending text.
        value: String,
    },
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrecognized { value } => {
                write!(
                    f,
                    "unrecognized date {value:?} (want YYYY-MM-DD, YYYY-MM, or RFC3339)"
                )
            }
        }
    }
}

impl std::error::Error for DateError {}

/// Query parse error for a malformed date filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A date filter was present but empty after trimming/quotes.
    EmptyDateExpression {
        /// The filter key (`created`, `before`, ...).
        field: String,
    },
    /// An `A..B` range had no usable bound on either side.
    InvalidDateRange {
        /// The range expression.
        expr: String,
    },
    /// A date operand failed to parse.
    InvalidDate {
        /// The offending operand.
        value: String,
        /// Why it failed.
        source: DateError,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDateExpression { field } => {
                write!(f, "empty date expression for {field:?}")
            }
            Self::InvalidDateRange { expr } => write!(f, "invalid date range {expr:?}"),
            Self::InvalidDate { value, source } => {
                write!(f, "invalid date {value:?}: {source}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidDate { source, .. } => Some(source),
            Self::EmptyDateExpression { .. } | Self::InvalidDateRange { .. } => None,
        }
    }
}

/// Parses `input` into a [`Query`].
///
/// # Errors
///
/// Returns [`Error`] when a `created:`/`updated:`/`date:`/`before:`/`after:`
/// operand is not a recognized date expression.
#[must_use = "the parsed query must be handled"]
pub fn parse(input: &str) -> Result<Query, Error> {
    let mut query = Query {
        raw: input.to_owned(),
        ..Query::default()
    };
    for token in tokenize(input) {
        if token.is_empty() {
            continue;
        }
        let Some(colon) = token.find(':') else {
            push_bare(&mut query, &token);
            continue;
        };
        let key = token.get(..colon).unwrap_or_default().trim().to_lowercase();
        let raw_value = token.get(colon + 1..).unwrap_or_default();
        let value = strip_quotes(raw_value.trim());
        if key.is_empty() {
            // No key, treat the whole token as bare.
            push_bare(&mut query, &token);
            continue;
        }
        match key.as_str() {
            "tag" | "tags" => push_tags(&mut query, value),
            "type" => value.clone_into(&mut query.doc_type),
            "title" => {
                if value.is_empty() {
                    continue;
                }
                if query.title.is_empty() {
                    value.clone_into(&mut query.title);
                } else {
                    query.title.push(' ');
                    query.title.push_str(value);
                }
            }
            "status" => value.clone_into(&mut query.status),
            "path" => value.clone_into(&mut query.path),
            "created" | "updated" | "date" | "before" | "after" => {
                if value.trim().is_empty() {
                    continue;
                }
                let filter = parse_date_filter(&key, value)?;
                if key == "updated" {
                    query.updated = Some(filter);
                } else {
                    // "either" semantics: stored in the created slot with
                    // `field` marking OR behavior; the updated slot stays
                    // intact so an explicit `updated:` can combine via AND.
                    query.created = Some(filter);
                }
            }
            _ => {
                query.generic.insert(key, value.to_owned());
            }
        }
    }
    Ok(query)
}

/// Parses a single date expression into an [`Instant`] plus its
/// [`Granularity`].
///
/// Accepted forms (mirroring Go `internal/query`): `RFC3339` timestamps,
/// `YYYY-MM-DD`, `YYYY-MM`, and zone-less `YYYY-MM-DDTHH:MM:SS` /
/// `YYYY-MM-DD HH:MM:SS` datetimes (fractional seconds allowed). Surrounding
/// whitespace and one pair of surrounding double quotes are ignored.
///
/// # Errors
///
/// Returns [`DateError::Unrecognized`] when no supported layout matches.
#[must_use = "the parsed instant must be handled"]
pub fn parse_one_date(input: &str) -> Result<(Instant, Granularity), DateError> {
    let text = strip_quotes(input.trim());
    if let Some(instant) = parse_rfc3339(text) {
        return Ok((instant, Granularity::Instant));
    }
    if let Some(instant) = parse_day(text) {
        return Ok((instant, Granularity::Day));
    }
    if let Some(instant) = parse_month(text) {
        return Ok((instant, Granularity::Month));
    }
    if let Some(instant) = parse_datetime(text, 'T') {
        return Ok((instant, Granularity::Instant));
    }
    if let Some(instant) = parse_datetime(text, ' ') {
        return Ok((instant, Granularity::Instant));
    }
    Err(DateError::Unrecognized {
        value: text.to_owned(),
    })
}

/// Splits on whitespace while respecting double quotes.
///
/// Quotes are retained in tokens for later stripping.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for character in input.chars() {
        match character {
            '"' => {
                in_quotes = !in_quotes;
                current.push(character);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Removes one pair of surrounding double quotes, if present.
fn strip_quotes(text: &str) -> &str {
    let text = text.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text.get(1..text.len() - 1).unwrap_or(text)
    } else {
        text
    }
}

fn push_bare(query: &mut Query, token: &str) {
    let bare = strip_quotes(token);
    if !bare.is_empty() {
        query.bare.push(bare.to_owned());
    }
}

fn push_tags(query: &mut Query, value: &str) {
    if value.is_empty() {
        return;
    }
    for part in value.split(',') {
        let part = strip_quotes(part.trim());
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix('-').or_else(|| part.strip_prefix('!')) {
            let negated = strip_quotes(rest.trim());
            if !negated.is_empty() {
                query.not_tags.push(negated.to_owned());
            }
        } else {
            query.tags.push(part.to_owned());
        }
    }
}

/// Parses a date expression into a [`DateFilter`].
///
/// Supported forms: exact date, `A..B` range, `>=D`, `>D`, `<=D`, `<D`, plus
/// `before:`/`after:` shorthand for the field.
fn parse_date_filter(field: &str, expr: &str) -> Result<DateFilter, Error> {
    let expr = strip_quotes(expr.trim());
    if expr.is_empty() {
        return Err(Error::EmptyDateExpression {
            field: field.to_owned(),
        });
    }
    let mut filter = DateFilter {
        field: field.to_owned(),
        from: None,
        to: None,
        from_incl: false,
        to_incl: false,
    };
    if let Some((left_raw, right_raw)) = expr.split_once("..") {
        let left = strip_quotes(left_raw.trim());
        let right = strip_quotes(right_raw.trim());
        if left.is_empty() && right.is_empty() {
            return Err(Error::InvalidDateRange {
                expr: expr.to_owned(),
            });
        }
        filter.from_incl = true;
        filter.to_incl = true;
        if !left.is_empty() {
            let (instant, granularity) = operand(left)?;
            filter.from = Some(start_of(instant, granularity));
        }
        if !right.is_empty() {
            let (instant, granularity) = operand(right)?;
            filter.to = Some(end_of(instant, granularity));
        }
        return Ok(filter);
    }
    if let Some(rest) = expr.strip_prefix(">=") {
        let (instant, granularity) = operand(rest.trim())?;
        filter.from = Some(start_of(instant, granularity));
        filter.from_incl = true;
        return Ok(filter);
    }
    if let Some(rest) = expr.strip_prefix("<=") {
        let (instant, granularity) = operand(rest.trim())?;
        filter.to = Some(end_of(instant, granularity));
        filter.to_incl = true;
        return Ok(filter);
    }
    if let Some(rest) = expr.strip_prefix('>') {
        let (instant, granularity) = operand(rest.trim())?;
        filter.from = Some(end_of(instant, granularity));
        return Ok(filter);
    }
    if let Some(rest) = expr.strip_prefix('<') {
        let (instant, granularity) = operand(rest.trim())?;
        filter.to = Some(start_of(instant, granularity));
        return Ok(filter);
    }
    // No operator: exact date, or before:/after: shorthand.
    let (instant, granularity) = operand(expr)?;
    match field {
        "before" => filter.to = Some(start_of(instant, granularity)),
        "after" => filter.from = Some(end_of(instant, granularity)),
        _ => {
            filter.from = Some(start_of(instant, granularity));
            filter.to = Some(end_of(instant, granularity));
            filter.from_incl = true;
            filter.to_incl = true;
        }
    }
    Ok(filter)
}

fn operand(value: &str) -> Result<(Instant, Granularity), Error> {
    let stripped = strip_quotes(value.trim());
    parse_one_date(stripped).map_err(|source| Error::InvalidDate {
        value: stripped.to_owned(),
        source,
    })
}

fn start_of(instant: Instant, granularity: Granularity) -> Instant {
    match granularity {
        Granularity::Month => {
            let (year, month, _) = instant.civil();
            utc_instant(year, month, 1, 0, 0, 0, 0)
        }
        Granularity::Day => {
            let (year, month, day) = instant.civil();
            utc_instant(year, month, day, 0, 0, 0, 0)
        }
        Granularity::Instant => instant,
    }
}

fn end_of(instant: Instant, granularity: Granularity) -> Instant {
    match granularity {
        Granularity::Month => {
            let (year, month, _) = instant.civil();
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let next_start = utc_instant(next_year, next_month, 1, 0, 0, 0, 0).unix_nanos();
            Instant::from_unix_nanos(next_start - 1)
        }
        Granularity::Day => {
            let day_start = start_of(instant, Granularity::Day).unix_nanos();
            Instant::from_unix_nanos(day_start + NANOS_PER_DAY - 1)
        }
        Granularity::Instant => instant,
    }
}

fn parse_rfc3339(text: &str) -> Option<Instant> {
    let date = text.get(..10)?;
    let time = text.get(10..)?.strip_prefix('T')?;
    let (year, month, day) = parse_date_ymd(date)?;
    let (hour, minute, second, nanos, tail) = parse_time_part(time)?;
    let offset_seconds = parse_offset(tail)?;
    let local_nanos = utc_instant(year, month, day, hour, minute, second, nanos).unix_nanos();
    Some(Instant::from_unix_nanos(
        local_nanos - i128::from(offset_seconds) * NANOS_PER_SECOND,
    ))
}

fn parse_day(text: &str) -> Option<Instant> {
    let (year, month, day) = parse_date_ymd(text)?;
    Some(utc_instant(year, month, day, 0, 0, 0, 0))
}

fn parse_month(text: &str) -> Option<Instant> {
    let bytes = text.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    let year = i64::from(parse_digits(bytes.get(..4)?)?);
    let month = parse_digits(bytes.get(5..7)?)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some(utc_instant(year, month, 1, 0, 0, 0, 0))
}

fn parse_date_ymd(text: &str) -> Option<(i64, u32, u32)> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = i64::from(parse_digits(bytes.get(..4)?)?);
    let month = parse_digits(bytes.get(5..7)?)?;
    let day = parse_digits(bytes.get(8..10)?)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some((year, month, day))
}

fn parse_datetime(text: &str, separator: char) -> Option<Instant> {
    let date = text.get(..10)?;
    let time = text.get(10..)?.strip_prefix(separator)?;
    let (year, month, day) = parse_date_ymd(date)?;
    let (hour, minute, second, nanos, tail) = parse_time_part(time)?;
    if !tail.is_empty() {
        return None;
    }
    Some(utc_instant(year, month, day, hour, minute, second, nanos))
}

/// Parses `HH:MM:SS`, returning the fields plus the unconsumed tail
/// (fraction allowed, matching Go's implicit fractional-second handling).
fn parse_time_part(text: &str) -> Option<(u32, u32, u32, u32, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() < 8 || bytes[2] != b':' || bytes[5] != b':' {
        return None;
    }
    let hour = parse_digits(bytes.get(..2)?)?;
    let minute = parse_digits(bytes.get(3..5)?)?;
    let second = parse_digits(bytes.get(6..8)?)?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut tail = text.get(8..)?;
    let mut nanos = 0;
    if let Some(fraction) = tail.strip_prefix('.').or_else(|| tail.strip_prefix(',')) {
        let count = fraction
            .as_bytes()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if count == 0 || count > 9 {
            return None;
        }
        nanos = parse_digits(fraction.as_bytes().get(..count)?)?;
        for _ in count..9 {
            nanos *= 10;
        }
        tail = fraction.get(count..)?;
    }
    Some((hour, minute, second, nanos, tail))
}

fn parse_offset(text: &str) -> Option<i64> {
    if text == "Z" {
        return Some(0);
    }
    let bytes = text.as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1_i64,
        b'-' => -1_i64,
        _ => return None,
    };
    let hours = parse_digits(bytes.get(1..3)?)?;
    let minutes = parse_digits(bytes.get(4..6)?)?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (i64::from(hours) * 3600 + i64::from(minutes) * 60))
}

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
    }
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn utc_instant(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: u32,
) -> Instant {
    let days = i128::from(days_from_civil(year, month, day));
    let seconds = i128::from(hour) * 3600 + i128::from(minute) * 60 + i128::from(second);
    Instant::from_unix_nanos(days * NANOS_PER_DAY + seconds * NANOS_PER_SECOND + i128::from(nanos))
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 for a proleptic
/// Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = if month > 2 {
        i64::from(month) - 3
    } else {
        i64::from(month) + 9
    };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Howard Hinnant's `civil_from_days`: inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, to_u32(month), to_u32(day))
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

fn saturating_i64(value: i128) -> i64 {
    i64::try_from(value).unwrap_or(if value.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> Query {
        parse(input).unwrap()
    }

    fn instant(text: &str) -> Instant {
        parse_one_date(text).unwrap().0
    }

    #[test]
    fn parse_bare_words_and_raw_input() {
        let query = parsed("hello world");
        assert_eq!(query.bare, ["hello", "world"]);
        assert_eq!(query.raw, "hello world");
        let query = parsed("  hello   world ");
        assert_eq!(query.bare, ["hello", "world"]);
        assert_eq!(query.raw, "  hello   world ");
        assert!(parsed("").bare.is_empty());
    }

    #[test]
    fn parse_tags() {
        let query = parsed("tag:foo");
        assert_eq!(query.tags, ["foo"]);

        let query = parsed("tags:a,b");
        assert_eq!(query.tags, ["a", "b"]);

        // Case-insensitive key, value case preserved.
        let query = parsed("TAG:Foo");
        assert_eq!(query.tags, ["Foo"]);
    }

    #[test]
    fn parse_tag_negation() {
        let query = parsed("tag:-foo");
        assert_eq!(query.not_tags, ["foo"]);
        assert!(query.tags.is_empty());

        let query = parsed("tags:a,-b,!c");
        assert_eq!(query.tags, ["a"]);
        assert_eq!(query.not_tags, ["b", "c"]);

        let query = parsed("tag:foo tag:-bar");
        assert_eq!(query.tags, ["foo"]);
        assert_eq!(query.not_tags, ["bar"]);
    }

    #[test]
    fn parse_type_title_status_path() {
        let query = parsed("type:Note title:hello status:draft path:sub/dir");
        assert_eq!(query.doc_type, "Note");
        assert_eq!(query.title, "hello");
        assert_eq!(query.status, "draft");
        assert_eq!(query.path, "sub/dir");

        // Case-insensitive keys, values keep case.
        let query = parsed("TYPE:Note TITLE:Hello STATUS:Draft PATH:Sub/");
        assert_eq!(query.doc_type, "Note");
        assert_eq!(query.title, "Hello");
        assert_eq!(query.status, "Draft");
        assert_eq!(query.path, "Sub/");

        // title: accumulates with a single space, mirrors Go.
        let query = parsed("title:one title:two");
        assert_eq!(query.title, "one two");
    }

    #[test]
    fn parse_quotes() {
        let query = parsed(r#"title:"my title" foo"#);
        assert_eq!(query.title, "my title");
        assert_eq!(query.bare, ["foo"]);

        let query = parsed(r#""foo bar" baz"#);
        assert_eq!(query.bare, ["foo bar", "baz"]);

        let query = parsed(r#"author:"John Doe""#);
        assert_eq!(query.generic["author"], "John Doe");
    }

    #[test]
    fn parse_generic_keys_and_values() {
        let query = parsed("author:John custom:value");
        assert_eq!(query.generic["author"], "John");
        assert_eq!(query.generic["custom"], "value");

        // Keys lowercased, values case-preserved.
        let query = parsed("CUSTOM:Value");
        assert_eq!(query.generic["custom"], "Value");

        let query = parsed("author:JoHn");
        assert_eq!(query.generic["author"], "JoHn");

        // A colon with no key makes the whole token bare.
        let query = parsed(":foo");
        assert_eq!(query.bare, [":foo"]);
    }

    #[test]
    fn parse_date_exact() {
        let query = parsed("created:2024-03-15");
        let created = query.created.unwrap();
        assert_eq!(created.field, "created");
        assert_eq!(created.from, Some(instant("2024-03-15")));
        assert_eq!(created.to, Some(instant("2024-03-15T23:59:59.999999999Z")));
        assert!(created.from_incl && created.to_incl);

        // Mid-day sits inside the whole-day range.
        let mid = instant("2024-03-15T12:00:00Z");
        assert!(created.from.unwrap() <= mid && mid <= created.to.unwrap());

        // Independent epoch check: 2024-03-15T00:00:00Z.
        assert_eq!(
            instant("2024-03-15").unix_nanos(),
            1_710_460_800 * NANOS_PER_SECOND
        );
    }

    #[test]
    fn parse_date_range() {
        let query = parsed("created:2024-01-01..2024-02-01");
        let created = query.created.unwrap();
        assert_eq!(created.from, Some(instant("2024-01-01")));
        let (year, month, day) = created.to.unwrap().civil();
        assert_eq!((year, month, day), (2024, 2, 1));
        assert_eq!(created.to, Some(instant("2024-02-01T23:59:59.999999999Z")));
        assert!(created.from_incl && created.to_incl);

        // Half-open ranges.
        let query = parsed("created:2024-01-01..");
        let created = query.created.unwrap();
        assert_eq!(created.from, Some(instant("2024-01-01")));
        assert!(created.to.is_none());

        let query = parsed("created:..2024-02-01");
        let created = query.created.unwrap();
        assert!(created.from.is_none());
        assert_eq!(created.to, Some(instant("2024-02-01T23:59:59.999999999Z")));
    }

    #[test]
    fn parse_date_comparisons() {
        let query = parsed("updated:>=2024-01-01");
        let updated = query.updated.unwrap();
        assert_eq!(updated.field, "updated");
        assert_eq!(updated.from, Some(instant("2024-01-01")));
        assert!(updated.to.is_none() && updated.from_incl);

        let query = parsed("updated:>2024-01-01");
        let updated = query.updated.unwrap();
        assert_eq!(
            updated.from,
            Some(instant("2024-01-01T23:59:59.999999999Z"))
        );
        assert!(!updated.from_incl);

        let query = parsed("updated:<=2024-01-01");
        let updated = query.updated.unwrap();
        assert_eq!(updated.to, Some(instant("2024-01-01T23:59:59.999999999Z")));
        assert!(updated.to_incl);

        let query = parsed("updated:<2024-01-01");
        let updated = query.updated.unwrap();
        assert_eq!(updated.to, Some(instant("2024-01-01")));
        assert!(!updated.to_incl);

        // Month granularity expands to whole-month bounds.
        let query = parsed("updated:>=2024-01");
        assert_eq!(query.updated.unwrap().from, Some(instant("2024-01-01")));
    }

    #[test]
    fn parse_date_month_and_rfc3339() {
        let query = parsed("created:2024-01");
        let created = query.created.unwrap();
        assert_eq!(created.from.unwrap().civil().1, 1);
        assert_eq!(created.to.unwrap().civil().1, 1);
        assert_eq!(created.to, Some(instant("2024-01-31T23:59:59.999999999Z")));
        let mid = instant("2024-01-15");
        assert!(created.from.unwrap() <= mid && mid <= created.to.unwrap());

        let query = parsed("created:2024-03-15T10:00:00Z");
        let created = query.created.unwrap();
        assert_eq!(created.from, created.to);
        assert_eq!(created.from, Some(instant("2024-03-15T10:00:00Z")));
    }

    #[test]
    fn parse_before_after_date() {
        let query = parsed("before:2024-01-01");
        let created = query.created.unwrap();
        assert_eq!(created.field, "before");
        assert_eq!(created.to, Some(instant("2024-01-01")));
        assert!(created.from.is_none() && !created.to_incl);

        let query = parsed("after:2024-06-01");
        let created = query.created.unwrap();
        assert_eq!(created.field, "after");
        assert_eq!(
            created.from,
            Some(instant("2024-06-01T23:59:59.999999999Z"))
        );
        assert!(created.to.is_none() && !created.from_incl);

        let query = parsed("date:2024-03-15");
        let created = query.created.unwrap();
        assert_eq!(created.field, "date");
        assert!(created.from.is_some() && created.to.is_some());
        assert!(created.from_incl && created.to_incl);
    }

    #[test]
    fn parse_bad_date_is_an_error() {
        assert!(parse("created:not-a-date").is_err());
        assert!(parse("updated:2024-13-01").is_err());
    }

    #[test]
    fn parse_mixed() {
        let query =
            parsed("fuzzy words tag:foo,-bar type:Note created:2024-01-01..2024-12-31 author:John");
        assert_eq!(query.bare, ["fuzzy", "words"]);
        assert_eq!(query.tags, ["foo"]);
        assert_eq!(query.not_tags, ["bar"]);
        assert_eq!(query.doc_type, "Note");
        assert!(query.created.is_some());
        assert_eq!(query.generic["author"], "John");
    }

    #[test]
    fn acceptance_tag_growth_kpi() {
        let query = parsed("tag:growth,-kpi");
        assert_eq!(query.tags, ["growth"]);
        assert_eq!(query.not_tags, ["kpi"]);
    }

    #[test]
    fn acceptance_quoted_tag_value() {
        let query = parsed(r#"tags:"Project X""#);
        assert_eq!(query.tags, ["Project X"]);
        assert!(query.not_tags.is_empty());
    }

    #[test]
    fn acceptance_date_range_before_after() {
        let query = parsed("created:2024-01-01..2024-04-01");
        let created = query.created.unwrap();
        assert_eq!(created.from, Some(instant("2024-01-01")));
        assert_eq!(created.to, Some(instant("2024-04-01T23:59:59.999999999Z")));

        let query = parsed("before:2024-03-01");
        let created = query.created.unwrap();
        assert_eq!(created.to, Some(instant("2024-03-01")));
        assert!(!created.to_incl);

        let query = parsed("after:2024-05-01");
        let created = query.created.unwrap();
        assert_eq!(
            created.from,
            Some(instant("2024-05-01T23:59:59.999999999Z"))
        );
        assert!(!created.from_incl);
    }

    #[test]
    fn acceptance_invalid_date_error_shapes() {
        let error = parse("created:not-a-date").unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"invalid date "not-a-date": unrecognized date "not-a-date" (want YYYY-MM-DD, YYYY-MM, or RFC3339)"#
        );
        assert!(std::error::Error::source(&error).is_some());

        assert_eq!(
            parse("updated:..").unwrap_err().to_string(),
            r#"invalid date range "..""#
        );
        assert_eq!(
            parse("created:2024-01-01..bogus").unwrap_err().to_string(),
            r#"invalid date "bogus": unrecognized date "bogus" (want YYYY-MM-DD, YYYY-MM, or RFC3339)"#
        );
        assert_eq!(
            parse("created:>=nope").unwrap_err().to_string(),
            r#"invalid date "nope": unrecognized date "nope" (want YYYY-MM-DD, YYYY-MM, or RFC3339)"#
        );
    }

    #[test]
    fn empty_date_values_are_skipped_not_errors() {
        let query = parsed("created: updated: date:");
        assert!(query.created.is_none() && query.updated.is_none());
        let query = parsed(r#"created:"" "#);
        assert!(query.created.is_none());
        let query = parsed("created: ");
        assert!(query.created.is_none());
    }

    #[test]
    fn token_order_is_preserved_without_dedup() {
        let query = parsed("alpha beta alpha");
        assert_eq!(query.bare, ["alpha", "beta", "alpha"]);

        let query = parsed("tag:a tag:a tag:b");
        assert_eq!(query.tags, ["a", "a", "b"]);

        // Generic keys overwrite (Go map semantics), value case-preserved.
        let query = parsed("word:1 word:2");
        assert_eq!(query.generic["word"], "2");
        assert_eq!(query.generic.len(), 1);
    }

    #[test]
    fn date_key_stores_created_slot_but_keeps_updated() {
        let query = parsed("before:2024-01-01 updated:2024-02-02");
        assert_eq!(query.created.unwrap().field, "before");
        let updated = query.updated.unwrap();
        assert_eq!(updated.field, "updated");
        assert!(updated.from.is_some());
    }

    #[test]
    fn tokenize_is_quote_aware_and_keeps_quotes() {
        assert_eq!(
            tokenize(r#"title:"my title" foo"#),
            [r#"title:"my title""#, "foo"]
        );
        assert_eq!(tokenize(r#""foo bar" baz"#), [r#""foo bar""#, "baz"]);
        assert_eq!(tokenize("a  b\tc"), ["a", "b", "c"]);
        assert!(tokenize("   ").is_empty());
        // Unterminated quote swallows the rest as one token.
        assert_eq!(tokenize(r#"a "b c"#), ["a", r#""b c"#]);
    }

    #[test]
    fn strip_quotes_edges() {
        assert_eq!(strip_quotes(r#"  "a b"  "#), "a b");
        assert_eq!(strip_quotes("\""), "\"");
        assert_eq!(strip_quotes("\"\""), "");
        assert_eq!(strip_quotes("\"\"\""), "\"");
        assert_eq!(strip_quotes("plain"), "plain");
        assert_eq!(strip_quotes(r#""a" extra"#), r#""a" extra"#);
        assert_eq!(strip_quotes(r#""a" "b""#), r#"a" "b"#);
    }

    #[test]
    fn parse_one_date_covers_all_layouts() {
        assert_eq!(parse_one_date("2024-03-15").unwrap().1, Granularity::Day);
        assert_eq!(
            parse_one_date("2024-03").unwrap(),
            (instant("2024-03-01"), Granularity::Month)
        );
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00Z").unwrap().1,
            Granularity::Instant
        );
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00").unwrap(),
            (instant("2024-03-15T10:00:00Z"), Granularity::Instant)
        );
        assert_eq!(
            parse_one_date("2024-03-15 10:00:00").unwrap(),
            (instant("2024-03-15T10:00:00Z"), Granularity::Instant)
        );
        assert_eq!(
            parse_one_date("\"2024-03-15\"").unwrap().1,
            Granularity::Day
        );
    }

    #[test]
    fn rfc3339_offsets_and_fractions() {
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00+02:00").unwrap().0,
            instant("2024-03-15T08:00:00Z")
        );
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00-01:30").unwrap().0,
            instant("2024-03-15T11:30:00Z")
        );
        let base = instant("2024-03-15T10:00:00Z").unix_nanos();
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00.5Z")
                .unwrap()
                .0
                .unix_nanos(),
            base + 500_000_000
        );
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00,25Z")
                .unwrap()
                .0
                .unix_nanos(),
            base + 250_000_000
        );
        assert_eq!(
            parse_one_date("2024-03-15T10:00:00.123456789Z")
                .unwrap()
                .0
                .unix_nanos(),
            base + 123_456_789
        );
    }

    #[test]
    fn instant_civil_conversion_covers_epoch_and_negatives() {
        assert_eq!(Instant::from_unix_nanos(0).civil(), (1970, 1, 1));
        assert_eq!(Instant::from_unix_nanos(-1).civil(), (1969, 12, 31));
        assert_eq!(
            Instant::from_unix_nanos(1_710_460_800 * NANOS_PER_SECOND).civil(),
            (2024, 3, 15)
        );
    }

    #[test]
    fn leap_years_follow_gregorian_rules() {
        assert_eq!(parse_one_date("2000-02-29").unwrap().1, Granularity::Day);
        assert!(parse_one_date("1900-02-29").is_err());
        assert!(parse_one_date("2024-02-30").is_err());
        // 2024-02-29 end-of-day rounds to a valid February date.
        let query = parsed("created:2024-02-29");
        assert_eq!(query.created.unwrap().to.unwrap().civil(), (2024, 2, 29));
    }

    #[test]
    fn invalid_date_operands_are_rejected() {
        for bad in [
            "",
            "not-a-date",
            "2024-13-01",
            "2024-02-30",
            "2024-1-5",
            "2024-03-15T10:00",
            "2024-03-15T10:00:00+0200",
            "2024-03-15T25:00:00Z",
            "2024-03-15T10:60:00Z",
            "2024-03-15T10:00:60Z",
            "2024/03/15",
            "15-03-2024",
            "2024-03-15T10:00:00.1234567890Z",
        ] {
            assert!(
                parse_one_date(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }
}
