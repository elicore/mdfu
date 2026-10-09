//! Document preview components: filename bar, title block, frontmatter panel,
//! and markdown body, all painting from [`PreviewBlock`] and theme styles.
//!
//! The body is rendered with `tui-markdown` through [`NoColorStyleSheet`]:
//! headings, code, links, quotes, tables, and images keep their structural
//! modifiers, but every color stays at the terminal default because the theme
//! carries no markdown palette yet. Hyperlink emission (OSC 8) is the link
//! module's job and is not wired in here.

use mdfu_core::theme::{PillShape, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use tui_markdown::{from_str_with_options, AlertKind, Options, StyleSheet};

use super::{
    color_from, rows, style_from, Component, COMPONENT_BODY, COMPONENT_FILENAME,
    COMPONENT_FRONTMATTER, COMPONENT_TITLE,
};
use crate::view_model::{Field, FieldPair, FieldValue, PreviewBlock};

/// The document's basename, styled with the theme's filename style; renders
/// nothing for an empty path (Go `filenameBar`).
pub struct FilenameBar {
    filename: String,
    style: Style,
    visible: bool,
}

impl FilenameBar {
    /// Snapshots the filename and filename style from `block` and `theme`.
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            filename: block.filename.clone(),
            style: style_from(&theme.filename),
            visible: true,
        }
    }
}

impl Component for FilenameBar {
    fn id(&self) -> &'static str {
        COMPONENT_FILENAME
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn height(&self, _width: u16) -> u16 {
        u16::from(!self.filename.is_empty())
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || self.filename.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        let line = Line::from(Span::styled(self.filename.clone(), self.style));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

/// The document title (or `"(untitled)"`), styled with the theme's title style
/// (Go `titleBlock`).
pub struct TitleBlock {
    title: String,
    style: Style,
    visible: bool,
}

impl TitleBlock {
    /// Snapshots the display title and title style from `block` and `theme`.
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            title: block.display_title().to_owned(),
            style: style_from(&theme.title),
            visible: true,
        }
    }
}

impl Component for TitleBlock {
    fn id(&self) -> &'static str {
        COMPONENT_TITLE
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn height(&self, _width: u16) -> u16 {
        1
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || area.width == 0 || area.height == 0 {
            return;
        }
        let line = Line::from(Span::styled(self.title.clone(), self.style));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

/// The normalized frontmatter rows, each `Label: value` with the label in the
/// theme's frontmatter-key style and pills in the theme's pill style
/// (Go `frontmatterPanel`).
pub struct FrontmatterPanel {
    fields: Vec<Field>,
    label_style: Style,
    pill: PillPaint,
    visible: bool,
}

impl FrontmatterPanel {
    /// Snapshots the frontmatter rows and their styles from `block` and
    /// `theme`.
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            fields: block.fields.clone(),
            label_style: style_from(&theme.frontmatter_key),
            pill: PillPaint::new(theme),
            visible: true,
        }
    }

    fn field_line(&self, field: &Field) -> Line<'static> {
        let mut spans: Vec<Span<'static>> =
            vec![Span::styled(format!("{}: ", field.label), self.label_style)];
        push_value(&mut spans, &field.value, &self.pill);
        Line::from(spans)
    }
}

impl Component for FrontmatterPanel {
    fn id(&self) -> &'static str {
        COMPONENT_FRONTMATTER
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn height(&self, _width: u16) -> u16 {
        rows(self.fields.len())
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || area.width == 0 || area.height == 0 {
            return;
        }
        for (index, field) in self.fields.iter().enumerate() {
            let offset = match u16::try_from(index) {
                Ok(offset) if offset < area.height => offset,
                _ => break,
            };
            let line = self.field_line(field);
            buf.set_line(area.x, area.y + offset, &line, area.width);
        }
    }
}

/// The resolved pill painter: foreground/background from the theme's pill
/// style, `horizontal_padding` spaces on each side, and rounded `│` side bars
/// in the pill background when the theme selects the round shape
/// (Go `renderPill`).
struct PillPaint {
    style: Style,
    bar: Style,
    padding: u16,
    round: bool,
}

impl PillPaint {
    fn new(theme: &Theme) -> Self {
        let bar = theme
            .pill
            .bg
            .as_deref()
            .and_then(color_from)
            .map_or_else(Style::new, |color| Style::new().fg(color));
        Self {
            style: style_from(&theme.pill),
            bar,
            padding: theme.pill.horizontal_padding,
            round: theme.pill_shape == PillShape::Round,
        }
    }

    fn spans(&self, text: &str) -> Vec<Span<'static>> {
        let padding = " ".repeat(usize::from(self.padding));
        let body = Span::styled(format!("{padding}{text}{padding}"), self.style);
        if self.round {
            vec![
                Span::styled("│", self.bar),
                body,
                Span::styled("│", self.bar),
            ]
        } else {
            vec![body]
        }
    }
}

/// Appends one frontmatter value's spans: scalars verbatim, pills as badges,
/// pairs joined with `", "`, and object-list groups with `"; "`
/// (Go `renderField` value branch).
fn push_value(spans: &mut Vec<Span<'static>>, value: &FieldValue, pill: &PillPaint) {
    match value {
        FieldValue::Scalar(text) => spans.push(Span::raw(text.clone())),
        FieldValue::Pills(pills) => {
            for (index, text) in pills.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.extend(pill.spans(text));
            }
        }
        FieldValue::Pairs(pairs) => push_pairs(spans, pairs, pill),
        FieldValue::PairGroups(groups) => {
            for (index, group) in groups.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("; "));
                }
                push_pairs(spans, group, pill);
            }
        }
    }
}

/// Appends flattened object pairs joined with `", "` (Go `renderPairs`).
fn push_pairs(spans: &mut Vec<Span<'static>>, pairs: &[FieldPair], pill: &PillPaint) {
    for (index, pair) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(", "));
        }
        spans.push(Span::raw(format!("{}: ", pair.key)));
        push_value(spans, &pair.value, pill);
    }
}

/// The markdown body excerpt, windowed to the theme's `body_lines` budget and
/// wrapped by the widget to the pane width (Go `bodyPanel`).
pub struct BodyPanel {
    body: String,
    lines: u16,
    visible: bool,
}

impl BodyPanel {
    /// Windows `block.body_markdown` to the theme's `body_lines` and captures
    /// the budget for height measurement.
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            body: first_lines(&block.body_markdown, theme.body_lines),
            lines: theme.body_lines,
            visible: true,
        }
    }

    /// The windowed markdown source this panel renders.
    #[must_use]
    pub fn body_window(&self) -> &str {
        &self.body
    }
}

impl Component for BodyPanel {
    fn id(&self) -> &'static str {
        COMPONENT_BODY
    }

    fn visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn height(&self, _width: u16) -> u16 {
        rows(self.body.lines().count()).min(self.lines)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || self.body.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        let text = markdown_text(&self.body, area.width);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

/// Converts markdown to a color-free ratatui [`Text`] wrapped for `width`.
///
/// Shared by the body panel and the task detail pane so every markdown surface
/// uses the same no-color, modifier-only styling.
pub(crate) fn markdown_text(source: &str, width: u16) -> Text<'_> {
    let options = Options::new(NoColorStyleSheet).table_width(width);
    from_str_with_options(source, &options)
}

/// Keeps the first `limit` lines of `text` (Go `matchWindow` without a match).
fn first_lines(text: &str, limit: u16) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.lines()
        .take(usize::from(limit.max(1)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A `tui-markdown` style sheet that sets modifiers but no colors.
///
/// `tui-markdown`'s default sheet hardcodes cyan/blue/green/... colors; the
/// widget layer instead keeps every markdown style color-free, so the body
/// inherits the terminal's default foreground and background. The separate
/// link/highlight module can layer theme colors on top later.
#[derive(Debug, Clone, Copy, Default)]
struct NoColorStyleSheet;

impl StyleSheet for NoColorStyleSheet {
    fn heading(&self, level: u8) -> Style {
        let style = Style::new().add_modifier(Modifier::BOLD);
        if level == 1 {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style
        }
    }

    fn code(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn link(&self) -> Style {
        Style::new().add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn heading_meta(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn metadata_block(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn html(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn math_inline(&self) -> Style {
        Style::new()
    }

    fn math_display(&self) -> Style {
        Style::new()
    }

    fn footnote_ref(&self) -> Style {
        Style::new()
    }

    fn footnote_def(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    fn definition_term(&self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    fn definition_description(&self) -> Style {
        Style::new()
    }

    fn alert(&self, _kind: AlertKind) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    fn table_header(&self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    fn table_cell(&self) -> Style {
        Style::new()
    }

    fn table_border(&self) -> Style {
        Style::new()
    }

    fn list_marker(&self) -> Style {
        Style::new()
    }

    fn image_alt(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::{buffer_lines, component_buffer, render, render_buffer};
    use mdfu_core::model::Document;
    use mdfu_core::theme::Theme;
    use ratatui::style::Color;

    fn empty_block() -> PreviewBlock {
        PreviewBlock::from(&Document::default())
    }

    fn frontmatter_only_block() -> PreviewBlock {
        let doc = mdfu_core::parse::parse_content(
            "notes/fm.md",
            "---\ntype: Metric\ntags: [growth, kpi]\n---\n",
        );
        PreviewBlock::from(&doc)
    }

    fn body_block() -> PreviewBlock {
        let doc = mdfu_core::parse::parse_content(
            "vault/body.md",
            "---\ntitle: Body Doc\n---\n# Heading\n\nA paragraph with **bold** text.\n",
        );
        PreviewBlock::from(&doc)
    }

    #[test]
    fn widgets_preview_empty_document_renders_only_untitled() {
        let block = empty_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        let lines = render(&preview, 40, 8);
        assert_eq!(lines[0], "(untitled)");
        assert!(lines[1..].iter().all(String::is_empty));
        assert!(!lines.iter().any(|line| line == "---"));
        assert!(!lines.iter().any(|line| line == "Preview"));

        let wide = render(&preview, 100, 10);
        assert_eq!(wide[0], "(untitled)");
        assert!(wide[1..].iter().all(String::is_empty));
    }

    #[test]
    fn widgets_preview_frontmatter_only_document_snapshot_40_cols() {
        let block = frontmatter_only_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        assert_eq!(
            render(&preview, 40, 10),
            [
                "fm.md",
                "fm",
                "Path: notes/fm.md",
                "Type: Metric",
                "Tags: │ growth │ │ kpi │",
                "",
                "",
                "",
                "",
                "",
            ]
        );
    }

    #[test]
    fn widgets_preview_frontmatter_only_document_snapshot_100_cols() {
        let block = frontmatter_only_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        assert_eq!(
            render(&preview, 100, 10),
            [
                "fm.md",
                "fm",
                "Path: notes/fm.md",
                "Type: Metric",
                "Tags: │ growth │ │ kpi │",
                "",
                "",
                "",
                "",
                "",
            ]
        );
    }

    #[test]
    fn widgets_preview_body_document_snapshot_40_cols() {
        let block = body_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        assert_eq!(
            render(&preview, 40, 12),
            [
                "body.md",
                "Body Doc",
                "Path: vault/body.md",
                "# Heading",
                "",
                "A paragraph with bold text.",
                "",
                "",
                "",
                "",
                "",
                "",
            ]
        );
    }

    #[test]
    fn widgets_preview_body_document_snapshot_100_cols() {
        let block = body_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        assert_eq!(
            render(&preview, 100, 12),
            [
                "body.md",
                "Body Doc",
                "Path: vault/body.md",
                "# Heading",
                "",
                "A paragraph with bold text.",
                "",
                "",
                "",
                "",
                "",
                "",
            ]
        );
    }

    #[test]
    fn widgets_preview_hidden_component_produces_no_output() {
        let block = frontmatter_only_block();
        let mut preview = super::super::Preview::new(&block, &Theme::default());
        let full = render(&preview, 40, 10);
        assert!(full.iter().any(|line| line.starts_with("Path:")));

        preview.set_visible(super::super::COMPONENT_FRONTMATTER, false);
        let without = render(&preview, 40, 10);
        assert!(!without.iter().any(|line| line.starts_with("Path:")));
        assert!(!without.iter().any(|line| line.starts_with("Type:")));
        assert!(!without.iter().any(|line| line.starts_with("Tags: ")));
        assert!(without.iter().any(|line| line == "fm.md"));
        assert!(without.iter().any(|line| line == "fm"));

        preview.set_visible(super::super::COMPONENT_BODY, false);
        preview.set_visible(super::super::COMPONENT_BODY, true);

        let mut panel = FrontmatterPanel::new(&block, &Theme::default());
        panel.set_visible(false);
        let buffer = component_buffer(&panel, 40, 4);
        assert!(buffer_lines(&buffer).iter().all(String::is_empty));

        let mut title = TitleBlock::new(&block, &Theme::default());
        title.set_visible(false);
        let buffer = component_buffer(&title, 40, 1);
        assert!(buffer_lines(&buffer).iter().all(String::is_empty));
    }

    #[test]
    fn widgets_body_panel_has_a_bounded_window() {
        let body = (1..=50)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let block = PreviewBlock {
            body_markdown: body,
            ..PreviewBlock::from(&Document::default())
        };
        let theme = Theme::default();
        let panel = BodyPanel::new(&block, &theme);
        assert_eq!(panel.body_window().lines().count(), 30);
        assert!(panel.body_window().starts_with("line 1"));
        assert!(panel.body_window().ends_with("line 30"));
    }

    #[test]
    fn widgets_body_panel_uses_no_hardcoded_colors() {
        let doc = mdfu_core::parse::parse_content(
            "colors.md",
            concat!(
                "# Heading\n\n",
                "A `code` span with a [link](https://example.com) and an ![image](a.png).\n\n",
                "> quoted\n\n",
                "| a | b |\n| - | - |\n| 1 | 2 |\n\n",
                "```\ncode block\n```\n"
            ),
        );
        let block = PreviewBlock::from(&doc);
        let panel = BodyPanel::new(&block, &Theme::default());
        let buffer = component_buffer(&panel, 60, 20);

        let mut saw_bold = false;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = buffer.cell((x, y)).expect("cell in bounds");
                assert_eq!(cell.fg, Color::Reset, "body must not set fg at {x},{y}");
                assert_eq!(cell.bg, Color::Reset, "body must not set bg at {x},{y}");
                saw_bold |= cell.modifier.contains(Modifier::BOLD);
            }
        }
        assert!(saw_bold, "heading modifiers must survive color stripping");

        let text = buffer_lines(&buffer).join("\n");
        assert!(text.contains("Heading"), "heading text renders:\n{text}");
        assert!(text.contains("link"), "link text renders:\n{text}");
        assert!(text.contains("[img]"), "image fallback renders:\n{text}");
    }

    #[test]
    fn widgets_components_report_expected_ids_and_heights() {
        let block = frontmatter_only_block();
        let theme = Theme::default();
        assert_eq!(FilenameBar::new(&block, &theme).height(40), 1);
        assert_eq!(TitleBlock::new(&block, &theme).height(40), 1);
        assert_eq!(FrontmatterPanel::new(&block, &theme).height(40), 3);
        assert_eq!(BodyPanel::new(&block, &theme).height(40), 0);
        assert_eq!(FilenameBar::new(&empty_block(), &theme).height(40), 0);
        assert_eq!(BodyPanel::new(&body_block(), &theme).height(40), 3);

        let mut bar = FilenameBar::new(&block, &theme);
        assert_eq!(bar.id(), crate::widgets::COMPONENT_FILENAME);
        assert!(bar.visible());
        bar.set_visible(false);
        assert!(!bar.visible());
        assert_eq!(bar.height(40), 1, "height is independent of visibility");
    }

    #[test]
    fn widgets_preview_inside_a_mismatched_buffer_is_clipped() {
        let block = body_block();
        let preview = super::super::Preview::new(&block, &Theme::default());
        let buffer = render_buffer(&preview, 40, 2);
        let lines = buffer_lines(&buffer);
        assert_eq!(lines[0], "body.md");
        assert_eq!(lines[1], "Body Doc");
    }
}
