//! Frame rendering for the picker: search bar, result list, preview pane, and
//! status bar.
//!
//! The layout mirrors Go `Model.View`: the search bar occupies the first row
//! only in search mode, the status bar the last row, and the result list the
//! rows in between. With the preview enabled, the body splits side-by-side at
//! width >= 100 and stacks the preview below the list otherwise, using the
//! shared row budgets from [`crate::preview`] and the [`PreviewState`] cache
//! so a frame never rescans a document body.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::Frame;

use mdfu_core::output;

use super::App;
use crate::preview::{self, PreviewLayout};
use crate::view_model::{NoteRow, StatusBar};
use crate::widgets::{style_from, NoteList, StatusBarWidget};

/// The picker's search prompt, matching the Go textinput prompt.
pub const SEARCH_PROMPT: &str = "> ";
/// The picker's search placeholder, matching the Go textinput placeholder.
pub const SEARCH_PLACEHOLDER: &str = "Search... (bare words fuzzy, key:value hard-filter)";

impl App {
    /// Paints the picker into `frame` (Go `Model.View`).
    pub fn view(&mut self, frame: &mut Frame<'_>) {
        frame.render_widget(self, frame.area());
    }

    /// Paints the picker into `area`/`buf`; the widget impl forwards here.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.searching {
            let chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);
            if let [search, body, status] = chunks.as_ref() {
                render_search_bar(self, *search, buf);
                self.render_body(*body, buf);
                render_status(self, *status, buf);
            }
        } else {
            let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
            if let [body, status] = chunks.as_ref() {
                self.render_body(*body, buf);
                render_status(self, *status, buf);
            }
        }
    }

    fn render_body(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if !self.show_preview {
            render_list(self, area, buf);
            return;
        }
        match PreviewLayout::for_width(area.width) {
            PreviewLayout::Horizontal {
                list_width,
                preview_width,
            } => {
                let list_area = Rect::new(area.x, area.y, list_width, area.height);
                let preview_area = Rect::new(
                    area.x + area.width - preview_width,
                    area.y,
                    preview_width,
                    area.height,
                );
                render_list(self, list_area, buf);
                self.render_preview(preview_area, buf);
            }
            PreviewLayout::Stacked => {
                let rows = self.stacked_preview_rows(area.height).min(area.height);
                let list_height = area.height - rows;
                let list_area = Rect::new(area.x, area.y, area.width, list_height);
                let preview_area = Rect::new(area.x, area.y + list_height, area.width, rows);
                render_list(self, list_area, buf);
                self.render_preview(preview_area, buf);
            }
        }
    }

    fn render_preview(&mut self, area: Rect, buf: &mut Buffer) {
        let doc = self
            .filtered
            .get(self.cursor)
            .and_then(|index| self.items.get(*index));
        self.preview
            .render(area, buf, doc, &self.theme, &self.terms, self.hyperlinks);
    }

    /// Go `previewRows` uses the full terminal height; a headless render (no
    /// `Resize` message yet) falls back to the body area it was given.
    fn stacked_preview_rows(&self, body_height: u16) -> u16 {
        if self.height == 0 {
            preview::preview_rows(body_height)
        } else {
            preview::preview_rows(self.height)
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        App::render(self, area, buf);
    }
}

fn render_search_bar(app: &App, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let prompt = Line::from(Span::raw(SEARCH_PROMPT));
    buf.set_line(area.x, area.y, &prompt, area.width);
    if area.width <= 2 {
        return;
    }
    let input_area = Rect::new(area.x + 2, area.y, area.width - 2, 1);
    if app.input.value().is_empty() {
        let placeholder = Line::from(Span::styled(SEARCH_PLACEHOLDER, style_from(&app.theme.dim)));
        buf.set_line(input_area.x, input_area.y, &placeholder, input_area.width);
    }
    (&app.input).render(input_area, buf);
}

fn render_list(app: &App, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let has_terms = app.terms.iter().any(|term| !term.trim().is_empty());
    let rows: Vec<NoteRow> = app
        .filtered
        .iter()
        .filter_map(|index| {
            let doc = app.items.get(*index)?;
            let positions: Vec<usize> = app.matched.get(index).cloned().unwrap_or_default();
            let mut row = NoteRow::from(doc).with_positions(positions.clone());
            // A title hit keeps the path; otherwise the active query earns a
            // body excerpt. The literal-term check covers filters that match
            // without positions (e.g. the substring fallback), so they keep
            // showing the path for title matches exactly like before.
            let title_hit = !positions.is_empty()
                || (has_terms
                    && app.terms.iter().any(|term| {
                        let term = term.trim().to_lowercase();
                        !term.is_empty() && row.title.to_lowercase().contains(term.as_str())
                    }));
            if !title_hit && has_terms {
                let excerpt = output::snippet(&doc.body, &app.terms, i32::from(area.width));
                if !excerpt.is_empty() {
                    row = row.with_snippet(excerpt);
                }
            }
            Some(row)
        })
        .collect();
    let selected: Vec<usize> = app
        .filtered
        .iter()
        .enumerate()
        .filter(|(_, index)| app.selected.contains(index))
        .map(|(position, _)| position)
        .collect();
    let list = NoteList::new(&rows, &app.theme)
        .cursor(app.cursor)
        .offset(app.offset)
        .selected(&selected);
    (&list).render(area, buf);
}

fn render_status(app: &App, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bar = StatusBar::notes(
        app.filtered.len(),
        app.items.len(),
        app.show_archived,
        app.show_frontmatter,
        app.searching,
        &app.status,
    );
    let widget = StatusBarWidget::new(&bar, &app.theme);
    (&widget).render(area, buf);
}

#[cfg(test)]
mod tests {
    use mdfu_core::model::Document;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;

    use super::*;
    use crate::notes::{Config, Msg};

    fn doc(path: &str, title: &str) -> Document {
        Document {
            path: path.to_owned(),
            title: title.to_owned(),
            body: format!("body of {title}"),
            ..Document::default()
        }
    }

    fn substring_filter(query: &str, items: &[Document]) -> Vec<crate::notes::FilterHit> {
        let hits = |index: usize| crate::notes::FilterHit {
            index,
            matched_indexes: Vec::new(),
        };
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return (0..items.len()).map(hits).collect();
        }
        items
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.title.to_lowercase().contains(&needle))
            .map(|(index, _)| hits(index))
            .collect()
    }

    fn picker(items: Vec<Document>) -> App {
        App::new(items, Config::default(), Box::new(substring_filter))
    }

    fn rune_key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn tab_key() -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
    }

    fn down_key() -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
    }

    fn apply(app: &mut App, msg: Msg) {
        let _ = app.update(msg);
    }

    fn render_lines(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
        terminal
            .draw(|frame| app.view(frame))
            .expect("TestBackend draw");
        crate::widgets::test_support::buffer_lines(terminal.backend().buffer())
    }

    fn first_non_empty(lines: &[String]) -> String {
        lines
            .iter()
            .find(|line| !line.trim().is_empty())
            .cloned()
            .unwrap_or_default()
    }

    fn note_view_docs() -> Vec<Document> {
        vec![doc("a.md", "alpha"), doc("b.md", "beta")]
    }

    #[test]
    fn note_view_list_mode_hides_search_bar() {
        let mut app = picker(note_view_docs());
        let lines = render_lines(&mut app, 40, 8);
        let text = lines.join("\n");
        assert!(
            !text.contains("Search..."),
            "list mode must not render the search bar:\n{text}"
        );
        let first = first_non_empty(&lines);
        assert!(
            first.contains("alpha"),
            "first row must be the first result, got {first:?}"
        );
        assert!(first.starts_with("> "), "focused row carries the marker");
    }

    #[test]
    fn note_view_search_mode_shows_prompt_and_placeholder() {
        let mut app = picker(note_view_docs());
        apply(&mut app, rune_key('/'));
        let lines = render_lines(&mut app, 40, 8);
        assert!(
            lines[0].starts_with("> "),
            "search input must be the first line, got {:?}",
            lines[0]
        );
        assert!(
            lines[0].contains("Search..."),
            "placeholder expected, got {:?}",
            lines[0]
        );
    }

    #[test]
    fn note_view_status_bar_and_hints() {
        let mut app = picker(note_view_docs());
        let text = render_lines(&mut app, 80, 8).join("\n");
        for want in [
            "2/2",
            "archived:hidden",
            "fm:shown",
            "/:search",
            "e:edit",
            "tab:multi",
            "enter:select",
        ] {
            assert!(text.contains(want), "expected {want:?} in status:\n{text}");
        }
    }

    #[test]
    fn note_view_search_mode_status_hints() {
        let mut app = picker(note_view_docs());
        apply(&mut app, rune_key('/'));
        let text = render_lines(&mut app, 80, 8).join("\n");
        assert!(text.contains("esc:back"), "expected esc:back:\n{text}");
        assert!(!text.contains("/:search"), "no list-mode hint in search");
        assert!(!text.contains("e:edit"), "no editor hint in search");
        for want in ["tab:multi", "enter:select", "2/2"] {
            assert!(text.contains(want), "expected {want:?}:\n{text}");
        }
    }

    #[test]
    fn note_view_checkboxes_only_when_multi_selecting() {
        let mut app = picker(note_view_docs());
        let before = render_lines(&mut app, 40, 8).join("\n");
        assert!(!before.contains("[ ]") && !before.contains("[x]"));

        apply(&mut app, tab_key());
        let during = render_lines(&mut app, 40, 8).join("\n");
        assert!(during.contains("[x]") && during.contains("[ ]"));

        apply(&mut app, tab_key());
        let after = render_lines(&mut app, 40, 8).join("\n");
        assert!(!after.contains("[ ]") && !after.contains("[x]"));
    }

    #[test]
    fn note_view_cursor_moves_with_navigation() {
        let mut app = picker(note_view_docs());
        apply(&mut app, down_key());
        let lines = render_lines(&mut app, 40, 8);
        assert!(lines[0].starts_with("  "), "first row loses the marker");
        assert!(lines[1].starts_with("> "), "second row gains the marker");
        assert!(lines[1].contains("beta"));
    }

    #[test]
    fn note_view_preview_renders_selected_document_and_toggles() {
        let mut first = doc("a.md", "alpha");
        first.doc_type = "Metric".to_owned();
        first.body = "# Heading\n\nthe deepterm appears here\n".to_owned();
        let mut app = picker(vec![first, doc("b.md", "beta")]);

        let without = render_lines(&mut app, 120, 12).join("\n");
        assert!(
            !without.contains("Heading") && !without.contains("Path:"),
            "preview off draws no preview content:\n{without}"
        );

        apply(&mut app, ctrl('p'));
        let lines = render_lines(&mut app, 120, 12);
        let text = lines.join("\n");
        assert!(text.contains("alpha"), "preview title:\n{text}");
        assert!(text.contains("Type: Metric"), "frontmatter row:\n{text}");
        assert!(text.contains("Heading"), "markdown body renders:\n{text}");
        assert!(
            lines.iter().any(|line| line.contains("deepterm")),
            "body window renders:\n{text}"
        );

        apply(&mut app, ctrl('f'));
        let without_fm = render_lines(&mut app, 120, 12).join("\n");
        assert!(
            !without_fm.contains("Type: Metric"),
            "frontmatter hidden after ctrl+f:\n{without_fm}"
        );
    }

    #[test]
    fn note_view_stacked_preview_splits_the_body() {
        let mut first = doc("a.md", "alpha");
        first.body = "stacked body line\n".to_owned();
        let mut app = picker(vec![first]);
        apply(&mut app, ctrl('p'));
        let lines = render_lines(&mut app, 60, 20);
        let text = lines.join("\n");
        assert!(text.contains("alpha"), "list stays visible:\n{text}");
        assert!(
            text.contains("stacked body line"),
            "preview stacks:\n{text}"
        );
        assert_eq!(lines.len(), 20);
    }

    #[test]
    fn note_view_zero_area_is_safe() {
        let mut app = picker(note_view_docs());
        assert!(render_lines(&mut app, 0, 0).is_empty());
    }

    #[test]
    fn note_view_list_shows_snippet_for_body_only_match() {
        let mut body_only = doc("a.md", "unrelated title");
        body_only.body = format!("{}kumquat {}", "filler ".repeat(20), "tail ".repeat(20));
        body_only.search_blob.clone_from(&body_only.body);
        let items = vec![body_only, doc("b.md", "kumquat title")];
        let mut app = App::with_default_filter(items, Config::default());
        app.set_query("kumquat");
        let text = render_lines(&mut app, 60, 8).join("\n");
        assert!(
            text.contains("kumquat") && !text.contains("a.md"),
            "body-only match shows the excerpt, not the path:\n{text}"
        );
        assert!(
            text.contains("..."),
            "long body is windowed with an ellipsis:\n{text}"
        );
        assert!(text.contains("b.md"), "title match keeps the path:\n{text}");
    }

    #[test]
    fn note_view_preview_wires_osc8_and_no_hyperlinks() {
        let body = "See [Charm](https://charm.sh) now.\n";
        let mut first = doc("a.md", "alpha");
        first.body = body.to_owned();

        let mut app = picker(vec![first.clone()]);
        apply(&mut app, ctrl('p'));
        let text = render_lines(&mut app, 120, 12).join("\n");
        assert!(
            text.contains("\x1b]8;;https://charm.sh\x07"),
            "preview emits the OSC 8 opener:\n{text:?}"
        );
        assert!(
            !text.contains("(https://charm.sh)"),
            "URL hidden behind the hyperlink:\n{text:?}"
        );

        let mut off = App::new(
            vec![first],
            Config {
                no_hyperlinks: true,
                ..Config::default()
            },
            Box::new(substring_filter),
        );
        apply(&mut off, ctrl('p'));
        let text = render_lines(&mut off, 120, 12).join("\n");
        assert!(
            text.contains("Charm (https://charm.sh)"),
            "--no-hyperlinks renders the label url fallback:\n{text:?}"
        );
        assert!(
            !text.contains("\x1b]8;"),
            "--no-hyperlinks emits no OSC 8:\n{text:?}"
        );
    }

    #[test]
    fn note_view_preview_hyperlink_survives_a_repaint() {
        // Regression: the diff measures a cell's symbol width, so an OSC 8
        // smuggled cell used to swallow the cells after it on the next frame.
        let link_doc = mdfu_core::parse::parse_content(
            "smoke.md",
            "---\ntitle: Smoke Note\ntype: Metric\n---\n# Smoke\n\n\
             See [Charm](https://charm.sh) for the parser track and more text to read.\n",
        );
        let mut app = picker(vec![doc("other.md", "other"), link_doc]);
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
        apply(&mut app, ctrl('p'));
        terminal.draw(|frame| app.view(frame)).expect("first draw");
        apply(&mut app, down_key());
        terminal.draw(|frame| app.view(frame)).expect("second draw");
        let text =
            crate::widgets::test_support::buffer_lines(terminal.backend().buffer()).join("\n");
        assert!(
            text.contains("\x1b]8;;https://charm.sh\x07"),
            "hyperlink opens before the label:\n{text:?}"
        );
        assert!(
            text.contains("Charm"),
            "the whole label repaints:\n{text:?}"
        );
        assert!(
            text.contains("for the parser track and more text to read."),
            "cells after the link are not swallowed:\n{text:?}"
        );
        assert!(
            !text.contains("https://charm.sh)"),
            "no fallback while hyperlinks are on:\n{text:?}"
        );
    }
}
