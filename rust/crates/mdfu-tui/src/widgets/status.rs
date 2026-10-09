//! Status/footer line widget.

use mdfu_core::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::style_from;
use crate::view_model::StatusBar;

/// The dim one-line status footer shared by both TUIs (Go `renderStatus` /
/// `renderTaskFooter`).
pub struct StatusBarWidget {
    line: String,
    style: Style,
}

impl StatusBarWidget {
    /// Renders `bar.line()` with the theme's dim style.
    #[must_use]
    pub fn new(bar: &StatusBar, theme: &Theme) -> Self {
        Self {
            line: bar.line(),
            style: style_from(&theme.dim),
        }
    }

    /// The plain status text, before styling.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.line
    }
}

impl Widget for &StatusBarWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let line = Line::from(Span::styled(self.line.clone(), self.style));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::render;

    #[test]
    fn widgets_status_bar_matches_view_model_lines() {
        let notes = StatusBar::notes(2, 5, false, true, false, "");
        assert_eq!(
            StatusBarWidget::new(&notes, &Theme::default()).text(),
            "2/5 • archived:hidden • fm:shown • /:search • e:edit • tab:multi • enter:select"
        );
        assert_eq!(
            render(&StatusBarWidget::new(&notes, &Theme::default()), 80, 1),
            ["2/5 • archived:hidden • fm:shown • /:search • e:edit • tab:multi • enter:select"]
        );

        let tasks = StatusBar::tasks(0, 0, true, "cannot write");
        assert_eq!(
            StatusBarWidget::new(&tasks, &Theme::default()).text(),
            "0/0 • blocked:shown • x:done • t:edit • ?:help • cannot write"
        );
    }

    #[test]
    fn widgets_status_bar_truncates_and_handles_zero_area() {
        let notes = StatusBar::notes(1, 1, false, false, false, "");
        let widget = StatusBarWidget::new(&notes, &Theme::default());
        assert_eq!(render(&widget, 5, 1), ["1/1 •"]);
        assert!(render(&widget, 0, 0).is_empty());
    }
}
