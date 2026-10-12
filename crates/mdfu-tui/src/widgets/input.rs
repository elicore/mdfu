//! One-line text input wrapping `tui-input`.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::Event;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

/// The picker's search line and the task TUI's filter line.
///
/// Wraps [`tui_input::Input`] (grapheme-aware insert/delete, word movement,
/// kill/yank) and paints it as a ratatui widget: the value scrolls
/// horizontally to keep the cursor visible, and a focused input reverses the
/// cursor cell so test backends and real terminals agree without a frame-level
/// cursor. `handle_event` forwards a crossterm event and reports whether the
/// value or cursor changed.
pub struct SearchInput {
    input: Input,
    focused: bool,
    style: Style,
}

impl SearchInput {
    /// Builds an empty, unfocused input with the default style.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an empty input focused with the given style, useful for
    /// rendering the search line with a theme foreground.
    #[must_use]
    pub fn with_style(style: Style) -> Self {
        Self {
            input: Input::default(),
            focused: true,
            style,
        }
    }

    /// Seeds the input with `value`, cursor at the end.
    #[must_use]
    pub fn with_value(value: impl Into<String>) -> Self {
        Self {
            input: Input::new(value.into()),
            ..Self::default()
        }
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> &str {
        self.input.value()
    }

    /// Whether the input draws its cursor cell.
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Starts drawing the cursor cell.
    pub fn focus(&mut self) {
        self.focused = true;
    }

    /// Stops drawing the cursor cell.
    pub fn blur(&mut self) {
        self.focused = false;
    }

    /// Replaces the value and places the cursor at its end.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.input = Input::new(value.into());
    }

    /// Clears the value and the cursor.
    pub fn reset(&mut self) {
        self.input.reset();
    }

    /// Returns the value and resets the input, as `tui-input` does.
    pub fn value_and_reset(&mut self) -> String {
        self.input.value_and_reset()
    }

    /// Forwards a crossterm event; `true` when the input changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        self.input.handle_event(event).is_some()
    }

    /// The cursor's display column within the value, from `tui-input`.
    #[must_use]
    pub fn visual_cursor(&self) -> usize {
        self.input.visual_cursor()
    }

    /// The display-column scroll that keeps the cursor visible at `width`.
    #[must_use]
    pub fn visual_scroll(&self, width: usize) -> usize {
        self.input.visual_scroll(width)
    }

    /// The buffer position a frame-level cursor should use for `area`,
    /// mirroring the reversed cursor cell the `Widget` implementation draws.
    #[must_use]
    pub fn cursor_position(&self, area: Rect) -> Option<Position> {
        if !self.focused || area.width == 0 || area.height == 0 {
            return None;
        }
        let width = usize::from(area.width);
        let scroll = self.input.visual_scroll(width);
        let column = cursor_column(
            self.input.visual_cursor(),
            scroll,
            usize::from(area.width.saturating_sub(1)),
        );
        Some(Position {
            x: area.x + u16::try_from(column).unwrap_or(0),
            y: area.y,
        })
    }
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            input: Input::default(),
            focused: false,
            style: Style::new(),
        }
    }
}

impl Widget for &SearchInput {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = usize::from(area.width);
        let scroll = self.input.visual_scroll(width);
        let scrolled = u16::try_from(scroll).unwrap_or(u16::MAX);
        Paragraph::new(self.input.value())
            .style(self.style)
            .scroll((0, scrolled))
            .render(area, buf);

        if self.focused {
            let column = cursor_column(
                self.input.visual_cursor(),
                scroll,
                usize::from(area.width.saturating_sub(1)),
            );
            if let Some(cell) = buf.cell_mut((area.x + u16::try_from(column).unwrap_or(0), area.y))
            {
                cell.set_style(self.style.add_modifier(Modifier::REVERSED));
            }
        }
    }
}

/// Clamps the cursor's display column into `max_column` after scrolling.
fn cursor_column(cursor: usize, scroll: usize, max_column: usize) -> usize {
    cursor.max(scroll).saturating_sub(scroll).min(max_column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::render;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn widgets_search_input_handles_editing_events() {
        let mut input = SearchInput::new();
        assert!(!input.focused());
        assert!(input.handle_event(&press(KeyCode::Char('a'))));
        assert!(input.handle_event(&press(KeyCode::Char('b'))));
        assert_eq!(input.value(), "ab");
        assert!(input.handle_event(&press(KeyCode::Backspace)));
        assert_eq!(input.value(), "a");
        assert!(
            !input.handle_event(&press(KeyCode::Tab)),
            "tab is not an edit"
        );

        input.set_value("hello");
        assert_eq!(input.value(), "hello");
        input.focus();
        assert!(input.focused());
        assert_eq!(input.value_and_reset(), "hello");
        assert_eq!(input.value(), "");
        input.blur();
        assert!(!input.focused());
    }

    #[test]
    fn widgets_search_input_renders_value_and_cursor() {
        let input = SearchInput::with_value("query");
        assert_eq!(render(&input, 10, 1), ["query"]);
        assert!(input.cursor_position(Rect::new(0, 0, 10, 1)).is_none());

        let mut focused = SearchInput::with_value("query");
        focused.focus();
        let buffer = crate::widgets::test_support::render_buffer(&focused, 10, 1);
        let cursor = focused
            .cursor_position(Rect::new(0, 0, 10, 1))
            .expect("cursor");
        assert_eq!(cursor.x, 5);
        let cell = buffer.cell((5, 0)).expect("cell");
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn widgets_search_input_scrolls_to_keep_cursor_visible() {
        let mut input = SearchInput::with_value("abcdef");
        input.focus();
        let lines = render(&input, 4, 1);
        assert_eq!(lines, ["cdef"]);
        let cursor = input
            .cursor_position(Rect::new(0, 0, 4, 1))
            .expect("cursor");
        assert_eq!(cursor.x, 3);
    }

    #[test]
    fn widgets_search_input_zero_area_is_safe() {
        let input = SearchInput::with_value("x");
        assert!(render(&input, 0, 0).is_empty());
        assert!(input.cursor_position(Rect::new(0, 0, 0, 0)).is_none());
    }
}
