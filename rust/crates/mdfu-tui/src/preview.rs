//! Preview-pane integration for the note picker (Go `tui.go` preview paths).
//!
//! [`PreviewState`] owns everything the picker needs to show the document
//! preview: the `ctrl+p` (pane) and `ctrl+f` (frontmatter) toggles, the
//! horizontal/stacked layout switch at width 100, the row budgets that clamp
//! the panes, and a body-window cache so a frame never rescans a multi-megabyte
//! body.
//!
//! A picker frame composes this module with the reusable [`crate::widgets`]
//! stack:
//!
//! ```text
//! // Keep one PreviewState for the picker's lifetime.
//! let mut state = PreviewState::new(cfg.preview, theme.show_frontmatter);
//!
//! // Keys (both layouts, list and search mode):
//! state.toggle_preview();      // ctrl+p
//! state.toggle_frontmatter();  // ctrl+f
//!
//! // Per frame, split the terminal with the layout math:
//! match PreviewLayout::for_width(width) {
//!     PreviewLayout::Horizontal { list_width, preview_width } => {
//!         let rows = visible_rows(height, searching, state.show_preview(), width);
//!         // render the list into the left `list_width` and
//!         // state.render(right_area, buf, selected_doc, &theme, &terms).
//!     }
//!     PreviewLayout::Stacked => {
//!         let rows = preview_rows(height);
//!         // reserve `rows` at the bottom and render the preview there.
//!     }
//! }
//! ```
//!
//! Rendering goes through the widget [`Preview`] stack (filename, title,
//! frontmatter, markdown body), so row order and pill/object formatting stay
//! owned by [`PreviewBlock`]. This module only replaces the raw body with the
//! windowed excerpt: CRLF normalization, the 8 KiB byte cap centered on the
//! first query match, and the match-centered `body_lines` line window
//! (Go `bodyWindow` + `matchWindow`). The resulting per-document window is
//! cached by `(path, width, query)`; call [`PreviewState::invalidate`] after an
//! `$EDITOR` reload rewrites a document in place, and
//! [`PreviewState::clear`] after a theme swap.
//!
//! The match window *is* the preview's scroll behavior in the reference
//! implementation: there is no manual scroll state, the excerpt jumps to the
//! first hit and keeps it near the top of the pane.

// The module is named for the Go `internal/tui` preview concept; the public
// `PreviewState`/`PreviewLayout` names are fixed by the port plan, so silence
// the module-name-repetition lint crate-locally.
#![allow(clippy::module_name_repetitions)]

use std::collections::{BTreeSet, HashMap};

use mdfu_core::model::Document;
use mdfu_core::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use regex::Regex;

use crate::links;
use crate::view_model::PreviewBlock;
use crate::widgets::{style_from, Preview, COMPONENT_FRONTMATTER};

/// Minimum terminal width for the side-by-side layout (Go `m.width >= 100`).
pub const HORIZONTAL_MIN_WIDTH: u16 = 100;

/// Minimum width of either pane when the preview is side-by-side
/// (Go `listW < 20` / `prevW < 20` clamps).
pub const MIN_PANE_WIDTH: u16 = 20;

/// Maximum body source bytes handed to the markdown renderer
/// (Go `maxPreviewBodyBytes`).
pub const MAX_PREVIEW_BODY_BYTES: usize = 8 * 1024;

/// How many `(document, width, query)` body windows a [`PreviewState`] keeps.
/// Reaching the cap clears the map; browsing stays bounded by the selection
/// history, not by the number of frames.
const WINDOW_CACHE_CAPACITY: usize = 64;

/// The picker layout at a given terminal width: side-by-side above 100 columns,
/// stacked below it (Go `renderPreviewPane`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewLayout {
    /// Terminal is at least [`HORIZONTAL_MIN_WIDTH`] wide: the list takes
    /// `list_width` columns and the preview `preview_width`.
    Horizontal {
        /// Column budget for the result list (Go `listW`).
        list_width: u16,
        /// Column budget for the preview pane (Go `prevW`).
        preview_width: u16,
    },
    /// Terminal is narrower than [`HORIZONTAL_MIN_WIDTH`]: the preview stacks
    /// under the list at full width.
    Stacked,
}

impl PreviewLayout {
    /// Resolves the layout for a terminal `width`, including Go's 20-column
    /// pane clamps.
    #[must_use]
    pub fn for_width(width: u16) -> Self {
        if width < HORIZONTAL_MIN_WIDTH {
            return Self::Stacked;
        }
        let mut list_width = width / 2 - 2;
        let mut preview_width = width - list_width - 4;
        if list_width < MIN_PANE_WIDTH {
            list_width = MIN_PANE_WIDTH;
        }
        if preview_width < MIN_PANE_WIDTH {
            preview_width = MIN_PANE_WIDTH;
        }
        Self::Horizontal {
            list_width,
            preview_width,
        }
    }

    /// Reports whether this is the side-by-side layout.
    #[must_use]
    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal { .. })
    }

    /// Width available to the preview pane at terminal `width` (full width when
    /// stacked).
    #[must_use]
    pub fn preview_width(&self, width: u16) -> u16 {
        match *self {
            Self::Horizontal { preview_width, .. } => preview_width,
            Self::Stacked => width,
        }
    }
}

/// Vertical budget for the stacked preview pane (Go `previewRows`): a third of
/// the terminal, clamped to 3..=12 rows; a headless model gets 10.
#[must_use]
pub fn preview_rows(height: u16) -> u16 {
    if height == 0 {
        return 10;
    }
    (height.saturating_sub(2) / 3).clamp(3, 12)
}

/// Result-list row budget (Go `visibleRows`): the terminal minus the status
/// bar, the search bar while `searching`, and the stacked preview pane (only
/// while it is shown below 100 columns); at least 3 rows, or 20 headless.
#[must_use]
pub fn visible_rows(height: u16, searching: bool, show_preview: bool, width: u16) -> u16 {
    if height == 0 {
        return 20;
    }
    let mut reserved = 1;
    if searching {
        reserved += 1;
    }
    if show_preview && width < HORIZONTAL_MIN_WIDTH {
        reserved += preview_rows(height);
    }
    height.saturating_sub(reserved).max(3)
}

/// Windows `body` for the preview (Go `matchWindow` with `terms_regexp`).
///
/// CRLF pairs normalize to `\n`; bodies above [`MAX_PREVIEW_BODY_BYTES`]
/// shrink to a rune-aligned 8 KiB slice centered on the first query match (or
/// the head without one); the result keeps at most `body_lines` lines around
/// that match, flagged with `…` when leading/trailing lines were skipped.
#[must_use]
pub fn body_window(body: &str, terms: &[String], body_lines: u16) -> String {
    let regex = links::terms_regexp(terms);
    match_window(body, regex.as_ref(), body_lines)
}

/// Renders one document preview into `area` through the widget [`Preview`]
/// stack, without retaining a cache (Go `PreviewText*`). Long-lived pickers
/// should keep a [`PreviewState`] instead so repeated frames reuse body windows.
pub fn render_preview(
    area: Rect,
    buf: &mut Buffer,
    doc: Option<&Document>,
    theme: &Theme,
    terms: &[String],
    show_frontmatter: bool,
) {
    let mut state = PreviewState::new(true, show_frontmatter);
    state.render(area, buf, doc, theme, terms);
}

/// Identity of a cached body window: the document path (the picker's stable
/// document ID), the render width, and the normalized query terms. `lines`
/// joins the key so a theme swap that changes the window size cannot serve a
/// stale excerpt.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WindowKey {
    path: String,
    width: u16,
    lines: u16,
    query: String,
}

/// Preview-pane state for the picker: visibility toggles, the body-window
/// cache, and the render entry point.
#[derive(Debug, Clone)]
pub struct PreviewState {
    show_preview: bool,
    show_frontmatter: bool,
    windows: HashMap<WindowKey, PreviewBlock>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl PreviewState {
    /// Creates the state from the picker's initial `show_preview` (Config) and
    /// `show_frontmatter` (theme / Config) values.
    #[must_use]
    pub fn new(show_preview: bool, show_frontmatter: bool) -> Self {
        Self {
            show_preview,
            show_frontmatter,
            windows: HashMap::new(),
        }
    }

    /// Reports whether the `ctrl+p` pane toggle is on.
    #[must_use]
    pub fn show_preview(&self) -> bool {
        self.show_preview
    }

    /// Reports whether the `ctrl+f` frontmatter toggle is on.
    #[must_use]
    pub fn show_frontmatter(&self) -> bool {
        self.show_frontmatter
    }

    /// Sets the pane visibility (`ctrl+p` state, also used by Config).
    pub fn set_show_preview(&mut self, show_preview: bool) {
        self.show_preview = show_preview;
    }

    /// Sets the frontmatter visibility (`ctrl+f` state, also used by Config).
    pub fn set_show_frontmatter(&mut self, show_frontmatter: bool) {
        self.show_frontmatter = show_frontmatter;
    }

    /// Flips the `ctrl+p` pane toggle and reports the new state.
    #[must_use]
    pub fn toggle_preview(&mut self) -> bool {
        self.show_preview = !self.show_preview;
        self.show_preview
    }

    /// Flips the `ctrl+f` frontmatter toggle and reports the new state.
    #[must_use]
    pub fn toggle_frontmatter(&mut self) -> bool {
        self.show_frontmatter = !self.show_frontmatter;
        self.show_frontmatter
    }

    /// Returns the cached body window for `doc` at `width`, computing it on
    /// first use. A frame re-rendering the same selection therefore never
    /// rescans the full body: the window is recomputed only when the path,
    /// width, body-lines budget, or query terms change.
    #[must_use]
    pub fn body_window_for(
        &mut self,
        doc: &Document,
        width: u16,
        terms: &[String],
        theme: &Theme,
    ) -> &str {
        let key = self.ensure_windowed(doc, width, terms, theme);
        self.windows
            .get(&key)
            .map_or("", |block| block.body_markdown.as_str())
    }

    /// Drops the cached window for `path` (call after an `$EDITOR` reload
    /// replaces the document in place).
    pub fn invalidate(&mut self, path: &str) {
        self.windows.retain(|key, _| key.path != path);
    }

    /// Drops every cached window (call after a theme swap).
    pub fn clear(&mut self) {
        self.windows.clear();
    }

    /// Renders the preview pane into `area`, which the caller has already
    /// clamped with [`PreviewLayout`] and [`visible_rows`] / [`preview_rows`].
    ///
    /// Draws nothing when the `ctrl+p` toggle is off; draws the dim
    /// `(no preview)` placeholder when no document is selected (Go
    /// `renderPreviewPane`).
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        doc: Option<&Document>,
        theme: &Theme,
        terms: &[String],
    ) {
        if !self.show_preview || area.width == 0 || area.height == 0 {
            return;
        }
        let Some(doc) = doc else {
            render_placeholder(area, buf, theme);
            return;
        };
        let block = self.windowed_block(doc, area.width, terms, theme);
        let mut preview = Preview::new(&block, theme);
        preview.set_visible(COMPONENT_FRONTMATTER, self.show_frontmatter);
        preview.render(area, buf);
    }

    /// Cached, body-windowed snapshot for `doc`.
    fn windowed_block(
        &mut self,
        doc: &Document,
        width: u16,
        terms: &[String],
        theme: &Theme,
    ) -> PreviewBlock {
        let key = self.ensure_windowed(doc, width, terms, theme);
        if let Some(block) = self.windows.get(&key) {
            block.clone()
        } else {
            PreviewBlock::from(doc)
        }
    }

    /// Inserts the windowed block for `doc` when missing and returns its key.
    fn ensure_windowed(
        &mut self,
        doc: &Document,
        width: u16,
        terms: &[String],
        theme: &Theme,
    ) -> WindowKey {
        let key = WindowKey {
            path: doc.path.clone(),
            width,
            lines: theme.body_lines,
            query: query_key(terms),
        };
        if !self.windows.contains_key(&key) {
            if self.windows.len() >= WINDOW_CACHE_CAPACITY {
                self.windows.clear();
            }
            let mut block = PreviewBlock::from(doc);
            block.body_markdown = body_window(&block.body_markdown, terms, theme.body_lines);
            self.windows.insert(key.clone(), block);
        }
        key
    }

    /// Number of cached windows; the cache test asserts a repeated frame does
    /// not grow it.
    #[cfg(test)]
    pub(crate) fn cache_len(&self) -> usize {
        self.windows.len()
    }
}

/// Order-independent query key: trimmed, de-duplicated terms joined with unit
/// separators, so the same query always hits the same cache entry.
fn query_key(terms: &[String]) -> String {
    let unique: BTreeSet<&str> = terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .collect();
    unique.into_iter().collect::<Vec<_>>().join("\u{1f}")
}

/// Go `matchWindow`: CRLF-normalize, byte-cap, then keep a `body_lines` window
/// around the first regexp hit with `…` markers on the truncated sides.
fn match_window(body: &str, regex: Option<&Regex>, body_lines: u16) -> String {
    if body_lines == 0 {
        return String::new();
    }
    let normalized = body.replace("\r\n", "\n");
    let windowed = cap_body_bytes(&normalized, regex);
    let lines: Vec<&str> = windowed.split('\n').collect();
    let limit = usize::from(body_lines);
    if regex.is_none() || lines.len() <= limit {
        return body_excerpt(&windowed, body_lines);
    }
    let Some(found) = regex.and_then(|regex| regex.find(&windowed)) else {
        return body_excerpt(&windowed, body_lines);
    };
    let match_line = windowed[..found.start()].matches('\n').count();
    let start = match_line.saturating_sub(2);
    let end = (start + limit).min(lines.len());
    let mut out = lines[start..end].join("\n");
    if start > 0 {
        out = format!("…\n{out}");
    }
    if end < lines.len() {
        out.push_str("\n…");
    }
    out
}

/// Go `bodyWindow`: bounces small bodies back unchanged, otherwise returns a
/// rune-aligned [`MAX_PREVIEW_BODY_BYTES`] slice starting a quarter-cap ahead
/// of the first query match (the body head without one).
fn cap_body_bytes(body: &str, regex: Option<&Regex>) -> String {
    if body.len() <= MAX_PREVIEW_BODY_BYTES {
        return body.to_owned();
    }
    let center = regex
        .and_then(|regex| regex.find(body))
        .map_or(0, |found| found.start());
    let mut start = center.saturating_sub(MAX_PREVIEW_BODY_BYTES / 4);
    let mut end = start + MAX_PREVIEW_BODY_BYTES;
    if end > body.len() {
        end = body.len();
        start = end.saturating_sub(MAX_PREVIEW_BODY_BYTES);
    }
    while start > 0 && !body.is_char_boundary(start) {
        start -= 1;
    }
    while end < body.len() && !body.is_char_boundary(end) {
        end += 1;
    }
    body[start..end].to_owned()
}

/// Go `BodyExcerpt`: the first `body_lines` lines with CRLF normalized.
fn body_excerpt(body: &str, body_lines: u16) -> String {
    if body_lines == 0 {
        return String::new();
    }
    let normalized = body.replace("\r\n", "\n");
    normalized
        .split('\n')
        .take(usize::from(body_lines))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draws Go's dim `(no preview)` placeholder for an empty selection.
fn render_placeholder(area: Rect, buf: &mut Buffer, theme: &Theme) {
    let line = Line::from(Span::styled("(no preview)", style_from(&theme.dim)));
    buf.set_line(area.x, area.y, &line, area.width);
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use crate::view_model::{Field, FieldValue};
    use crate::widgets::test_support::buffer_lines;
    use mdfu_core::parse::parse_content;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn fixture(name: &str) -> String {
        let path = format!("{}/../../../testdata/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).expect("fixture reads")
    }

    fn labels(fields: &[Field]) -> Vec<&str> {
        fields.iter().map(|field| field.label.as_str()).collect()
    }

    fn render_lines(
        state: &mut PreviewState,
        doc: Option<&Document>,
        theme: &Theme,
        terms: &[String],
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        state.render(area, &mut buffer, doc, theme, terms);
        buffer_lines(&buffer)
    }

    /// A body comfortably above one megabyte with a unique late match.
    fn megabyte_body() -> String {
        let mut body = String::new();
        for index in 0..80_000 {
            let _ = writeln!(body, "filler line {index}");
        }
        body.push_str("the deepterm appears here\n");
        for index in 0..2_000 {
            let _ = writeln!(body, "trailing filler {index}");
        }
        body
    }

    #[test]
    fn preview_layout_switches_at_width_100() {
        assert_eq!(PreviewLayout::for_width(99), PreviewLayout::Stacked);
        assert_eq!(PreviewLayout::for_width(80), PreviewLayout::Stacked);
        assert!(!PreviewLayout::for_width(80).is_horizontal());

        let exact = PreviewLayout::for_width(100);
        assert_eq!(
            exact,
            PreviewLayout::Horizontal {
                list_width: 48,
                preview_width: 48,
            }
        );
        assert!(exact.is_horizontal());
        assert_eq!(exact.preview_width(100), 48);
        assert_eq!(PreviewLayout::Stacked.preview_width(80), 80);

        assert_eq!(
            PreviewLayout::for_width(101),
            PreviewLayout::Horizontal {
                list_width: 48,
                preview_width: 49,
            }
        );
        assert_eq!(
            PreviewLayout::for_width(160),
            PreviewLayout::Horizontal {
                list_width: 78,
                preview_width: 78,
            }
        );
    }

    #[test]
    fn preview_row_budgets_match_go_clamps() {
        assert_eq!(preview_rows(0), 10);
        assert_eq!(preview_rows(4), 3);
        assert_eq!(preview_rows(24), 7);
        assert_eq!(preview_rows(38), 12);
        assert_eq!(preview_rows(41), 12);

        assert_eq!(visible_rows(0, false, true, 80), 20);
        assert_eq!(visible_rows(2, false, false, 80), 3);
        assert_eq!(visible_rows(24, false, false, 80), 23);
        assert_eq!(visible_rows(24, false, true, 80), 16);
        assert_eq!(visible_rows(24, true, true, 80), 15);
        assert_eq!(visible_rows(24, false, true, 120), 23);
        assert_eq!(visible_rows(24, true, true, 120), 22);
    }

    #[test]
    fn preview_rows_follow_build_frontmatter_rows_order() {
        let portent = parse_content("testdata/portent-task.md", &fixture("portent-task.md"));
        assert_eq!(
            labels(&PreviewBlock::from(&portent).fields),
            [
                "Path",
                "Type",
                "Status",
                "Tags",
                "Belongs to",
                "Related to",
                "Created",
            ]
        );

        let okf = parse_content("testdata/okf-v02-metric.md", &fixture("okf-v02-metric.md"));
        assert_eq!(
            labels(&PreviewBlock::from(&okf).fields),
            [
                "Path",
                "Type",
                "Status",
                "Description",
                "Tags",
                "Created",
                "Updated",
                "generated",
                "sources",
                "verified",
            ]
        );
    }

    #[test]
    fn preview_renders_portent_document() {
        let doc = parse_content("testdata/portent-task.md", &fixture("portent-task.md"));
        let mut state = PreviewState::default();
        let out = render_lines(&mut state, Some(&doc), &Theme::default(), &[], 100, 16);
        let text = out.join("\n");

        assert!(text.contains("portent-task.md"), "filename header:\n{text}");
        assert!(text.contains("Ship mdfu MVP"), "title header:\n{text}");
        assert!(text.contains("Type: Task"), "type row:\n{text}");
        assert!(text.contains("Status: draft"), "status row:\n{text}");
        assert!(
            text.contains("Tags: │ tolaria │ │ launch │ │ Project Atlas │"),
            "tags pills:\n{text}"
        );
        assert!(
            text.contains("Created: 2026-01-01T00:00:00Z"),
            "created row:\n{text}"
        );
        assert!(
            text.contains("Old Working Title"),
            "markdown heading renders:\n{text}"
        );
        assert!(
            text.contains("parser track"),
            "markdown body window renders:\n{text}"
        );
    }

    #[test]
    fn preview_renders_okf_json_metadata_document() {
        let doc = parse_content(
            "testdata/okf-json-metadata.md",
            &fixture("okf-json-metadata.md"),
        );
        let mut state = PreviewState::default();
        let out = render_lines(&mut state, Some(&doc), &Theme::default(), &[], 100, 20);
        let text = out.join("\n");

        assert!(text.contains("okf-json-metadata.md"), "filename:\n{text}");
        assert!(text.contains("Type: Metric"), "type row:\n{text}");
        assert!(
            text.contains("meta: created_by: Hermes, created_at: 2026-09-01T10:00:00Z"),
            "JSON-object string flattens inline:\n{text}"
        );
        assert!(
            text.contains("sources: title: Hermes Ingest Dashboard"),
            "object-list row renders:\n{text}"
        );
        assert!(text.contains("p99 ingest latency"), "body:\n{text}");

        let wide = render_lines(&mut state, Some(&doc), &Theme::default(), &[], 160, 20);
        let wide_text = wide.join("\n");
        assert!(
            wide_text.contains(
                "sources: title: Hermes Ingest Dashboard, resource: \
                                 https://metrics.example.com/hermes/ingest, id: ingest-dashboard, \
                                 author: hermes"
            ),
            "object-list pairs join deterministically:\n{wide_text}"
        );
    }

    #[test]
    fn preview_body_window_is_bounded_for_a_megabyte_body() {
        let body = megabyte_body();
        assert!(body.len() > 1_000_000, "fixture body is large");

        let terms = ["deepterm".to_owned()];
        let window = body_window(&body, &terms, 30);
        assert!(
            window.len() <= MAX_PREVIEW_BODY_BYTES + 8,
            "window {} bytes exceeds the byte cap",
            window.len()
        );
        assert!(window.lines().count() <= 32, "window keeps the line budget");
        assert!(window.contains("deepterm"), "match stays visible");
        assert!(
            window.starts_with('…'),
            "leading lines are flagged as skipped"
        );

        let head = body_window(&body, &[], 30);
        assert!(head.len() <= MAX_PREVIEW_BODY_BYTES + 8);
        assert!(head.starts_with("filler line 0"));
        assert!(!head.contains("deepterm"));

        let blob = format!("{} deepterm", "x".repeat(2_000_000));
        let blob_window = body_window(&blob, &terms, 30);
        assert!(blob_window.len() <= MAX_PREVIEW_BODY_BYTES + 8);

        assert_eq!(body_window(&body, &[], 0), "");
        assert_eq!(body_window("a\r\nb", &[], 30), "a\nb");
    }

    #[test]
    fn preview_state_caches_body_windows_by_document_width_and_query() {
        let doc = Document {
            path: "big.md".to_owned(),
            body: megabyte_body(),
            ..Document::default()
        };
        let theme = Theme::default();
        let mut state = PreviewState::default();

        let first = state.body_window_for(&doc, 80, &[], &theme).to_owned();
        for _ in 0..25 {
            let again = state.body_window_for(&doc, 80, &[], &theme);
            assert_eq!(again, first, "cached windows stay stable");
        }
        assert_eq!(state.cache_len(), 1, "one document/width/query entry");

        let terms = ["deepterm".to_owned()];
        let with_query = state.body_window_for(&doc, 80, &terms, &theme).to_owned();
        assert!(with_query.contains("deepterm"));
        assert_eq!(state.cache_len(), 2, "a new query is a new entry");

        let _ = state.body_window_for(&doc, 100, &terms, &theme);
        assert_eq!(state.cache_len(), 3, "a new width is a new entry");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 12));
        for _ in 0..10 {
            state.render(
                Rect::new(0, 0, 80, 12),
                &mut buffer,
                Some(&doc),
                &theme,
                &[],
            );
        }
        assert_eq!(state.cache_len(), 3, "frames reuse the cached windows");

        state.invalidate("big.md");
        assert_eq!(state.cache_len(), 0, "reload drops the path's windows");
        let _ = state.body_window_for(&doc, 80, &[], &theme);
        state.clear();
        assert_eq!(state.cache_len(), 0);
    }

    #[test]
    fn preview_toggles_follow_ctrl_p_and_ctrl_f() {
        let doc = parse_content("testdata/portent-task.md", &fixture("portent-task.md"));
        let theme = Theme::default();
        let mut state = PreviewState::new(true, true);
        assert!(state.show_preview());
        assert!(state.show_frontmatter());

        assert!(!state.toggle_preview(), "ctrl+p hides the pane");
        assert!(!state.show_preview());
        let hidden = render_lines(&mut state, Some(&doc), &theme, &[], 80, 10);
        assert!(
            hidden.iter().all(String::is_empty),
            "hidden pane draws nothing: {hidden:?}"
        );

        assert!(state.toggle_preview(), "ctrl+p shows the pane again");
        assert!(!state.toggle_frontmatter(), "ctrl+f hides frontmatter");
        assert!(!state.show_frontmatter());
        let without_fm = render_lines(&mut state, Some(&doc), &theme, &[], 80, 10);
        let text = without_fm.join("\n");
        assert!(!text.contains("Type: Task"), "frontmatter hidden:\n{text}");
        assert!(text.contains("portent-task.md"), "filename stays:\n{text}");
        assert!(text.contains("Ship mdfu MVP"), "title stays:\n{text}");

        assert!(state.toggle_frontmatter(), "ctrl+f restores frontmatter");
        state.set_show_preview(false);
        state.set_show_frontmatter(false);
        assert!(!state.show_preview());
        assert!(!state.show_frontmatter());
        state.set_show_preview(true);
        state.set_show_frontmatter(true);
        assert!(state.show_preview());
        assert!(state.show_frontmatter());
    }

    #[test]
    fn preview_nested_map_rows_are_deterministic() {
        let doc = parse_content(
            "nested.md",
            concat!(
                "---\n",
                "nested:\n  a:\n    b: 1\n  z: 2\n",
                "deep:\n  a:\n    b:\n      c:\n        d: 1\n",
                "---\n",
            ),
        );
        let first = PreviewBlock::from(&doc);
        for _ in 0..10 {
            assert_eq!(PreviewBlock::from(&doc), first, "stable row conversion");
        }
        assert_eq!(labels(&first.fields), ["Path", "deep", "nested"]);

        let FieldValue::Pairs(deep_pairs) = &first.fields[1].value else {
            panic!("deep row must flatten to pairs: {:?}", first.fields[1]);
        };
        assert_eq!(deep_pairs.len(), 1);
        assert_eq!(deep_pairs[0].key, "a.b.c");

        let FieldValue::Pairs(nested_pairs) = &first.fields[2].value else {
            panic!("nested row must flatten to pairs: {:?}", first.fields[2]);
        };
        assert_eq!(nested_pairs.len(), 2);
        assert_eq!(nested_pairs[0].key, "z");
        assert_eq!(nested_pairs[1].key, "a.b");
        assert_eq!(nested_pairs[1].value, FieldValue::Scalar("1".to_owned()));

        let mut state = PreviewState::default();
        let out = render_lines(&mut state, Some(&doc), &Theme::default(), &[], 60, 8);
        let text = out.join("\n");
        assert!(text.contains("a.b: 1"), "dotted key renders:\n{text}");
        assert!(text.contains("z: 2"), "descending pair order:\n{text}");
    }

    #[test]
    fn preview_placeholder_without_a_document() {
        let mut state = PreviewState::default();
        let out = render_lines(&mut state, None, &Theme::default(), &[], 40, 3);
        assert_eq!(out[0], "(no preview)");
        assert!(out[1..].iter().all(String::is_empty));

        state.set_show_preview(false);
        let out = render_lines(&mut state, None, &Theme::default(), &[], 40, 3);
        assert!(out.iter().all(String::is_empty));
    }

    #[test]
    fn render_preview_free_function_matches_state_render() {
        let doc = parse_content("testdata/portent-task.md", &fixture("portent-task.md"));
        let theme = Theme::default();
        let area = Rect::new(0, 0, 80, 12);
        let mut direct = Buffer::empty(area);
        render_preview(area, &mut direct, Some(&doc), &theme, &[], true);

        let mut state = PreviewState::new(true, true);
        let mut via_state = Buffer::empty(area);
        state.render(area, &mut via_state, Some(&doc), &theme, &[]);

        assert_eq!(buffer_lines(&direct), buffer_lines(&via_state));
    }
}
