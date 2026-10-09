//! Note-picker result list over [`NoteRow`] view-models.

use mdfu_core::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::style_from;
use crate::view_model::NoteRow;

/// The note picker's result list: one row per [`NoteRow`] with the cursor
/// marker, optional multi-select checkbox, title, type/archived suffix, query
/// snippet, and dim path (Go `Model.renderList`).
pub struct NoteList<'a> {
    rows: &'a [NoteRow],
    cursor: usize,
    offset: usize,
    selected: &'a [usize],
    theme: Theme,
}

impl<'a> NoteList<'a> {
    /// Builds the list over `rows` with `theme` styles; no row is selected and
    /// the cursor and offset start at zero.
    #[must_use]
    pub fn new(rows: &'a [NoteRow], theme: &Theme) -> Self {
        Self {
            rows,
            cursor: 0,
            offset: 0,
            selected: &[],
            theme: theme.clone(),
        }
    }

    /// Sets the focused row index.
    #[must_use]
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    /// Sets the first visible row index.
    #[must_use]
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the multi-selected row indices; a non-empty selection turns the
    /// checkbox column on, exactly like Go's `len(m.selected) > 0`.
    #[must_use]
    pub fn selected(mut self, selected: &'a [usize]) -> Self {
        self.selected = selected;
        self
    }

    fn row_line(&self, index: usize) -> Line<'static> {
        let row = &self.rows[index];
        let cursor = index == self.cursor;
        let cursor_style = style_from(&self.theme.cursor);
        let selected_style = style_from(&self.theme.selected);
        let dim_style = style_from(&self.theme.dim);

        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(if cursor {
            Span::styled("> ", cursor_style)
        } else {
            Span::raw("  ")
        });
        if !self.selected.is_empty() {
            spans.push(if self.selected.contains(&index) {
                Span::styled("[x]", selected_style)
            } else {
                Span::raw("[ ]")
            });
            spans.push(Span::raw(" "));
        }
        let title = row.display_title();
        spans.push(Span::raw(title.to_owned()));
        let mut extra = String::new();
        if !row.doc_type.is_empty() {
            extra.push_str(" [");
            extra.push_str(&row.doc_type);
            extra.push(']');
        }
        if row.archived {
            extra.push_str(" (archived)");
        }
        if !extra.is_empty() {
            spans.push(Span::raw(extra));
        }
        let mut show_path = !row.path.is_empty() && row.path != title;
        if !row.snippet.is_empty() {
            spans.push(Span::raw(format!("  {}", row.snippet)));
            show_path = false;
        }
        if cursor {
            for span in &mut spans {
                span.style = cursor_style.patch(span.style);
            }
        }
        let mut line = Line::from(spans);
        if show_path {
            line.push_span(Span::styled(format!("  {}", row.path), dim_style));
        }
        line
    }
}

impl Widget for &NoteList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let dim_style = style_from(&self.theme.dim);
        if self.rows.is_empty() {
            let line = Line::from(Span::styled("(no matches)", dim_style));
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        }
        let max_rows = usize::from(area.height).max(1);
        let start = if self.offset < self.rows.len() {
            self.offset
        } else {
            0
        };
        let mut end = (start + max_rows).min(self.rows.len());
        if end < self.rows.len() && max_rows > 1 {
            end -= 1;
        }
        let mut y = area.y;
        for index in start..end {
            let line = self.row_line(index);
            buf.set_line(area.x, y, &line, area.width);
            y += 1;
        }
        if end < self.rows.len() {
            let more = format!("... {} more", self.rows.len() - end);
            let line = Line::from(Span::styled(more, dim_style));
            buf.set_line(area.x, y, &line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::{buffer_lines, render, render_buffer};
    use ratatui::style::Color;

    fn row(title: &str, path: &str, doc_type: &str, archived: bool) -> NoteRow {
        NoteRow {
            title: title.to_owned(),
            path: path.to_owned(),
            doc_type: doc_type.to_owned(),
            snippet: String::new(),
            archived,
        }
    }

    #[test]
    fn widgets_note_list_renders_titles_types_and_archived() {
        let rows = vec![
            row("Ship mdfu", "notes/ship.md", "Task", false),
            row("", "notes/untitled.md", "", true),
        ];
        let list = NoteList::new(&rows, &Theme::default());
        assert_eq!(
            render(&list, 60, 4),
            [
                "> Ship mdfu [Task]  notes/ship.md",
                "  notes/untitled.md (archived)",
                "",
                "",
            ]
        );
    }

    #[test]
    fn widgets_note_list_cursor_uses_theme_color_and_marker() {
        let rows = vec![
            row("First", "a.md", "", false),
            row("Second", "b.md", "", false),
        ];
        let list = NoteList::new(&rows, &Theme::default()).cursor(1);
        let lines = render(&list, 40, 2);
        assert_eq!(lines, ["  First  a.md", "> Second  b.md"]);

        let buffer = render_buffer(&list, 40, 2);
        let cursor_cell = buffer.cell((0, 1)).expect("cursor cell");
        assert_eq!(
            cursor_cell.fg,
            Color::Indexed(212),
            "cursor style from theme"
        );
        let plain_cell = buffer.cell((0, 0)).expect("plain cell");
        assert_eq!(plain_cell.fg, Color::Reset);
    }

    #[test]
    fn widgets_note_list_multi_select_shows_checkbox_column() {
        let rows = vec![
            row("First", "a.md", "", false),
            row("Second", "b.md", "", false),
        ];
        let selected = [0_usize];
        let list = NoteList::new(&rows, &Theme::default()).selected(&selected);
        assert_eq!(
            render(&list, 40, 2),
            ["> [x] First  a.md", "  [ ] Second  b.md"]
        );
    }

    #[test]
    fn widgets_note_list_empty_and_more_hint() {
        let empty: Vec<NoteRow> = Vec::new();
        let list = NoteList::new(&empty, &Theme::default());
        assert_eq!(render(&list, 20, 3), ["(no matches)", "", ""]);

        let rows = vec![
            row("One", "a.md", "", false),
            row("Two", "b.md", "", false),
            row("Three", "c.md", "", false),
        ];
        let list = NoteList::new(&rows, &Theme::default());
        assert_eq!(render(&list, 20, 2), ["> One  a.md", "... 2 more"]);

        let offset = NoteList::new(&rows, &Theme::default()).offset(1);
        assert_eq!(render(&offset, 20, 2), ["  Two  b.md", "  Three  c.md"]);

        let past_end = NoteList::new(&rows, &Theme::default()).offset(9);
        assert_eq!(render(&past_end, 20, 2), ["> One  a.md", "... 2 more"]);
    }

    #[test]
    fn widgets_note_list_snippet_replaces_path() {
        let rows = vec![NoteRow::from(&mdfu_core::model::Document {
            title: "T".to_owned(),
            path: "a.md".to_owned(),
            ..mdfu_core::model::Document::default()
        })
        .with_snippet("…match…")];
        let list = NoteList::new(&rows, &Theme::default());
        assert_eq!(render(&list, 40, 1), ["> T  …match…"]);
        assert!(buffer_lines(&render_buffer(&list, 40, 1))[0].contains("…match…"));
    }

    #[test]
    fn widgets_note_list_zero_area_is_safe() {
        let rows = vec![row("One", "a.md", "", false)];
        let list = NoteList::new(&rows, &Theme::default());
        assert!(render(&list, 0, 0).is_empty());
    }
}
