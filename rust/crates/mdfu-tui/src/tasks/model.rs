//! The task TUI model: state, key handling, filtering, and frame rendering.
//!
//! Mirrors Go `internal/tui/tasks_model.go` and the browse frame of
//! `tasks_view.go`. The model is headless: [`App::update`] is a pure message
//! handler over the state it owns, so tests drive synthetic key events exactly
//! like the Go `applyTaskKey` helper.
//!
//! Filtering is injected as [`TaskFilterFn`] — the task UI's own type. The
//! module never references the note picker's filter type or global; production
//! wires the real task filter from the command layer (todos 26/29+), tests
//! inject stubs, and [`default_task_filter`] is the stdlib fallback (Go
//! `FilterTaskItems`).
//!
//! The browse key map is Go `DefaultTaskKeyMap` verbatim: navigation
//! (`up`/`k`, `down`/`j`, `pgup`/`ctrl+b`, `pgdn`/`ctrl+f`, `home`/`g`,
//! `end`/`G`), `/` filter, `esc` clear/quit, `tab` pane focus, `x` toggle done
//! (in memory only; the disk write is todo 26), `b` blocked visibility, `?`
//! help, and `q`/`ctrl+c` quit. `o`/`t`/`e`/`n`/`m`/`a` and the edit-mode
//! `ctrl+s` save key are reserved for todos 25-26.

use mdfu_core::task::blocker;
use mdfu_core::task::config as task_config;
use mdfu_core::task::Task;
use mdfu_core::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::Frame;

use crate::view_model::{StatusBar, TaskDetail, TaskEditor, TaskRow};
use crate::widgets::{
    style_from, task_pane_block, SearchInput, StatusBarWidget, TaskDetailWidget, TaskEditorWidget,
    TaskList,
};

/// Terminal width at or above which the list and detail panes join side by
/// side; below it they stack (Go `taskWideLayout`).
pub const WIDE_LAYOUT: u16 = 100;

/// Injected task filter: maps the current query plus the full task snapshot to
/// the **indices** of the tasks to display, in display order.
///
/// Indices are the task UI's stable identity: they survive re-filtering and map
/// straight back to the source [`Task`] for the rows, detail pane, and (in
/// todos 25-26) the write actions. Go's `TaskFilterFunc` receives and returns
/// `[]TaskItem` copies; indices are the Rust equivalent that avoids a second
/// identity table, exactly like the note picker's filter indices.
///
/// This is the task UI's OWN type — not the note picker's filter hook. The
/// filter receives the App's current tasks (not a captured copy), so a scope
/// reload that replaces the list is visible to the next call; an empty query
/// must return every index. The constructor is the only way to install a
/// filter: there is no global or static fallback.
pub type TaskFilterFn = Box<dyn Fn(&str, &[Task]) -> Vec<usize>>;

/// Current input mode of the task TUI (Go `TaskMode`).
///
/// `Browse` and `Filter` are live in this slice; `Help` renders the full key
/// help; the edit modes and the move prompt are reserved for todos 25-26 and
/// cannot be entered yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// List navigation; the default mode.
    #[default]
    Browse,
    /// The filter input owns printable keys.
    Filter,
    /// The title editor (todo 25).
    EditTitle,
    /// The body editor (todo 25).
    EditBody,
    /// The move-destination prompt (todos 25-26).
    MovePrompt,
    /// Full key help; any dismiss key returns to browse.
    Help,
}

/// Messages the task UI reacts to (Go's `tea.Msg` key/resize window plus the
/// injection seams).
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// A key press.
    Key(KeyEvent),
    /// Terminal size report.
    Resize {
        /// Terminal width in cells.
        width: u16,
        /// Terminal height in cells.
        height: u16,
    },
    /// Extension point owned by **todo 26** (actions/reload): delivered after
    /// the `$EDITOR` process spawned by [`Cmd::ExecEditor`] exits. This slice
    /// reserves the variant and matches it to `None`; the actions todo owns
    /// the error status and the scope reload.
    EditorFinished {
        /// Editor failure text; `None` means the editor exited successfully.
        error: Option<String>,
    },
}

/// Effects the task UI asks its driver to perform after an update (Go's
/// `tea.Cmd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Leave the task loop; inspect `is_aborted`/`result` afterwards.
    Quit,
    /// Extension point owned by **todo 26**: suspend the terminal, run `argv`,
    /// and feed back a [`Msg::EditorFinished`]. Nothing in this slice emits it.
    ExecEditor(Vec<String>),
}

/// One documented key binding (Go `bubbles/key.Binding`).
///
/// `keys` lists the alternatives in the exact order Go's `DefaultTaskKeyMap`
/// declares them; `help_key`/`desc` are the footer strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// bubbletea-style key names: `"up"`, `"ctrl+b"`, `"G"`, `"/"`, ...
    pub keys: &'static [&'static str],
    /// Human-readable key list for the footer, e.g. `"↑/k"`.
    pub help_key: &'static str,
    /// One-line description, e.g. `"toggle done"`.
    pub desc: &'static str,
}

impl Binding {
    /// Reports whether `event` matches any of this binding's keys (Go
    /// `key.Matches`).
    #[must_use]
    pub fn matches(&self, event: &KeyEvent) -> bool {
        let name = key_name(event);
        self.keys.iter().any(|key| *key == name)
    }
}

/// Renders a crossterm key event as the bubbletea `KeyMsg.String()` name the
/// bindings use: `"up"`, `"pgup"`, `"tab"`, `"esc"`, `"enter"`,
/// `"ctrl+s"` (lowercased), or the plain rune (`"G"`, `"/"`, `"?"`).
///
/// Shift is folded into the rune (`G`), exactly like bubbletea; modifier
/// combinations outside the key map fall back to a debug-ish name and simply
/// match nothing.
#[must_use]
pub fn key_name(event: &KeyEvent) -> String {
    let control = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.code {
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pgup".to_owned(),
        KeyCode::PageDown => "pgdn".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::BackTab => "shift+tab".to_owned(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Char(c) if control => format!("ctrl+{}", c.to_ascii_lowercase()),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

/// Every key binding the task TUI understands (Go `TaskKeyMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyMap {
    /// Cursor up.
    pub up: Binding,
    /// Cursor down.
    pub down: Binding,
    /// Page up.
    pub page_up: Binding,
    /// Page down.
    pub page_down: Binding,
    /// Jump to the top.
    pub home: Binding,
    /// Jump to the bottom.
    pub end: Binding,
    /// Enter filter mode.
    pub filter: Binding,
    /// Clear the filter / quit.
    pub esc: Binding,
    /// Toggle the focused pane.
    pub tab: Binding,
    /// Toggle done (in memory; the write is todo 26).
    pub toggle_done: Binding,
    /// Toggle blocked visibility.
    pub toggle_blk: Binding,
    /// Open in `$EDITOR` (reserved, todo 26).
    pub open: Binding,
    /// Edit the body (reserved, todo 25).
    pub edit_body: Binding,
    /// Edit the title (reserved, todo 25).
    pub edit_title: Binding,
    /// New task (reserved, todo 25).
    pub new: Binding,
    /// Move to file (reserved, todos 25-26).
    pub move_to: Binding,
    /// Archive (reserved, todo 26).
    pub archive: Binding,
    /// Toggle help.
    pub help: Binding,
    /// Quit.
    pub quit: Binding,
    /// Save (reserved for the edit modes, todo 25).
    pub save: Binding,
    /// Cancel (reserved for the edit/move modes, todos 25-26).
    pub cancel: Binding,
}

impl Default for KeyMap {
    /// The documented bindings, exactly Go `DefaultTaskKeyMap`.
    // One binding per field with the Go-declared key order; splitting the
    // frozen table would scatter it.
    #[allow(clippy::too_many_lines)]
    fn default() -> Self {
        Self {
            up: Binding {
                keys: &["up", "k"],
                help_key: "↑/k",
                desc: "up",
            },
            down: Binding {
                keys: &["down", "j"],
                help_key: "↓/j",
                desc: "down",
            },
            page_up: Binding {
                keys: &["pgup", "ctrl+b"],
                help_key: "pgup/ctrl+b",
                desc: "page up",
            },
            page_down: Binding {
                keys: &["pgdn", "ctrl+f"],
                help_key: "pgdn/ctrl+f",
                desc: "page down",
            },
            home: Binding {
                keys: &["home", "g"],
                help_key: "home/g",
                desc: "top",
            },
            end: Binding {
                keys: &["end", "G"],
                help_key: "end/G",
                desc: "bottom",
            },
            filter: Binding {
                keys: &["/"],
                help_key: "/",
                desc: "filter",
            },
            esc: Binding {
                keys: &["esc"],
                help_key: "esc",
                desc: "clear filter / quit",
            },
            tab: Binding {
                keys: &["tab"],
                help_key: "tab",
                desc: "pane focus",
            },
            toggle_done: Binding {
                keys: &["x"],
                help_key: "x",
                desc: "toggle done",
            },
            toggle_blk: Binding {
                keys: &["b"],
                help_key: "b",
                desc: "show blocked",
            },
            open: Binding {
                keys: &["o"],
                help_key: "o",
                desc: "open in $EDITOR",
            },
            edit_body: Binding {
                keys: &["e"],
                help_key: "e",
                desc: "edit body",
            },
            edit_title: Binding {
                keys: &["t"],
                help_key: "t",
                desc: "edit title",
            },
            new: Binding {
                keys: &["n"],
                help_key: "n",
                desc: "new task",
            },
            move_to: Binding {
                keys: &["m"],
                help_key: "m",
                desc: "move to file",
            },
            archive: Binding {
                keys: &["a"],
                help_key: "a",
                desc: "archive",
            },
            help: Binding {
                keys: &["?"],
                help_key: "?",
                desc: "help",
            },
            quit: Binding {
                keys: &["q", "ctrl+c"],
                help_key: "q/ctrl+c",
                desc: "quit",
            },
            save: Binding {
                keys: &["ctrl+s"],
                help_key: "ctrl+s",
                desc: "save",
            },
            cancel: Binding {
                keys: &["esc"],
                help_key: "esc",
                desc: "cancel",
            },
        }
    }
}

impl KeyMap {
    /// The bindings shown in the compact footer (Go `ShortHelp`).
    #[must_use]
    pub fn short_help(&self) -> Vec<&Binding> {
        vec![
            &self.up,
            &self.down,
            &self.filter,
            &self.toggle_done,
            &self.edit_title,
            &self.help,
            &self.quit,
        ]
    }

    /// Every binding, grouped into footer columns (Go `FullHelp`).
    #[must_use]
    pub fn full_help(&self) -> Vec<Vec<&Binding>> {
        vec![
            vec![
                &self.up,
                &self.down,
                &self.page_up,
                &self.page_down,
                &self.home,
                &self.end,
            ],
            vec![
                &self.filter,
                &self.esc,
                &self.tab,
                &self.toggle_done,
                &self.toggle_blk,
                &self.help,
            ],
            vec![
                &self.open,
                &self.edit_title,
                &self.edit_body,
                &self.new,
                &self.move_to,
                &self.archive,
            ],
            vec![&self.save, &self.cancel, &self.quit],
        ]
    }
}

/// Returns the documented bindings (Go `DefaultTaskKeyMap`); identical to
/// [`KeyMap::default`].
#[must_use]
pub fn default_task_key_map() -> KeyMap {
    KeyMap::default()
}

/// Task TUI configuration (Go `TaskConfig`).
///
/// `theme` `None` resolves to [`Theme::default`]. `base` and `task_config` are
/// the resolved task scope the write/reload actions operate against; they are
/// carried for todos 25-26 and unused by the browse/filter slice.
#[derive(Default)]
pub struct Config {
    /// Theme override; `None` resolves to [`Theme::default`].
    pub theme: Option<Theme>,
    /// Start with blocked tasks visible.
    pub show_blocked: bool,
    /// Start with done tasks visible.
    pub show_done: bool,
    /// Resolved task scope (a base directory or a single `--path` file).
    pub base: String,
    /// Engine task configuration used by the write/reload actions.
    pub task_config: task_config::Config,
}

/// The task TUI state (Go `TaskModel`).
///
/// The struct is value-style state like the note picker's `App`: `update`
/// mutates it in place and `view` paints it without touching the terminal.
/// Fields are `pub(crate)` so the edit/action modules of todos 25-26 can extend
/// the impl blocks without re-plumbing the model.
#[allow(clippy::struct_excessive_bools)] // the bool set is the frozen Go model shape
pub struct App {
    pub(crate) tasks: Vec<Task>,
    pub(crate) filtered: Vec<usize>,
    pub(crate) filter: TaskFilterFn,

    pub(crate) cursor: usize,
    pub(crate) offset: usize,
    pub(crate) width: u16,
    pub(crate) height: u16,

    pub(crate) mode: Mode,
    pub(crate) focus_pane: u8,
    pub(crate) show_blocked: bool,
    pub(crate) show_done: bool,

    pub(crate) input: SearchInput,
    pub(crate) keys: KeyMap,
    pub(crate) theme: Theme,

    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) aborted: bool,

    // --- extension state owned by todos 25 (editors) and 26 (actions) ---
    /// Title/body editor buffer (Go `editor` textarea); todo 25 seeds it.
    pub(crate) editor_text: String,
    /// Widget text seeded when an edit mode is entered (Go `pendingEdit`).
    pub(crate) pending_edit: String,
    /// Move-destination input (Go `moveInput`); the prompt is todo 25.
    pub(crate) move_input: SearchInput,
    /// Whether the title editor is creating a new task (Go `newTask`).
    pub(crate) new_task: bool,
    /// Resolved task scope (Go `base`).
    pub(crate) base: String,
    /// Engine task configuration (Go `taskCfg`).
    pub(crate) task_config: task_config::Config,
    /// Last action name reported through the final result (Go `action`).
    pub(crate) action: String,
}

impl App {
    /// Builds the task UI around `tasks` with `filter` injected (Go
    /// `NewTaskModelWithFilter`). The list opens in browse mode with an empty
    /// query, `filter` applied, and the done/blocked gates from `config`.
    #[must_use]
    pub fn new(tasks: Vec<Task>, config: Config, filter: TaskFilterFn) -> Self {
        let theme = config.theme.unwrap_or_default();
        let mut input = SearchInput::new();
        // Focused from the start like the Go textinput; browse mode simply
        // never renders or forwards to it until `/` reveals the line.
        input.focus();
        let mut app = Self {
            tasks,
            filtered: Vec::new(),
            filter,
            cursor: 0,
            offset: 0,
            width: 0,
            height: 0,
            mode: Mode::Browse,
            focus_pane: 0,
            show_blocked: config.show_blocked,
            show_done: config.show_done,
            input,
            keys: KeyMap::default(),
            theme,
            status: String::new(),
            error: None,
            aborted: false,
            editor_text: String::new(),
            pending_edit: String::new(),
            move_input: SearchInput::new(),
            new_task: false,
            base: config.base,
            task_config: config.task_config,
            action: String::new(),
        };
        app.refilter();
        app
    }

    /// Builds the task UI with the built-in AND-substring fallback filter (Go
    /// `NewTaskModel`). Production wires its own filter through [`App::new`];
    /// this constructor keeps the fallback injectable instead of global.
    #[must_use]
    pub fn with_default_filter(tasks: Vec<Task>, config: Config) -> Self {
        Self::new(tasks, config, Box::new(default_task_filter))
    }

    /// Handles one message, returning a driver effect when one is needed (Go
    /// `TaskModel.Update` returning a `tea.Cmd`).
    ///
    /// Key handling dispatches on the mode. The edit modes and the move prompt
    /// are reserved for todos 25-26: their keys are dropped here (the modes
    /// cannot be entered yet), and [`Msg::EditorFinished`] is inert until the
    /// actions todo wires the reload.
    // By-value matches Go's `tea.Msg` flow and the note picker's update; the
    // EditorFinished payload is consumed by todo 26, so this bound cannot be
    // narrowed without breaking API parity.
    #[allow(clippy::needless_pass_by_value)]
    #[must_use = "the returned command drives quitting or the external editor"]
    pub fn update(&mut self, msg: Msg) -> Option<Cmd> {
        match msg {
            Msg::Key(key) => match self.mode {
                Mode::Browse => self.update_browse(&key),
                Mode::Filter => self.update_filter(&key),
                Mode::Help => self.update_help(&key),
                // todo 25 owns updateEdit/updateMove (tasks_edit.go /
                // tasks_edit_body.go ports); no mode above enters these yet.
                Mode::EditTitle | Mode::EditBody | Mode::MovePrompt => None,
            },
            Msg::Resize { width, height } => {
                self.width = width;
                self.height = height;
                self.clamp_cursor();
                self.ensure_visible();
                None
            }
            // todo 26 owns the post-$EDITOR reload and error status.
            Msg::EditorFinished { .. } => None,
        }
    }

    /// Browse-mode keys, checked in the Go order so a browse-mode `x` toggles
    /// done instead of typing. `o`/`t`/`e`/`n`/`m`/`a` are reserved for todos
    /// 25-26 and intentionally fall through.
    fn update_browse(&mut self, key: &KeyEvent) -> Option<Cmd> {
        if self.keys.quit.matches(key) {
            self.aborted = true;
            return Some(Cmd::Quit);
        }
        if self.keys.esc.matches(key) {
            if self.input.value().is_empty() {
                self.aborted = true;
                return Some(Cmd::Quit);
            }
            self.input.set_value("");
            self.refilter();
            return None;
        }
        if self.keys.filter.matches(key) {
            self.mode = Mode::Filter;
            self.input.focus();
            return None;
        }
        if self.keys.up.matches(key) {
            self.move_cursor(-1);
            return None;
        }
        if self.keys.down.matches(key) {
            self.move_cursor(1);
            return None;
        }
        if self.keys.page_up.matches(key) {
            let delta = -self.page_size();
            self.move_cursor(delta);
            return None;
        }
        if self.keys.page_down.matches(key) {
            let delta = self.page_size();
            self.move_cursor(delta);
            return None;
        }
        if self.keys.home.matches(key) {
            self.cursor = 0;
            self.ensure_visible();
            return None;
        }
        if self.keys.end.matches(key) {
            if !self.filtered.is_empty() {
                self.cursor = self.filtered.len() - 1;
            }
            self.ensure_visible();
            return None;
        }
        if self.keys.tab.matches(key) {
            self.focus_pane = 1 - self.focus_pane;
            return None;
        }
        if self.keys.toggle_done.matches(key) {
            self.toggle_done();
            return None;
        }
        if self.keys.toggle_blk.matches(key) {
            self.show_blocked = !self.show_blocked;
            self.refilter();
            return None;
        }
        if self.keys.help.matches(key) {
            self.mode = Mode::Help;
            return None;
        }
        None
    }

    /// Filter-mode keys: `esc` clears and returns to browse, `q`/`ctrl+c`
    /// abort, `up`/`down` navigate, and every other key is forwarded to the
    /// input. Critically, `x` and `b` are not special here (Go `updateFilter`).
    fn update_filter(&mut self, key: &KeyEvent) -> Option<Cmd> {
        if self.keys.esc.matches(key) {
            self.input.set_value("");
            self.refilter();
            self.mode = Mode::Browse;
            self.input.blur();
            return None;
        }
        if self.keys.quit.matches(key) {
            self.aborted = true;
            return Some(Cmd::Quit);
        }
        if self.keys.up.matches(key) {
            self.move_cursor(-1);
            return None;
        }
        if self.keys.down.matches(key) {
            self.move_cursor(1);
            return None;
        }
        let before = self.input.value().to_owned();
        self.input.handle_event(&Event::Key(*key));
        if self.input.value() != before {
            self.refilter();
        }
        None
    }

    /// Help-mode keys: `?` or `esc` returns to browse; `q`/`ctrl+c` quits (Go
    /// `updateHelp`).
    fn update_help(&mut self, key: &KeyEvent) -> Option<Cmd> {
        if self.keys.help.matches(key) || self.keys.esc.matches(key) {
            self.mode = Mode::Browse;
            return None;
        }
        if self.keys.quit.matches(key) {
            self.aborted = true;
            return Some(Cmd::Quit);
        }
        None
    }

    /// Rebuilds the visible index list from the injected filter plus the
    /// done/blocked gates, keeping the cursor on the same task when it survives
    /// (Go `refilter`).
    ///
    /// Output is defensively bounded to valid task indices so a buggy injected
    /// filter can never make the view index out of bounds.
    fn refilter(&mut self) {
        let previous = self
            .filtered
            .get(self.cursor)
            .and_then(|index| self.tasks.get(*index))
            .map(task_key);
        let query = self.query().to_owned();
        let mut base = (self.filter)(&query, &self.tasks);
        base.retain(|index| *index < self.tasks.len());
        let mut out = Vec::with_capacity(base.len());
        for index in base {
            let Some(task) = self.tasks.get(index) else {
                continue;
            };
            if task.checked && !self.show_done {
                continue;
            }
            if !self.show_blocked && blocker::has_unresolved_blockers(task, &self.tasks) {
                continue;
            }
            out.push(index);
        }
        self.filtered = out;
        if let Some(key) = previous {
            if let Some(position) = self
                .filtered
                .iter()
                .position(|index| self.tasks.get(*index).map(task_key) == Some(key.clone()))
            {
                self.cursor = position;
            } else {
                self.cursor = 0;
                self.offset = 0;
            }
        }
        self.clamp_cursor();
        self.ensure_visible();
    }

    /// Bounds the cursor and offset to the filtered slice (Go `clampCursor`).
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

    /// Scrolls the offset so the cursor stays inside the viewport (Go
    /// `ensureVisible`).
    fn ensure_visible(&mut self) {
        let max_rows = self.visible_rows();
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset.saturating_add(max_rows) {
            self.offset = self.cursor - max_rows + 1;
        }
    }

    /// How many list rows fit in the current height (Go `visibleRows`).
    ///
    /// The status footer always competes for a row, the filter line takes one
    /// more in filter mode, and the stacked (narrow) layout also reserves the
    /// detail pane's rows; in the side-by-side layout both panes share the full
    /// body budget.
    fn visible_rows(&self) -> usize {
        if self.height == 0 {
            return 20;
        }
        let mut reserved = 1; // status
        if self.mode == Mode::Filter {
            reserved += 1;
        }
        if self.width < WIDE_LAYOUT {
            reserved += self.detail_rows();
        }
        let rows = usize::from(self.height).saturating_sub(reserved);
        rows.max(3)
    }

    /// Vertical budget for the stacked detail pane (Go `detailRows`).
    fn detail_rows(&self) -> usize {
        if self.height == 0 {
            return 10;
        }
        let part = usize::from(self.height.saturating_sub(1)) / 3;
        part.clamp(3, 12)
    }

    /// Moves the cursor by `delta`, wrapping in both directions (Go
    /// `moveCursor`).
    fn move_cursor(&mut self, delta: i64) {
        if self.filtered.is_empty() {
            self.cursor = 0;
            return;
        }
        let len = i64::try_from(self.filtered.len()).unwrap_or(i64::MAX);
        let cursor = i64::try_from(self.cursor).unwrap_or(0);
        let wrapped = cursor.saturating_add(delta).rem_euclid(len);
        self.cursor = usize::try_from(wrapped).unwrap_or(0);
        self.ensure_visible();
    }

    /// Number of rows a page-up/page-down jump moves (Go `pageSize`).
    fn page_size(&self) -> i64 {
        i64::try_from(self.visible_rows().max(1)).unwrap_or(i64::MAX)
    }

    /// Flips the focused item's done state in memory (Go `toggleDone`). The
    /// disk write is todo 26's job; this keeps the model headless-testable and,
    /// like Go, does not refilter (a row stays visible until the next
    /// refilter).
    fn toggle_done(&mut self) {
        let Some(index) = self.filtered.get(self.cursor).copied() else {
            return;
        };
        let Some(task) = self.tasks.get_mut(index) else {
            return;
        };
        task.checked = !task.checked;
        task.status = if task.checked { b'x' } else { b' ' };
    }

    // --- accessors for tests / integration (Go's exported helpers) ---

    /// Every loaded task.
    #[must_use]
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// Indices of the currently visible tasks, in display order.
    #[must_use]
    pub fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    /// The visible tasks in display order.
    #[must_use]
    pub fn filtered_tasks(&self) -> Vec<&Task> {
        self.filtered
            .iter()
            .filter_map(|index| self.tasks.get(*index))
            .collect()
    }

    /// Current filter text.
    #[must_use]
    pub fn query(&self) -> &str {
        self.input.value()
    }

    /// Replaces the filter text and refilters (headless-friendly, Go
    /// `SetQuery`).
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.input.set_value(query);
        self.refilter();
    }

    /// Cursor position within [`App::filtered`].
    #[must_use]
    pub fn cursor_index(&self) -> usize {
        self.cursor
    }

    /// First visible row within [`App::filtered`].
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The current input mode.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The focused pane (`0` list, `1` detail).
    #[must_use]
    pub fn focus_pane(&self) -> u8 {
        self.focus_pane
    }

    /// Blocked visibility.
    #[must_use]
    pub fn show_blocked(&self) -> bool {
        self.show_blocked
    }

    /// Done visibility.
    #[must_use]
    pub fn show_done(&self) -> bool {
        self.show_done
    }

    /// The transient status message; empty hides it.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The last error surfaced by a hook, if any.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether the user aborted (`esc` on an empty filter / `q` / `ctrl+c`).
    #[must_use]
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// Whether the user confirmed. The tasks TUI has no commit key yet; this
    /// exists for parity with the note picker's result contract.
    #[must_use]
    pub fn is_confirmed(&self) -> bool {
        false
    }

    /// Whether the filter input currently draws its cursor.
    #[must_use]
    pub fn filter_focused(&self) -> bool {
        self.input.focused()
    }

    /// The installed key map.
    #[must_use]
    pub fn keys(&self) -> &KeyMap {
        &self.keys
    }

    /// The resolved theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Edit-buffer text reserved for todo 25.
    #[must_use]
    pub fn editor_text(&self) -> &str {
        &self.editor_text
    }

    /// Seeded-edit snapshot reserved for todo 25.
    #[must_use]
    pub fn pending_edit(&self) -> &str {
        &self.pending_edit
    }

    /// Move-destination input reserved for todos 25-26.
    #[must_use]
    pub fn move_input(&self) -> &SearchInput {
        &self.move_input
    }

    /// New-task marker reserved for todo 25.
    #[must_use]
    pub fn is_new_task(&self) -> bool {
        self.new_task
    }

    /// Resolved task scope reserved for todos 25-26.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Engine task configuration reserved for todos 25-26.
    #[must_use]
    pub fn task_config(&self) -> &task_config::Config {
        &self.task_config
    }

    /// Last action name reserved for todo 26's final result.
    #[must_use]
    pub fn action(&self) -> &str {
        &self.action
    }
}

/// Identity of a task across refilters (Go `taskItemKey`).
///
/// Identified tasks key by ID (stable across a move); an ID-less checkbox keys
/// by file and line, which is unique even when two lines have the same raw
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskKey {
    Id(String),
    FileLine(String, usize),
}

/// Builds the identity of one task (Go `taskItemKey`).
fn task_key(task: &Task) -> TaskKey {
    if task.id.is_empty() {
        TaskKey::FileLine(task.file.clone(), task.line)
    } else {
        TaskKey::Id(task.id.clone())
    }
}

impl App {
    /// Paints one task-TUI frame (Go `TaskModel.View`).
    ///
    /// Edit modes render the editor frame through [`TaskEditorWidget`]; every
    /// other mode — including the move prompt, exactly like Go — renders the
    /// browse frame.
    pub fn view(&self, frame: &mut Frame<'_>) {
        frame.render_widget(self, frame.area());
    }

    /// Paints into `area`/`buf`; the widget impl forwards here.
    fn render_frame(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        match self.mode {
            Mode::EditTitle | Mode::EditBody => self.render_editor(area, buf),
            _ => self.render_browse(area, buf),
        }
    }

    /// Edit-mode frame: the editor buffer plus the save/cancel status. The
    /// metadata side panel and the body re-indent preview are todo 25's.
    fn render_editor(&self, area: Rect, buf: &mut Buffer) {
        let editor = TaskEditor {
            text: self.editor_text.clone(),
            is_body: self.mode == Mode::EditBody,
            status: self.status.clone(),
            ..TaskEditor::default()
        };
        let widget = TaskEditorWidget::new(&editor, &self.theme);
        (&widget).render(area, buf);
    }

    /// Browse frame: optional filter line, body, footer (Go `renderTaskBrowse`).
    fn render_browse(&self, area: Rect, buf: &mut Buffer) {
        let filter_rows = u16::from(self.mode == Mode::Filter);
        let chunks = Layout::vertical([
            Constraint::Length(filter_rows),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
        let [filter_area, body, footer] = chunks.as_ref() else {
            return;
        };
        if filter_rows > 0 {
            self.render_filter_line(*filter_area, buf);
        }
        self.render_body(*body, buf);
        self.render_footer(*footer, buf);
    }

    /// Filter line: `"/ "` prompt, dim placeholder, and the focused input.
    fn render_filter_line(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let prompt = Line::from(Span::raw("/ "));
        buf.set_line(area.x, area.y, &prompt, area.width);
        if area.width <= 2 {
            return;
        }
        let input_area = Rect::new(area.x + 2, area.y, area.width - 2, 1);
        if self.input.value().is_empty() {
            let placeholder =
                Line::from(Span::styled("Filter tasks...", style_from(&self.theme.dim)));
            buf.set_line(input_area.x, input_area.y, &placeholder, input_area.width);
        }
        (&self.input).render(input_area, buf);
    }

    /// Body: side-by-side panes at width >= [`WIDE_LAYOUT`], stacked below.
    fn render_body(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if area.width >= WIDE_LAYOUT {
            self.render_wide(area, buf);
        } else {
            self.render_stacked(area, buf);
        }
    }

    /// Wide layout: `listW = width/2 - 3`, `detailW = width - listW - 6`,
    /// each with a two-column pane frame and a two-column gap (Go math).
    fn render_wide(&self, area: Rect, buf: &mut Buffer) {
        let list_content = (usize::from(area.width) / 2).saturating_sub(3).max(20);
        let detail_content = usize::from(area.width)
            .saturating_sub(list_content.saturating_add(6))
            .max(20);
        let list_width = u16::try_from(list_content.saturating_add(2))
            .unwrap_or(area.width)
            .min(area.width);
        let remaining = area.width.saturating_sub(list_width.saturating_add(2));
        let detail_width = u16::try_from(detail_content.saturating_add(2))
            .unwrap_or(area.width)
            .min(remaining);
        let detail_x = area.x.saturating_add(list_width).saturating_add(2);
        let list_area = Rect::new(area.x, area.y, list_width, area.height);
        let detail_area = Rect::new(detail_x, area.y, detail_width, area.height);
        self.render_list_pane(list_area, buf);
        self.render_detail_pane(detail_area, buf);
    }

    /// Stacked layout: the list on top, the detail pane below it.
    fn render_stacked(&self, area: Rect, buf: &mut Buffer) {
        let detail_height = u16::try_from(self.detail_rows())
            .unwrap_or(u16::MAX)
            .min(area.height);
        let list_height = area.height.saturating_sub(detail_height);
        let list_area = Rect::new(area.x, area.y, area.width, list_height);
        let detail_area = Rect::new(
            area.x,
            area.y.saturating_add(list_height),
            area.width,
            detail_height,
        );
        self.render_list_pane(list_area, buf);
        self.render_detail_pane(detail_area, buf);
    }

    /// One framed list pane; an empty list renders `(no tasks)`.
    fn render_list_pane(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let block = task_pane_block(&self.theme);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let rows: Vec<TaskRow> = self
            .filtered
            .iter()
            .filter_map(|index| self.tasks.get(*index))
            .map(|task| TaskRow::from_scoped(task, &self.tasks))
            .collect();
        let list = TaskList::new(&rows, &self.theme)
            .cursor(self.cursor)
            .offset(self.offset);
        (&list).render(inner, buf);
    }

    /// One framed detail pane; no focus renders `(no task selected)`.
    fn render_detail_pane(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let block = task_pane_block(&self.theme);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let detail = self
            .filtered
            .get(self.cursor)
            .and_then(|index| self.tasks.get(*index))
            .map(|task| TaskDetail::from_scoped(task, &self.tasks));
        if let Some(detail) = detail {
            let widget = TaskDetailWidget::new(&detail, &self.theme);
            (&widget).render(inner, buf);
        } else {
            let widget = TaskDetailWidget::empty(&self.theme);
            (&widget).render(inner, buf);
        }
    }
    /// Footer: the compact status line, or the full key help in help mode.
    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.mode == Mode::Help {
            self.render_help(area, buf);
            return;
        }
        let bar = StatusBar::tasks(
            self.filtered.len(),
            self.tasks.len(),
            self.show_blocked,
            &self.status,
        );
        (&StatusBarWidget::new(&bar, &self.theme)).render(area, buf);
    }

    /// Full key help: one footer row per `FullHelp` column, dim-styled.
    fn render_help(&self, area: Rect, buf: &mut Buffer) {
        let style = style_from(&self.theme.dim);
        for (row, column) in (area.y..).zip(self.keys.full_help()) {
            if row >= area.y.saturating_add(area.height) {
                break;
            }
            let text = column
                .iter()
                .map(|binding| format!("{} {}", binding.help_key, binding.desc))
                .collect::<Vec<_>>()
                .join("  ");
            buf.set_line(
                area.x,
                row,
                &Line::from(Span::styled(text, style)),
                area.width,
            );
        }
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_frame(area, buf);
    }
}

/// The default task filter (Go `FilterTaskItems`): case-insensitive
/// whitespace-token AND over ID, title, priority, tags, and property keys and
/// values. An empty query returns every index, in original order.
#[must_use]
pub fn default_task_filter(query: &str, tasks: &[Task]) -> Vec<usize> {
    let tokens: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task_matches_all(task, &tokens))
        .map(|(index, _)| index)
        .collect()
}

/// Whether every token matches the task.
fn task_matches_all(task: &Task, tokens: &[String]) -> bool {
    tokens.iter().all(|token| task_matches_token(task, token))
}

/// Whether one token matches the task fields Go checks.
fn task_matches_token(task: &Task, token: &str) -> bool {
    if contains_fold(&task.id, token)
        || contains_fold(&task.title, token)
        || contains_fold(&task.priority, token)
    {
        return true;
    }
    if task.tags.iter().any(|tag| contains_fold(tag, token)) {
        return true;
    }
    task.properties
        .iter()
        .any(|(key, value)| contains_fold(key, token) || contains_fold(value, token))
}

/// Case-insensitive substring containment (Go `strings.Contains(strings.ToLower(...))`).
fn contains_fold(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ratatui::style::Color;

    use super::*;
    use crate::widgets::test_support::{buffer_lines, render, render_buffer};

    /// Builds a task with the given id, title, and checkbox state.
    fn task(id: &str, title: &str, checked: bool) -> Task {
        Task {
            id: id.to_owned(),
            title: title.to_owned(),
            checked,
            status: if checked { b'x' } else { b' ' },
            ..Task::default()
        }
    }

    /// Builds a property map for task fixtures.
    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// Builds a blocked task whose `blocked_by` points at `blocker`.
    fn blocked(id: &str, title: &str, blocker: &str) -> Task {
        Task {
            property_order: vec!["blocked_by".to_owned()],
            properties: props(&[("blocked_by", blocker)]),
            ..task(id, title, false)
        }
    }

    fn three_tasks() -> Vec<Task> {
        vec![
            task("PRJ-001", "alpha", false),
            task("PRJ-002", "beta", false),
            task("PRJ-003", "gamma", false),
        ]
    }

    fn app() -> App {
        App::with_default_filter(three_tasks(), Config::default())
    }

    fn apply(app: &mut App, msg: Msg) -> Option<Cmd> {
        app.update(msg)
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Msg {
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn rune_key(c: char) -> Msg {
        key(KeyCode::Char(c))
    }

    fn resize(app: &mut App, width: u16, height: u16) {
        let _ = apply(app, Msg::Resize { width, height });
    }

    fn render_lines(app: &App, width: u16, height: u16) -> Vec<String> {
        buffer_lines(&render_buffer(app, width, height))
    }

    fn ids<'a>(tasks: &'a [Task], indices: &[usize]) -> Vec<&'a str> {
        indices
            .iter()
            .filter_map(|index| tasks.get(*index))
            .map(|task| task.id.as_str())
            .collect()
    }

    #[test]
    fn task_ui_keymap_matches_go_bindings() {
        let km = default_task_key_map();
        let want: [(&Binding, &[&str]); 21] = [
            (&km.up, &["up", "k"]),
            (&km.down, &["down", "j"]),
            (&km.page_up, &["pgup", "ctrl+b"]),
            (&km.page_down, &["pgdn", "ctrl+f"]),
            (&km.home, &["home", "g"]),
            (&km.end, &["end", "G"]),
            (&km.filter, &["/"]),
            (&km.esc, &["esc"]),
            (&km.tab, &["tab"]),
            (&km.toggle_done, &["x"]),
            (&km.toggle_blk, &["b"]),
            (&km.open, &["o"]),
            (&km.edit_body, &["e"]),
            (&km.edit_title, &["t"]),
            (&km.new, &["n"]),
            (&km.move_to, &["m"]),
            (&km.archive, &["a"]),
            (&km.help, &["?"]),
            (&km.quit, &["q", "ctrl+c"]),
            (&km.save, &["ctrl+s"]),
            (&km.cancel, &["esc"]),
        ];
        for (binding, keys) in want {
            assert_eq!(binding.keys, keys, "binding {}", binding.desc);
        }
        assert!(!km.short_help().is_empty(), "ShortHelp must not be empty");
        assert!(!km.full_help().is_empty(), "FullHelp must not be empty");
    }

    #[test]
    fn task_ui_keymap_help_columns_match_go() {
        let km = default_task_key_map();
        let short = km
            .short_help()
            .iter()
            .map(|binding| binding.desc)
            .collect::<Vec<_>>();
        assert_eq!(
            short,
            [
                "up",
                "down",
                "filter",
                "toggle done",
                "edit title",
                "help",
                "quit"
            ]
        );
        let full = km
            .full_help()
            .iter()
            .map(|column| {
                column
                    .iter()
                    .map(|binding| binding.desc)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            full,
            [
                vec!["up", "down", "page up", "page down", "top", "bottom"],
                vec![
                    "filter",
                    "clear filter / quit",
                    "pane focus",
                    "toggle done",
                    "show blocked",
                    "help"
                ],
                vec![
                    "open in $EDITOR",
                    "edit title",
                    "edit body",
                    "new task",
                    "move to file",
                    "archive"
                ],
                vec!["save", "cancel", "quit"],
            ]
        );
    }

    #[test]
    fn task_ui_key_name_covers_go_names() {
        let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(key_name(&plain(KeyCode::Up)), "up");
        assert_eq!(key_name(&plain(KeyCode::PageUp)), "pgup");
        assert_eq!(key_name(&plain(KeyCode::PageDown)), "pgdn");
        assert_eq!(key_name(&plain(KeyCode::Tab)), "tab");
        assert_eq!(key_name(&plain(KeyCode::Esc)), "esc");
        assert_eq!(key_name(&plain(KeyCode::Enter)), "enter");
        assert_eq!(key_name(&plain(KeyCode::Char('G'))), "G");
        assert_eq!(key_name(&plain(KeyCode::Char('/'))), "/");
        assert_eq!(key_name(&plain(KeyCode::Char('?'))), "?");
        assert_eq!(
            key_name(&KeyEvent::new(
                KeyCode::Char('S'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            "ctrl+s"
        );
        let km = default_task_key_map();
        assert!(km
            .quit
            .matches(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(!km.quit.matches(&plain(KeyCode::Char('c'))));
    }

    #[test]
    fn task_ui_default_filter_matches_go_table() {
        let items = vec![
            Task {
                id: "PRJ-001".to_owned(),
                title: "Fix login bug".to_owned(),
                tags: vec!["auth".to_owned()],
                priority: "P1".to_owned(),
                property_order: vec!["blocked_by".to_owned()],
                properties: props(&[("blocked_by", "PRJ-009")]),
                ..Task::default()
            },
            Task {
                id: "PRJ-002".to_owned(),
                title: "Write docs".to_owned(),
                tags: vec!["docs".to_owned(), "website".to_owned()],
                priority: "P2".to_owned(),
                ..Task::default()
            },
            Task {
                id: "OPS-003".to_owned(),
                title: "Rotate credentials".to_owned(),
                tags: vec!["secret".to_owned()],
                priority: "P0".to_owned(),
                ..Task::default()
            },
        ];
        let cases: [(&str, &[&str]); 16] = [
            ("", &["PRJ-001", "PRJ-002", "OPS-003"]),
            ("PRJ-001", &["PRJ-001"]),
            ("prj-001", &["PRJ-001"]),
            ("ops-", &["OPS-003"]),
            ("login", &["PRJ-001"]),
            ("ROTATE", &["OPS-003"]),
            ("website", &["PRJ-002"]),
            ("AUTH", &["PRJ-001"]),
            ("p1", &["PRJ-001"]),
            ("p0", &["OPS-003"]),
            ("prj-009", &["PRJ-001"]),
            ("blocked", &["PRJ-001"]),
            ("docs website", &["PRJ-002"]),
            ("docs auth", &[]),
            ("p2 website", &["PRJ-002"]),
            ("nonexistent-zzz", &[]),
        ];
        for (query, want) in cases {
            let got = ids(&items, &default_task_filter(query, &items));
            assert_eq!(got, want, "query {query:?}");
        }
    }

    #[test]
    fn task_ui_browse_x_toggles_done() {
        let mut app = app();
        assert!(!app.filtered_tasks()[0].checked, "starts open");
        let cmd = apply(&mut app, rune_key('x'));
        assert_eq!(cmd, None);
        assert_eq!(app.mode(), Mode::Browse, "x must not change mode");
        assert!(app.filtered_tasks()[0].checked, "x toggles done");
        assert_eq!(app.filtered_tasks()[0].status, b'x');
        let _ = apply(&mut app, rune_key('x'));
        assert!(!app.filtered_tasks()[0].checked, "second x untoggles");
        assert_eq!(app.filtered_tasks()[0].status, b' ');
    }

    #[test]
    fn task_ui_browse_b_toggles_blocked_gate() {
        let items = vec![
            task("PRJ-001", "alpha", false),
            blocked("PRJ-003", "blocked item", "PRJ-999"),
        ];
        let mut app = App::with_default_filter(items, Config::default());
        assert!(!app.show_blocked(), "blocked starts hidden");
        assert_eq!(app.filtered().len(), 1, "blocked task gated out");
        let _ = apply(&mut app, rune_key('b'));
        assert!(app.show_blocked());
        assert_eq!(app.filtered().len(), 2, "b reveals the blocked task");
        let _ = apply(&mut app, rune_key('b'));
        assert!(!app.show_blocked());
        assert_eq!(app.filtered().len(), 1);
    }

    #[test]
    fn task_ui_done_gate_hidden_unless_show_done() {
        let items = vec![
            task("PRJ-001", "open", false),
            task("PRJ-002", "done", true),
        ];
        let hidden = App::with_default_filter(items.clone(), Config::default());
        assert_eq!(hidden.filtered().len(), 1, "done hidden by default");
        let shown = App::with_default_filter(
            items,
            Config {
                show_done: true,
                ..Config::default()
            },
        );
        assert_eq!(shown.filtered().len(), 2, "ShowDone reveals done");
    }

    #[test]
    fn task_ui_resolved_blocker_is_visible() {
        let mut done_blocker = task("PRJ-009", "blocker done", true);
        done_blocker.status = b'x';
        let items = vec![done_blocker, blocked("PRJ-003", "blocked item", "PRJ-009")];
        let app = App::with_default_filter(
            items,
            Config {
                show_done: true,
                ..Config::default()
            },
        );
        // The blocker resolves against the full scope, so the dependent task
        // is not gated even with show_blocked == false.
        assert!(app.filtered().contains(&1));
        assert_eq!(app.filtered().len(), 2);
    }

    #[test]
    fn task_ui_slash_enters_filter_and_types() {
        let mut app = app();
        let _ = apply(&mut app, rune_key('/'));
        assert_eq!(app.mode(), Mode::Filter);
        assert!(app.filter_focused());
        for c in "beta".chars() {
            let _ = apply(&mut app, rune_key(c));
        }
        assert_eq!(app.query(), "beta");
        assert_eq!(app.filtered_tasks().len(), 1);
        assert_eq!(app.filtered_tasks()[0].id, "PRJ-002");
    }

    #[test]
    fn task_ui_filter_mode_x_types_and_does_not_toggle_done() {
        let mut app = app();
        let _ = apply(&mut app, rune_key('/'));
        let _ = apply(&mut app, rune_key('x'));
        assert_eq!(app.query(), "x");
        assert!(
            app.tasks().iter().all(|task| !task.checked),
            "x in filter mode must not toggle done"
        );
        assert_eq!(app.mode(), Mode::Filter);
    }

    #[test]
    fn task_ui_filter_mode_esc_clears_and_blurs() {
        let mut app = app();
        let _ = apply(&mut app, rune_key('/'));
        let _ = apply(&mut app, rune_key('a'));
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.query(), "");
        assert_eq!(app.filtered().len(), 3);
        assert!(!app.filter_focused());
        assert!(!app.is_aborted(), "esc in filter mode must not abort");
    }

    #[test]
    fn task_ui_cursor_wraps_both_ways() {
        let mut app = app();
        let _ = apply(&mut app, key(KeyCode::Up));
        assert_eq!(app.cursor_index(), 2, "up wraps to the bottom");
        let _ = apply(&mut app, rune_key('j'));
        assert_eq!(app.cursor_index(), 0, "down wraps to the top");
    }

    #[test]
    fn task_ui_esc_aborts_on_empty_filter() {
        let mut app = app();
        let cmd = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(cmd, Some(Cmd::Quit));
        assert!(app.is_aborted());
    }

    #[test]
    fn task_ui_esc_clears_filter_first_then_aborts() {
        let mut app = app();
        let _ = apply(&mut app, rune_key('/'));
        let _ = apply(&mut app, rune_key('a'));
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert!(!app.is_aborted());
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert!(app.is_aborted(), "second esc aborts");
    }

    #[test]
    fn task_ui_browse_esc_clears_programmatic_query() {
        let mut app = app();
        app.set_query("beta");
        assert_eq!(app.filtered().len(), 1);
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(app.query(), "");
        assert_eq!(app.filtered().len(), 3);
        assert!(!app.is_aborted());
    }

    #[test]
    fn task_ui_quit_keys_abort() {
        for msg in [rune_key('q'), ctrl('c')] {
            let mut app = app();
            let cmd = apply(&mut app, msg);
            assert_eq!(cmd, Some(Cmd::Quit));
            assert!(app.is_aborted());
        }
    }

    #[test]
    fn task_ui_cursor_follows_task_across_refilter() {
        let mut app = App::with_default_filter(
            vec![
                task("PRJ-001", "alpha", false),
                task("PRJ-002", "beta", false),
                task("PRJ-003", "tag gamma", false),
                task("PRJ-004", "tag delta", false),
            ],
            Config::default(),
        );
        let _ = apply(&mut app, rune_key('j'));
        let _ = apply(&mut app, rune_key('j'));
        assert_eq!(app.filtered_tasks()[app.cursor_index()].id, "PRJ-003");

        app.set_query("tag");
        assert_eq!(ids(app.tasks(), app.filtered()), ["PRJ-003", "PRJ-004"]);
        assert_eq!(app.cursor_index(), 0, "cursor follows PRJ-003");
        assert_eq!(app.filtered_tasks()[0].id, "PRJ-003");

        app.set_query("delta");
        assert_eq!(app.cursor_index(), 0, "cursor resets when its task is gone");
        assert_eq!(app.filtered_tasks()[0].id, "PRJ-004");
    }

    #[test]
    fn task_ui_home_end_and_page_keys() {
        let items: Vec<Task> = (0..6)
            .map(|i| task(&format!("PRJ-{i:03}"), "item", false))
            .collect();
        let mut app = App::with_default_filter(items, Config::default());
        resize(&mut app, 80, 8);
        // visible rows: 8 - 1 status - detailRows((8-1)/3 = 3) = 4.
        let _ = apply(&mut app, key(KeyCode::PageDown));
        assert_eq!(app.cursor_index(), 4);
        let _ = apply(&mut app, key(KeyCode::PageDown));
        assert_eq!(app.cursor_index(), 2, "page wraps 4+4 mod 6");
        let _ = apply(&mut app, key(KeyCode::Home));
        assert_eq!(app.cursor_index(), 0);
        let _ = apply(&mut app, key(KeyCode::End));
        assert_eq!(app.cursor_index(), 5);
        let _ = apply(&mut app, key(KeyCode::PageUp));
        assert_eq!(app.cursor_index(), 1, "page up 5-4");
    }

    #[test]
    fn task_ui_tab_toggles_focus_pane() {
        let mut app = app();
        assert_eq!(app.focus_pane(), 0);
        let _ = apply(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus_pane(), 1);
        let _ = apply(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus_pane(), 0);
    }

    #[test]
    fn task_ui_help_mode_toggles() {
        let mut app = app();
        let _ = apply(&mut app, rune_key('?'));
        assert_eq!(app.mode(), Mode::Help);
        let _ = apply(&mut app, key(KeyCode::Esc));
        assert_eq!(app.mode(), Mode::Browse);
        let _ = apply(&mut app, rune_key('?'));
        let _ = apply(&mut app, rune_key('?'));
        assert_eq!(app.mode(), Mode::Browse, "? dismisses help too");
    }

    #[test]
    fn task_ui_resize_keeps_cursor_visible() {
        let items: Vec<Task> = (0..30)
            .map(|i| task(&format!("PRJ-{i:03}"), "item", false))
            .collect();
        let mut app = App::with_default_filter(items, Config::default());
        resize(&mut app, 80, 5);
        for _ in 0..5 {
            let _ = apply(&mut app, rune_key('j'));
        }
        assert_eq!(app.cursor_index(), 5);
        // visible rows: 5 - 1 status - detailRows((5-1)/3 clamped to 3) = 3.
        assert_eq!(app.offset(), 3, "three visible rows scroll the window");
    }

    #[test]
    fn task_ui_injected_filter_is_used() {
        let filter: TaskFilterFn = Box::new(|query: &str, tasks: &[Task]| {
            tasks
                .iter()
                .enumerate()
                .filter(|(_, task)| task.title == query)
                .map(|(index, _)| index)
                .collect()
        });
        let mut app = App::new(three_tasks(), Config::default(), filter);
        assert_eq!(app.filtered().len(), 0, "empty query matches no title");
        app.set_query("beta");
        assert_eq!(app.filtered(), [1]);
    }

    fn view_tasks() -> Vec<Task> {
        let mut first = task("PRJ-001", "Fix login bug", false);
        first.header_raw =
            "- [ ] PRJ-001 Fix login bug #auth #ui !P1 @owner:me @blocked_by:PRJ-009".to_owned();
        first.tags = vec!["auth".to_owned(), "ui".to_owned()];
        first.priority = "P1".to_owned();
        first.property_order = vec!["owner".to_owned(), "blocked_by".to_owned()];
        first.properties = props(&[("owner", "me"), ("blocked_by", "PRJ-009")]);
        first.body = "first body line\nsecond body line".to_owned();
        vec![first, task("PRJ-002", "Done thing", true)]
    }

    fn view_app() -> App {
        App::with_default_filter(
            view_tasks(),
            Config {
                show_blocked: true,
                show_done: true,
                ..Config::default()
            },
        )
    }

    #[test]
    fn task_ui_view_renders_list_and_detail() {
        let app = view_app();
        let lines = render_lines(&app, 120, 40);
        let text = lines.join("\n");
        for want in [
            "PRJ-001",
            "Fix login bug",
            "#auth",
            "#ui",
            "!P1",
            "first body line",
            "Done thing",
        ] {
            assert!(text.contains(want), "frame missing {want:?}:\n{text}");
        }
    }

    #[test]
    fn task_ui_view_filter_line_and_status_footer() {
        let mut app = view_app();
        let browse = render_lines(&app, 80, 10);
        assert!(
            browse.last().is_some_and(|line| line.contains("2/2")),
            "footer shows visible/total:\n{browse:?}"
        );
        assert!(
            browse
                .last()
                .is_some_and(|line| line.contains("blocked:shown")),
            "footer shows the blocked toggle:\n{browse:?}"
        );
        assert!(
            browse
                .last()
                .is_some_and(|line| line.contains("x:done") && line.contains("?:help")),
            "footer shows the key hints:\n{browse:?}"
        );

        let _ = apply(&mut app, rune_key('/'));
        let filtering = render_lines(&app, 80, 10);
        assert!(
            filtering[0].starts_with("/ "),
            "filter prompt is the first line: {:?}",
            filtering[0]
        );
        assert!(
            filtering[0].contains("Filter tasks..."),
            "placeholder expected: {:?}",
            filtering[0]
        );
        let _ = apply(&mut app, rune_key('b'));
        let typed = render_lines(&app, 80, 10);
        assert!(
            typed[0].contains('b'),
            "typed query renders: {:?}",
            typed[0]
        );
    }

    #[test]
    fn task_ui_view_wide_joins_narrow_stacks() {
        let mut app = view_app();
        resize(&mut app, 120, 40);
        let wide = render_lines(&app, 120, 40);
        assert!(
            wide[0].matches('╭').count() >= 2,
            "wide first line joins two panes: {:?}",
            wide[0]
        );
        resize(&mut app, 80, 40);
        let stacked = render_lines(&app, 80, 40);
        assert_eq!(
            stacked[0].matches('╭').count(),
            1,
            "stacked first line has one pane: {:?}",
            stacked[0]
        );
    }

    #[test]
    fn task_ui_view_empty_list_renders_no_tasks() {
        let app = App::with_default_filter(Vec::new(), Config::default());
        let lines = render_lines(&app, 80, 20);
        assert!(
            lines.iter().any(|line| line.contains("(no tasks)")),
            "empty list placeholder:\n{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("(no task selected)")),
            "empty detail placeholder:\n{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("0/0")),
            "footer counts zero:\n{lines:?}"
        );
    }

    #[test]
    fn task_ui_view_clamps_every_frame() {
        for (width, height) in [(120, 40), (100, 30), (80, 30), (40, 10), (1, 1), (20, 5)] {
            let mut app = view_app();
            resize(&mut app, width, height);
            let lines = render_lines(&app, width, height);
            assert_eq!(lines.len(), usize::from(height), "size {width}x{height}");
            for line in &lines {
                assert!(
                    line.chars().count() <= usize::from(width),
                    "size {width}x{height}: line {line:?} exceeds width"
                );
            }
        }
    }

    #[test]
    fn task_ui_view_many_items_do_not_overflow() {
        let many: Vec<Task> = (0..500)
            .map(|i| task(&format!("PRJ-{i:03}"), "task", false))
            .collect();
        let mut app = App::with_default_filter(many, Config::default());
        resize(&mut app, 40, 10);
        let lines = render_lines(&app, 40, 10);
        assert_eq!(lines.len(), 10);
        let text = lines.join("\n");
        assert!(text.contains("PRJ-000"), "first row renders");
        assert!(text.contains("more"), "more-rows hint renders:\n{text}");
        for line in &lines {
            assert!(line.chars().count() <= 40);
        }
    }

    #[test]
    fn task_ui_view_styles_come_from_theme() {
        let app = view_app();
        let buffer = render_buffer(&app, 120, 40);
        let seen = |wanted: Color| {
            (0..40).any(|y| {
                (0..120).any(|x| buffer.cell((x, y)).is_some_and(|cell| cell.fg == wanted))
            })
        };
        assert!(
            seen(Color::Indexed(240)),
            "done row uses the theme task_done color"
        );
        assert!(
            seen(Color::Indexed(82)),
            "cursor row uses the theme task_selected color"
        );
        assert!(
            seen(Color::Indexed(238)),
            "panes use the theme task_pane border color"
        );
    }

    #[test]
    fn task_ui_view_blocker_style_from_theme() {
        let items = vec![
            task("PRJ-001", "alpha", false),
            blocked("PRJ-003", "blocked item", "PRJ-999"),
        ];
        let app = App::with_default_filter(
            items,
            Config {
                show_blocked: true,
                ..Config::default()
            },
        );
        let buffer = render_buffer(&app, 80, 20);
        let blocker_seen = (0..20).any(|y| {
            (0..80).any(|x| {
                buffer
                    .cell((x, y))
                    .is_some_and(|cell| cell.fg == Color::Indexed(203))
            })
        });
        assert!(
            blocker_seen,
            "@blocked_by uses the theme task_blocker color"
        );
    }

    #[test]
    fn task_ui_view_help_and_editor_frames_render() {
        let mut app = view_app();
        let _ = apply(&mut app, rune_key('?'));
        let help = render_lines(&app, 80, 10);
        let text = help.join("\n");
        assert!(text.contains("page up"), "help shows bindings:\n{text}");
        assert!(!text.contains("x:done"), "help replaces the status line");

        // The edit frame is an extension point (todo 25 seeds the buffer), but
        // the render path is defined now and must not panic or overflow.
        app.mode = Mode::EditTitle;
        let editor = render_lines(&app, 60, 8);
        assert!(
            editor.iter().any(|line| line.contains("ctrl+s save")),
            "editor status renders:\n{editor:?}"
        );
    }

    #[test]
    fn task_ui_extension_state_reserved_for_later_todos() {
        let mut app = app();
        assert_eq!(app.editor_text(), "");
        assert_eq!(app.pending_edit(), "");
        assert_eq!(app.move_input().value(), "");
        assert!(!app.is_new_task());
        assert_eq!(app.base(), "");
        assert_eq!(app.task_config(), &task_config::Config::default());
        assert_eq!(app.action(), "");
        assert_eq!(app.error(), None);
        assert!(!app.is_confirmed());
        // The reserved message/command seam is inert in this slice.
        assert_eq!(apply(&mut app, Msg::EditorFinished { error: None }), None);
        assert_eq!(apply(&mut app, rune_key('t')), None, "t is todo 25's");
        assert_eq!(app.mode(), Mode::Browse);
    }

    #[test]
    fn task_ui_zero_area_is_safe() {
        let app = app();
        assert!(render(&app, 0, 0).is_empty());
    }

    #[test]
    fn task_ui_view_detail_holds_full_body() {
        let app = view_app();
        let lines = render_lines(&app, 120, 60);
        let text = lines.join("\n");
        assert!(text.contains("first body line"), "detail body:\n{text}");
        assert!(text.contains("second body line"), "detail body:\n{text}");
    }
}
