//! Document preview components: filename bar, title block, frontmatter panel,
//! and markdown body, all painting from [`PreviewBlock`] and theme styles.
//!
//! The body runs the Go preview pipeline (`internal/tui/links.go` + `tui.go`):
//! [`crate::links::extract_and_hide`] hides URLs behind marker runes (or keeps
//! the `label (url)` fallback when hyperlinks are off), `tui-markdown` renders
//! through [`NoColorStyleSheet`] (structural modifiers, no colors), the
//! rendered text is emphasized with [`crate::links::highlight`], marker runes
//! become OSC 8 hyperlinks via [`crate::links::patch_links`], and
//! [`crate::links::close_open_hyperlinks`] seals whatever the pane height
//! clips. The final ANSI string is painted cell by cell: SGR maps back to
//! ratatui styles, and OSC sequences ride the next cell's symbol so the
//! terminal receives them in-band (ratatui drops control characters in
//! `Buffer::set_stringn`, so a `Paragraph` could never carry a hyperlink).

use std::num::NonZeroU16;

use mdfu_core::theme::{PillShape, Theme};
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
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
    hyperlinks: bool,
    terms: Vec<String>,
    highlight_sgr: String,
    link_sgr: String,
}

impl BodyPanel {
    /// Windows `block.body_markdown` to the theme's `body_lines` and snapshots
    /// the render-time link/emphasis inputs: the hyperlink preference and
    /// query terms from `block`, and the theme's SGR strings (resolved here so
    /// no SGR is ever baked into the view model).
    #[must_use]
    pub fn new(block: &PreviewBlock, theme: &Theme) -> Self {
        Self {
            body: first_lines(&block.body_markdown, theme.body_lines),
            lines: theme.body_lines,
            visible: true,
            hyperlinks: block.hyperlinks,
            terms: block.terms.clone(),
            highlight_sgr: theme.highlight_sgr.clone(),
            link_sgr: theme.link_sgr.clone(),
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
        // The order is the Go preview renderer's: hide URLs so the renderer
        // sees labels, render markdown, emphasize terms in the rendered text
        // (`links::highlight` with the snapshotted theme SGR is
        // `links::highlight_with_theme`), patch markers into OSC 8, then seal
        // whatever the pane clips.
        let (marked, links) = crate::links::extract_and_hide(&self.body, self.hyperlinks);
        let rendered = text_to_ansi(&markdown_text(&marked, area.width));
        let highlighted = crate::links::highlight(&rendered, &self.terms, &self.highlight_sgr);
        let patched =
            crate::links::patch_links(&highlighted, &links, self.hyperlinks, &self.link_sgr);
        let wrapped = wrap_ansi(&patched, area.width);
        let visible = wrapped
            .lines()
            .take(usize::from(area.height))
            .collect::<Vec<_>>()
            .join("\n");
        let sealed = crate::links::close_open_hyperlinks(&visible);
        paint_ansi(area, buf, &sealed);
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

/// Serializes rendered markdown into an ANSI-styled string so the
/// link/highlight pipeline runs on it exactly like Go's glamour output: every
/// styled span becomes `SGR params + content + reset`, unstyled spans stay
/// bare, and lines join with `\n`.
fn text_to_ansi(text: &Text<'_>) -> String {
    let mut out = String::new();
    for (index, line) in text.lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        for span in &line.spans {
            let style = line.style.patch(span.style);
            if style == Style::default() {
                out.push_str(&span.content);
                continue;
            }
            out.push_str("\x1b[");
            out.push_str(&style_sgr_params(&style));
            out.push('m');
            out.push_str(&span.content);
            out.push_str("\x1b[0m");
        }
    }
    out
}

/// Converts a ratatui style to SGR parameters (`1;4;38;5;212`): modifiers the
/// markdown stylesheet produces plus standard/bright/indexed/RGB colors, so
/// any style survives the string round trip.
fn style_sgr_params(style: &Style) -> String {
    let mut params: Vec<String> = Vec::new();
    let active = style.add_modifier - style.sub_modifier;
    for (flag, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::SLOW_BLINK, "5"),
        (Modifier::RAPID_BLINK, "6"),
        (Modifier::REVERSED, "7"),
        (Modifier::HIDDEN, "8"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if active.contains(flag) {
            params.push(code.to_owned());
        }
    }
    if let Some(fg) = style.fg {
        params.push(color_sgr_params(fg, true));
    }
    if let Some(bg) = style.bg {
        params.push(color_sgr_params(bg, false));
    }
    params.join(";")
}

/// SGR parameters for one color: `30`-`37`/`40`-`47`, the bright `90`/`100`
/// ranges, `38;5;n`/`48;5;n` indexed, `38;2;r;g;b`/`48;2;...` RGB, and the
/// `39`/`49` defaults.
fn color_sgr_params(color: Color, foreground: bool) -> String {
    let standard = if foreground { 30 } else { 40 };
    let bright = if foreground { 90 } else { 100 };
    let extended = if foreground { 38 } else { 48 };
    match color {
        Color::Reset => (standard + 9).to_string(),
        Color::Black => standard.to_string(),
        Color::Red => (standard + 1).to_string(),
        Color::Green => (standard + 2).to_string(),
        Color::Yellow => (standard + 3).to_string(),
        Color::Blue => (standard + 4).to_string(),
        Color::Magenta => (standard + 5).to_string(),
        Color::Cyan => (standard + 6).to_string(),
        Color::Gray => (standard + 7).to_string(),
        Color::DarkGray => bright.to_string(),
        Color::LightRed => (bright + 1).to_string(),
        Color::LightGreen => (bright + 2).to_string(),
        Color::LightYellow => (bright + 3).to_string(),
        Color::LightBlue => (bright + 4).to_string(),
        Color::LightMagenta => (bright + 5).to_string(),
        Color::LightCyan => (bright + 6).to_string(),
        Color::White => (bright + 7).to_string(),
        Color::Indexed(index) => format!("{extended};5;{index}"),
        Color::Rgb(red, green, blue) => format!("{extended};2;{red};{green};{blue}"),
    }
}

/// Applies one SGR parameter list (`0;1;4;38;5;212`) to `style`, covering the
/// attribute sets/clears and color forms the theme SGR strings and the
/// serialized markdown styles use.
///
/// Shared with the note list, which resolves the theme's `highlight_sgr` to a
/// ratatui style the same way the body panel does.
pub(crate) fn apply_sgr(style: &mut Style, params: &str) {
    let mut iter = params.split(';');
    while let Some(param) = iter.next() {
        match param {
            "" | "0" => *style = Style::default(),
            "1" => *style = style.add_modifier(Modifier::BOLD),
            "2" => *style = style.add_modifier(Modifier::DIM),
            "3" => *style = style.add_modifier(Modifier::ITALIC),
            p if p == "4" || p.starts_with("4:") => {
                *style = style.add_modifier(Modifier::UNDERLINED);
            }
            "5" => *style = style.add_modifier(Modifier::SLOW_BLINK),
            "6" => *style = style.add_modifier(Modifier::RAPID_BLINK),
            "7" => *style = style.add_modifier(Modifier::REVERSED),
            "8" => *style = style.add_modifier(Modifier::HIDDEN),
            "9" => *style = style.add_modifier(Modifier::CROSSED_OUT),
            "22" => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            "23" => *style = style.remove_modifier(Modifier::ITALIC),
            "24" => *style = style.remove_modifier(Modifier::UNDERLINED),
            "25" => *style = style.remove_modifier(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
            "27" => *style = style.remove_modifier(Modifier::REVERSED),
            "28" => *style = style.remove_modifier(Modifier::HIDDEN),
            "29" => *style = style.remove_modifier(Modifier::CROSSED_OUT),
            "39" => style.fg = None,
            "49" => style.bg = None,
            "38" | "48" => {
                if let Some(color) = extended_color(&mut iter) {
                    if param == "38" {
                        style.fg = Some(color);
                    } else {
                        style.bg = Some(color);
                    }
                }
            }
            _ => {
                if let Ok(code) = param.parse::<u8>() {
                    match code {
                        30..=37 => style.fg = Some(ansi_color(code - 30)),
                        40..=47 => style.bg = Some(ansi_color(code - 40)),
                        90..=97 => style.fg = Some(ansi_bright_color(code - 90)),
                        100..=107 => style.bg = Some(ansi_bright_color(code - 100)),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Reads the `5;n` or `2;r;g;b` parameter tail of an extended SGR color.
fn extended_color(iter: &mut std::str::Split<'_, char>) -> Option<Color> {
    match iter.next()? {
        "5" => iter
            .next()
            .and_then(|n| n.parse::<u8>().ok())
            .map(Color::Indexed),
        "2" => {
            let mut channels = [0_u8; 3];
            for channel in &mut channels {
                *channel = iter.next().and_then(|n| n.parse::<u8>().ok())?;
            }
            let [red, green, blue] = channels;
            Some(Color::Rgb(red, green, blue))
        }
        _ => None,
    }
}

/// Standard ANSI colors 0-7 (`30`-`37` / `40`-`47`).
fn ansi_color(code: u8) -> Color {
    match code {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::Gray,
    }
}

/// Bright ANSI colors 0-7 (`90`-`97` / `100`-`107`).
fn ansi_bright_color(code: u8) -> Color {
    match code {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        _ => Color::White,
    }
}

/// Scans one ANSI escape sequence at the start of `s`: a CSI sequence ending
/// in `m` (SGR is the only kind the body pipeline emits) or an OSC sequence
/// terminated by BEL or ST. Returns the sequence and the remainder.
fn scan_escape(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('\x1b')?;
    if let Some(csi) = rest.strip_prefix('[') {
        let end = csi.find('m')?;
        return Some(s.split_at(2 + end + 1));
    }
    if let Some(osc) = rest.strip_prefix(']') {
        if let Some(bel) = osc.find('\x07') {
            return Some(s.split_at(2 + bel + 1));
        }
        let st = osc.find("\x1b\\")?;
        return Some(s.split_at(2 + st + 2));
    }
    None
}

/// Display width of one rune: ASCII fast path, otherwise measured through
/// ratatui's unicode-width integration (CJK-wide counts two columns,
/// combining/control runes zero).
fn char_width(ch: char) -> usize {
    if ch.is_ascii() {
        return usize::from(!ch.is_ascii_control());
    }
    Span::raw(ch.to_string()).width()
}

/// Consumes one word at the start of `s` — printable runes up to the next
/// space/newline, with escape sequences riding along — and returns it with
/// the remainder.
fn take_word(s: &str) -> (&str, &str) {
    let mut end = 0;
    while end < s.len() {
        if let Some((seq, _)) = scan_escape(&s[end..]) {
            end += seq.len();
            continue;
        }
        let ch = s[end..].chars().next().unwrap_or('\u{FFFD}');
        if ch == ' ' || ch == '\n' {
            break;
        }
        end += ch.len_utf8();
    }
    s.split_at(end)
}

/// Word-wraps an ANSI-styled string to `width` columns (Go `reflow` on the
/// glamour output): escape sequences are zero-width passthroughs, words break
/// greedily at spaces, and an over-wide word hard-splits. Existing newlines
/// are preserved.
fn wrap_ansi(source: &str, width: u16) -> String {
    let width = usize::from(width.max(1));
    let mut out = String::with_capacity(source.len());
    let mut col = 0_usize;
    let mut rest = source;
    while !rest.is_empty() {
        if let Some((seq, tail)) = scan_escape(rest) {
            out.push_str(seq);
            rest = tail;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        if ch == '\n' {
            out.push('\n');
            rest = &rest[1..];
            col = 0;
            continue;
        }
        if ch == ' ' {
            out.push(' ');
            rest = &rest[1..];
            col += 1;
            if col >= width {
                out.push('\n');
                col = 0;
            }
            continue;
        }
        let (word, tail) = take_word(rest);
        rest = tail;
        let word_width = word_display_width(word);
        if col > 0 && col + word_width > width {
            out.push('\n');
            col = 0;
        }
        if word_width <= width {
            out.push_str(word);
            col += word_width;
            continue;
        }
        // Hard-split a word wider than the area, escape sequences included.
        let mut word_rest = word;
        while !word_rest.is_empty() {
            if let Some((seq, tail)) = scan_escape(word_rest) {
                out.push_str(seq);
                word_rest = tail;
                continue;
            }
            let ch = word_rest.chars().next().unwrap_or('\u{FFFD}');
            let w = char_width(ch);
            if col + w > width {
                out.push('\n');
                col = 0;
            }
            out.push(ch);
            word_rest = &word_rest[ch.len_utf8()..];
            col += w;
        }
    }
    out
}

/// Display width of a word that may contain escape sequences.
fn word_display_width(word: &str) -> usize {
    let mut width = 0;
    let mut rest = word;
    while !rest.is_empty() {
        if let Some((_, tail)) = scan_escape(rest) {
            rest = tail;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        width += char_width(ch);
        rest = &rest[ch.len_utf8()..];
    }
    width
}

/// Paints an ANSI-styled string into `area`: SGR sequences update the active
/// style, OSC 8 sequences are attached to the next cell's symbol so the
/// terminal receives them in-band at the right column (a trailing sequence
/// rides the last painted cell), and content past the area is clipped — the
/// caller seals a clipped hyperlink with [`crate::links::close_open_hyperlinks`].
fn paint_ansi(area: Rect, buf: &mut Buffer, source: &str) {
    let mut style = Style::default();
    let mut pending = String::new();
    let mut x = area.x;
    let mut y = area.y;
    let mut last: Option<(u16, u16)> = None;
    let mut rest = source;
    while !rest.is_empty() {
        if let Some((seq, tail)) = scan_escape(rest) {
            if let Some(params) = seq
                .strip_prefix("\x1b[")
                .and_then(|body| body.strip_suffix('m'))
            {
                apply_sgr(&mut style, params);
            } else {
                pending.push_str(seq);
            }
            rest = tail;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        rest = &rest[ch.len_utf8()..];
        if ch == '\n' {
            y = y.saturating_add(1);
            x = area.x;
            if y >= area.bottom() {
                break;
            }
            continue;
        }
        let width = char_width(ch);
        if width == 0 {
            // Zero-width rune (combining mark): ride the previous cell.
            if let Some((lx, ly)) = last {
                let symbol = format!("{}{ch}", buf[(lx, ly)].symbol());
                buf[(lx, ly)].set_symbol(&symbol);
            }
            continue;
        }
        let w = u16::try_from(width).unwrap_or(u16::MAX);
        if x.saturating_add(w) > area.right() {
            y = y.saturating_add(1);
            x = area.x;
            if y >= area.bottom() {
                break;
            }
        }
        let smuggled = !pending.is_empty();
        let symbol = if smuggled {
            std::mem::take(&mut pending) + &ch.to_string()
        } else {
            ch.to_string()
        };
        let cell = buf[(x, y)].set_symbol(&symbol).set_style(style);
        if smuggled {
            // The escape bytes are zero-width on the terminal, but the diff
            // measures the symbol string; pin the cell's width so the
            // following cells are not skipped.
            if let Some(forced) = NonZeroU16::new(w) {
                cell.set_diff_option(CellDiffOption::ForcedWidth(forced));
            }
        }
        if width == 2 && x + 1 < area.right() {
            buf[(x + 1, y)].reset();
        }
        last = Some((x, y));
        x = x.saturating_add(w);
    }
    if !pending.is_empty() {
        if let Some((lx, ly)) = last {
            let symbol = format!("{}{pending}", buf[(lx, ly)].symbol());
            buf[(lx, ly)].set_symbol(&symbol);
        }
    }
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

        // The link label is the one cell run the theme may color: it takes the
        // theme's `link_sgr` (default `1;4;38;5;212`). Every markdown
        // structural style stays at the terminal defaults.
        let link_fg = Color::Indexed(212);
        let mut saw_bold = false;
        let mut saw_link = false;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = buffer.cell((x, y)).expect("cell in bounds");
                if cell.fg == link_fg {
                    assert!(
                        cell.modifier
                            .contains(Modifier::BOLD | Modifier::UNDERLINED),
                        "link label carries the theme link SGR at {x},{y}"
                    );
                    saw_link = true;
                    continue;
                }
                assert_eq!(cell.fg, Color::Reset, "body must not set fg at {x},{y}");
                assert_eq!(cell.bg, Color::Reset, "body must not set bg at {x},{y}");
                saw_bold |= cell.modifier.contains(Modifier::BOLD);
            }
        }
        assert!(saw_bold, "heading modifiers must survive color stripping");
        assert!(saw_link, "link label must take the theme link style");

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

    fn link_block() -> PreviewBlock {
        let doc = mdfu_core::parse::parse_content(
            "vault/link.md",
            "See [Charm](https://charm.sh) now.\n",
        );
        PreviewBlock::from(&doc)
    }

    #[test]
    fn widgets_body_panel_emits_osc8_hyperlinks() {
        let panel = BodyPanel::new(&link_block(), &Theme::default());
        let buffer = component_buffer(&panel, 60, 5);
        let text = buffer_lines(&buffer).join("\n");
        assert!(
            text.contains("\x1b]8;;https://charm.sh\x07"),
            "OSC 8 opener emitted for a web link:\n{text:?}"
        );
        assert!(
            text.contains("\x1b]8;;\x07"),
            "OSC 8 reset closes the link:\n{text:?}"
        );
        assert!(text.contains("Charm"), "label renders:\n{text:?}");
        assert!(
            !text.contains("(https://charm.sh)"),
            "URL hidden behind the hyperlink:\n{text:?}"
        );
        // The smuggled escape bytes must not widen the cell for the diff:
        // every escape-carrying cell pins ForcedWidth to its printable width.
        let mut saw_forced = false;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = buffer.cell((x, y)).expect("cell in bounds");
                if cell.symbol().contains('\x1b') {
                    assert_eq!(
                        cell.diff_option,
                        ratatui::buffer::CellDiffOption::ForcedWidth(
                            std::num::NonZeroU16::new(1).expect("nonzero")
                        ),
                        "escape cell at {x},{y} pins its width"
                    );
                    saw_forced = true;
                }
            }
        }
        assert!(saw_forced, "at least the opener and reset cells");
    }

    #[test]
    fn widgets_body_panel_without_hyperlinks_shows_label_url_fallback() {
        let block = link_block().with_hyperlinks(false);
        let panel = BodyPanel::new(&block, &Theme::default());
        let buffer = component_buffer(&panel, 60, 5);
        let text = buffer_lines(&buffer).join("\n");
        assert!(
            text.contains("Charm (https://charm.sh)"),
            "label url fallback renders:\n{text:?}"
        );
        assert!(
            !text.contains("\x1b]8;"),
            "no OSC 8 when hyperlinks are off:\n{text:?}"
        );
    }

    #[test]
    fn widgets_body_panel_seals_a_hyperlink_the_pane_truncates() {
        let doc = mdfu_core::parse::parse_content("vault/link.md", "[Charm](https://charm.sh)\n");
        let block = PreviewBlock::from(&doc);
        let panel = BodyPanel::new(&block, &Theme::default());
        // Width 3 hard-splits the label across rows; height 1 clips the row
        // carrying the reset, so the opener would dangle without sealing.
        let buffer = component_buffer(&panel, 3, 1);
        let text = buffer_lines(&buffer).join("\n");
        assert_eq!(
            text.matches("\x1b]8;;https://charm.sh\x07").count(),
            1,
            "one opener survives the clip:\n{text:?}"
        );
        let last_osc = text.rfind("\x1b]8;").expect("OSC 8 present");
        assert!(
            text[last_osc..].starts_with("\x1b]8;;\x07"),
            "the last OSC 8 is the reset (no dangling opener):\n{text:?}"
        );
    }

    #[test]
    fn widgets_body_panel_emphasizes_query_terms_with_theme_sgr() {
        let doc = mdfu_core::parse::parse_content("vault/t.md", "the parser track\n");
        let block = PreviewBlock::from(&doc).with_terms(vec!["parser".to_owned()]);
        let panel = BodyPanel::new(&block, &Theme::default());
        let buffer = component_buffer(&panel, 40, 3);

        // Default `highlight_sgr` is `1;30;103`: bold, black on bright yellow.
        let mut highlighted = String::new();
        let mut plain_prefix = String::new();
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, 0)).expect("cell in bounds");
            if cell.symbol().is_empty() {
                break;
            }
            if cell.bg == Color::LightYellow {
                assert_eq!(cell.fg, Color::Black, "highlight fg at {x}");
                assert!(
                    cell.modifier.contains(Modifier::BOLD),
                    "highlight bold at {x}"
                );
                highlighted.push_str(cell.symbol());
            } else {
                assert_eq!(cell.fg, Color::Reset, "unmatched fg at {x}");
                assert_eq!(cell.bg, Color::Reset, "unmatched bg at {x}");
                plain_prefix.push_str(cell.symbol());
            }
        }
        assert_eq!(highlighted, "parser");
        assert!(
            plain_prefix.starts_with("the "),
            "unstyled prefix: {plain_prefix:?}"
        );
    }
}
