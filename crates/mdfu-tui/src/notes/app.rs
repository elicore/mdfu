//! The picker model (`App`) and its message handling.
//!
//! The struct layout mirrors Go `tui.Model` in `internal/tui/tui.go`: `items`
//! holds every loaded document, `filtered` the visible subset in display
//! order, and the remaining fields track mode, navigation, toggles, and the
//! transient status message.

use std::collections::{BTreeSet, HashMap};

use mdfu_core::model::Document;
use mdfu_core::theme::Theme;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::links;
use crate::preview::{self, PreviewState};
use crate::view_model::PreviewBlock;
use crate::widgets::SearchInput;

/// One filter hit: the item index plus the query's matched byte offsets into
/// that item's title, in ascending order (empty when the filter matched
/// elsewhere, or when the filter does not compute positions, like
/// [`default_filter`]).
///
/// Indices are the picker's stable identity: they survive re-filtering, key
/// multi-selection, and map straight back to the source [`Document`] for the
/// preview and editor flows. Go passes `Item` values around and keys them by
/// `path:` strings; the index form is the Rust equivalent that avoids a second
/// identity table. The positions feed the list's match emphasis
/// (`search::rank_with_matches` computes them; the theme's `highlight_sgr`
/// styles them at render time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterHit {
    /// Index into the item slice passed to the filter.
    pub index: usize,
    /// Ascending byte offsets of the matched characters in the item's title.
    pub matched_indexes: Vec<usize>,
}

/// Injected query filter: maps the current query plus the full item snapshot
/// to the **hits** of the documents to display, in display order.
///
/// The filter receives the App's *current* items (not a captured copy), so a
/// re-parse that replaces a document is visible to the next filter call. An
/// empty query must return every index. The constructor is the only way to
/// install a filter: there is no global/static fallback.
pub type FilterFn = Box<dyn Fn(&str, &[Document]) -> Vec<FilterHit>>;

/// Injected re-parse hook used after an external editor exits (Go
/// `ReloadFunc`). The picker never imports `mdfu_core::parse`; the CLI wires
/// this to it.
pub type ReloadFn = Box<dyn Fn(&str) -> Result<Document, String>>;

/// Picker configuration (Go `tui.Config`).
#[derive(Default)]
pub struct Config {
    /// Maximum number of visible results; `0` means unlimited.
    pub limit: usize,
    /// Start with archived documents visible (`ctrl+a` toggles at runtime).
    pub show_archived: bool,
    /// Start with the preview pane enabled (`ctrl+p` toggles at runtime).
    pub preview: bool,
    /// Disable OSC 8 hyperlinks in the preview (`--no-hyperlinks`).
    pub no_hyperlinks: bool,
    /// Theme override; `None` resolves to [`Theme::default`].
    pub theme: Option<Theme>,
    /// Frontmatter panel override; `None` keeps the theme default (`ctrl+f`
    /// toggles at runtime).
    pub show_frontmatter: Option<bool>,
    /// Re-parse hook for the external editor flow; `None` disables reloading.
    pub reload: Option<ReloadFn>,
    /// Resolved `$EDITOR` command line. `None` or blank reports
    /// `"$EDITOR is not set"` on `e`, exactly like the Go picker; the CLI
    /// resolves it from the process environment so this crate stays
    /// env-agnostic.
    pub editor: Option<String>,
}

/// Messages the picker reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// A key press.
    Key(KeyEvent),
    /// Terminal size report.
    Resize { width: u16, height: u16 },
    /// The injected editor process finished.
    EditorFinished { path: String, error: Option<String> },
}

/// Effects the picker asks its driver to perform after an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Leave the picker loop; inspect `is_confirmed`/`is_aborted`/`result`.
    Quit,
    /// Open a URL in the browser (`ctrl+o`).
    OpenUrl(String),
    /// Suspend and run the editor with this argv (`e`), then feed back a
    /// [`Msg::EditorFinished`]. Run it through
    /// [`exec_editor`](crate::notes::exec_editor), which suspends the terminal,
    /// waits for the editor, and resumes.
    ExecEditor(Vec<String>),
}

/// One classified key press: the shortcuts the picker handles, plus the
/// catch-all that search mode forwards to the text input.
enum Key {
    Up,
    Down,
    Enter,
    Esc,
    Tab,
    Ctrl(char),
    Char(char),
    Other,
}

/// Classifies a key event the way bubbletea's `KeyMsg.String()` groups keys.
///
/// A ctrl-modified character compares as its lowercase form (`ctrl+k`), a plain
/// character as itself, and anything else as the shortcut-free `Other` (the
/// list-mode allowlist drops it).
fn classify(event: &KeyEvent) -> Key {
    match (event.code, event.modifiers.contains(KeyModifiers::CONTROL)) {
        (KeyCode::Up, _) => Key::Up,
        (KeyCode::Down, _) => Key::Down,
        (KeyCode::Enter, _) => Key::Enter,
        (KeyCode::Esc, _) => Key::Esc,
        (KeyCode::Tab, _) => Key::Tab,
        (KeyCode::Char(c), true) => Key::Ctrl(c.to_ascii_lowercase()),
        (KeyCode::Char(c), false) => Key::Char(c),
        _ => Key::Other,
    }
}

/// The note picker state (Go `tui.Model`).
///
/// The bool count is the frozen Go model shape (parity), not a design smell;
/// collapsing the independent toggles into enums would change the public API
/// for no behavioral gain.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub(crate) items: Vec<Document>,
    pub(crate) filtered: Vec<usize>,
    /// Match positions per visible item index, rebuilt with `filtered` on
    /// every [`App::refilter`]; drives the list's match emphasis.
    pub(crate) matched: HashMap<usize, Vec<usize>>,
    pub(crate) filter: FilterFn,
    pub(crate) input: SearchInput,
    pub(crate) cursor: usize,
    pub(crate) offset: usize,
    pub(crate) selected: BTreeSet<usize>,
    pub(crate) show_preview: bool,
    pub(crate) show_archived: bool,
    pub(crate) limit: usize,
    pub(crate) searching: bool,
    pub(crate) status: String,
    pub(crate) reload: Option<ReloadFn>,
    pub(crate) hyperlinks: bool,
    pub(crate) terms: Vec<String>,
    pub(crate) show_frontmatter: bool,
    /// Preview-pane cache and renderer (todo 22's [`PreviewState`]); its
    /// toggles mirror `show_preview`/`show_frontmatter` so the picker has a
    /// single place that flips them.
    pub(crate) preview: PreviewState,
    pub(crate) theme: Theme,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) confirmed: bool,
    pub(crate) aborted: bool,
    pub(crate) editor: Option<String>,
}

impl App {
    /// Builds the picker around `items` with `filter` injected.
    ///
    /// The picker opens in list mode with an empty query, `filter` applied to
    /// the full item set (Go `NewModelWithFilter`).
    #[must_use]
    pub fn new(items: Vec<Document>, config: Config, filter: FilterFn) -> Self {
        let theme = config.theme.unwrap_or_default();
        let show_frontmatter = config.show_frontmatter.unwrap_or(theme.show_frontmatter);
        let mut input = SearchInput::new();
        // Focused from the start like the Go model; list mode simply never
        // renders or forwards to it until `/` reveals the bar.
        input.focus();
        let mut app = Self {
            items,
            filtered: Vec::new(),
            matched: HashMap::new(),
            filter,
            input,
            cursor: 0,
            offset: 0,
            selected: BTreeSet::new(),
            show_preview: config.preview,
            show_archived: config.show_archived,
            limit: config.limit,
            searching: false,
            status: String::new(),
            reload: config.reload,
            hyperlinks: !config.no_hyperlinks,
            terms: Vec::new(),
            show_frontmatter,
            preview: PreviewState::new(config.preview, show_frontmatter),
            theme,
            width: 0,
            height: 0,
            confirmed: false,
            aborted: false,
            editor: config.editor,
        };
        app.refilter();
        app
    }

    /// Builds the picker with the built-in AND-substring fallback filter (Go
    /// `defaultFilter`). Production wires its own filter; this constructor
    /// keeps the fallback injectable instead of global.
    #[must_use]
    pub fn with_default_filter(items: Vec<Document>, config: Config) -> Self {
        Self::new(items, config, Box::new(default_filter))
    }

    /// Handles one message, returning an effect for the driver when one is
    /// needed (Go `Model.Update` returning a `tea.Cmd`).
    ///
    /// Key handling dispatches on the mode; every key clears the transient
    /// status first, exactly like Go's `updateList`/`updateSearch`.
    #[must_use = "the returned command drives the editor, browser, or quit"]
    pub fn update(&mut self, msg: Msg) -> Option<Cmd> {
        match msg {
            Msg::Key(key) => {
                self.status.clear();
                if self.searching {
                    self.update_search(&key)
                } else {
                    self.update_list(&key)
                }
            }
            Msg::Resize { width, height } => {
                self.width = width;
                self.height = height;
                self.clamp_cursor();
                None
            }
            Msg::EditorFinished { path, error } => self.editor_finished(&path, error.as_deref()),
        }
    }

    /// List-mode keys. Only the allowlist below is handled: up/down or
    /// ctrl-k/ctrl-j navigate, enter confirm, esc clears a non-empty query
    /// before aborting, ctrl-c aborts, tab toggles multi-select, ctrl-a
    /// archived, ctrl-p preview, ctrl-f the frontmatter panel, ctrl-o the
    /// first web link, `/` reveals and focuses the search bar, and `e` opens
    /// the focused document in `$EDITOR` (Go `updateList`).
    fn update_list(&mut self, key: &KeyEvent) -> Option<Cmd> {
        match classify(key) {
            Key::Up | Key::Ctrl('k') => {
                self.move_up();
                None
            }
            Key::Down | Key::Ctrl('j') => {
                self.move_down();
                None
            }
            Key::Enter => {
                self.confirmed = true;
                Some(Cmd::Quit)
            }
            Key::Esc => {
                if self.input.value().is_empty() {
                    self.aborted = true;
                    Some(Cmd::Quit)
                } else {
                    self.input.set_value("");
                    self.refilter();
                    None
                }
            }
            Key::Ctrl('c') => {
                self.aborted = true;
                Some(Cmd::Quit)
            }
            Key::Tab => {
                self.toggle_select();
                None
            }
            Key::Ctrl('a') => {
                self.show_archived = !self.show_archived;
                self.refilter();
                None
            }
            Key::Ctrl('p') => {
                self.toggle_preview();
                None
            }
            Key::Ctrl('f') => {
                self.toggle_frontmatter();
                None
            }
            Key::Ctrl('o') => self.open_first_link(),
            Key::Char('/') => {
                self.searching = true;
                self.input.focus();
                self.clamp_cursor();
                self.ensure_visible();
                None
            }
            Key::Char('e') => self.open_in_editor(),
            // Everything else (printable runes, editing keys) is dropped
            // before it can reach the hidden text input.
            _ => None,
        }
    }

    /// Search-mode keys: esc returns to the list keeping the query, the shared
    /// navigation/toggle/confirm keys stay active, ctrl-c still aborts, and
    /// every other key is forwarded to the text input (Go `updateSearch`).
    fn update_search(&mut self, key: &KeyEvent) -> Option<Cmd> {
        match classify(key) {
            Key::Esc => {
                self.searching = false;
                self.input.blur();
                self.clamp_cursor();
                self.ensure_visible();
                None
            }
            Key::Up | Key::Ctrl('k') => {
                self.move_up();
                None
            }
            Key::Down | Key::Ctrl('j') => {
                self.move_down();
                None
            }
            Key::Enter => {
                self.confirmed = true;
                Some(Cmd::Quit)
            }
            Key::Ctrl('c') => {
                self.aborted = true;
                Some(Cmd::Quit)
            }
            Key::Tab => {
                self.toggle_select();
                None
            }
            Key::Ctrl('a') => {
                self.show_archived = !self.show_archived;
                self.refilter();
                None
            }
            Key::Ctrl('p') => {
                self.toggle_preview();
                None
            }
            Key::Ctrl('f') => {
                self.toggle_frontmatter();
                None
            }
            Key::Ctrl('o') => self.open_first_link(),
            _ => {
                let before = self.input.value().to_owned();
                self.input.handle_event(&Event::Key(*key));
                if self.input.value() != before.as_str() {
                    self.refilter();
                }
                None
            }
        }
    }

    /// Handles a finished editor run (Go `editorFinishedMsg`): an error is
    /// reported through the status bar, a missing or failing reload hook is a
    /// no-op, and a successful re-parse replaces every item with the matching
    /// path before refiltering.
    fn editor_finished(&mut self, path: &str, error: Option<&str>) -> Option<Cmd> {
        if let Some(error) = error {
            error.clone_into(&mut self.status);
            return None;
        }
        let result = {
            let reload = self.reload.as_ref()?;
            reload(path)
        };
        match result {
            Err(message) => {
                self.status = message;
                None
            }
            Ok(doc) => {
                for item in &mut self.items {
                    if item.path == path {
                        *item = doc.clone();
                    }
                }
                self.preview.invalidate(path);
                self.refilter();
                self.status.clear();
                None
            }
        }
    }

    /// Rebuilds the visible set from the current query (Go `refilter`).
    ///
    /// Re-anchors the cursor on the previously focused item when it survives;
    /// otherwise the cursor starts over at the top. Filter output is
    /// defensively bounded to valid item indices so a buggy injected filter
    /// can never make the view index out of bounds.
    fn refilter(&mut self) {
        let previous = self.filtered.get(self.cursor).copied();
        let query = self.input.value().to_owned();
        self.terms = links::query_terms(&query);
        let mut hits: Vec<FilterHit> = (self.filter)(&query, &self.items);
        hits.retain(|hit| hit.index < self.items.len());
        if !self.show_archived {
            hits.retain(|hit| !self.items[hit.index].is_archived());
        }
        if self.limit > 0 && hits.len() > self.limit {
            hits.truncate(self.limit);
        }
        self.matched = hits
            .iter()
            .map(|hit| (hit.index, hit.matched_indexes.clone()))
            .collect();
        self.filtered = hits.into_iter().map(|hit| hit.index).collect();
        if let Some(previous) = previous {
            if let Some(position) = self.filtered.iter().position(|index| *index == previous) {
                self.cursor = position;
            } else {
                self.cursor = 0;
                self.offset = 0;
            }
        }
        self.clamp_cursor();
        self.ensure_visible();
    }

    fn clamp_cursor(&mut self) {
        if self.filtered.is_empty() {
            self.cursor = 0;
            self.offset = 0;
            return;
        }
        if self.cursor >= self.filtered.len() {
            self.cursor = self.filtered.len() - 1;
        }
        if self.offset >= self.filtered.len() {
            self.offset = self.filtered.len() - 1;
        }
    }

    fn ensure_visible(&mut self) {
        let max_rows = self.visible_rows();
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset.saturating_add(max_rows) {
            self.offset = self.cursor - max_rows + 1;
        }
    }

    /// Rows the result list may render (Go `visibleRows`, via the preview
    /// module's shared row-budget math).
    fn visible_rows(&self) -> usize {
        usize::from(preview::visible_rows(
            self.height,
            self.searching,
            self.show_preview,
            self.width,
        ))
    }

    fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
        self.preview.set_show_preview(self.show_preview);
    }

    fn toggle_frontmatter(&mut self) {
        self.show_frontmatter = !self.show_frontmatter;
        self.preview.set_show_frontmatter(self.show_frontmatter);
    }

    fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.cursor = if self.cursor > 0 {
            self.cursor - 1
        } else {
            self.filtered.len() - 1
        };
        self.ensure_visible();
    }

    fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.cursor = if self.cursor < self.filtered.len() - 1 {
            self.cursor + 1
        } else {
            0
        };
        self.ensure_visible();
    }

    fn toggle_select(&mut self) {
        if let Some(&index) = self.filtered.get(self.cursor) {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
        }
    }

    /// The path of the focused item, when it can be opened in an editor.
    fn focused_path(&self) -> Option<&str> {
        let index = *self.filtered.get(self.cursor)?;
        let path = self.items.get(index)?.path.as_str();
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }

    /// `ctrl+o`: opens the focused document's first web link (Go's
    /// `ctrl+o` case + `docFirstLink`).
    fn open_first_link(&mut self) -> Option<Cmd> {
        let index = *self.filtered.get(self.cursor)?;
        let doc = self.items.get(index)?;
        let url = links::doc_first_link(&PreviewBlock::from(doc));
        if url.is_empty() {
            None
        } else {
            Some(Cmd::OpenUrl(url))
        }
    }

    /// `e`: builds the editor argv for the focused document. Without a
    /// focusable file it is a silent no-op; without a usable `$EDITOR` it
    /// reports through the status bar (Go `openInEditor`).
    fn open_in_editor(&mut self) -> Option<Cmd> {
        let path = self.focused_path()?.to_owned();
        match editor_argv(self.editor.as_deref(), &path) {
            Ok(argv) => Some(Cmd::ExecEditor(argv)),
            Err(message) => {
                self.status = message;
                None
            }
        }
    }

    // --- accessors for tests / integration (Go's exported helpers) ---

    /// Current query text.
    #[must_use]
    pub fn query(&self) -> &str {
        self.input.value()
    }

    /// Replaces the query and refilters (headless-friendly, Go `SetQuery`).
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.input.set_value(query);
        self.refilter();
    }

    /// Whether the search bar is visible (search mode).
    #[must_use]
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    /// Whether the search input currently draws its cursor.
    #[must_use]
    pub fn search_focused(&self) -> bool {
        self.input.focused()
    }

    /// The transient status-bar message; empty hides it.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Cursor position within the filtered set.
    #[must_use]
    pub fn cursor_index(&self) -> usize {
        self.cursor
    }

    /// First visible row within the filtered set.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Indices of the currently visible items, in display order.
    #[must_use]
    pub fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    /// Every loaded item.
    #[must_use]
    pub fn items(&self) -> &[Document] {
        &self.items
    }

    /// One loaded item by index.
    #[must_use]
    pub fn item(&self, index: usize) -> Option<&Document> {
        self.items.get(index)
    }

    /// The visible documents in display order.
    #[must_use]
    pub fn filtered_docs(&self) -> Vec<&Document> {
        self.filtered
            .iter()
            .filter_map(|index| self.items.get(*index))
            .collect()
    }

    /// Multi-selected item indices in filtered-display order, plus any
    /// selected items hidden by the current filter appended in original order
    /// (Go `SelectedItems`).
    #[must_use]
    pub fn selected_indices(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .filtered
            .iter()
            .copied()
            .filter(|index| self.selected.contains(index))
            .collect();
        let mut seen: BTreeSet<usize> = out.iter().copied().collect();
        for index in 0..self.items.len() {
            if self.selected.contains(&index) && seen.insert(index) {
                out.push(index);
            }
        }
        out
    }

    /// Multi-selected documents (Go `SelectedItems`).
    #[must_use]
    pub fn selected_items(&self) -> Vec<&Document> {
        self.selected_indices()
            .iter()
            .filter_map(|index| self.items.get(*index))
            .collect()
    }

    /// Final selection indices: the multi-selection when non-empty, else the
    /// cursor item when confirmed (Go `Result`).
    #[must_use]
    pub fn result_indices(&self) -> Vec<usize> {
        let selected = self.selected_indices();
        if !selected.is_empty() {
            return selected;
        }
        if self.confirmed {
            if let Some(&index) = self.filtered.get(self.cursor) {
                return vec![index];
            }
        }
        Vec::new()
    }

    /// Final selection documents (Go `Result`).
    #[must_use]
    pub fn result(&self) -> Vec<&Document> {
        self.result_indices()
            .iter()
            .filter_map(|index| self.items.get(*index))
            .collect()
    }

    /// Whether the user aborted (esc or ctrl-c).
    #[must_use]
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// Whether the user confirmed (enter).
    #[must_use]
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Archived visibility.
    #[must_use]
    pub fn show_archived(&self) -> bool {
        self.show_archived
    }

    /// Preview-pane visibility.
    #[must_use]
    pub fn show_preview(&self) -> bool {
        self.show_preview
    }

    /// Frontmatter-panel visibility.
    #[must_use]
    pub fn show_frontmatter(&self) -> bool {
        self.show_frontmatter
    }

    /// Whether the preview emits OSC 8 hyperlinks.
    #[must_use]
    pub fn hyperlinks(&self) -> bool {
        self.hyperlinks
    }

    /// Literal substrings derived from the current query and emphasized in
    /// the list/preview panes (Go `Model.terms`).
    #[must_use]
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    /// Configured result cap; `0` means unlimited.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }
}

/// The picker's stdlib-only AND-substring fallback filter (Go
/// `defaultFilter`): every whitespace-separated query token must appear in the
/// document's searchable blob; an empty query keeps every item.
///
/// Scores are irrelevant here — order is original order, exactly like Go's
/// fallback. Positions are always empty: this filter only knows a document
/// matched, not where.
#[must_use]
pub fn default_filter(query: &str, items: &[Document]) -> Vec<FilterHit> {
    let hits = |index: usize| FilterHit {
        index,
        matched_indexes: Vec::new(),
    };
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (0..items.len()).map(hits).collect();
    }
    let tokens: Vec<String> = trimmed.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, doc)| {
            let blob = item_blob(doc);
            tokens.iter().all(|token| blob.contains(token.as_str()))
        })
        .map(|(index, _)| hits(index))
        .collect()
}

/// Go `itemBlob`: lowercased title, path, type, search blob, and tags joined
/// with newlines.
fn item_blob(doc: &Document) -> String {
    let mut blob = String::new();
    blob.push_str(&doc.title.to_lowercase());
    blob.push('\n');
    blob.push_str(&doc.path.to_lowercase());
    blob.push('\n');
    blob.push_str(&doc.doc_type.to_lowercase());
    blob.push('\n');
    blob.push_str(&doc.search_blob.to_lowercase());
    blob.push('\n');
    for tag in &doc.tags {
        blob.push_str(&tag.to_lowercase());
        blob.push('\n');
    }
    blob
}

/// Builds the `$EDITOR` argv exactly like Go `editorArgv`: the editor string
/// is split on whitespace (no shell quoting) and `path` is appended as one
/// final argument.
///
/// # Errors
///
/// Returns `"$EDITOR is not set"` when `editor` is `None`, empty, or
/// whitespace-only.
#[must_use = "the caller must handle the missing-editor error"]
pub fn editor_argv(editor: Option<&str>, path: &str) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = editor
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if argv.is_empty() {
        return Err("$EDITOR is not set".to_owned());
    }
    argv.push(path.to_owned());
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    fn doc(path: &str, title: &str) -> Document {
        Document {
            path: path.to_owned(),
            title: title.to_owned(),
            body: format!("body of {title}"),
            ..Document::default()
        }
    }

    /// Injected stub mirroring Go's `substringStub`: case-insensitive
    /// substring match on the title. Positions stay empty on purpose so tests
    /// can tell filter-provided positions apart from absent ones.
    fn substring_filter(query: &str, items: &[Document]) -> Vec<FilterHit> {
        let hits = |index: usize| FilterHit {
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

    fn picker(items: Vec<Document>, config: Config) -> App {
        App::new(items, config, Box::new(substring_filter))
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn rune_key(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn apply(app: &mut App, msg: Msg) -> Option<Cmd> {
        app.update(msg)
    }

    fn titles(app: &App) -> Vec<String> {
        app.filtered_docs()
            .iter()
            .map(|doc| doc.title.clone())
            .collect()
    }

    fn three_notes() -> Vec<Document> {
        vec![
            doc("a.md", "alpha"),
            doc("b.md", "beta"),
            doc("c.md", "gamma"),
        ]
    }

    #[test]
    fn note_filter_narrows_via_stub() {
        let mut app = picker(three_notes(), Config::default());
        assert_eq!(app.filtered().len(), 3, "expected 3 items initially");
        app.set_query("alpha");
        assert_eq!(app.filtered().len(), 1, "expected 1 item after query");
        assert_eq!(app.filtered_docs()[0].title, "alpha");
        assert_eq!(app.query(), "alpha");
    }

    #[test]
    fn note_typing_via_key_events_narrows() {
        let mut app = picker(three_notes(), Config::default());
        apply(&mut app, rune_key('/'));
        for c in "alp".chars() {
            apply(&mut app, rune_key(c));
        }
        assert_eq!(app.query(), "alp");
        assert_eq!(app.filtered().len(), 1);
        assert_eq!(titles(&app), ["alpha"]);
    }

    #[test]
    fn note_list_mode_ignores_plain_letters() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        for c in "abq".chars() {
            apply(&mut app, rune_key(c));
        }
        assert_eq!(app.query(), "", "list-mode runes must be ignored");
        assert_eq!(app.filtered().len(), 2);
        assert!(!app.is_aborted());
    }

    #[test]
    fn note_list_mode_editing_keys_do_not_mutate_query() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        app.set_query("foo");
        apply(&mut app, key(KeyCode::Backspace));
        apply(&mut app, ctrl('w'));
        apply(&mut app, key(KeyCode::Left));
        assert_eq!(app.query(), "foo", "hidden query must stay untouched");
    }

    #[test]
    fn note_esc_in_list_clears_query_then_aborts() {
        let mut app = picker(three_notes(), Config::default());
        app.set_query("ta");
        assert_eq!(app.filtered().len(), 1, "precondition: query narrows to 1");
        apply(&mut app, key(KeyCode::Esc));
        assert_eq!(app.query(), "");
        assert_eq!(app.filtered().len(), 3, "esc restores the full list");
        assert!(!app.is_aborted(), "first esc keeps the picker open");
        apply(&mut app, key(KeyCode::Esc));
        assert!(app.is_aborted(), "second esc aborts");
    }

    #[test]
    fn note_slash_enters_search_and_focuses_input() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        assert!(!app.is_searching(), "picker starts in list mode");
        let cmd = apply(&mut app, rune_key('/'));
        assert!(cmd.is_none());
        assert!(app.is_searching());
        assert!(app.search_focused());
        assert_eq!(app.query(), "");
    }

    #[test]
    fn note_slash_in_search_types_literal() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        apply(&mut app, rune_key('/'));
        apply(&mut app, rune_key('/'));
        assert_eq!(app.query(), "/");
        assert!(app.is_searching());
    }

    #[test]
    fn note_esc_returns_to_list_retaining_query() {
        let mut app = picker(three_notes(), Config::default());
        apply(&mut app, rune_key('/'));
        for c in "alp".chars() {
            apply(&mut app, rune_key(c));
        }
        assert_eq!(app.query(), "alp");
        apply(&mut app, key(KeyCode::Esc));
        assert!(!app.is_searching(), "esc returns to list mode");
        assert!(!app.is_aborted(), "search-mode esc must not abort");
        assert_eq!(app.query(), "alp", "query retained after esc");
        assert_eq!(app.filtered().len(), 1);
    }

    #[test]
    fn note_cursor_movement() {
        let mut app = picker(three_notes(), Config::default());
        assert_eq!(app.cursor_index(), 0);
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor_index(), 1);
        apply(&mut app, ctrl('j'));
        assert_eq!(app.cursor_index(), 2);
        apply(&mut app, key(KeyCode::Up));
        assert_eq!(app.cursor_index(), 1);
        apply(&mut app, ctrl('k'));
        assert_eq!(app.cursor_index(), 0);
        apply(&mut app, key(KeyCode::Up));
        assert_eq!(app.cursor_index(), 2, "up wraps to the bottom");
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor_index(), 0, "down wraps to the top");
    }

    #[test]
    fn note_cursor_tracks_document_across_query_change() {
        let mut app = picker(
            vec![
                doc("a.md", "alpha"),
                doc("b.md", "beta"),
                doc("g.md", "gamma"),
                doc("d.md", "delta"),
            ],
            Config::default(),
        );
        apply(&mut app, key(KeyCode::Down));
        apply(&mut app, key(KeyCode::Down));
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.filtered_docs()[app.cursor_index()].path, "d.md");

        app.set_query("ta");
        assert_eq!(titles(&app), ["beta", "delta"]);
        assert_eq!(app.cursor_index(), 1, "cursor follows delta to index 1");
        assert_eq!(app.filtered_docs()[app.cursor_index()].path, "d.md");
    }

    #[test]
    fn note_cursor_resets_when_document_filtered_out() {
        let mut app = picker(
            vec![
                doc("a.md", "alpha"),
                doc("b.md", "beta"),
                doc("g.md", "gamma"),
                doc("d.md", "delta"),
            ],
            Config::default(),
        );
        apply(&mut app, key(KeyCode::Down));
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.filtered_docs()[app.cursor_index()].path, "g.md");

        app.set_query("ta");
        assert_eq!(app.cursor_index(), 0, "cursor resets when its doc is gone");
        assert_eq!(app.filtered_docs()[0].path, "b.md");
    }

    #[test]
    fn note_toggle_select_multi() {
        let mut app = picker(three_notes(), Config::default());
        apply(&mut app, key(KeyCode::Tab));
        assert_eq!(app.selected_items().len(), 1);
        apply(&mut app, key(KeyCode::Down));
        apply(&mut app, key(KeyCode::Tab));
        assert_eq!(app.selected_items().len(), 2);
        apply(&mut app, key(KeyCode::Up));
        apply(&mut app, key(KeyCode::Tab));
        assert_eq!(app.selected_items().len(), 1, "tab toggles off");
        assert_eq!(app.selected_items()[0].path, "b.md");

        let cmd = apply(&mut app, key(KeyCode::Enter));
        assert_eq!(cmd, Some(Cmd::Quit));
        assert!(app.is_confirmed());
        let result = app.result();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "b.md");
    }

    #[test]
    fn note_enter_single_select() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        apply(&mut app, key(KeyCode::Down));
        let cmd = apply(&mut app, key(KeyCode::Enter));
        assert_eq!(cmd, Some(Cmd::Quit));
        let result = app.result();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "beta");
    }

    #[test]
    fn note_abort_esc_and_ctrl_c() {
        let mut app = picker(vec![doc("a.md", "alpha")], Config::default());
        let cmd = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(cmd, Some(Cmd::Quit));
        assert!(app.is_aborted());

        let mut app = picker(vec![doc("a.md", "alpha")], Config::default());
        let cmd = apply(&mut app, ctrl('c'));
        assert_eq!(cmd, Some(Cmd::Quit));
        assert!(app.is_aborted());
    }

    #[test]
    fn note_archived_toggle() {
        let mut open = doc("a.md", "alpha");
        open.archived = Some(false);
        let mut archived = doc("arch.md", "archived note");
        archived.archived = Some(true);
        let plain = doc("c.md", "gamma");
        let mut app = picker(vec![open, archived, plain], Config::default());
        assert_eq!(app.filtered().len(), 2, "archived hidden by default");
        assert!(!app.show_archived());

        apply(&mut app, ctrl('a'));
        assert!(app.show_archived());
        assert_eq!(app.filtered().len(), 3, "ctrl+a reveals archived");
        apply(&mut app, ctrl('a'));
        assert!(!app.show_archived());
        assert_eq!(app.filtered().len(), 2);
    }

    #[test]
    fn note_preview_toggle() {
        let mut app = picker(vec![doc("a.md", "alpha")], Config::default());
        assert!(!app.show_preview());
        apply(&mut app, ctrl('p'));
        assert!(app.show_preview());
        apply(&mut app, ctrl('p'));
        assert!(!app.show_preview());
    }

    #[test]
    fn note_frontmatter_toggle() {
        let mut app = picker(vec![doc("a.md", "alpha")], Config::default());
        assert!(app.show_frontmatter(), "frontmatter shown by default");
        apply(&mut app, ctrl('f'));
        assert!(!app.show_frontmatter());
        apply(&mut app, ctrl('f'));
        assert!(app.show_frontmatter());
    }

    #[test]
    fn note_limit_is_honored() {
        let app = picker(
            three_notes(),
            Config {
                limit: 2,
                ..Config::default()
            },
        );
        assert_eq!(app.filtered().len(), 2, "limit caps the visible set");
        assert_eq!(app.limit(), 2);
    }

    #[test]
    fn note_default_filter_empty_query_returns_all() {
        let app = App::with_default_filter(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        assert_eq!(app.filtered().len(), 2);
    }

    #[test]
    fn note_default_filter_matches_search_blob_and_tags() {
        let mut blob_doc = doc("a.md", "unrelated title");
        blob_doc.search_blob = "the deepterm appears in the body".to_owned();
        let mut tagged = doc("b.md", "other");
        tagged.tags = vec!["launch".to_owned()];
        let items = vec![blob_doc, tagged, doc("c.md", "third")];

        let hit = |index: usize| FilterHit {
            index,
            matched_indexes: Vec::new(),
        };
        assert_eq!(default_filter("deepterm", &items), vec![hit(0)]);
        assert_eq!(default_filter("launch", &items), vec![hit(1)]);
        assert_eq!(
            default_filter("DEEPTERM LAUNCH", &items),
            Vec::<FilterHit>::new()
        );
        assert_eq!(
            default_filter("unrelated launch", &items),
            Vec::<FilterHit>::new()
        );
        assert_eq!(default_filter("  ", &items), vec![hit(0), hit(1), hit(2)]);
    }

    #[test]
    fn note_terms_follow_query() {
        let mut app = picker(three_notes(), Config::default());
        app.set_query("tag:launch bare created:2024 tag:-skip");
        assert_eq!(app.terms(), ["launch", "bare"]);
    }

    #[test]
    fn note_status_field_transient() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: Some("$EDITOR is not set".to_owned()),
            },
        );
        assert_eq!(app.status(), "$EDITOR is not set");
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.status(), "", "next key clears the status");
    }

    #[test]
    fn note_resize_and_scroll_keep_cursor_visible() {
        let items: Vec<Document> = (0..30)
            .map(|i| doc(&format!("n{i}.md"), &format!("note {i}")))
            .collect();
        let mut app = picker(items, Config::default());
        apply(
            &mut app,
            Msg::Resize {
                width: 80,
                height: 5,
            },
        );
        for _ in 0..5 {
            apply(&mut app, key(KeyCode::Down));
        }
        assert_eq!(app.cursor_index(), 5);
        assert_eq!(app.offset(), 2, "four visible rows scroll the window");
    }

    #[test]
    fn note_out_of_range_filter_indices_are_dropped() {
        let hit = |index: usize| FilterHit {
            index,
            matched_indexes: Vec::new(),
        };
        let filter: FilterFn = Box::new(move |_, _| vec![hit(0), hit(99), hit(1)]);
        let mut app = App::new(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
            filter,
        );
        assert_eq!(app.filtered(), [0, 1]);
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor_index(), 1);
        assert_eq!(app.filtered_docs()[1].path, "b.md");
    }

    #[test]
    fn note_selected_items_include_hidden_selection() {
        let mut app = picker(three_notes(), Config::default());
        apply(&mut app, key(KeyCode::Tab)); // alpha
        apply(&mut app, key(KeyCode::Down));
        apply(&mut app, key(KeyCode::Tab)); // beta
        app.set_query("ga"); // only gamma visible; alpha+beta hidden
        assert_eq!(titles(&app), ["gamma"]);
        let selected = app.selected_items();
        assert_eq!(
            selected
                .iter()
                .map(|doc| doc.title.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"],
            "hidden selections append in original order"
        );
    }

    #[test]
    fn note_ctrl_o_opens_first_link() {
        let mut with_resource = doc("a.md", "alpha");
        with_resource.resource = "https://res.example".to_owned();
        let mut app = picker(vec![with_resource], Config::default());
        let cmd = apply(&mut app, ctrl('o'));
        assert_eq!(cmd, Some(Cmd::OpenUrl("https://res.example".to_owned())));

        let mut plain = picker(vec![doc("b.md", "beta")], Config::default());
        assert_eq!(apply(&mut plain, ctrl('o')), None);
    }

    #[test]
    fn note_editor_argv_splits_and_appends_path() {
        let cases: [(&str, &str, &[&str]); 4] = [
            ("code --wait", "a.md", &["code", "--wait", "a.md"]),
            ("vim", "notes/b.md", &["vim", "notes/b.md"]),
            ("  vim   --wait  ", "a.md", &["vim", "--wait", "a.md"]),
            (
                "nvim --clean",
                "dir with space/a.md",
                &["nvim", "--clean", "dir with space/a.md"],
            ),
        ];
        for (editor, path, want) in cases {
            let argv = editor_argv(Some(editor), path).expect("usable editor");
            assert_eq!(argv, want, "editor {editor:?}");
        }
        for editor in [None, Some(""), Some("   "), Some("\t\n")] {
            let error = editor_argv(editor, "a.md").expect_err("blank editor errors");
            assert_eq!(error, "$EDITOR is not set");
        }
    }

    #[test]
    fn note_e_without_editor_sets_status() {
        for editor in [None, Some(String::new()), Some("   ".to_owned())] {
            let mut app = picker(
                vec![doc("a.md", "alpha"), doc("b.md", "beta")],
                Config {
                    editor,
                    ..Config::default()
                },
            );
            let cmd = apply(&mut app, rune_key('e'));
            assert!(cmd.is_none(), "no command for a blank $EDITOR");
            assert_eq!(app.status(), "$EDITOR is not set");
            assert!(!app.is_aborted() && !app.is_confirmed());
            assert_eq!(app.query(), "");
            assert_eq!(app.filtered().len(), 2);
        }
    }

    #[test]
    fn note_e_ignored_in_search_mode() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        apply(&mut app, rune_key('/'));
        let cmd = apply(&mut app, rune_key('e'));
        assert!(cmd.is_none());
        assert!(app.is_searching());
        assert_eq!(app.query(), "e", "search mode types e as a literal");
        assert_eq!(app.status(), "");
    }

    #[test]
    fn note_e_with_no_selection_is_noop() {
        let cases: Vec<Vec<Document>> = vec![
            Vec::new(),
            vec![Document::default()],
            vec![Document {
                title: "alpha".to_owned(),
                ..Document::default()
            }],
        ];
        for items in cases {
            let mut app = picker(
                items,
                Config {
                    editor: Some("vim --wait".to_owned()),
                    ..Config::default()
                },
            );
            let cmd = apply(&mut app, rune_key('e'));
            assert!(cmd.is_none(), "no command without a focused file");
            assert_eq!(app.status(), "");
            assert!(!app.is_aborted());
        }
    }

    #[test]
    fn note_e_with_editor_returns_exec_cmd() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config {
                editor: Some("vim --wait".to_owned()),
                ..Config::default()
            },
        );
        apply(&mut app, key(KeyCode::Down));
        let cmd = apply(&mut app, rune_key('e'));
        assert_eq!(
            cmd,
            Some(Cmd::ExecEditor(vec![
                "vim".to_owned(),
                "--wait".to_owned(),
                "b.md".to_owned()
            ]))
        );
        assert_eq!(app.status(), "");
        assert!(!app.is_aborted());
    }

    fn reload_stub(title: &str) -> (ReloadFn, Rc<Cell<usize>>) {
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let title = title.to_owned();
        let reload: ReloadFn = Box::new(move |path: &str| {
            counter.set(counter.get() + 1);
            Ok(Document {
                path: path.to_owned(),
                title: title.clone(),
                ..Document::default()
            })
        });
        (reload, calls)
    }

    #[test]
    fn note_editor_finished_reloads_doc() {
        let (reload, calls) = reload_stub("beta edited");
        let mut app = picker(
            three_notes(),
            Config {
                reload: Some(reload),
                ..Config::default()
            },
        );
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: None,
            },
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(app.status(), "");
        assert_eq!(app.items()[1].title, "beta edited");
        assert_eq!(app.items()[1].path, "b.md");
        assert_eq!(app.items()[0].title, "alpha");
        assert_eq!(titles(&app), ["alpha", "beta edited", "gamma"]);
    }

    #[test]
    fn note_editor_finished_keeps_cursor_on_title_edit() {
        let (reload, _) = reload_stub("beta edited");
        let mut app = picker(
            three_notes(),
            Config {
                reload: Some(reload),
                ..Config::default()
            },
        );
        apply(&mut app, key(KeyCode::Down));
        assert_eq!(app.filtered_docs()[app.cursor_index()].path, "b.md");

        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: None,
            },
        );
        assert_eq!(app.cursor_index(), 1);
        assert_eq!(app.filtered_docs()[1].path, "b.md");
        assert_eq!(app.filtered_docs()[1].title, "beta edited");
    }

    #[test]
    fn note_editor_finished_reload_error_keeps_old_doc() {
        let reload: ReloadFn = Box::new(|_| Err("parse: b.md: boom".to_owned()));
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config {
                reload: Some(reload),
                ..Config::default()
            },
        );
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: None,
            },
        );
        assert_eq!(app.status(), "parse: b.md: boom");
        assert_eq!(app.items()[1].title, "beta");
        assert_eq!(app.filtered_docs()[1].title, "beta");
    }

    #[test]
    fn note_editor_finished_editor_error_does_not_reload() {
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let reload: ReloadFn = Box::new(move |path: &str| {
            counter.set(counter.get() + 1);
            Ok(Document {
                path: path.to_owned(),
                ..Document::default()
            })
        });
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config {
                reload: Some(reload),
                ..Config::default()
            },
        );
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: Some("editor exited with status 1".to_owned()),
            },
        );
        assert_eq!(calls.get(), 0, "editor errors abort before parsing");
        assert_eq!(app.status(), "editor exited with status 1");
        assert_eq!(app.items()[1].title, "beta");
    }

    #[test]
    fn note_editor_finished_nil_reload_is_noop() {
        let mut app = picker(
            vec![doc("a.md", "alpha"), doc("b.md", "beta")],
            Config::default(),
        );
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: None,
            },
        );
        assert_eq!(app.items().len(), 2);
        assert_eq!(app.items()[1].title, "beta");
        assert_eq!(app.status(), "");
    }

    #[test]
    fn note_editor_finished_doc_dropped_when_no_longer_matches() {
        let (reload, _) = reload_stub("renamed");
        let mut app = picker(
            three_notes(),
            Config {
                reload: Some(reload),
                ..Config::default()
            },
        );
        app.set_query("beta");
        assert_eq!(app.filtered().len(), 1);
        apply(
            &mut app,
            Msg::EditorFinished {
                path: "b.md".to_owned(),
                error: None,
            },
        );
        assert_eq!(app.filtered().len(), 0, "renamed doc leaves the filter");
        assert_eq!(app.cursor_index(), 0);
        assert_eq!(app.items()[1].title, "renamed", "stored item still updated");
    }
}
