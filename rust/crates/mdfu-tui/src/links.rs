//! Link extraction, OSC 8 hyperlink emission, and ANSI-aware match emphasis.
//!
//! This module ports `internal/tui/links.go` plus the highlight helpers
//! (`queryTerms`, `termsRegexp`, `highlightRe`, `updateActiveSGR`) from
//! `internal/tui/tui.go`. The pipeline mirrors the Go preview renderer:
//!
//! 1. [`extract_and_hide`] rewrites markdown links so a renderer shows only the
//!    label. Web links carry Private Use Area marker runes that remember their
//!    target; relative links and anchors become plain label text. Links inside
//!    inline code and fenced code blocks are left untouched.
//! 2. [`highlight_re`] emphasizes query matches in the rendered markdown
//!    without corrupting the ANSI SGR sequences or OSC payloads the renderer
//!    emitted.
//! 3. [`patch_links`] replaces the marker runes with OSC 8 hyperlinks and
//!    [`close_open_hyperlinks`] seals a hyperlink a truncated preview left
//!    open.
//!
//! Everything is plain view-model data in and out ([`PreviewBlock`] fields or
//! strings derived from them); no core type is touched, and no style is baked
//! into data. Emphasis and link labels take their SGR parameters from the
//! active [`Theme`] at render time (`theme.highlight_sgr` / `theme.link_sgr`);
//! see [`highlight_with_theme`].

use std::collections::HashSet;
use std::sync::LazyLock;

use mdfu_core::theme::Theme;
use regex::Regex;

use crate::view_model::{FieldValue, PreviewBlock};

/// First Private Use Area code point used as a link marker; a start marker is
/// `LINK_START_BASE + index` into the extracted link table.
pub const LINK_START_BASE: u32 = 0xE000;

/// Closing marker for every marked link. Links never nest, so one rune closes
/// the innermost (and only) open link.
pub const LINK_END_RUNE: char = '\u{F8FF}';

/// Largest number of links that fit before a start marker would collide with
/// [`LINK_END_RUNE`]; further links degrade to plain labels.
const MAX_LINKS: usize = (LINK_END_RUNE as u32 - LINK_START_BASE) as usize;

/// Resets every SGR attribute (mirrors Go `resetSGR` and `hlReset`).
const RESET_SGR: &str = "\x1b[0m";

/// A web hyperlink discovered in a document body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInfo {
    /// Visible label the renderer keeps.
    pub label: String,
    /// Browser-openable destination.
    pub url: String,
}

/// CSI Select-Graphic-Rendition sequences (mirrors Go `ansiSGRRe`).
static ANSI_SGR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("SGR regex is valid"));

/// OSC sequences terminated by BEL or ST (mirrors Go `ansiOSCRe`).
static ANSI_OSC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").expect("OSC regex is valid"));

/// OSC 8 hyperlinks with the URI captured; the URI is empty for a reset
/// (mirrors Go `osc8HyperlinkRe`).
static OSC8_HYPERLINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\]8;[^;]*;([^\x07\x1b]*)(?:\x07|\x1b\\)").expect("OSC 8 regex is valid")
});

/// Starts a terminal hyperlink pointing at `uri` (OSC 8, BEL-terminated,
/// matching `ansi.SetHyperlink`).
#[must_use]
pub fn set_hyperlink(uri: &str) -> String {
    format!("\x1b]8;;{uri}\x07")
}

/// Closes the active terminal hyperlink (OSC 8 with an empty URI, matching
/// `ansi.ResetHyperlink`).
#[must_use]
pub const fn reset_hyperlink() -> &'static str {
    "\x1b]8;;\x07"
}

/// Reports whether `raw` is a URL a browser can open: `http`, `https`, or
/// `mailto` after `net/url`-style parsing.
#[must_use]
pub fn is_web_link(raw: &str) -> bool {
    parse_url(raw).is_some_and(|u| matches!(u.scheme.as_str(), "http" | "https" | "mailto"))
}

/// Short display label for a URL. Autolinks and bare URLs carry no markdown
/// label, so they show the host; `mailto:` links show the address.
#[must_use]
pub fn link_label(raw: &str) -> String {
    let Some(u) = parse_url(raw) else {
        return raw.to_owned();
    };
    if u.scheme == "mailto" {
        if !u.opaque.is_empty() {
            return u.opaque;
        }
        return raw.strip_prefix("mailto:").unwrap_or(raw).to_owned();
    }
    if !u.host.is_empty() {
        return u.host;
    }
    raw.to_owned()
}

/// Renders `label` as a terminal hyperlink pointing at `raw`, styling the
/// label with the SGR parameters `link_sgr` (from [`Theme::link_sgr`]).
#[must_use]
pub fn osc8_link(raw: &str, label: &str, link_sgr: &str) -> String {
    format!(
        "{}\x1b[{link_sgr}m{label}{RESET_SGR}{}",
        set_hyperlink(raw),
        reset_hyperlink()
    )
}

/// Rewrites markdown links so the renderer shows only the label, never the raw
/// URL.
///
/// Web links are wrapped in marker runes for later [`patch_links`] conversion;
/// non-web links (relative paths, anchors) become plain label text. Links
/// inside fenced code blocks and inline code spans are left untouched. Images
/// (`![alt](src)`) pass through whole so their destination is not rescanned as
/// a bare URL.
///
/// When `hyperlinks` is `false` the source is returned unchanged and no links
/// are collected: the renderer then shows its normal `label url` fallback,
/// keeping the target visible and copyable on terminals without OSC 8 support.
#[must_use]
pub fn extract_and_hide(src: &str, hyperlinks: bool) -> (String, Vec<LinkInfo>) {
    if !hyperlinks {
        return (src.to_owned(), Vec::new());
    }
    let mut links = Vec::new();
    let mut out = String::with_capacity(src.len());
    let mut fence: Option<(u8, usize)> = None;
    for line in src.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some((ch, n)) = fence {
            out.push_str(line);
            if is_fence_close(body, ch, n) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, n)) = fence_open(body) {
            fence = Some((ch, n));
            out.push_str(line);
            continue;
        }
        out.push_str(&scan_inline(body, &mut links));
        if line.ends_with('\n') {
            out.push('\n');
        }
    }
    (out, links)
}

/// Rewrites links on a single non-fenced line.
fn scan_inline(line: &str, links: &mut Vec<LinkInfo>) -> String {
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let n = run_len(bytes, i, b'`');
                if let Some(end) = code_span_end(bytes, i + n, n) {
                    out.extend_from_slice(&bytes[i..end]);
                    i = end;
                } else {
                    out.extend_from_slice(&bytes[i..i + n]);
                    i += n;
                }
            }
            b'!' => {
                // Leave image syntax intact so its destination is not picked
                // up as a bare URL.
                if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                    if let Some((_, _, end)) = parse_inline_link(line, i + 1) {
                        out.extend_from_slice(&bytes[i..end]);
                        i = end;
                        continue;
                    }
                }
                out.push(b'!');
                i += 1;
            }
            b'[' => {
                if let Some((label, dest, end)) = parse_inline_link(line, i) {
                    write_link(&mut out, links, &dest, &label);
                    i = end;
                } else {
                    out.push(b'[');
                    i += 1;
                }
            }
            b'<' => {
                if let Some((label, dest, end)) = parse_autolink(line, i) {
                    write_link(&mut out, links, &dest, &label);
                    i = end;
                } else {
                    out.push(b'<');
                    i += 1;
                }
            }
            b'h' | b'H' if is_word_boundary(bytes, i) => {
                if let Some((dest, end)) = parse_bare_url(line, i) {
                    let label = link_label(&dest);
                    write_link(&mut out, links, &dest, &label);
                    i = end;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Appends a link to `out`. Web links get marker runes and a table entry;
/// other links are reduced to their label.
fn write_link(out: &mut Vec<u8>, links: &mut Vec<LinkInfo>, dest: &str, label: &str) {
    if !is_web_link(dest) {
        out.extend_from_slice(label.as_bytes());
        return;
    }
    let idx = links.len();
    if idx >= MAX_LINKS {
        // Out of marker range: degrade to a plain label rather than corrupt
        // the payload.
        out.extend_from_slice(label.as_bytes());
        return;
    }
    links.push(LinkInfo {
        label: label.to_owned(),
        url: dest.to_owned(),
    });
    let code = LINK_START_BASE + u32::try_from(idx).unwrap_or(u32::MAX);
    if let Some(marker) = char::from_u32(code) {
        let mut buf = [0_u8; 4];
        out.extend_from_slice(marker.encode_utf8(&mut buf).as_bytes());
    }
    out.extend_from_slice(label.as_bytes());
    let mut buf = [0_u8; 4];
    out.extend_from_slice(LINK_END_RUNE.encode_utf8(&mut buf).as_bytes());
}

/// Parses `[label](destination "title")` starting at `i`, returning the label,
/// destination, and the index just past the closing `)`.
fn parse_inline_link(s: &str, i: usize) -> Option<(String, String, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let mut depth: i32 = 0;
    let mut j = i;
    let mut label_end = None;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => {
                j += 2;
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    label_end = Some(j);
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }
    let j = label_end?;
    if bytes.get(j + 1) != Some(&b'(') {
        return None;
    }
    let label = String::from_utf8_lossy(&bytes[i + 1..j]).into_owned();
    let mut k = j + 2;
    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
        k += 1;
    }
    let dest;
    if bytes.get(k) == Some(&b'<') {
        let close = bytes[k + 1..].iter().position(|b| *b == b'>')?;
        dest = String::from_utf8_lossy(&bytes[k + 1..k + 1 + close]).into_owned();
        k = k + 1 + close + 1;
    } else {
        let start = k;
        let mut paren_depth = 0;
        while k < bytes.len() {
            match bytes[k] {
                b'\\' => {
                    if k + 1 < bytes.len() {
                        k += 1;
                    }
                }
                b'(' => paren_depth += 1,
                b')' => {
                    if paren_depth == 0 {
                        break;
                    }
                    paren_depth -= 1;
                }
                b' ' | b'\t' => break,
                _ => {}
            }
            k += 1;
        }
        dest = String::from_utf8_lossy(&bytes[start..k]).into_owned();
    }
    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
        k += 1;
    }
    if k < bytes.len() && (bytes[k] == b'"' || bytes[k] == b'\'') {
        let quote = bytes[k];
        k += 1;
        while k < bytes.len() && bytes[k] != quote {
            if bytes[k] == b'\\' && k + 1 < bytes.len() {
                k += 1;
            }
            k += 1;
        }
        if k >= bytes.len() {
            return None;
        }
        k += 1;
        while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
    }
    if bytes.get(k) != Some(&b')') {
        return None;
    }
    Some((label, dest, k + 1))
}

/// Parses `<scheme:...>` starting at `i`. Only web schemes are accepted so
/// HTML tags and relative references are left for the renderer.
fn parse_autolink(s: &str, i: usize) -> Option<(String, String, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(i) != Some(&b'<') {
        return None;
    }
    let rel = bytes[i + 1..].iter().position(|b| *b == b'>')?;
    let inner = &bytes[i + 1..i + 1 + rel];
    if inner.is_empty()
        || inner
            .iter()
            .any(|b| matches!(*b, b' ' | b'\t' | b'\n' | b'<' | b'"'))
    {
        return None;
    }
    let inner = String::from_utf8_lossy(inner).into_owned();
    if !is_web_link(&inner) {
        return None;
    }
    let label = link_label(&inner);
    Some((label, inner, i + 1 + rel + 1))
}

/// Finds an http(s) URL starting at `i` and trims trailing sentence
/// punctuation so `see https://x.com.` does not swallow the period.
fn parse_bare_url(s: &str, i: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let rest = &bytes[i..];
    if !starts_with_ignore_ascii_case(rest, b"http://")
        && !starts_with_ignore_ascii_case(rest, b"https://")
    {
        return None;
    }
    let mut j = i;
    while j < bytes.len() && !is_url_terminator(bytes[j]) {
        j += 1;
    }
    let mut raw = s[i..j].trim_end_matches(['.', ',', ';', ':', '!', '?']);
    while raw.ends_with(')') && count_byte(raw, b')') > count_byte(raw, b'(') {
        raw = &raw[..raw.len() - 1];
    }
    let parsed = parse_url(raw)?;
    if parsed.host.is_empty() {
        return None;
    }
    Some((raw.to_owned(), i + raw.len()))
}

fn is_url_terminator(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | b'<' | b'>' | b'"' | b'\'' | b'`'
    )
}

fn is_word_boundary(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let b = bytes[i - 1];
    !(b.is_ascii_alphanumeric() || b == b'_')
}

fn count_byte(s: &str, c: u8) -> usize {
    s.bytes().filter(|b| *b == c).count()
}

fn starts_with_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
}

fn run_len(bytes: &[u8], i: usize, c: u8) -> usize {
    let mut n = 0;
    while i + n < bytes.len() && bytes[i + n] == c {
        n += 1;
    }
    n
}

/// Returns the index just past a closing backtick run of exactly `n`
/// backticks, or `None` when there is none.
fn code_span_end(s: &[u8], from: usize, n: usize) -> Option<usize> {
    let mut j = from;
    while j < s.len() {
        if s[j] == b'`' {
            let k = run_len(s, j, b'`');
            if k == n {
                return Some(j + k);
            }
            j += k;
            continue;
        }
        j += 1;
    }
    None
}

/// Reports a fenced-code-block opening: three or more backticks or tildes.
fn fence_open(line: &str) -> Option<(u8, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 || trimmed.is_empty() {
        return None;
    }
    let c = trimmed.as_bytes()[0];
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = run_len(trimmed.as_bytes(), 0, c);
    if n < 3 {
        return None;
    }
    Some((c, n))
}

/// Reports a fence closing line matching the opening char and length.
fn is_fence_close(line: &str, c: u8, n: usize) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let m = run_len(trimmed.as_bytes(), 0, c);
    if m < n {
        return false;
    }
    trimmed[m..].trim().is_empty()
}

/// Converts marker runes in rendered output into OSC 8 hyperlinks, styling the
/// enclosed label with `link_sgr` (from [`Theme::link_sgr`]).
///
/// Any marker that cannot be matched is stripped so Private Use runes never
/// leak into the preview; a start marker whose link was dropped by the caller
/// is removed too. The SGR state active at a marker is saved and restored
/// around the label, and OSC payloads in the rendered text are copied
/// untouched.
#[must_use]
pub fn patch_links(rendered: &str, links: &[LinkInfo], hyperlinks: bool, link_sgr: &str) -> String {
    if links.is_empty() || !rendered.chars().any(is_link_marker) {
        return rendered.to_owned();
    }
    let label_start = format!("\x1b[{link_sgr}m");
    let mut out = String::with_capacity(rendered.len() + links.len() * 16);
    let mut active = String::new();
    let mut saved: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rendered.len() {
        if let Some(m) = ANSI_SGR_RE.find(&rendered[i..]) {
            if m.start() == 0 {
                let seq = &rendered[i..i + m.end()];
                out.push_str(seq);
                update_active_sgr(&mut active, seq);
                i += m.end();
                continue;
            }
        }
        let ch = rendered[i..].chars().next().unwrap_or('\u{FFFD}');
        let size = ch.len_utf8();
        if ch != LINK_END_RUNE && is_link_marker(ch) {
            let idx = (u32::from(ch) - LINK_START_BASE)
                .try_into()
                .unwrap_or(usize::MAX);
            if let Some(link) = links.get(idx) {
                saved.push(active.clone());
                if hyperlinks {
                    out.push_str(&set_hyperlink(&link.url));
                }
                out.push_str(&label_start);
            } else {
                out.push_str(&rendered[i..i + size]);
            }
        } else if ch == LINK_END_RUNE {
            out.push_str(RESET_SGR);
            if let Some(restore) = saved.pop() {
                out.push_str(&restore);
            }
            if hyperlinks {
                out.push_str(reset_hyperlink());
            }
        } else {
            out.push_str(&rendered[i..i + size]);
        }
        i += size;
    }
    if out.chars().any(is_link_marker) {
        remove_link_markers(&out)
    } else {
        out
    }
}

/// Reports whether `c` is one of the link-carrier runes.
fn is_link_marker(c: char) -> bool {
    (LINK_START_BASE..=u32::from(LINK_END_RUNE)).contains(&u32::from(c))
}

/// Drops any leftover carrier runes.
fn remove_link_markers(s: &str) -> String {
    s.chars().filter(|c| !is_link_marker(*c)).collect()
}

/// Appends an OSC 8 reset when `s` ends while a hyperlink is still open.
///
/// Truncating rendered preview text can drop the closing sequence while
/// keeping the opener; SGR resets do not close a hyperlink, so the status bar
/// and later output would otherwise inherit it.
#[must_use]
pub fn close_open_hyperlinks(s: &str) -> String {
    let mut last_uri_open = false;
    let mut found = false;
    for caps in OSC8_HYPERLINK_RE.captures_iter(s) {
        found = true;
        last_uri_open = caps.get(1).is_some_and(|uri| !uri.as_str().is_empty());
    }
    if found && last_uri_open {
        let mut out = String::with_capacity(s.len() + reset_hyperlink().len());
        out.push_str(s);
        out.push_str(reset_hyperlink());
        out
    } else {
        s.to_owned()
    }
}

/// Returns the first web link a preview block exposes: its `Resource`
/// frontmatter row when that is browser-openable, otherwise the first web link
/// in the body; empty when there is none.
#[must_use]
pub fn doc_first_link(block: &PreviewBlock) -> String {
    let resource = block.fields.iter().find(|field| field.label == "Resource");
    if let Some(field) = resource {
        if let FieldValue::Scalar(value) = &field.value {
            if is_web_link(value) {
                return value.clone();
            }
        }
    }
    extract_and_hide(&block.body_markdown, true)
        .1
        .first()
        .map_or_else(String::new, |link| link.url.clone())
}

/// Minimal `net/url`-shaped parse result: the fields the link helpers use.
#[derive(Debug, Clone)]
struct ParsedUrl {
    /// Lowercased scheme without `:`; empty for relative references.
    scheme: String,
    /// Authority minus userinfo (port included); empty for opaque/relative.
    host: String,
    /// Everything after `scheme:` when the URL has no `//` authority, up to
    /// `?` or `#`.
    opaque: String,
}

/// Parses enough of a URL to mirror `net/url.Parse` for the link helpers:
/// scheme, host, opaque part, control-byte rejection, percent-escape
/// validation (query strings are unvalidated, like Go), and port validation.
fn parse_url(raw: &str) -> Option<ParsedUrl> {
    let s = raw.trim();
    let bytes = s.as_bytes();
    if bytes.iter().any(|b| *b < b' ' || *b == 0x7f) {
        return None;
    }
    if !valid_percent_encoding(bytes) {
        return None;
    }
    let Some(colon) = scheme_split(bytes) else {
        return Some(ParsedUrl {
            scheme: String::new(),
            host: String::new(),
            opaque: s.to_owned(),
        });
    };
    let scheme = s[..colon].to_ascii_lowercase();
    let rest = &s[colon + 1..];
    if let Some(after) = rest.strip_prefix("//") {
        let auth_end = after.find(['/', '?', '#']).unwrap_or(after.len());
        let host = parse_host(&after[..auth_end])?;
        return Some(ParsedUrl {
            scheme,
            host,
            opaque: String::new(),
        });
    }
    let opaque_end = rest.find(['?', '#']).unwrap_or(rest.len());
    Some(ParsedUrl {
        scheme,
        host: String::new(),
        opaque: rest[..opaque_end].to_owned(),
    })
}

/// Returns the index of the `:` ending a valid scheme, or `None` for a
/// relative reference (mirrors Go `net/url.getScheme`).
fn scheme_split(bytes: &[u8]) -> Option<usize> {
    for (i, b) in bytes.iter().enumerate() {
        if b.is_ascii_alphabetic() {
            continue;
        }
        if *b == b':' {
            return if i == 0 { None } else { Some(i) };
        }
        if i > 0 && (b.is_ascii_digit() || matches!(*b, b'+' | b'-' | b'.')) {
            continue;
        }
        return None;
    }
    None
}

/// Parses the authority into a host (port included), mirroring the validation
/// `net/url` performs on ports and bracketed IPv6 literals.
fn parse_host(authority: &str) -> Option<String> {
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if hostport.is_empty() {
        return Some(String::new());
    }
    if hostport.contains([' ', '"', '<', '>', '\\', '^', '`', '{', '|', '}']) {
        return None;
    }
    if let Some(rest) = hostport.strip_prefix('[') {
        let close = rest.find(']')?;
        let after = &rest[close + 1..];
        if after.is_empty() {
            return Some(hostport.to_owned());
        }
        let port = after.strip_prefix(':')?;
        return valid_port(port).then(|| hostport.to_owned());
    }
    if hostport.matches(':').count() > 1 {
        return None;
    }
    if let Some((_, port)) = hostport.rsplit_once(':') {
        if !valid_port(port) {
            return None;
        }
    }
    Some(hostport.to_owned())
}

fn valid_port(port: &str) -> bool {
    port.bytes().all(|b| b.is_ascii_digit())
}

/// Validates `%XX` escapes outside the query string, mirroring the components
/// `net/url` unescapes during `Parse`.
fn valid_percent_encoding(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut in_query = false;
    while i < bytes.len() {
        match bytes[i] {
            b'?' if !in_query => in_query = true,
            b'#' => in_query = false,
            b'%' if !in_query => {
                let Some(hi) = bytes.get(i + 1) else {
                    return false;
                };
                let Some(lo) = bytes.get(i + 2) else {
                    return false;
                };
                if !hi.is_ascii_hexdigit() || !lo.is_ascii_hexdigit() {
                    return false;
                }
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    true
}

/// Extracts the literal substrings worth emphasizing from a raw query.
///
/// Deliberately syntax-light, like Go `queryTerms`: bare words and `key:value`
/// values are kept, while date expressions and negated tags are skipped.
#[must_use]
pub fn query_terms(q: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for tok in split_query_tokens(q) {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some(i) = tok.find(':') {
            let key = tok[..i].trim().to_lowercase();
            let val = &tok[i + 1..];
            match key.as_str() {
                "created" | "updated" | "date" | "before" | "after" => {}
                "tag" | "tags" => {
                    for part in val.split(',') {
                        let part = strip_wrapping_quotes(part.trim());
                        if part.is_empty() || part.starts_with('-') || part.starts_with('!') {
                            continue;
                        }
                        add_term(&mut terms, part);
                    }
                }
                _ => add_term(&mut terms, strip_wrapping_quotes(val.trim())),
            }
        } else {
            add_term(&mut terms, strip_wrapping_quotes(tok));
        }
    }
    terms
}

/// Adds `term` when it contains a word rune; a multi-word term is also matched
/// word by word so it can be emphasized across styled runs.
fn add_term(terms: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if term.is_empty() || !has_word_rune(term) {
        return;
    }
    terms.push(term.to_owned());
    let fields: Vec<&str> = term.split_whitespace().collect();
    if fields.len() > 1 {
        for field in fields {
            terms.push(field.to_owned());
        }
    }
}

fn has_word_rune(s: &str) -> bool {
    s.chars().any(char::is_alphanumeric)
}

/// Splits on whitespace while respecting double quotes.
fn split_query_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for r in input.chars() {
        match r {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(r);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// Removes one pair of surrounding double quotes.
fn strip_wrapping_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Builds a case-insensitive alternation of the literal terms, longest first
/// so the most specific match wins; `None` when there is nothing to highlight.
#[must_use]
pub fn terms_regexp(terms: &[String]) -> Option<Regex> {
    let mut seen = HashSet::new();
    let mut uniq: Vec<&str> = Vec::new();
    for term in terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        if seen.insert(term.to_lowercase()) {
            uniq.push(term);
        }
    }
    if uniq.is_empty() {
        return None;
    }
    uniq.sort_by_key(|term| std::cmp::Reverse(term.len()));
    let alternatives = uniq
        .iter()
        .map(|term| regex::escape(term))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!("(?i)({alternatives})")).ok()
}

/// Emphasizes terms in `s`, compiling their match regexp on the fly. Callers
/// rendering many strings should prefer [`highlight_re`] with a cached regexp.
#[must_use]
pub fn highlight(s: &str, terms: &[String], hl_sgr: &str) -> String {
    highlight_re(s, terms_regexp(terms).as_ref(), hl_sgr)
}

/// Emphasizes terms in `s` with the active theme's highlight style.
///
/// This is the render-time entry point for widgets: it reads
/// [`Theme::highlight_sgr`] and never bakes SGR parameters into view-model
/// data.
#[must_use]
pub fn highlight_with_theme(s: &str, terms: &[String], theme: &Theme) -> String {
    highlight(s, terms, &theme.highlight_sgr)
}

/// Emphasizes every occurrence matched by `re` in `s`, wrapping matches in the
/// SGR parameters `hl_sgr`.
///
/// Safe on text containing ANSI SGR sequences: they are preserved and the
/// style active at the start of a span is re-applied after it ends. OSC
/// sequences (hyperlinks) pass through untouched so a match never corrupts
/// their payload.
#[must_use]
pub fn highlight_re(s: &str, re: Option<&Regex>, hl_sgr: &str) -> String {
    let Some(re) = re else {
        return s.to_owned();
    };
    if s.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(s.len());
    let mut active = String::new();
    let mut i = 0;
    while i < s.len() {
        // Only an ESC "]" pair can begin an OSC; probing the OSC regexp on
        // every other position would rescan the whole remaining string and
        // make highlighting quadratic on SGR-dense renderer output.
        if s.as_bytes()[i] == 0x1b && s.as_bytes().get(i + 1) == Some(&b']') {
            if let Some(m) = ANSI_OSC_RE.find(&s[i..]) {
                if m.start() == 0 {
                    out.push_str(&s[i..i + m.end()]);
                    i += m.end();
                    continue;
                }
            }
        }
        let sgr = ANSI_SGR_RE.find(&s[i..]);
        if let Some(m) = &sgr {
            if m.start() == 0 {
                let seq = &s[i..i + m.end()];
                out.push_str(seq);
                update_active_sgr(&mut active, seq);
                i += m.end();
                continue;
            }
        }
        let mut end = s.len();
        if let Some(m) = &sgr {
            end = i + m.start();
        }
        if let Some(m) = ANSI_OSC_RE.find(&s[i..end]) {
            end = i + m.start();
        }
        write_highlighted_run(&mut out, &s[i..end], re, &active, hl_sgr);
        i = end;
    }
    out
}

/// Wraps term matches inside a single unstyled run (a span free of ANSI
/// escapes) and restores `active` afterwards.
fn write_highlighted_run(out: &mut String, run: &str, re: &Regex, active: &str, hl_sgr: &str) {
    if run.is_empty() {
        return;
    }
    let mut matches = re.find_iter(run);
    let Some(first) = matches.next() else {
        out.push_str(run);
        return;
    };
    let start = format!("\x1b[{hl_sgr}m");
    let mut last = 0;
    for m in std::iter::once(first).chain(matches) {
        out.push_str(&run[last..m.start()]);
        out.push_str(&start);
        out.push_str(&run[m.start()..m.end()]);
        out.push_str(RESET_SGR);
        out.push_str(active);
        last = m.end();
    }
    out.push_str(&run[last..]);
}

/// Tracks the cumulative SGR state so it can be re-applied after an
/// emphasized span. A reset clears the accumulated state.
fn update_active_sgr(active: &mut String, seq: &str) {
    let params = seq
        .strip_prefix("\x1b[")
        .and_then(|rest| rest.strip_suffix('m'))
        .unwrap_or(seq);
    let mut reset = params.is_empty() || params == "0";
    if !reset {
        reset = params.split(';').any(|p| p == "0");
    }
    if reset {
        active.clear();
        if params.is_empty() || params == "0" {
            return;
        }
    }
    active.push_str(seq);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdfu_core::model::Document;

    const DEFAULT_HL: &str = "1;30;103";
    const DEFAULT_LINK: &str = "1;4;38;5;212";
    /// `LINK_END_RUNE - LINK_START_BASE`, the marker budget.
    const CAP: usize = 0x18FF;

    fn marker(idx: u32) -> char {
        char::from_u32(LINK_START_BASE + idx).unwrap()
    }

    fn link(label: &str, url: &str) -> LinkInfo {
        LinkInfo {
            label: label.to_owned(),
            url: url.to_owned(),
        }
    }

    fn extract(src: &str) -> (String, Vec<LinkInfo>) {
        extract_and_hide(src, true)
    }

    type LinkCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

    fn visible(s: &str) -> String {
        remove_link_markers(s)
    }

    fn plain(s: &str) -> String {
        let no_osc = OSC8_HYPERLINK_RE.replace_all(s, "");
        ANSI_SGR_RE.replace_all(&no_osc, "").into_owned()
    }

    #[test]
    fn is_web_link_accepts_only_web_schemes() {
        let cases = [
            ("https://x.example", true),
            ("http://x.example", true),
            ("HTTP://X.EXAMPLE", true),
            ("http:foo", true),
            ("mailto:foo@bar", true),
            (" https://x.example ", true),
            ("notes/x.md", false),
            ("#anchor", false),
            ("ftp://x.example", false),
            ("javascript:alert(1)", false),
            ("", false),
            ("http://x.example:port", false),
            ("http://[::1", false),
            ("http://x.example/%zz", false),
            ("http://x.example?q=%zz", true),
        ];
        for (raw, want) in cases {
            assert_eq!(is_web_link(raw), want, "is_web_link({raw:?})");
        }
    }

    #[test]
    fn link_label_shortens_hosts_and_mailtos() {
        let cases = [
            ("https://charm.sh/x", "charm.sh"),
            ("https://user@host.example:8080/p?q=1", "host.example:8080"),
            ("https://[::1]:8080/x", "[::1]:8080"),
            ("mailto:foo@bar", "foo@bar"),
            ("mailto:foo@bar?subject=x", "foo@bar"),
            ("notes/x.md", "notes/x.md"),
            ("http:", "http:"),
            ("", ""),
            ("http://x.example:port", "http://x.example:port"),
        ];
        for (raw, want) in cases {
            assert_eq!(link_label(raw), want, "link_label({raw:?})");
        }
    }

    #[test]
    fn link_label_mirrors_go_mailto_fallbacks() {
        // Opaque mailto: label is the address.
        assert_eq!(link_label("mailto:foo@bar"), "foo@bar");
        // `mailto://a@b` has an authority, not an opaque part: Go returns the
        // raw string minus a *case-sensitive* `mailto:` prefix.
        assert_eq!(link_label("mailto://a@b"), "//a@b");
        assert_eq!(link_label("MAILTO:foo"), "foo");
        assert_eq!(link_label("MAILTO:"), "MAILTO:");
    }

    #[test]
    fn extract_and_hide_inline_link_marks_web_links() {
        let (out, links) = extract("See [Charm](https://charm.sh) for more.\n");
        assert_eq!(links, vec![link("Charm", "https://charm.sh")]);
        let want = format!(
            "See {m}Charm{END} for more.\n",
            m = marker(0),
            END = LINK_END_RUNE
        );
        assert_eq!(out, want);
        assert_eq!(visible(&out), "See Charm for more.\n");
    }

    #[test]
    fn extract_and_hide_preserves_crlf_and_empty_input() {
        let (out, links) = extract("a\r\n[b](https://x.example)\r\n");
        assert_eq!(links, vec![link("b", "https://x.example")]);
        assert!(out.starts_with("a\r\n"));
        assert!(out.ends_with("\r\n"));
        assert!(out.contains(marker(0)));
        assert_eq!(extract_and_hide("", true), (String::new(), Vec::new()));
    }

    #[test]
    fn extract_and_hide_covers_every_web_link_form() {
        let cases: &[LinkCase] = &[
            (
                "[label](https://x.example)",
                &[("label", "https://x.example")],
                "label",
            ),
            (
                "<https://example.com>",
                &[("example.com", "https://example.com")],
                "example.com",
            ),
            (
                "see https://example.com/a now",
                &[("example.com", "https://example.com/a")],
                "see example.com now",
            ),
            (
                "HTTPS://Example.COM/Path",
                &[("Example.COM", "HTTPS://Example.COM/Path")],
                "Example.COM",
            ),
            (
                "[docs](https://example.com/a_(b))",
                &[("docs", "https://example.com/a_(b)")],
                "docs",
            ),
            ("[a](mailto:foo@bar)", &[("a", "mailto:foo@bar")], "a"),
            (
                "[a](https://x.example \"title\")",
                &[("a", "https://x.example")],
                "a",
            ),
            (
                "[a [b] c](https://x.example)",
                &[("a [b] c", "https://x.example")],
                "a [b] c",
            ),
            (
                "[a\\]b](https://x.example)",
                &[("a\\]b", "https://x.example")],
                "a\\]b",
            ),
            (
                "(https://x.example)",
                &[("x.example", "https://x.example")],
                "(x.example)",
            ),
            (
                "see https://x.example. done",
                &[("x.example", "https://x.example")],
                "see x.example. done",
            ),
            (
                "see https://x.example/a) ok",
                &[("x.example", "https://x.example/a")],
                "see x.example) ok",
            ),
            (
                "see https://x.example/a_(b) now",
                &[("x.example", "https://x.example/a_(b)")],
                "see x.example now",
            ),
        ];
        for (src, links, visible_want) in cases {
            let (out, got) = extract(src);
            let want: Vec<LinkInfo> = links.iter().map(|(l, u)| link(l, u)).collect();
            assert_eq!(got, want, "links for {src:?}");
            assert_eq!(visible(&out), *visible_want, "visible for {src:?}");
        }
    }

    #[test]
    fn extract_and_hide_reduces_non_web_links_to_labels() {
        let cases = [
            "[other](notes/other.md)",
            "[top](#section)",
            "[ftp](ftp://x.example)",
            "[a](b.md)",
        ];
        for src in cases {
            let (out, links) = extract(src);
            assert!(links.is_empty(), "links for {src:?}");
            assert_eq!(out, src[1..src.len() - 1].split("](").next().unwrap());
        }
    }

    #[test]
    fn extract_and_hide_skips_images() {
        let src = "![diagram](https://img.example/d_(1).png) then [doc](https://doc.example)\n";
        let (out, links) = extract(src);
        assert_eq!(links, vec![link("doc", "https://doc.example")]);
        assert!(out.contains("![diagram](https://img.example/d_(1).png)"));
        assert_eq!(
            visible(&out),
            "![diagram](https://img.example/d_(1).png) then doc\n"
        );
    }

    #[test]
    fn extract_and_hide_ignores_bare_urls_inside_words() {
        let (_, links) = extract("xhttps://x.example and https://y.example");
        assert_eq!(links, vec![link("y.example", "https://y.example")]);
    }

    #[test]
    fn extract_and_hide_leaves_code_untouched() {
        let cases = [
            "Inline `[x](https://inline.example)`.",
            "``a [x](https://a.example)``",
            "```\n[y](https://fenced.example)\n```\n",
            "~~~\n[y](https://f.example)\n~~~\n",
            "   ```\n[y](https://f.example)\n   ```\n",
            "```\n[y](https://f.example)\n",
        ];
        for src in cases {
            let (out, links) = extract(src);
            assert!(links.is_empty(), "links for {src:?}");
            assert_eq!(out, src, "output changed for {src:?}");
        }
    }

    #[test]
    fn extract_and_hide_resumes_after_fence_and_unclosed_code() {
        let src = "```\n[y](https://fenced.example)\n```\n[z](https://after.example)\n";
        let (out, links) = extract(src);
        assert_eq!(links, vec![link("z", "https://after.example")]);
        assert!(out.starts_with("```\n[y](https://fenced.example)\n```\n"));

        // An unclosed backtick does NOT shield the rest of the line: Go emits
        // the run and keeps scanning.
        let (_, links) = extract("`unclosed [x](https://a.example)");
        assert_eq!(links, vec![link("x", "https://a.example")]);
    }

    #[test]
    fn extract_and_hide_fallback_keeps_label_and_url() {
        let src = "See [Charm](https://charm.sh) for more.\n";
        let (out, links) = extract_and_hide(src, false);
        assert_eq!(out, src);
        assert!(links.is_empty());
        assert!(out.contains("Charm"));
        assert!(out.contains("https://charm.sh"));
    }

    #[test]
    fn extract_and_hide_degrades_past_marker_cap() {
        let src: String = std::iter::repeat_n("[x](https://e.example) ", 6400).collect();
        let (out, links) = extract(&src);
        assert_eq!(links.len(), CAP);
        assert_eq!(links.last().unwrap().url, "https://e.example");
        let markers = out.chars().filter(|c| is_link_marker(*c)).count();
        assert_eq!(markers, CAP * 2);
        assert_eq!(out.matches(LINK_END_RUNE).count(), CAP);
    }

    #[test]
    fn doc_first_link_prefers_resource_row_then_body() {
        let doc = Document {
            resource: "https://res.example".to_owned(),
            body: "[b](https://body.example)".to_owned(),
            ..Document::default()
        };
        assert_eq!(
            doc_first_link(&PreviewBlock::from(&doc)),
            "https://res.example"
        );

        let doc = Document {
            resource: "notes/x.md".to_owned(),
            body: "text [b](https://body.example) more".to_owned(),
            ..Document::default()
        };
        assert_eq!(
            doc_first_link(&PreviewBlock::from(&doc)),
            "https://body.example"
        );

        let doc = Document {
            body: "no links here".to_owned(),
            ..Document::default()
        };
        assert_eq!(doc_first_link(&PreviewBlock::from(&doc)), "");
        assert_eq!(
            doc_first_link(&PreviewBlock::from(&Document::default())),
            ""
        );
    }

    #[test]
    fn osc8_link_wraps_label_and_resets() {
        let out = osc8_link("https://charm.sh", "Charm", DEFAULT_LINK);
        let want = format!(
            "{}\x1b[{DEFAULT_LINK}mCharm{RESET_SGR}{}",
            set_hyperlink("https://charm.sh"),
            reset_hyperlink()
        );
        assert_eq!(out, want);
    }

    #[test]
    fn links_handle_multibyte_text() {
        let src = "日本語 [リンク](https://x.example) émoji\n";
        let (out, links) = extract(src);
        assert_eq!(links, vec![link("リンク", "https://x.example")]);
        assert_eq!(visible(&out), "日本語 リンク émoji\n");

        let rendered = format!(
            "日本語 {m}ラベル{END} émoji",
            m = marker(0),
            END = LINK_END_RUNE
        );
        let patched = patch_links(&rendered, &links, true, DEFAULT_LINK);
        assert!(!patched.chars().any(is_link_marker));
        assert_eq!(plain(&patched), "日本語 ラベル émoji");

        let hl = highlight("日本語 ラベル", &["ラベル".to_owned()], DEFAULT_HL);
        assert_eq!(hl, format!("日本語 \x1b[{DEFAULT_HL}mラベル{RESET_SGR}"));
    }

    #[test]
    fn patch_links_emits_osc8_and_restores_active_sgr() {
        let rendered = format!(
            "\x1b[1;34mlead {m}Charm{END}\x1b[0m",
            m = marker(0),
            END = LINK_END_RUNE
        );
        let out = patch_links(
            &rendered,
            &[link("Charm", "https://charm.sh")],
            true,
            DEFAULT_LINK,
        );
        let want = format!(
            "\x1b[1;34mlead {}\x1b[{DEFAULT_LINK}mCharm{RESET_SGR}\x1b[1;34m{}\x1b[0m",
            set_hyperlink("https://charm.sh"),
            reset_hyperlink()
        );
        assert_eq!(out, want);
    }

    #[test]
    fn patch_links_fallback_strips_markers_without_osc8() {
        let rendered = format!("{m}Charm{END}", m = marker(0), END = LINK_END_RUNE);
        let out = patch_links(
            &rendered,
            &[link("Charm", "https://charm.sh")],
            false,
            DEFAULT_LINK,
        );
        assert!(!out.contains("\x1b]8;"));
        assert!(out.contains(&format!("\x1b[{DEFAULT_LINK}mCharm{RESET_SGR}")));
        assert!(!out.chars().any(is_link_marker));
    }

    #[test]
    fn patch_links_never_leaks_marker_runes() {
        let links = vec![link("A", "https://a.example")];
        let rendered = format!(
            "{m0}A{E}{m7}B{E}",
            m0 = marker(0),
            m7 = marker(7),
            E = LINK_END_RUNE
        );
        let out = patch_links(&rendered, &links, true, DEFAULT_LINK);
        assert!(!out.chars().any(is_link_marker), "marker leaked: {out:?}");
        assert_eq!(plain(&out), "AB");
        assert_eq!(out.matches(&set_hyperlink("https://a.example")).count(), 1);

        let dangling_end = format!("x{LINK_END_RUNE}y");
        let out = patch_links(&dangling_end, &links, true, DEFAULT_LINK);
        assert!(!out.chars().any(is_link_marker));
        assert!(out.contains(reset_hyperlink()));
    }

    #[test]
    fn patch_links_is_identity_without_links_or_markers() {
        let rendered = "plain text";
        assert_eq!(patch_links(rendered, &[], true, DEFAULT_LINK), rendered);
        // Go returns early when the table is empty even if markers are present.
        let marked = format!("{m}x", m = marker(0));
        assert_eq!(patch_links(&marked, &[], true, DEFAULT_LINK), marked);
        assert_eq!(
            patch_links(
                rendered,
                &[link("x", "https://x.example")],
                true,
                DEFAULT_LINK
            ),
            rendered
        );
    }

    #[test]
    fn close_open_hyperlinks_seals_only_dangling_openers() {
        let open = format!("before {}Charm", set_hyperlink("https://charm.sh"));
        assert_eq!(
            close_open_hyperlinks(&open),
            format!("{open}{}", reset_hyperlink())
        );
        let closed = format!("{open}{}", reset_hyperlink());
        assert_eq!(close_open_hyperlinks(&closed), closed);
        assert_eq!(close_open_hyperlinks("plain text"), "plain text");
        let reset_only = format!("before {}after", reset_hyperlink());
        assert_eq!(close_open_hyperlinks(&reset_only), reset_only);
        let st = "\x1b]8;;https://x.example\x1b\\text";
        assert_eq!(
            close_open_hyperlinks(st),
            format!("{st}{}", reset_hyperlink())
        );
        let with_params = "\x1b]8;id=1;https://x.example\x07text";
        assert_eq!(
            close_open_hyperlinks(with_params),
            format!("{with_params}{}", reset_hyperlink())
        );
        let reopened = format!(
            "{open}{}{}",
            reset_hyperlink(),
            set_hyperlink("https://y.example")
        );
        assert_eq!(
            close_open_hyperlinks(&reopened),
            format!("{reopened}{}", reset_hyperlink())
        );
    }

    #[test]
    fn truncated_patched_output_is_closed() {
        let body = "start [Charm](https://charm.sh) end\n";
        let (hidden, links) = extract(body);
        let patched = patch_links(&hidden, &links, true, DEFAULT_LINK);
        assert!(!patched.chars().any(is_link_marker));
        assert_eq!(
            patched.matches(&set_hyperlink("https://charm.sh")).count(),
            1
        );
        assert_eq!(patched.matches(reset_hyperlink()).count(), 1);
        assert_eq!(close_open_hyperlinks(&patched), patched);

        let cut = patched.find("Charm").unwrap() + 2;
        let truncated = &patched[..cut];
        assert!(!truncated.contains(reset_hyperlink()));
        let sealed = close_open_hyperlinks(truncated);
        assert!(sealed.ends_with(reset_hyperlink()));
        assert_eq!(sealed, format!("{truncated}{}", reset_hyperlink()));
    }

    #[test]
    fn query_terms_mirrors_go_table() {
        let got = query_terms(r#"tag:launch,beta "ship mdfu" created:2024 bare tag:-skip"#);
        assert_eq!(
            got,
            vec!["launch", "beta", "ship mdfu", "ship", "mdfu", "bare"]
        );
    }

    #[test]
    fn query_terms_skips_dates_negations_and_punctuation() {
        assert!(query_terms("created:2024 updated:2024 date:2024 before:x after:y").is_empty());
        assert!(query_terms("tag:!skip,-also,ok").eq(&["ok"]));
        assert!(query_terms("... --- ").is_empty());
        assert!(query_terms("").is_empty());
        assert_eq!(query_terms("type:Task"), vec!["Task"]);
        assert_eq!(
            query_terms(r#""quoted phrase""#),
            vec!["quoted phrase", "quoted", "phrase"]
        );
    }

    #[test]
    fn terms_regexp_dedupes_sorts_and_escapes() {
        assert!(terms_regexp(&[]).is_none());
        assert!(terms_regexp(&["   ".to_owned()]).is_none());

        let re = terms_regexp(&["Foo".to_owned(), "FOO".to_owned()]).unwrap();
        assert_eq!(re.as_str(), "(?i)(Foo)");

        let re = terms_regexp(&["ab".to_owned(), "abc".to_owned()]).unwrap();
        assert_eq!(re.as_str(), "(?i)(abc|ab)");
        assert_eq!(re.find("xabcx").unwrap().as_str(), "abc");

        let re = terms_regexp(&["a.b".to_owned()]).unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("axb"));

        let re = terms_regexp(&["parser".to_owned()]).unwrap();
        assert!(re.is_match("PARSER"));
    }

    #[test]
    fn highlight_preserves_ansi_like_go() {
        let input = "\x1b[31mhello world\x1b[0m";
        let out = highlight(input, &["world".to_owned()], DEFAULT_HL);
        let want = format!("\x1b[31mhello \x1b[{DEFAULT_HL}mworld{RESET_SGR}\x1b[31m\x1b[0m");
        assert_eq!(out, want);
    }

    #[test]
    fn highlight_does_not_match_inside_osc_payloads() {
        let osc = set_hyperlink("https://world.example");
        let input = format!("{osc}hello world");
        let out = highlight(&input, &["world".to_owned()], DEFAULT_HL);
        assert!(out.contains(&osc), "OSC payload corrupted: {out:?}");
        assert_eq!(out.matches(&format!("\x1b[{DEFAULT_HL}m")).count(), 1);
    }

    #[test]
    fn highlight_restores_cumulative_and_reset_sgr() {
        let input = "\x1b[1m\x1b[34mA term B\x1b[0m";
        let out = highlight(input, &["term".to_owned()], DEFAULT_HL);
        let want =
            format!("\x1b[1m\x1b[34mA \x1b[{DEFAULT_HL}mterm{RESET_SGR}\x1b[1m\x1b[34m B\x1b[0m");
        assert_eq!(out, want);

        // A parameter list containing 0 clears the accumulated state but the
        // whole sequence is re-applied (Go `updateActiveSGR`).
        let input = "\x1b[0;31mA term";
        let out = highlight(input, &["term".to_owned()], "7");
        assert_eq!(out, "\x1b[0;31mA \x1b[7mterm\x1b[0m\x1b[0;31m");

        // An empty parameter list resets without being re-applied.
        let input = "\x1b[1mA \x1b[mterm";
        let out = highlight(input, &["term".to_owned()], "7");
        assert_eq!(out, "\x1b[1mA \x1b[m\x1b[7mterm\x1b[0m");
    }

    #[test]
    fn highlight_handles_no_terms_and_no_match() {
        assert_eq!(highlight_re("x", None, DEFAULT_HL), "x");
        assert_eq!(highlight("x", &[], DEFAULT_HL), "x");
        assert_eq!(highlight("abc", &["zzz".to_owned()], DEFAULT_HL), "abc");
        let re = terms_regexp(&["a".to_owned()]);
        assert_eq!(highlight_re("", re.as_ref(), DEFAULT_HL), "");
    }

    #[test]
    fn highlight_with_theme_uses_the_theme_sgr() {
        let default = highlight_with_theme("parser", &["parser".to_owned()], &Theme::default());
        assert!(default.contains(&format!("\x1b[{DEFAULT_HL}m")));

        let theme = Theme {
            highlight_sgr: "7;30;102".to_owned(),
            ..Theme::default()
        };
        let custom = highlight_with_theme("parser", &["parser".to_owned()], &theme);
        assert!(custom.contains("\x1b[7;30;102m"));
        assert!(!custom.contains("\x1b[1;30;103m"));
    }
}
