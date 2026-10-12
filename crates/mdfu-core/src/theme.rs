//! XDG YAML theme configuration merged per key over builtin defaults.
//!
//! Port of `internal/theme/theme.go`. This module owns the TUI theme YAML
//! (`$XDG_CONFIG_HOME/mdfu/config.yaml` or `$MDFU_CONFIG`); it is unrelated to
//! the task engine's JSON config. A missing theme file is never an error: the
//! builtin defaults reproduce the stock TUI look.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use etcetera::base_strategy::{choose_native_strategy, BaseStrategy as _};
use serde::Deserialize;

/// Builtin default color for the cursor row.
const DEFAULT_CURSOR_COLOR: &str = "212";
/// Builtin default color for multi-selected rows.
const DEFAULT_SELECTED_COLOR: &str = "82";
/// Builtin default color for the preview title.
const DEFAULT_TITLE_COLOR: &str = "212";
/// Builtin default color for the preview filename header.
const DEFAULT_FILENAME_COLOR: &str = "244";
/// Builtin default color for frontmatter labels.
const DEFAULT_FRONTMATTER_KEY_COLOR: &str = "245";
/// Builtin default foreground for pills.
const DEFAULT_PILL_FOREGROUND: &str = "231";
/// Builtin default background for pills.
const DEFAULT_PILL_BACKGROUND: &str = "62";
/// Builtin default horizontal padding for pills.
const DEFAULT_PILL_PADDING: u16 = 1;
/// Upper clamp for `pill_padding`.
const MAX_PILL_PADDING: u16 = 2;
/// Builtin default number of preview body lines.
const DEFAULT_BODY_LINES: u16 = 30;
/// Lower clamp for `body_lines`.
const MIN_BODY_LINES: i64 = 1;
/// Upper clamp for `body_lines`.
const MAX_BODY_LINES: i64 = 200;
/// Builtin default SGR parameters for match emphasis.
const DEFAULT_HIGHLIGHT_SGR: &str = "1;30;103";
/// Builtin default SGR parameters for hyperlink labels.
const DEFAULT_LINK_SGR: &str = "1;4;38;5;212";
/// Builtin default color for task IDs.
const DEFAULT_TASK_ID_COLOR: &str = "245";
/// Builtin default color for completed task rows.
const DEFAULT_TASK_DONE_COLOR: &str = "240";
/// Builtin default color for task priority markers.
const DEFAULT_TASK_PRIORITY_COLOR: &str = "212";
/// Builtin default color for task tags.
const DEFAULT_TASK_TAG_COLOR: &str = "62";
/// Builtin default color for blocked task markers.
const DEFAULT_TASK_BLOCKER_COLOR: &str = "203";
/// Builtin default color for the selected task row.
const DEFAULT_TASK_SELECTED_COLOR: &str = "82";
/// Builtin default color for the task pane border.
const DEFAULT_TASK_PANE_BORDER_COLOR: &str = "238";

/// Pill border shape (`pill_shape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PillShape {
    /// No border.
    None,
    /// Rounded border (builtin default).
    #[default]
    Round,
}

impl PillShape {
    /// The config spelling of this shape.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Round => "round",
        }
    }
}

/// Glamour-style selection for the preview body (`markdown_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownStyle {
    /// Auto-detect the terminal background (builtin default).
    #[default]
    Auto,
    /// Force the dark style.
    Dark,
    /// Force the light style.
    Light,
}

impl MarkdownStyle {
    /// The config spelling of this style; auto is the empty string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Task pane border shape (`task_pane_border`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneBorder {
    /// No border.
    #[default]
    None,
    /// Rounded border (builtin default).
    Rounded,
}

impl PaneBorder {
    /// The config spelling of this border.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rounded => "rounded",
        }
    }
}

/// A resolved, renderer-neutral text style.
///
/// Colors stay as the exact strings from the config (`212`, `#ff0000`, ...) so
/// the TUI can map them onto ratatui colors. `horizontal_padding` is only ever
/// set on [`Theme::pill`]; `border` and `border_fg` only on
/// [`Theme::task_pane`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    /// Foreground color string, if any.
    pub fg: Option<String>,
    /// Background color string, if any.
    pub bg: Option<String>,
    /// Render the text bold.
    pub bold: bool,
    /// Render the text faint.
    pub faint: bool,
    /// Underline the text.
    pub underline: bool,
    /// Horizontal (left/right) padding in cells.
    pub horizontal_padding: u16,
    /// Border shape.
    pub border: PaneBorder,
    /// Border color string, if any.
    pub border_fg: Option<String>,
}

/// Resolved set of styles and scalar settings consumed by the TUI.
///
/// Mirrors Go's `theme.Theme`, minus the lipgloss values: styles are plain
/// data, scalars stay readable so callers can emit raw SGR bytes or pick a
/// markdown style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Cursor row in the result list (bold).
    pub cursor: Style,
    /// Multi-selected rows in the result list (bold).
    pub selected: Style,
    /// Faint rendering for dimmed text such as status hints.
    pub dim: Style,
    /// Document title in the preview (bold).
    pub title: Style,
    /// Filename header at the top of the preview.
    pub filename: Style,
    /// Preview header (bold + underline; a color is optional).
    pub preview_header: Style,
    /// Labels of the frontmatter rows.
    pub frontmatter_key: Style,
    /// Tag and list pills.
    pub pill: Style,
    /// Task row ID, e.g. `PRJ-3`.
    pub task_id: Style,
    /// Completed task rows.
    pub task_done: Style,
    /// Task priority marker.
    pub task_priority: Style,
    /// Task tags.
    pub task_tag: Style,
    /// Blocked task marker.
    pub task_blocker: Style,
    /// Selected task row (bold).
    pub task_selected: Style,
    /// Task pane border.
    pub task_pane: Style,
    /// Pill border shape.
    pub pill_shape: PillShape,
    /// Glamour style for the preview body.
    pub markdown_style: MarkdownStyle,
    /// Frontmatter block visible at startup.
    pub show_frontmatter: bool,
    /// Body lines shown in the preview (clamped to `1..=200`).
    pub body_lines: u16,
    /// SGR parameters for match emphasis, e.g. `1;30;103`.
    pub highlight_sgr: String,
    /// SGR parameters for hyperlink labels.
    pub link_sgr: String,
}

/// Returns the builtin theme, which reproduces the stock TUI look when no
/// config file exists.
impl Default for Theme {
    fn default() -> Self {
        Self {
            cursor: bold_colored(DEFAULT_CURSOR_COLOR),
            selected: bold_colored(DEFAULT_SELECTED_COLOR),
            dim: Style {
                faint: true,
                ..Style::default()
            },
            title: bold_colored(DEFAULT_TITLE_COLOR),
            filename: colored(DEFAULT_FILENAME_COLOR),
            preview_header: Style {
                bold: true,
                underline: true,
                ..Style::default()
            },
            frontmatter_key: colored(DEFAULT_FRONTMATTER_KEY_COLOR),
            pill: Style {
                fg: Some(DEFAULT_PILL_FOREGROUND.to_owned()),
                bg: Some(DEFAULT_PILL_BACKGROUND.to_owned()),
                horizontal_padding: DEFAULT_PILL_PADDING,
                ..Style::default()
            },
            task_id: colored(DEFAULT_TASK_ID_COLOR),
            task_done: colored(DEFAULT_TASK_DONE_COLOR),
            task_priority: colored(DEFAULT_TASK_PRIORITY_COLOR),
            task_tag: colored(DEFAULT_TASK_TAG_COLOR),
            task_blocker: colored(DEFAULT_TASK_BLOCKER_COLOR),
            task_selected: bold_colored(DEFAULT_TASK_SELECTED_COLOR),
            task_pane: Style {
                border: PaneBorder::Rounded,
                border_fg: Some(DEFAULT_TASK_PANE_BORDER_COLOR.to_owned()),
                ..Style::default()
            },
            pill_shape: PillShape::Round,
            markdown_style: MarkdownStyle::Auto,
            show_frontmatter: true,
            body_lines: DEFAULT_BODY_LINES,
            highlight_sgr: DEFAULT_HIGHLIGHT_SGR.to_owned(),
            link_sgr: DEFAULT_LINK_SGR.to_owned(),
        }
    }
}

/// Builtin defaults as a commented YAML document, backing `mdfu --config
/// default`: printing it lets a user see every supported key and copy the
/// result to their config path as a starting point.
///
/// Byte-identical to Go's `defaultYAML` (`internal/theme/theme.go:154-209`),
/// including the commented-out `preview_header_color` line; the copy under
/// `tests/fixtures/default-config.yaml` is asserted equal in the tests.
pub const DEFAULT_YAML: &str = r#"# mdfu configuration — builtin defaults.
#
# Copy this file to $XDG_CONFIG_HOME/mdfu/config.yaml (or pass it via --config
# / $MDFU_CONFIG) and edit. Every key is optional: omitted keys keep the
# builtin defaults shown here. Unknown keys are ignored, invalid enum values
# keep the default, and numeric values are clamped to their supported range.

# Glamour style for the preview body: dark | light (empty = auto).
markdown_style: ""
# Frontmatter block visible at startup (ctrl+f toggles it live).
show_frontmatter: true
# Body lines shown in the preview (1..200).
body_lines: 30
# Cursor row in the result list (bold).
cursor_color: "212"
# Multi-selected rows in the result list (bold).
selected_color: "82"
# Document title in the preview (bold).
title_color: "212"
# Filename header at the top of the preview.
filename_color: "244"
# Preview header style. Unset means bold + underline; setting a color adds it.
# preview_header_color: "212"
# Labels of the frontmatter rows.
frontmatter_key_color: "245"
# Faint rendering for dimmed text such as status hints.
dim_faint: true
# Tag/list pill text.
pill_foreground: "231"
# Tag/list pill background.
pill_background: "62"
# Pill border shape: none | round.
pill_shape: round
# Horizontal padding inside pills (0..2).
pill_padding: 1
# Query-match emphasis (bold, black on bright yellow).
highlight_sgr: "1;30;103"
# Hyperlink labels in the preview.
link_sgr: "1;4;38;5;212"
# Task row ID, e.g. PRJ-3.
task_id_color: "245"
# Completed task rows.
task_done_color: "240"
# Task priority marker.
task_priority_color: "212"
# Task tags.
task_tag_color: "62"
# Blocked task marker.
task_blocker_color: "203"
# Selected task row (bold).
task_selected_color: "82"
# Task pane border color.
task_pane_border_color: "238"
# Task pane border shape: none | rounded.
task_pane_border: rounded
"#;

/// Returns [`DEFAULT_YAML`].
#[must_use]
pub const fn default_yaml() -> &'static str {
    DEFAULT_YAML
}

/// Raw YAML file shape: every field is optional so an absent key keeps the
/// builtin default. Unknown keys are ignored, matching yaml.v3.
#[derive(Debug, Deserialize)]
struct FileConfig {
    markdown_style: Option<String>,
    show_frontmatter: Option<bool>,
    body_lines: Option<i64>,
    cursor_color: Option<String>,
    selected_color: Option<String>,
    title_color: Option<String>,
    filename_color: Option<String>,
    preview_header_color: Option<String>,
    frontmatter_key_color: Option<String>,
    dim_faint: Option<bool>,
    pill_foreground: Option<String>,
    pill_background: Option<String>,
    pill_shape: Option<String>,
    pill_padding: Option<i64>,
    highlight_sgr: Option<String>,
    link_sgr: Option<String>,
    task_id_color: Option<String>,
    task_done_color: Option<String>,
    task_priority_color: Option<String>,
    task_tag_color: Option<String>,
    task_blocker_color: Option<String>,
    task_selected_color: Option<String>,
    task_pane_border_color: Option<String>,
    task_pane_border: Option<String>,
}

/// Reads the YAML theme at `path` and merges it per key over [`Theme::default`].
///
/// Unknown keys are ignored; invalid enum values keep the builtin default and
/// numeric settings are clamped into their supported ranges.
///
/// # Errors
///
/// Returns [`Error::Read`] when the file cannot be read (including a missing
/// path), and [`Error::Parse`] when the YAML is malformed or a value has the
/// wrong type.
pub fn load_file(path: &Path) -> Result<Theme, Error> {
    let data = std::fs::read(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let config: FileConfig = yaml_serde::from_slice(&data).map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(config.apply(Theme::default()))
}

/// Locates the default theme file `<user config dir>/mdfu/config.yaml` and
/// returns it only when it exists and is not a directory.
///
/// The user config directory follows the OS-native convention, matching Go's
/// `os.UserConfigDir`: `$XDG_CONFIG_HOME` (or `~/.config`) on Linux and other
/// Unixes, `~/Library/Application Support` on macOS, and the roaming app data
/// directory on Windows.
#[must_use]
pub fn discover() -> Option<PathBuf> {
    let strategy = choose_native_strategy().ok()?;
    let path = strategy.config_dir().join("mdfu").join("config.yaml");
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Picks the theme by precedence: `explicit` > `$MDFU_CONFIG` > discovered
/// XDG file > builtin defaults. An explicitly requested or `$MDFU_CONFIG`
/// file that is missing, unreadable, or malformed is an error; a malformed
/// discovered file downgrades to the builtin defaults with exactly one
/// warning line written to `warn`.
///
/// # Errors
///
/// Returns [`Error::Read`] or [`Error::Parse`] when the explicit path or
/// `$MDFU_CONFIG` cannot be loaded.
pub fn resolve(explicit: Option<&Path>, warn: &mut dyn Write) -> Result<Theme, Error> {
    resolve_with(explicit, |key| std::env::var(key).ok(), discover, warn)
}

/// [`resolve`] with injected environment and discovery lookups, so tests do
/// not have to mutate the process environment.
fn resolve_with<E, D>(
    explicit: Option<&Path>,
    env_get: E,
    discover: D,
    warn: &mut dyn Write,
) -> Result<Theme, Error>
where
    E: FnOnce(&str) -> Option<String>,
    D: FnOnce() -> Option<PathBuf>,
{
    let explicit = explicit.filter(|path| !path.as_os_str().is_empty());
    let (path, discovered) = if let Some(path) = explicit {
        (Some(path.to_path_buf()), false)
    } else if let Some(path) = env_get("MDFU_CONFIG").filter(|value| !value.is_empty()) {
        (Some(PathBuf::from(path)), false)
    } else if let Some(path) = discover() {
        (Some(path), true)
    } else {
        (None, false)
    };

    let Some(path) = path else {
        return Ok(Theme::default());
    };

    match load_file(&path) {
        Ok(theme) => Ok(theme),
        Err(error) if discovered => {
            // Best effort: a failing writer must not turn the valid builtin
            // fallback into an error.
            let _ = writeln!(
                warn,
                "mdfu: ignoring theme file {}: {error}",
                path.display()
            );
            Ok(Theme::default())
        }
        Err(error) => Err(error),
    }
}

impl FileConfig {
    #[must_use]
    fn apply(&self, mut theme: Theme) -> Theme {
        self.apply_text_styles(&mut theme);
        self.apply_pill(&mut theme);
        self.apply_scalars(&mut theme);
        self.apply_task_styles(&mut theme);
        theme
    }

    fn apply_text_styles(&self, theme: &mut Theme) {
        if let Some(color) = self.cursor_color.as_deref() {
            theme.cursor = bold_colored(color);
        }
        if let Some(color) = self.selected_color.as_deref() {
            theme.selected = bold_colored(color);
        }
        if let Some(color) = self.title_color.as_deref() {
            theme.title = bold_colored(color);
        }
        if let Some(color) = self.filename_color.as_deref() {
            theme.filename = colored(color);
        }
        if let Some(color) = self.preview_header_color.as_deref() {
            theme.preview_header = Style {
                fg: Some(color.to_owned()),
                bold: true,
                underline: true,
                ..Style::default()
            };
        }
        if let Some(color) = self.frontmatter_key_color.as_deref() {
            theme.frontmatter_key = colored(color);
        }
        if let Some(faint) = self.dim_faint {
            theme.dim = Style {
                faint,
                ..Style::default()
            };
        }
    }

    fn apply_pill(&self, theme: &mut Theme) {
        if self.pill_foreground.is_some()
            || self.pill_background.is_some()
            || self.pill_padding.is_some()
        {
            let fg = self
                .pill_foreground
                .as_deref()
                .unwrap_or(DEFAULT_PILL_FOREGROUND);
            let bg = self
                .pill_background
                .as_deref()
                .unwrap_or(DEFAULT_PILL_BACKGROUND);
            let padding = self.pill_padding.map_or(DEFAULT_PILL_PADDING, |value| {
                u16::try_from(value.clamp(0, i64::from(MAX_PILL_PADDING)))
                    .unwrap_or(DEFAULT_PILL_PADDING)
            });
            theme.pill = Style {
                fg: Some(fg.to_owned()),
                bg: Some(bg.to_owned()),
                horizontal_padding: padding,
                ..Style::default()
            };
        }
        if let Some(shape) = self.pill_shape.as_deref() {
            match shape {
                "none" => theme.pill_shape = PillShape::None,
                "round" => theme.pill_shape = PillShape::Round,
                _ => {}
            }
        }
    }

    fn apply_scalars(&self, theme: &mut Theme) {
        if let Some(style) = self.markdown_style.as_deref() {
            match style {
                "" => theme.markdown_style = MarkdownStyle::Auto,
                "dark" => theme.markdown_style = MarkdownStyle::Dark,
                "light" => theme.markdown_style = MarkdownStyle::Light,
                _ => {}
            }
        }
        if let Some(show) = self.show_frontmatter {
            theme.show_frontmatter = show;
        }
        if let Some(lines) = self.body_lines {
            theme.body_lines = u16::try_from(lines.clamp(MIN_BODY_LINES, MAX_BODY_LINES))
                .unwrap_or(DEFAULT_BODY_LINES);
        }
        if let Some(sgr) = self.highlight_sgr.as_deref() {
            sgr.clone_into(&mut theme.highlight_sgr);
        }
        if let Some(sgr) = self.link_sgr.as_deref() {
            sgr.clone_into(&mut theme.link_sgr);
        }
    }

    fn apply_task_styles(&self, theme: &mut Theme) {
        if let Some(color) = self.task_id_color.as_deref() {
            theme.task_id = colored(color);
        }
        if let Some(color) = self.task_done_color.as_deref() {
            theme.task_done = colored(color);
        }
        if let Some(color) = self.task_priority_color.as_deref() {
            theme.task_priority = colored(color);
        }
        if let Some(color) = self.task_tag_color.as_deref() {
            theme.task_tag = colored(color);
        }
        if let Some(color) = self.task_blocker_color.as_deref() {
            theme.task_blocker = colored(color);
        }
        if let Some(color) = self.task_selected_color.as_deref() {
            theme.task_selected = bold_colored(color);
        }
        if self.task_pane_border_color.is_some() || self.task_pane_border.is_some() {
            let color = self
                .task_pane_border_color
                .as_deref()
                .unwrap_or(DEFAULT_TASK_PANE_BORDER_COLOR);
            let border = match self.task_pane_border.as_deref() {
                Some("none") => PaneBorder::None,
                _ => PaneBorder::Rounded,
            };
            theme.task_pane = if border == PaneBorder::None {
                // Go's taskPaneStyle("none", _) drops the border color too.
                Style::default()
            } else {
                Style {
                    border,
                    border_fg: Some(color.to_owned()),
                    ..Style::default()
                }
            };
        }
    }
}

/// A style with a bold foreground color.
#[must_use]
fn bold_colored(color: &str) -> Style {
    Style {
        fg: Some(color.to_owned()),
        bold: true,
        ..Style::default()
    }
}

/// A style with a plain foreground color.
#[must_use]
fn colored(color: &str) -> Style {
    Style {
        fg: Some(color.to_owned()),
        ..Style::default()
    }
}

/// A theme file load or parse failure.
#[derive(Debug)]
pub enum Error {
    /// The theme file could not be read (for example because it is missing).
    Read {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The theme file is not valid YAML or has a mismatched value type.
    Parse {
        /// Path that was parsed.
        path: PathBuf,
        /// Underlying YAML error.
        source: yaml_serde::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "theme: read {}: {source}", path.display()),
            Self::Parse { path, source } => write!(f, "theme: parse {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/default-config.yaml"
    ));

    static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("mdfu-theme-{}-{seq}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn theme_from_yaml(yaml: &str) -> Theme {
        let config: FileConfig = yaml_serde::from_str(yaml).unwrap();
        config.apply(Theme::default())
    }

    fn warning_text(warn: &[u8]) -> String {
        String::from_utf8(warn.to_vec()).unwrap()
    }

    #[test]
    fn defaults_match_builtin_look() {
        let theme = Theme::default();

        assert_eq!(theme.cursor.fg.as_deref(), Some("212"));
        assert!(theme.cursor.bold);
        assert_eq!(theme.selected.fg.as_deref(), Some("82"));
        assert!(theme.selected.bold);
        assert!(theme.dim.faint);
        assert!(theme.dim.fg.is_none());
        assert_eq!(theme.title.fg.as_deref(), Some("212"));
        assert!(theme.title.bold);
        assert_eq!(theme.filename.fg.as_deref(), Some("244"));
        assert!(!theme.filename.bold);
        assert!(theme.preview_header.bold);
        assert!(theme.preview_header.underline);
        assert!(theme.preview_header.fg.is_none());
        assert_eq!(theme.frontmatter_key.fg.as_deref(), Some("245"));

        assert_eq!(theme.pill.fg.as_deref(), Some("231"));
        assert_eq!(theme.pill.bg.as_deref(), Some("62"));
        assert_eq!(theme.pill.horizontal_padding, 1);

        assert_eq!(theme.task_id.fg.as_deref(), Some("245"));
        assert_eq!(theme.task_done.fg.as_deref(), Some("240"));
        assert_eq!(theme.task_priority.fg.as_deref(), Some("212"));
        assert_eq!(theme.task_tag.fg.as_deref(), Some("62"));
        assert_eq!(theme.task_blocker.fg.as_deref(), Some("203"));
        assert_eq!(theme.task_selected.fg.as_deref(), Some("82"));
        assert!(theme.task_selected.bold);
        assert_eq!(theme.task_pane.border, PaneBorder::Rounded);
        assert_eq!(theme.task_pane.border_fg.as_deref(), Some("238"));

        assert_eq!(theme.pill_shape, PillShape::Round);
        assert_eq!(theme.markdown_style, MarkdownStyle::Auto);
        assert!(theme.show_frontmatter);
        assert_eq!(theme.body_lines, 30);
        assert_eq!(theme.highlight_sgr, "1;30;103");
        assert_eq!(theme.link_sgr, "1;4;38;5;212");
    }

    #[test]
    fn fixture_is_byte_identical_to_embedded_default() {
        assert_eq!(DEFAULT_YAML, FIXTURE);
        assert_eq!(default_yaml(), DEFAULT_YAML);
        assert!(DEFAULT_YAML.contains("# preview_header_color: \"212\""));
        assert!(!DEFAULT_YAML.contains("\npreview_header_color:"));
    }

    #[test]
    fn default_yaml_loads_to_builtin_defaults() {
        let dir = TempDir::new();
        let path = dir.write("config.yaml", DEFAULT_YAML);
        let loaded = load_file(&path).unwrap();
        assert_eq!(loaded, Theme::default());
    }

    #[test]
    fn partial_override_merges_over_defaults() {
        let theme = theme_from_yaml("cursor_color: \"160\"\nbody_lines: 12\n");
        assert_eq!(theme.cursor.fg.as_deref(), Some("160"));
        assert!(theme.cursor.bold);
        assert_eq!(theme.body_lines, 12);
        assert_eq!(theme.selected.fg.as_deref(), Some("82"));
        assert_eq!(theme.title.fg.as_deref(), Some("212"));
        assert!(theme.show_frontmatter);
        assert_eq!(theme.highlight_sgr, "1;30;103");
        assert_eq!(theme.link_sgr, "1;4;38;5;212");
        assert_eq!(theme.task_pane, Theme::default().task_pane);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let theme = theme_from_yaml("nonsense: true\nnested: {a: 1}\n");
        assert_eq!(theme.body_lines, 30);
        assert_eq!(theme.pill_shape, PillShape::Round);
    }

    #[test]
    fn enum_validation_keeps_defaults_and_applies_valid_values() {
        assert_eq!(
            theme_from_yaml("markdown_style: solarized\n").markdown_style,
            MarkdownStyle::Auto
        );
        assert_eq!(
            theme_from_yaml("markdown_style: dark\n").markdown_style,
            MarkdownStyle::Dark
        );
        assert_eq!(
            theme_from_yaml("markdown_style: light\n").markdown_style,
            MarkdownStyle::Light
        );
        assert_eq!(
            theme_from_yaml("markdown_style: \"\"\n").markdown_style,
            MarkdownStyle::Auto
        );
        assert_eq!(
            theme_from_yaml("pill_shape: square\n").pill_shape,
            PillShape::Round
        );
        assert_eq!(
            theme_from_yaml("pill_shape: none\n").pill_shape,
            PillShape::None
        );
    }

    #[test]
    fn enum_spellings_round_trip() {
        assert_eq!(PillShape::None.as_str(), "none");
        assert_eq!(PillShape::Round.as_str(), "round");
        assert_eq!(MarkdownStyle::Auto.as_str(), "");
        assert_eq!(MarkdownStyle::Dark.as_str(), "dark");
        assert_eq!(MarkdownStyle::Light.as_str(), "light");
        assert_eq!(PaneBorder::None.as_str(), "none");
        assert_eq!(PaneBorder::Rounded.as_str(), "rounded");
    }

    #[test]
    fn body_lines_clamps_to_supported_range() {
        assert_eq!(theme_from_yaml("body_lines: 0\n").body_lines, 1);
        assert_eq!(theme_from_yaml("body_lines: -3\n").body_lines, 1);
        assert_eq!(theme_from_yaml("body_lines: 500\n").body_lines, 200);
        assert_eq!(theme_from_yaml("body_lines: 200\n").body_lines, 200);
    }

    #[test]
    fn pill_padding_clamps_to_supported_range() {
        assert_eq!(
            theme_from_yaml("pill_padding: 9\n").pill.horizontal_padding,
            2
        );
        assert_eq!(
            theme_from_yaml("pill_padding: 0\n").pill.horizontal_padding,
            0
        );
        assert_eq!(
            theme_from_yaml("pill_padding: -1\n")
                .pill
                .horizontal_padding,
            0
        );
    }

    #[test]
    fn colors_flags_pills_and_sgr_override() {
        let theme = theme_from_yaml(
            "title_color: \"#ff0000\"\ndim_faint: false\nshow_frontmatter: false\n\
             preview_header_color: \"99\"\npill_foreground: \"16\"\npill_background: \"220\"\n\
             highlight_sgr: \"7\"\nlink_sgr: \"4\"\n",
        );
        assert_eq!(theme.title.fg.as_deref(), Some("#ff0000"));
        assert!(!theme.dim.faint);
        assert!(!theme.show_frontmatter);
        assert_eq!(theme.preview_header.fg.as_deref(), Some("99"));
        assert!(theme.preview_header.bold);
        assert!(theme.preview_header.underline);
        assert_eq!(theme.pill.fg.as_deref(), Some("16"));
        assert_eq!(theme.pill.bg.as_deref(), Some("220"));
        assert_eq!(theme.pill.horizontal_padding, 1);
        assert_eq!(theme.highlight_sgr, "7");
        assert_eq!(theme.link_sgr, "4");
    }

    #[test]
    fn task_color_keys_override() {
        let theme = theme_from_yaml(
            "task_id_color: \"1\"\ntask_done_color: \"2\"\ntask_priority_color: \"3\"\n\
             task_tag_color: \"4\"\ntask_blocker_color: \"5\"\ntask_selected_color: \"6\"\n\
             task_pane_border_color: \"7\"\n",
        );
        assert_eq!(theme.task_id.fg.as_deref(), Some("1"));
        assert_eq!(theme.task_done.fg.as_deref(), Some("2"));
        assert_eq!(theme.task_priority.fg.as_deref(), Some("3"));
        assert_eq!(theme.task_tag.fg.as_deref(), Some("4"));
        assert_eq!(theme.task_blocker.fg.as_deref(), Some("5"));
        assert_eq!(theme.task_selected.fg.as_deref(), Some("6"));
        assert!(theme.task_selected.bold);
        assert_eq!(theme.task_pane.border, PaneBorder::Rounded);
        assert_eq!(theme.task_pane.border_fg.as_deref(), Some("7"));
    }

    #[test]
    fn task_pane_border_variants() {
        let default = Theme::default();
        assert_eq!(
            theme_from_yaml("task_pane_border: octagon\n").task_pane,
            default.task_pane
        );

        let rounded = theme_from_yaml("task_pane_border: rounded\n");
        assert_eq!(rounded.task_pane.border, PaneBorder::Rounded);
        assert_eq!(rounded.task_pane.border_fg.as_deref(), Some("238"));

        let none = theme_from_yaml("task_pane_border: none\n");
        assert_eq!(none.task_pane, Style::default());

        let none_with_color =
            theme_from_yaml("task_pane_border: none\ntask_pane_border_color: \"7\"\n");
        assert_eq!(none_with_color.task_pane, Style::default());
    }

    #[test]
    fn load_file_reports_missing_and_malformed_files() {
        let dir = TempDir::new();
        let missing = load_file(&dir.path().join("missing.yaml"));
        assert!(matches!(missing, Err(Error::Read { .. })));

        let path = dir.write("malformed.yaml", "markdown_style: [unclosed\n");
        let malformed = load_file(&path);
        assert!(matches!(malformed, Err(Error::Parse { .. })));
    }

    #[test]
    fn resolve_without_any_source_returns_defaults() {
        let mut warn = Vec::new();
        let theme = resolve_with(None, |_| None, || None, &mut warn).unwrap();
        assert_eq!(theme, Theme::default());
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_explicit_missing_path_is_error() {
        let dir = TempDir::new();
        let mut warn = Vec::new();
        let error = resolve_with(
            Some(&dir.path().join("missing.yaml")),
            |_| None,
            || None,
            &mut warn,
        )
        .unwrap_err();
        assert!(matches!(error, Error::Read { .. }));
        assert!(error.to_string().starts_with("theme: read "));
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_explicit_malformed_file_is_error() {
        let dir = TempDir::new();
        let path = dir.write("config.yaml", "markdown_style: [unclosed\n");
        let mut warn = Vec::new();
        let error = resolve_with(Some(&path), |_| None, || None, &mut warn).unwrap_err();
        assert!(matches!(error, Error::Parse { .. }));
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_env_file_is_used_and_beats_discovered() {
        let dir = TempDir::new();
        let env_path = dir.write("env.yaml", "body_lines: 22\n");
        let discovered_path = dir.write("discovered.yaml", "body_lines: 11\n");

        let mut warn = Vec::new();
        let env_display = env_path.display().to_string();
        let theme = resolve_with(
            None,
            |key| {
                if key == "MDFU_CONFIG" {
                    Some(env_display.clone())
                } else {
                    None
                }
            },
            || Some(discovered_path.clone()),
            &mut warn,
        )
        .unwrap();
        assert_eq!(theme.body_lines, 22);
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_empty_env_value_falls_back_to_discovered() {
        let dir = TempDir::new();
        let discovered_path = dir.write("discovered.yaml", "body_lines: 11\n");
        let mut warn = Vec::new();
        let theme = resolve_with(
            None,
            |_| Some(String::new()),
            || Some(discovered_path.clone()),
            &mut warn,
        )
        .unwrap();
        assert_eq!(theme.body_lines, 11);
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_env_missing_file_is_error_not_warning() {
        let dir = TempDir::new();
        let env_path = dir.path().join("missing.yaml");
        let mut warn = Vec::new();
        let error = resolve_with(
            None,
            |_| Some(env_path.display().to_string()),
            || None,
            &mut warn,
        )
        .unwrap_err();
        assert!(matches!(error, Error::Read { .. }));
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_explicit_beats_env() {
        let dir = TempDir::new();
        let env_path = dir.write("env.yaml", "body_lines: 22\n");
        let explicit_path = dir.write("explicit.yaml", "body_lines: 33\n");
        let mut warn = Vec::new();
        let env_display = env_path.display().to_string();
        let theme = resolve_with(
            Some(&explicit_path),
            |_| Some(env_display.clone()),
            || None,
            &mut warn,
        )
        .unwrap();
        assert_eq!(theme.body_lines, 33);
    }

    #[test]
    fn resolve_with_empty_explicit_path_falls_through() {
        let dir = TempDir::new();
        let env_path = dir.write("env.yaml", "body_lines: 22\n");
        let mut warn = Vec::new();
        let env_display = env_path.display().to_string();
        let theme = resolve_with(
            Some(Path::new("")),
            |_| Some(env_display.clone()),
            || None,
            &mut warn,
        )
        .unwrap();
        assert_eq!(theme.body_lines, 22);
    }

    #[test]
    fn resolve_with_discovered_file_is_loaded() {
        let dir = TempDir::new();
        let path = dir.write("config.yaml", "pill_shape: none\n");
        let mut warn = Vec::new();
        let theme = resolve_with(None, |_| None, || Some(path.clone()), &mut warn).unwrap();
        assert_eq!(theme.pill_shape, PillShape::None);
        assert!(warn.is_empty());
    }

    #[test]
    fn resolve_with_discovered_malformed_file_warns_once_and_falls_back() {
        let dir = TempDir::new();
        let path = dir.write("config.yaml", "markdown_style: [unclosed\n");
        let mut warn = Vec::new();
        let theme = resolve_with(None, |_| None, || Some(path.clone()), &mut warn).unwrap();

        assert_eq!(theme, Theme::default());
        let text = warning_text(&warn);
        assert_eq!(text.matches('\n').count(), 1);
        assert!(text.contains("mdfu: ignoring theme file"));
        assert!(text.contains(&path.display().to_string()));
        assert!(text.contains("theme: parse"));
    }

    #[test]
    fn resolve_public_wrapper_reports_explicit_failure() {
        let dir = TempDir::new();
        let mut warn = Vec::new();
        let error = resolve(Some(&dir.path().join("missing.yaml")), &mut warn).unwrap_err();
        assert!(matches!(error, Error::Read { .. }));
    }
}
