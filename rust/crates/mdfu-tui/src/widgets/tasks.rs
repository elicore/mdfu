//! Task TUI widgets over [`TaskRow`], [`TaskDetail`], and [`TaskEditor`].

use mdfu_core::theme::{PaneBorder, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use super::preview::markdown_text;
use super::{color_from, rows, style_from};
use crate::view_model::{TaskDetail, TaskEditor, TaskRow};

/// The task pane frame from the theme's `task_pane` style: a rounded border in
/// the theme's border color, or no border at all when the theme selects none.
/// Callers wrap [`TaskList`] and [`TaskDetailWidget`] in it, mirroring Go
/// `TaskModel.pane`.
#[must_use]
pub fn task_pane_block(theme: &Theme) -> Block<'static> {
    let mut block = Block::new();
    if theme.task_pane.border == PaneBorder::Rounded {
        block = block.border_type(BorderType::Rounded).borders(Borders::ALL);
    }
    if let Some(color) = theme.task_pane.border_fg.as_deref().and_then(color_from) {
        block = block.border_style(Style::new().fg(color));
    }
    block
}

/// The task TUI's list pane: one row per [`TaskRow`] with checkbox, ID, title,
/// tags, priority, and display properties; the cursor row wears the theme's
/// selected style, a done row the done style, and `@blocked_by` the blocker
/// style (Go `renderTaskList` / `renderTaskRow`).
pub struct TaskList<'a> {
    rows: &'a [TaskRow],
    cursor: usize,
    offset: usize,
    theme: Theme,
}

impl<'a> TaskList<'a> {
    /// Builds the list over `rows` with `theme` styles.
    #[must_use]
    pub fn new(rows: &'a [TaskRow], theme: &Theme) -> Self {
        Self {
            rows,
            cursor: 0,
            offset: 0,
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

    fn row_line(&self, index: usize) -> Line<'static> {
        let row = &self.rows[index];
        let cursor = index == self.cursor;
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::raw(if row.checked { "[x]" } else { "[ ]" }));
        if !row.id.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                row.id.clone(),
                style_from(&self.theme.task_id),
            ));
        }
        spans.push(Span::raw(" "));
        spans.push(Span::raw(row.title.clone()));
        for tag in &row.tags {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("#{tag}"),
                style_from(&self.theme.task_tag),
            ));
        }
        if !row.priority.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("!{}", row.priority),
                style_from(&self.theme.task_priority),
            ));
        }
        for (key, value) in &row.properties {
            spans.push(Span::raw(" "));
            let text = format!("@{key}:{value}");
            if key == "blocked_by" {
                spans.push(Span::styled(text, style_from(&self.theme.task_blocker)));
            } else {
                spans.push(Span::raw(text));
            }
        }
        let overlay = if cursor {
            Some(style_from(&self.theme.task_selected))
        } else if row.checked {
            Some(style_from(&self.theme.task_done))
        } else {
            None
        };
        if let Some(overlay) = overlay {
            for span in &mut spans {
                span.style = overlay.patch(span.style);
            }
        }
        Line::from(spans)
    }
}

impl Widget for &TaskList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let dim_style = style_from(&self.theme.dim);
        if self.rows.is_empty() {
            let line = Line::from(Span::styled("(no tasks)", dim_style));
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

/// The task TUI's detail pane: markdown header, labelled metadata rows, and
/// the markdown body, or a dim placeholder when no task is focused
/// (Go `renderTaskDetail` / `renderTaskMeta`).
pub struct TaskDetailWidget<'a> {
    detail: Option<&'a TaskDetail>,
    theme: Theme,
}

impl<'a> TaskDetailWidget<'a> {
    /// Builds the pane for a focused task.
    #[must_use]
    pub fn new(detail: &'a TaskDetail, theme: &Theme) -> Self {
        Self {
            detail: Some(detail),
            theme: theme.clone(),
        }
    }

    /// Builds the pane's empty state, `"(no task selected)"`.
    #[must_use]
    pub fn empty(theme: &Theme) -> Self {
        Self {
            detail: None,
            theme: theme.clone(),
        }
    }

    fn meta_rows(&self, detail: &TaskDetail) -> Vec<(String, Vec<Span<'static>>)> {
        let mut rows: Vec<(String, Vec<Span<'static>>)> = Vec::new();
        if !detail.id.is_empty() {
            rows.push(("ID".to_owned(), vec![Span::raw(detail.id.clone())]));
        }
        rows.push((
            "Status".to_owned(),
            vec![Span::raw(detail.status_word().to_owned())],
        ));
        if !detail.title.is_empty() {
            rows.push(("Title".to_owned(), vec![Span::raw(detail.title.clone())]));
        }
        if !detail.priority.is_empty() {
            rows.push((
                "Priority".to_owned(),
                vec![Span::styled(
                    format!("!{}", detail.priority),
                    style_from(&self.theme.task_priority),
                )],
            ));
        }
        if !detail.tags.is_empty() {
            let tags = detail
                .tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" ");
            rows.push((
                "Tags".to_owned(),
                vec![Span::styled(tags, style_from(&self.theme.task_tag))],
            ));
        }
        for (key, value) in &detail.properties {
            let span = if key == "blocked_by" {
                Span::styled(value.clone(), style_from(&self.theme.task_blocker))
            } else {
                Span::raw(value.clone())
            };
            rows.push((format!("@{key}"), vec![span]));
        }
        if !detail.file.is_empty() {
            rows.push((
                "File".to_owned(),
                vec![Span::raw(format!("{}:{}", detail.file, detail.line))],
            ));
        }
        rows
    }

    fn render_meta(
        &self,
        rows: Vec<(String, Vec<Span<'static>>)>,
        area: Rect,
        buf: &mut Buffer,
        start: u16,
    ) {
        let label_style = style_from(&self.theme.frontmatter_key);
        let bottom = area.y.saturating_add(area.height);
        for (y, (label, value)) in (start..).zip(rows) {
            if y >= bottom {
                return;
            }
            let mut spans = vec![Span::styled(label, label_style)];
            spans.push(Span::raw("  "));
            spans.extend(value);
            buf.set_line(area.x, y, &Line::from(spans), area.width);
        }
    }
}

impl Widget for &TaskDetailWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let Some(detail) = self.detail else {
            let line = Line::from(Span::styled(
                "(no task selected)",
                style_from(&self.theme.dim),
            ));
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        };
        let bottom = area.y.saturating_add(area.height);
        let mut y = area.y;
        if !detail.header_raw.trim().is_empty() {
            let text = markdown_text(&detail.header_raw, area.width);
            for line in &text.lines {
                if y >= bottom {
                    return;
                }
                buf.set_line(area.x, y, line, area.width);
                y += 1;
            }
        }
        y = y.saturating_add(1);
        let meta = self.meta_rows(detail);
        let meta_end = y.saturating_add(rows(meta.len()));
        self.render_meta(meta, area, buf, y);
        if !detail.body.trim().is_empty() && meta_end < bottom {
            let body_start = meta_end.saturating_add(1);
            if body_start < bottom {
                let rect = Rect {
                    x: area.x,
                    y: body_start,
                    width: area.width,
                    height: bottom - body_start,
                };
                let text = markdown_text(&detail.body, area.width);
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .render(rect, buf);
            }
        }
    }
}

/// The task TUI's edit frame: the editor buffer, the parsed header preview in
/// the side panel, and the save/cancel status line (Go `renderTaskEditor`).
pub struct TaskEditorWidget<'a> {
    editor: &'a TaskEditor,
    theme: Theme,
}

impl<'a> TaskEditorWidget<'a> {
    /// Builds the frame over `editor` with `theme` styles.
    #[must_use]
    pub fn new(editor: &'a TaskEditor, theme: &Theme) -> Self {
        Self {
            editor,
            theme: theme.clone(),
        }
    }

    /// The plain status line, matching the Go edit-mode strings.
    #[must_use]
    pub fn status_text(&self) -> String {
        let mut text = if self.editor.is_body {
            format!(
                "body indent: {} • ctrl+s save • esc cancel",
                quote_indent(&self.editor.body_indent)
            )
        } else {
            "ctrl+s save • esc cancel".to_owned()
        };
        if !self.editor.status.is_empty() {
            text.push_str(" • ");
            text.push_str(&self.editor.status);
        }
        text
    }

    fn render_meta(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let label_style = style_from(&self.theme.frontmatter_key);
        let bottom = area.y.saturating_add(area.height);
        for (y, (label, value)) in (area.y..).zip(&self.editor.meta) {
            if y >= bottom {
                return;
            }
            let line = Line::from(vec![
                Span::styled(label.clone(), label_style),
                Span::raw("  "),
                Span::raw(value.clone()),
            ]);
            buf.set_line(area.x, y, &line, area.width);
        }
    }

    fn render_status(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let line = Line::from(Span::styled(
            self.status_text(),
            style_from(&self.theme.dim),
        ));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Widget for &TaskEditorWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let status_height = u16::from(area.height >= 1);
        if self.editor.is_body || self.editor.meta.is_empty() {
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(status_height)])
                .split(area);
            render_editor_text(&self.editor.text, chunks[0], buf);
            self.render_status(chunks[1], buf);
            return;
        }
        let meta_height =
            rows(self.editor.meta.len()).min(area.height.saturating_sub(status_height));
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(meta_height),
            Constraint::Length(status_height),
        ])
        .split(area);
        render_editor_text(&self.editor.text, chunks[0], buf);
        self.render_meta(chunks[1], buf);
        self.render_status(chunks[2], buf);
    }
}

/// Writes the editor buffer's lines into `area`, clipping at its bottom.
fn render_editor_text(text: &str, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bottom = area.y.saturating_add(area.height);
    for (y, line) in (area.y..).zip(text.lines()) {
        if y >= bottom {
            break;
        }
        buf.set_line(area.x, y, &Line::raw(line.to_owned()), area.width);
    }
}

/// Renders a whitespace indent the way Go `quoteIndent` does: `""` for empty,
/// else double-quoted with tabs escaped.
fn quote_indent(indent: &str) -> String {
    if indent.is_empty() {
        return "\"\"".to_owned();
    }
    format!("\"{}\"", indent.replace('\t', "\\t"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::render;
    use ratatui::style::Color;

    fn task_row(id: &str, title: &str, checked: bool) -> TaskRow {
        TaskRow {
            checked,
            id: id.to_owned(),
            title: title.to_owned(),
            ..TaskRow::default()
        }
    }

    fn task_detail() -> TaskDetail {
        TaskDetail {
            header_raw: "- [ ] BLK-2 Backfill the analytics table #launch !high".to_owned(),
            id: "BLK-2".to_owned(),
            title: "Backfill the analytics table".to_owned(),
            priority: "high".to_owned(),
            tags: vec!["launch".to_owned()],
            properties: vec![
                ("owner".to_owned(), "alice".to_owned()),
                ("blocked_by".to_owned(), "BLK-1".to_owned()),
            ],
            file: "tasks.md".to_owned(),
            line: 7,
            body: "Body line.".to_owned(),
            ..TaskDetail::default()
        }
    }

    #[test]
    fn widgets_task_list_renders_rows_and_styles() {
        let mut blocked = task_row("BLK-3", "Flip the read path", false);
        blocked.priority = "low".to_owned();
        blocked.tags = vec!["launch".to_owned()];
        blocked.properties = vec![("blocked_by".to_owned(), "BLK-404".to_owned())];
        let rows = vec![task_row("BLK-1", "Done task", true), blocked];
        let list = TaskList::new(&rows, &Theme::default()).cursor(0);
        assert_eq!(
            render(&list, 80, 3),
            [
                "[x] BLK-1 Done task",
                "[ ] BLK-3 Flip the read path #launch !low @blocked_by:BLK-404",
                "",
            ]
        );

        let buffer = crate::widgets::test_support::render_buffer(&list, 80, 2);
        assert_eq!(
            buffer.cell((0, 0)).expect("cell").fg,
            Color::Indexed(82),
            "cursor row uses task_selected"
        );
        let blocked_row = crate::widgets::test_support::render_buffer(&list, 80, 2);
        let blocker_cells: Vec<_> = (0..80)
            .filter_map(|x| blocked_row.cell((x, 1)))
            .filter(|cell| cell.fg == Color::Indexed(203))
            .collect();
        assert!(
            !blocker_cells.is_empty(),
            "@blocked_by must use the theme blocker color"
        );
        let done = TaskList::new(&rows, &Theme::default()).cursor(1);
        let buffer = crate::widgets::test_support::render_buffer(&done, 80, 2);
        assert_eq!(
            buffer.cell((0, 0)).expect("cell").fg,
            Color::Indexed(240),
            "done row uses task_done"
        );
    }

    #[test]
    fn widgets_task_list_empty_and_more_hint() {
        let empty: Vec<TaskRow> = Vec::new();
        assert_eq!(
            render(&TaskList::new(&empty, &Theme::default()), 20, 2),
            ["(no tasks)", ""]
        );

        let rows = vec![
            task_row("A-1", "one", false),
            task_row("A-2", "two", false),
            task_row("A-3", "three", false),
        ];
        let list = TaskList::new(&rows, &Theme::default());
        assert_eq!(render(&list, 30, 2), ["[ ] A-1 one", "... 2 more"]);
        let offset = TaskList::new(&rows, &Theme::default()).offset(1);
        assert_eq!(render(&offset, 30, 2), ["[ ] A-2 two", "[ ] A-3 three"]);
    }

    #[test]
    fn widgets_task_detail_renders_header_meta_and_body() {
        let detail = task_detail();
        let widget = TaskDetailWidget::new(&detail, &Theme::default());
        let lines = render(&widget, 60, 12);
        assert_eq!(
            lines[0],
            "- [ ] BLK-2 Backfill the analytics table #launch !high"
        );
        assert_eq!(lines[2], "ID  BLK-2");
        assert_eq!(lines[3], "Status  open");
        assert_eq!(lines[4], "Title  Backfill the analytics table");
        assert_eq!(lines[5], "Priority  !high");
        assert_eq!(lines[6], "Tags  #launch");
        assert_eq!(lines[7], "@owner  alice");
        assert_eq!(lines[8], "@blocked_by  BLK-1");
        assert_eq!(lines[9], "File  tasks.md:7");
        assert_eq!(lines[11], "Body line.");
    }

    #[test]
    fn widgets_task_detail_empty_placeholder() {
        let widget = TaskDetailWidget::empty(&Theme::default());
        assert_eq!(render(&widget, 30, 2), ["(no task selected)", ""]);
    }

    #[test]
    fn widgets_task_editor_title_frame() {
        let editor = TaskEditor {
            text: "- [ ] BLK-2 Backfill".to_owned(),
            meta: vec![
                ("ID".to_owned(), "BLK-2".to_owned()),
                ("Status".to_owned(), "open".to_owned()),
            ],
            ..TaskEditor::default()
        };
        let widget = TaskEditorWidget::new(&editor, &Theme::default());
        assert_eq!(widget.status_text(), "ctrl+s save • esc cancel");
        assert_eq!(
            render(&widget, 40, 4),
            [
                "- [ ] BLK-2 Backfill",
                "ID  BLK-2",
                "Status  open",
                "ctrl+s save • esc cancel",
            ]
        );
    }

    #[test]
    fn widgets_task_editor_body_frame() {
        let editor = TaskEditor {
            text: "body\nlines".to_owned(),
            is_body: true,
            body_indent: "\t".to_owned(),
            status: "saved body".to_owned(),
            ..TaskEditor::default()
        };
        let widget = TaskEditorWidget::new(&editor, &Theme::default());
        assert_eq!(
            widget.status_text(),
            "body indent: \"\\t\" • ctrl+s save • esc cancel • saved body"
        );
        assert_eq!(
            render(&widget, 60, 4),
            [
                "body",
                "lines",
                "",
                "body indent: \"\\t\" • ctrl+s save • esc cancel • saved body",
            ]
        );

        let empty = TaskEditor::default();
        assert_eq!(
            TaskEditorWidget::new(&empty, &Theme::default()).status_text(),
            "ctrl+s save • esc cancel"
        );
    }

    #[test]
    fn widgets_task_pane_block_reads_theme_border() {
        let theme = Theme::default();
        let block = task_pane_block(&theme);
        let area = Rect::new(0, 0, 6, 3);
        let mut buffer = Buffer::empty(area);
        block.render(area, &mut buffer);
        let lines = crate::widgets::test_support::buffer_lines(&buffer);
        assert_eq!(lines, ["╭────╮", "│    │", "╰────╯"]);
        assert_eq!(
            buffer.cell((0, 0)).expect("corner").fg,
            Color::Indexed(238),
            "border uses task_pane_border_color"
        );

        let bare = Theme {
            task_pane: mdfu_core::theme::Style::default(),
            ..Theme::default()
        };
        let area = Rect::new(0, 0, 4, 2);
        let mut buffer = Buffer::empty(area);
        task_pane_block(&bare).render(area, &mut buffer);
        assert!(
            crate::widgets::test_support::buffer_lines(&buffer)
                .iter()
                .all(String::is_empty),
            "task_pane border none draws nothing"
        );
    }

    #[test]
    fn widgets_task_widgets_zero_area_is_safe() {
        let rows = vec![task_row("A-1", "one", false)];
        assert!(render(&TaskList::new(&rows, &Theme::default()), 0, 0).is_empty());
        let detail = task_detail();
        assert!(render(&TaskDetailWidget::new(&detail, &Theme::default()), 0, 0).is_empty());
        let editor = TaskEditor::default();
        assert!(render(&TaskEditorWidget::new(&editor, &Theme::default()), 0, 0).is_empty());
    }
}
