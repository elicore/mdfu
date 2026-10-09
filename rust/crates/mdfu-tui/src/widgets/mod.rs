//! Reusable ratatui widgets over the [`crate::view_model`] boundary.
//!
//! Every widget renders plain data snapshotted by [`crate::view_model`] plus
//! styles resolved from [`mdfu_core::theme`]; no widget ever reaches into a
//! core [`mdfu_core::model::Document`] or [`mdfu_core::task::Task`], and no
//! widget calls a core engine function while rendering. Colors are never
//! literals: they come from the theme (or, for the markdown body, stay at the
//! terminal defaults).
//!
//! Composition:
//! - [`Preview`] is the fixed-order stack of hideable [`Component`]s
//!   ([`FilenameBar`], [`TitleBlock`], [`FrontmatterPanel`], [`BodyPanel`]);
//!   the stack splits its area with [`ratatui::layout::Layout`].
//! - [`SearchInput`] wraps `tui-input` for the picker/task filter line.
//! - [`NoteList`] renders note-picker rows, [`TaskList`]/[`TaskDetailWidget`]/
//!   [`TaskEditorWidget`] the task views, and [`StatusBarWidget`] the footer.

mod input;
mod list;
mod preview;
mod status;
mod tasks;

pub use input::SearchInput;
pub use list::NoteList;
pub use preview::{BodyPanel, FilenameBar, FrontmatterPanel, TitleBlock};
pub use status::StatusBarWidget;
pub use tasks::{task_pane_block, TaskDetailWidget, TaskEditorWidget, TaskList};

use mdfu_core::theme::{Style as ThemeStyle, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::view_model::PreviewBlock;

/// Preview component ID: the document basename line.
pub const COMPONENT_FILENAME: &str = "filename";
/// Preview component ID: the document title line.
pub const COMPONENT_TITLE: &str = "title";
/// Preview component ID: the retained Path row plus frontmatter rows.
pub const COMPONENT_FRONTMATTER: &str = "frontmatter";
/// Preview component ID: the markdown body.
pub const COMPONENT_BODY: &str = "body";

/// One hideable block of the document preview (Go `tui.Component`).
///
/// `height` is the ratatui extension Go does not need: it reports the rows the
/// component wants at `width`, which [`Preview`] uses to split the buffer into
/// stacked areas before calling `render(area, buf)`.
pub trait Component {
    /// Stable component ID (one of the `COMPONENT_*` constants).
    fn id(&self) -> &'static str;
    /// Whether the component currently renders.
    fn visible(&self) -> bool;
    /// Shows or hides the component.
    fn set_visible(&mut self, visible: bool);
    /// Rows this component wants at `width`.
    fn height(&self, width: u16) -> u16;
    /// Draws the component into `area`; a hidden component draws nothing.
    fn render(&self, area: Rect, buf: &mut Buffer);
}

/// The document preview: filename, title, frontmatter, and body in fixed
/// order, each independently hideable (Go `tui.Preview`).
pub struct Preview {
    components: Vec<Box<dyn Component>>,
}

impl Preview {
    /// Builds the stack for `block` with `theme` styles. Components capture the
    /// snapshot at construction, so a preview is created (and re-created per
    /// selected document) like Go `NewPreview`.
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            components: vec![
                Box::new(FilenameBar::new(block, theme)),
                Box::new(TitleBlock::new(block, theme)),
                Box::new(FrontmatterPanel::new(block, theme)),
                Box::new(BodyPanel::new(block, theme)),
            ],
        }
    }

    /// Returns the component IDs in render order.
    #[must_use]
    pub fn ids(&self) -> Vec<&'static str> {
        self.components
            .iter()
            .map(|component| component.id())
            .collect()
    }

    /// Shows or hides the component with `id`; unknown IDs are ignored.
    pub fn set_visible(&mut self, id: &str, visible: bool) {
        if let Some(component) = self
            .components
            .iter_mut()
            .find(|component| component.id() == id)
        {
            component.set_visible(visible);
        }
    }

    /// Reports whether the component with `id` is shown; unknown IDs are false.
    #[must_use]
    pub fn visible(&self, id: &str) -> bool {
        self.component(id).is_some_and(Component::visible)
    }

    /// Flips the component's visibility and reports the new state; unknown IDs
    /// report false.
    pub fn toggle(&mut self, id: &str) -> bool {
        let Some(component) = self
            .components
            .iter_mut()
            .find(|component| component.id() == id)
        else {
            return false;
        };
        let visible = !component.visible();
        component.set_visible(visible);
        visible
    }

    /// Renders the visible components top to bottom: each gets its measured
    /// height, and the last visible component gets the remaining rows.
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let visible: Vec<&dyn Component> = self
            .components
            .iter()
            .filter(|component| component.visible())
            .map(Box::as_ref)
            .collect();
        if visible.is_empty() {
            return;
        }
        let mut remaining = area.height;
        let mut constraints = Vec::with_capacity(visible.len());
        for (index, component) in visible.iter().enumerate() {
            if index + 1 == visible.len() {
                constraints.push(Constraint::Length(remaining));
            } else {
                let height = component.height(area.width).min(remaining);
                constraints.push(Constraint::Length(height));
                remaining -= height;
            }
        }
        let chunks = Layout::vertical(constraints).split(area);
        for (component, chunk) in visible.iter().zip(chunks.iter()) {
            if chunk.height > 0 {
                component.render(*chunk, buf);
            }
        }
    }

    fn component(&self, id: &str) -> Option<&dyn Component> {
        self.components
            .iter()
            .find(|component| component.id() == id)
            .map(Box::as_ref)
    }
}

impl Widget for &Preview {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Preview::render(self, area, buf);
    }
}

/// Maps a theme style onto a ratatui style.
///
/// The theme keeps colors as config strings (`212`, `#ff0000`, ...); an
/// unparseable color is dropped instead of panicking. `horizontal_padding`,
/// `border`, and `border_fg` are not text styles and are consumed by the
/// widgets that own pill and pane painting.
#[must_use]
pub(crate) fn style_from(theme_style: &ThemeStyle) -> Style {
    let mut style = Style::new();
    if let Some(color) = theme_style.fg.as_deref().and_then(color_from) {
        style = style.fg(color);
    }
    if let Some(color) = theme_style.bg.as_deref().and_then(color_from) {
        style = style.bg(color);
    }
    if theme_style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if theme_style.faint {
        style = style.add_modifier(Modifier::DIM);
    }
    if theme_style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

/// Parses a theme color string the way lipgloss colors resolve: `212` is an
/// indexed color, `#f00`/`#ff0000` an RGB color, and ANSI names (`red`,
/// `bright-red`, `gray`, ...) their terminal color. Unknown spellings yield
/// `None` so the widget keeps the terminal default.
#[must_use]
pub(crate) fn color_from(text: &str) -> Option<Color> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        return hex_color(hex);
    }
    if let Ok(index) = text.parse::<u8>() {
        return Some(Color::Indexed(index));
    }
    ansi_color(text)
}

fn hex_color(hex: &str) -> Option<Color> {
    if !matches!(hex.len(), 3 | 6) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    if hex.len() == 3 {
        let red = u8::try_from((value >> 8) & 0xf).ok()? * 17;
        let green = u8::try_from((value >> 4) & 0xf).ok()? * 17;
        let blue = u8::try_from(value & 0xf).ok()? * 17;
        return Some(Color::Rgb(red, green, blue));
    }
    Some(Color::Rgb(
        u8::try_from((value >> 16) & 0xff).ok()?,
        u8::try_from((value >> 8) & 0xff).ok()?,
        u8::try_from(value & 0xff).ok()?,
    ))
}

fn ansi_color(name: &str) -> Option<Color> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" | "bright-white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "dark-gray" | "dark-grey" | "bright-black" => Color::DarkGray,
        "light-red" | "bright-red" => Color::LightRed,
        "light-green" | "bright-green" => Color::LightGreen,
        "light-yellow" | "bright-yellow" => Color::LightYellow,
        "light-blue" | "bright-blue" => Color::LightBlue,
        "light-magenta" | "bright-magenta" => Color::LightMagenta,
        "light-cyan" | "bright-cyan" => Color::LightCyan,
        _ => return None,
    })
}

/// Converts a row count to the `u16` ratatui areas use, saturating.
#[must_use]
pub(crate) fn rows(len: usize) -> u16 {
    u16::try_from(len).unwrap_or(u16::MAX)
}

#[cfg(test)]
pub(crate) mod test_support {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;
    use ratatui::Terminal;

    use super::Component;

    pub(crate) fn component_buffer(component: &dyn Component, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        component.render(area, &mut buffer);
        buffer
    }

    /// Renders `widget` on a `TestBackend` and returns one trailing-trimmed
    /// string per row.
    pub(crate) fn render(widget: impl Widget, width: u16, height: u16) -> Vec<String> {
        buffer_lines(&render_buffer(widget, width, height))
    }

    /// Renders `widget` on a `TestBackend` and returns the raw buffer.
    pub(crate) fn render_buffer(widget: impl Widget, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
        terminal
            .draw(|frame| frame.render_widget(widget, frame.area()))
            .expect("TestBackend draw");
        terminal.backend().buffer().clone()
    }

    /// Flattens a buffer into one trailing-trimmed string per row.
    pub(crate) fn buffer_lines(buffer: &Buffer) -> Vec<String> {
        let area = buffer.area;
        let mut lines = Vec::with_capacity(usize::from(area.height));
        for y in 0..area.height {
            let mut text = String::new();
            for x in 0..area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    text.push_str(cell.symbol());
                }
            }
            lines.push(text.trim_end().to_owned());
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_model::PreviewBlock;

    #[test]
    fn color_from_parses_indexed_hex_and_names() {
        assert_eq!(color_from("212"), Some(Color::Indexed(212)));
        assert_eq!(color_from(" 82 "), Some(Color::Indexed(82)));
        assert_eq!(color_from("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(color_from("#f00"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(color_from("#0aF"), Some(Color::Rgb(0, 170, 255)));
        assert_eq!(color_from("red"), Some(Color::Red));
        assert_eq!(color_from("Bright-Blue"), Some(Color::LightBlue));
        assert_eq!(color_from("gray"), Some(Color::Gray));
    }

    #[test]
    fn color_from_rejects_unknown_and_out_of_range() {
        for bad in ["", "300", "#12345", "#ggghhh", "#1234567", "solarized"] {
            assert_eq!(color_from(bad), None, "must reject {bad:?}");
        }
    }

    #[test]
    fn style_from_maps_colors_and_modifiers() {
        let theme_style = ThemeStyle {
            fg: Some("#010203".to_owned()),
            bg: Some("62".to_owned()),
            bold: true,
            faint: true,
            underline: true,
            ..ThemeStyle::default()
        };
        let style = style_from(&theme_style);
        assert_eq!(style.fg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(style.bg, Some(Color::Indexed(62)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));

        assert_eq!(style_from(&ThemeStyle::default()), Style::new());
    }

    #[test]
    fn preview_ids_order_and_visibility_round_trip() {
        let block = PreviewBlock::from(&mdfu_core::model::Document::default());
        let mut preview = Preview::new(&block, &Theme::default());
        assert_eq!(
            preview.ids(),
            [
                COMPONENT_FILENAME,
                COMPONENT_TITLE,
                COMPONENT_FRONTMATTER,
                COMPONENT_BODY
            ]
        );
        assert!(preview.visible(COMPONENT_BODY));

        preview.set_visible(COMPONENT_FRONTMATTER, false);
        assert!(!preview.visible(COMPONENT_FRONTMATTER));
        assert!(preview.visible(COMPONENT_TITLE));

        assert!(preview.toggle(COMPONENT_FRONTMATTER));
        assert!(!preview.toggle(COMPONENT_BODY));
        assert!(!preview.toggle("nope"));
        assert!(!preview.visible("nope"));
    }
}
