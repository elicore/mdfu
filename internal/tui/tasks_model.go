package tui

import (
	"strconv"
	"strings"

	bubbleskey "github.com/charmbracelet/bubbles/key"
	"github.com/charmbracelet/bubbles/textarea"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/task"
	"github.com/elicore/mdfu/internal/theme"
)

// TaskItem is the tasks TUI's view of one task. The engine value is embedded
// (not held in a field named Task) so item.Title, item.ID, item.Checked and
// friends promote directly and the package selector task.Task stays usable
// inside this file.
type TaskItem struct {
	task.Task
	Value string
	Score float64
}

// TaskFilterFunc narrows task items by query. It is a hook so the real task
// filter can be injected by the runner without this model depending on it.
type TaskFilterFunc func(query string, items []TaskItem) []TaskItem

// TaskMode is the current input mode of the tasks TUI. Browse and filter are
// the list modes; the edit modes own the text widgets.
type TaskMode int

const (
	taskModeBrowse TaskMode = iota
	taskModeFilter
	taskModeEditTitle
	taskModeEditBody
	taskModeMovePrompt
	taskModeHelp
)

// TaskKeyMap holds every key binding the tasks TUI understands. It implements
// bubbles/help's KeyMap interface (ShortHelp/FullHelp) so a help footer can be
// rendered directly from it.
type TaskKeyMap struct {
	Up       bubbleskey.Binding
	Down     bubbleskey.Binding
	PageUp   bubbleskey.Binding
	PageDown bubbleskey.Binding
	Home     bubbleskey.Binding
	End      bubbleskey.Binding

	Filter bubbleskey.Binding
	Esc    bubbleskey.Binding
	Tab    bubbleskey.Binding

	ToggleDone bubbleskey.Binding
	ToggleBlk  bubbleskey.Binding
	Open       bubbleskey.Binding
	EditBody   bubbleskey.Binding
	EditTitle  bubbleskey.Binding
	New        bubbleskey.Binding
	Move       bubbleskey.Binding
	Archive    bubbleskey.Binding
	Help       bubbleskey.Binding
	Quit       bubbleskey.Binding

	// Save and Cancel apply only inside the edit modes.
	Save   bubbleskey.Binding
	Cancel bubbleskey.Binding
}

// DefaultTaskKeyMap returns the documented bindings. Every binding lists the
// alternatives in the order the tests assert them.
func DefaultTaskKeyMap() TaskKeyMap {
	return TaskKeyMap{
		Up:       bubbleskey.NewBinding(bubbleskey.WithKeys("up", "k"), bubbleskey.WithHelp("↑/k", "up")),
		Down:     bubbleskey.NewBinding(bubbleskey.WithKeys("down", "j"), bubbleskey.WithHelp("↓/j", "down")),
		PageUp:   bubbleskey.NewBinding(bubbleskey.WithKeys("pgup", "ctrl+b"), bubbleskey.WithHelp("pgup/ctrl+b", "page up")),
		PageDown: bubbleskey.NewBinding(bubbleskey.WithKeys("pgdn", "ctrl+f"), bubbleskey.WithHelp("pgdn/ctrl+f", "page down")),
		Home:     bubbleskey.NewBinding(bubbleskey.WithKeys("home", "g"), bubbleskey.WithHelp("home/g", "top")),
		End:      bubbleskey.NewBinding(bubbleskey.WithKeys("end", "G"), bubbleskey.WithHelp("end/G", "bottom")),

		Filter: bubbleskey.NewBinding(bubbleskey.WithKeys("/"), bubbleskey.WithHelp("/", "filter")),
		Esc:    bubbleskey.NewBinding(bubbleskey.WithKeys("esc"), bubbleskey.WithHelp("esc", "clear filter / quit")),
		Tab:    bubbleskey.NewBinding(bubbleskey.WithKeys("tab"), bubbleskey.WithHelp("tab", "pane focus")),

		ToggleDone: bubbleskey.NewBinding(bubbleskey.WithKeys("x"), bubbleskey.WithHelp("x", "toggle done")),
		ToggleBlk:  bubbleskey.NewBinding(bubbleskey.WithKeys("b"), bubbleskey.WithHelp("b", "show blocked")),
		Open:       bubbleskey.NewBinding(bubbleskey.WithKeys("o"), bubbleskey.WithHelp("o", "open in $EDITOR")),
		EditBody:   bubbleskey.NewBinding(bubbleskey.WithKeys("e"), bubbleskey.WithHelp("e", "edit body")),
		EditTitle:  bubbleskey.NewBinding(bubbleskey.WithKeys("t"), bubbleskey.WithHelp("t", "edit title")),
		New:        bubbleskey.NewBinding(bubbleskey.WithKeys("n"), bubbleskey.WithHelp("n", "new task")),
		Move:       bubbleskey.NewBinding(bubbleskey.WithKeys("m"), bubbleskey.WithHelp("m", "move to file")),
		Archive:    bubbleskey.NewBinding(bubbleskey.WithKeys("a"), bubbleskey.WithHelp("a", "archive")),
		Help:       bubbleskey.NewBinding(bubbleskey.WithKeys("?"), bubbleskey.WithHelp("?", "help")),
		Quit:       bubbleskey.NewBinding(bubbleskey.WithKeys("q", "ctrl+c"), bubbleskey.WithHelp("q/ctrl+c", "quit")),

		Save:   bubbleskey.NewBinding(bubbleskey.WithKeys("ctrl+s"), bubbleskey.WithHelp("ctrl+s", "save")),
		Cancel: bubbleskey.NewBinding(bubbleskey.WithKeys("esc"), bubbleskey.WithHelp("esc", "cancel")),
	}
}

// ShortHelp lists the bindings shown in the compact footer.
func (k TaskKeyMap) ShortHelp() []bubbleskey.Binding {
	return []bubbleskey.Binding{k.Up, k.Down, k.Filter, k.ToggleDone, k.EditTitle, k.Help, k.Quit}
}

// FullHelp lists every binding, grouped by column.
func (k TaskKeyMap) FullHelp() [][]bubbleskey.Binding {
	return [][]bubbleskey.Binding{
		{k.Up, k.Down, k.PageUp, k.PageDown, k.Home, k.End},
		{k.Filter, k.Esc, k.Tab, k.ToggleDone, k.ToggleBlk, k.Help},
		{k.Open, k.EditTitle, k.EditBody, k.New, k.Move, k.Archive},
		{k.Save, k.Cancel, k.Quit},
	}
}

// TaskConfig controls tasks TUI behaviour. Theme nil resolves to theme.Default.
// Base and Config are additive: Base is the resolved task scope (a base
// directory or a single --path file) and Config is the task configuration,
// both used by the write/reload actions.
type TaskConfig struct {
	Theme       *theme.Theme
	ShowBlocked bool
	ShowDone    bool
	Base        string
	Config      task.Config
}

// TaskModel is the BubbleTea model for the tasks TUI. Like the note picker's
// Model it is kept value-receiver and headless-testable.
type TaskModel struct {
	items    []TaskItem
	filtered []TaskItem
	filter   TaskFilterFunc

	cursor int
	offset int
	width  int
	height int

	mode        TaskMode
	focusPane   int
	showBlocked bool
	showDone    bool

	input     textinput.Model
	editor    textarea.Model
	moveInput textinput.Model
	keys      TaskKeyMap
	th        theme.Theme

	status  string
	err     error
	aborted bool

	// onSave is the disk-write hook the later edit todos replace. It is nil by
	// default, so ctrl+s in an edit mode currently returns to browse without
	// touching disk.
	onSave func() error

	// pendingEdit holds the widget text seeded by enterEdit, so a save/cancel
	// can decide whether anything changed even after the widget is blurred.
	pendingEdit string

	// base and taskCfg are the resolved task scope the write/reload actions
	// operate against; newTask marks a taskModeEditTitle editor as a new-task
	// creation rather than an edit of the focused task; action is the last
	// TaskResult action.
	base    string
	taskCfg task.Config
	newTask bool
	action  string
}

// NewTaskModel builds a TaskModel with the default task filter.
func NewTaskModel(items []TaskItem, cfg TaskConfig) TaskModel {
	return NewTaskModelWithFilter(items, cfg, nil)
}

// NewTaskModelWithFilter builds a TaskModel with an explicit filter hook. A
// nil filter falls back to FilterTaskItems. This is the only entry point the
// runner should use.
func NewTaskModelWithFilter(items []TaskItem, cfg TaskConfig, f func(string, []TaskItem) []TaskItem) TaskModel {
	if f == nil {
		f = FilterTaskItems
	}
	ti := textinput.New()
	ti.Placeholder = "Filter tasks..."
	ti.Prompt = "/ "
	ti.CharLimit = 500
	ti.Width = 50
	_ = ti.Focus()

	mi := textinput.New()
	mi.Placeholder = "destination file"
	mi.Prompt = "move> "
	mi.CharLimit = 500
	mi.Width = 50

	ed := textarea.New()
	ed.Placeholder = "task body"
	ed.SetWidth(60)
	ed.SetHeight(8)
	ed.CharLimit = 0

	th := theme.Default()
	if cfg.Theme != nil {
		th = *cfg.Theme
	}

	cp := make([]TaskItem, len(items))
	copy(cp, items)

	m := TaskModel{
		items:       cp,
		filter:      f,
		input:       ti,
		editor:      ed,
		moveInput:   mi,
		keys:        DefaultTaskKeyMap(),
		th:          th,
		showBlocked: cfg.ShowBlocked,
		showDone:    cfg.ShowDone,
		base:        cfg.Base,
		taskCfg:     cfg.Config,
	}
	m.refilter()
	return m
}

// Init implements tea.Model.
func (m TaskModel) Init() tea.Cmd {
	return textinput.Blink
}

// Update implements tea.Model.
func (m TaskModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		if msg.Width > 10 {
			w := msg.Width - 4
			if w < 10 {
				w = 10
			}
			m.input.Width = w
			m.moveInput.Width = w
		}
		if msg.Height > 4 {
			m.editor.SetHeight(msg.Height - 4)
		}
		m.clampCursor()
		m.ensureVisible()
		return m, nil
	case taskEditorFinishedMsg:
		if msg.err != nil {
			m.err = msg.err
			m.status = msg.err.Error()
			return m, nil
		}
		m.reload()
		return m, nil
	case tea.KeyMsg:
		switch m.mode {
		case taskModeEditTitle, taskModeEditBody:
			return m.updateEdit(msg)
		case taskModeMovePrompt:
			return m.updateMove(msg)
		case taskModeFilter:
			return m.updateFilter(msg)
		case taskModeHelp:
			return m.updateHelp(msg)
		default:
			return m.updateBrowse(msg)
		}
	}
	// Let cursor blinking and friends flow to whichever widget is focused.
	return m.updateFocusedWidget(msg)
}

// updateBrowse handles taskModeBrowse. Key bindings are checked here before any
// text widget sees the key, so a browse-mode "x" toggles done rather than
// typing into a filter box.
func (m TaskModel) updateBrowse(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case bubbleskey.Matches(msg, m.keys.Quit):
		m.aborted = true
		return m, tea.Quit
	case bubbleskey.Matches(msg, m.keys.Esc):
		if m.input.Value() != "" {
			m.input.SetValue("")
			m.refilter()
			return m, nil
		}
		m.aborted = true
		return m, tea.Quit
	case bubbleskey.Matches(msg, m.keys.Filter):
		m.mode = taskModeFilter
		_ = m.input.Focus()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Up):
		m.moveCursor(-1)
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Down):
		m.moveCursor(1)
		return m, nil
	case bubbleskey.Matches(msg, m.keys.PageUp):
		m.moveCursor(-m.pageSize())
		return m, nil
	case bubbleskey.Matches(msg, m.keys.PageDown):
		m.moveCursor(m.pageSize())
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Home):
		m.cursor = 0
		m.ensureVisible()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.End):
		if len(m.filtered) > 0 {
			m.cursor = len(m.filtered) - 1
		}
		m.ensureVisible()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Tab):
		m.focusPane = 1 - m.focusPane
		return m, nil
	case bubbleskey.Matches(msg, m.keys.ToggleDone):
		if err := m.toggleDoneTask(); err != nil {
			m.err = err
			m.status = err.Error()
		}
		return m, nil
	case bubbleskey.Matches(msg, m.keys.ToggleBlk):
		m.showBlocked = !m.showBlocked
		m.refilter()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Open):
		if cmd := m.openInEditorCmd(); cmd != nil {
			return m, cmd
		}
		return m, nil
	case bubbleskey.Matches(msg, m.keys.EditTitle):
		m.enterEdit(taskModeEditTitle)
		return m, nil
	case bubbleskey.Matches(msg, m.keys.EditBody):
		m.enterEdit(taskModeEditBody)
		return m, nil
	case bubbleskey.Matches(msg, m.keys.New):
		m.enterNewTask()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Move):
		m.enterMove()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Archive):
		m.archiveFocused()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Help):
		m.mode = taskModeHelp
		return m, nil
	}
	return m, nil
}

// updateFilter handles taskModeFilter. All printable runes land in the filter
// input; only the mode-control keys are intercepted first. Critically, "x" and
// "b" are not special here.
func (m TaskModel) updateFilter(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case bubbleskey.Matches(msg, m.keys.Esc):
		m.input.SetValue("")
		m.refilter()
		m.mode = taskModeBrowse
		m.input.Blur()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Quit):
		m.aborted = true
		return m, tea.Quit
	case bubbleskey.Matches(msg, m.keys.Up):
		m.moveCursor(-1)
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Down):
		m.moveCursor(1)
		return m, nil
	}
	prev := m.input.Value()
	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	if m.input.Value() != prev {
		m.refilter()
	}
	return m, cmd
}

// updateEdit dispatches taskModeEditTitle and taskModeEditBody to their
// dedicated handlers in tasks_edit.go and tasks_edit_body.go.
func (m TaskModel) updateEdit(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if m.mode == taskModeEditBody {
		return m.updateEditBody(msg)
	}
	return m.updateEditTitle(msg)
}

// updateMove handles taskModeMovePrompt.
func (m TaskModel) updateMove(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case msg.String() == "enter":
		if err := m.commitMove(); err != nil {
			m.err = err
			m.status = err.Error()
			return m, nil
		}
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Cancel):
		m.status = "cancelled"
		m.mode = taskModeBrowse
		m.moveInput.Blur()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Quit):
		m.aborted = true
		return m, tea.Quit
	}
	var cmd tea.Cmd
	m.moveInput, cmd = m.moveInput.Update(msg)
	return m, cmd
}

// updateHelp handles taskModeHelp: any dismiss key returns to browse.
func (m TaskModel) updateHelp(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case bubbleskey.Matches(msg, m.keys.Help), bubbleskey.Matches(msg, m.keys.Esc):
		m.mode = taskModeBrowse
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Quit):
		m.aborted = true
		return m, tea.Quit
	}
	return m, nil
}

// updateFocusedWidget forwards non-key messages to the focused widget.
func (m TaskModel) updateFocusedWidget(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd
	switch m.mode {
	case taskModeEditTitle, taskModeEditBody:
		m.editor, cmd = m.editor.Update(msg)
	case taskModeMovePrompt:
		m.moveInput, cmd = m.moveInput.Update(msg)
	default:
		m.input, cmd = m.input.Update(msg)
	}
	return m, cmd
}

// enterEdit seeds the editor with the focused task's header (title mode) or
// body (body mode) and switches to the given edit mode.
func (m *TaskModel) enterEdit(mode TaskMode) {
	if m.cursor < 0 || m.cursor >= len(m.filtered) {
		return
	}
	it := m.filtered[m.cursor]
	if mode == taskModeEditBody {
		m.editor.SetValue(it.Body)
	} else {
		m.editor.SetValue(it.HeaderRaw)
	}
	m.pendingEdit = m.editor.Value()
	m.newTask = false
	m.mode = mode
	_ = m.editor.Focus()
}

// enterNewTask seeds the editor with an empty task header and switches to the
// title editor as a new-task creation.
func (m *TaskModel) enterNewTask() {
	m.editor.SetValue("- [ ] ")
	m.pendingEdit = m.editor.Value()
	m.newTask = true
	m.status = "new task"
	m.mode = taskModeEditTitle
	_ = m.editor.Focus()
}

// enterMove seeds the move prompt with the focused task's current file.
func (m *TaskModel) enterMove() {
	if m.cursor < 0 || m.cursor >= len(m.filtered) {
		return
	}
	m.moveInput.SetValue(m.filtered[m.cursor].File)
	m.mode = taskModeMovePrompt
	_ = m.moveInput.Focus()
}

// toggleDone flips the focused item's done state in memory. The disk write is
// the later actions todo's job; this keeps the model headless-testable.
func (m *TaskModel) toggleDone() {
	if m.cursor < 0 || m.cursor >= len(m.filtered) {
		return
	}
	it := m.filtered[m.cursor]
	it.Checked = !it.Checked
	if it.Checked {
		it.Task.Status = 'x'
	} else {
		it.Task.Status = ' '
	}
	m.filtered[m.cursor] = it
	// Mirror the flip into the backing slice so a refilter keeps it.
	for i := range m.items {
		if m.items[i].ID == it.ID && m.items[i].File == it.File && m.items[i].Line == it.Line {
			m.items[i] = it
			break
		}
	}
}

// moveCursor moves the cursor by delta, wrapping, and keeps it visible.
func (m *TaskModel) moveCursor(delta int) {
	if len(m.filtered) == 0 {
		m.cursor = 0
		return
	}
	m.cursor += delta
	n := len(m.filtered)
	m.cursor = ((m.cursor % n) + n) % n
	m.ensureVisible()
}

// pageSize is the number of rows a page-up/page-down jump moves.
func (m *TaskModel) pageSize() int {
	n := m.visibleRows()
	if n < 1 {
		return 1
	}
	return n
}

// refilter rebuilds the visible list from items using the injected filter and
// the showDone/showBlocked gates, keeping the cursor on the same task when it
// survives.
func (m *TaskModel) refilter() {
	prevKey := ""
	if m.cursor >= 0 && m.cursor < len(m.filtered) {
		prevKey = taskItemKey(m.filtered[m.cursor])
	}

	query := m.input.Value()
	var base []TaskItem
	if m.filter != nil {
		base = m.filter(query, m.items)
	} else {
		base = FilterTaskItems(query, m.items)
	}
	if base == nil {
		base = []TaskItem{}
	}
	all := m.allTasks()
	out := make([]TaskItem, 0, len(base))
	for _, it := range base {
		if it.Checked && !m.showDone {
			continue
		}
		if !m.showBlocked && task.HasUnresolvedBlockers(it.Task, all) {
			continue
		}
		out = append(out, it)
	}
	m.filtered = out

	if prevKey != "" {
		if idx := taskIndexOfKey(m.filtered, prevKey); idx >= 0 {
			m.cursor = idx
		} else {
			m.cursor = 0
			m.offset = 0
		}
	}
	m.clampCursor()
	m.ensureVisible()
}

// clampCursor bounds the cursor and offset to the filtered slice.
func (m *TaskModel) clampCursor() {
	if len(m.filtered) == 0 {
		m.cursor = 0
		m.offset = 0
		return
	}
	if m.cursor < 0 {
		m.cursor = 0
	}
	if m.cursor >= len(m.filtered) {
		m.cursor = len(m.filtered) - 1
	}
	if m.offset < 0 {
		m.offset = 0
	}
	if m.offset >= len(m.filtered) {
		m.offset = len(m.filtered) - 1
	}
}

// ensureVisible scrolls the offset so the cursor is inside the viewport.
func (m *TaskModel) ensureVisible() {
	maxRows := m.visibleRows()
	if m.cursor < m.offset {
		m.offset = m.cursor
	}
	if m.cursor >= m.offset+maxRows {
		m.offset = m.cursor - maxRows + 1
	}
	if m.offset < 0 {
		m.offset = 0
	}
}

// visibleRows returns how many list rows fit in the current height. The detail
// pane only competes for vertical space in the stacked (narrow) layout; in the
// side-by-side layout both panes share the full body budget.
func (m *TaskModel) visibleRows() int {
	if m.height <= 0 {
		return 20
	}
	reserved := 1 // status
	if m.mode == taskModeFilter {
		reserved++
	}
	if m.width < taskWideLayout {
		reserved += m.detailRows()
	}
	n := m.height - reserved
	if n < 3 {
		n = 3
	}
	return n
}

// detailRows returns the vertical budget for the stacked detail pane.
func (m *TaskModel) detailRows() int {
	if m.height <= 0 {
		return 10
	}
	p := (m.height - 1) / 3
	if p < 3 {
		p = 3
	}
	if p > 12 {
		p = 12
	}
	return p
}

// taskItemKey identifies an item across refilters. Identified tasks key by ID
// (stable across a move); an ID-less checkbox keys by file and line, which is
// unique even when two lines have the same raw text.
func taskItemKey(it TaskItem) string {
	if it.ID != "" {
		return "id:" + it.ID
	}
	return it.File + ":" + strconv.Itoa(it.Line)
}

// taskIndexOfKey finds the item with the given key.
func taskIndexOfKey(items []TaskItem, k string) int {
	for i := range items {
		if taskItemKey(items[i]) == k {
			return i
		}
	}
	return -1
}

// --- accessors for tests / integration ---

// Query returns the current filter text.
func (m *TaskModel) Query() string { return m.input.Value() }

// SetQuery sets the filter text programmatically and refilters.
func (m *TaskModel) SetQuery(q string) {
	m.input.SetValue(q)
	m.refilter()
}

// CursorIndex returns the cursor position within FilteredItems.
func (m *TaskModel) CursorIndex() int { return m.cursor }

// FilteredItems returns the currently visible items.
func (m *TaskModel) FilteredItems() []TaskItem { return m.filtered }

// AllItems returns every item.
func (m *TaskModel) AllItems() []TaskItem { return m.items }

// TaskMode returns the current mode.
func (m *TaskModel) TaskMode() TaskMode { return m.mode }

// IsAborted reports whether the user aborted (esc on an empty filter / q / ctrl+c).
func (m *TaskModel) IsAborted() bool { return m.aborted }

// IsConfirmed reports whether the user confirmed. The tasks TUI has no commit
// key yet; this exists for parity with the note picker's result contract.
func (m *TaskModel) IsConfirmed() bool { return false }

// ShowBlocked reports blocked visibility.
func (m *TaskModel) ShowBlocked() bool { return m.showBlocked }

// ShowDone reports done visibility.
func (m *TaskModel) ShowDone() bool { return m.showDone }

// Status returns the last status line text.
func (m *TaskModel) Status() string { return m.status }

// Err returns the last error surfaced by a hook.
func (m *TaskModel) Err() error { return m.err }

// SetOnSave installs the disk-write hook the edit/move modes call. Passing nil
// restores the no-op behaviour.
func (m *TaskModel) SetOnSave(fn func() error) { m.onSave = fn }

// FilterTaskItems is the default task filter. It matches the ID, title, tags,
// priority, and property values (keys and values), case-insensitively. A query
// is split on whitespace and every token must match (tokens AND-combined).
// An empty query returns all items.
func FilterTaskItems(query string, items []TaskItem) []TaskItem {
	tokens := strings.Fields(strings.ToLower(query))
	out := make([]TaskItem, 0, len(items))
	for _, it := range items {
		if taskMatchesAll(it, tokens) {
			out = append(out, it)
		}
	}
	return out
}

func taskMatchesAll(it TaskItem, tokens []string) bool {
	for _, tok := range tokens {
		if !taskMatchesToken(it, tok) {
			return false
		}
	}
	return true
}

func taskMatchesToken(it TaskItem, tok string) bool {
	if strings.Contains(strings.ToLower(it.ID), tok) {
		return true
	}
	if strings.Contains(strings.ToLower(it.Title), tok) {
		return true
	}
	if strings.Contains(strings.ToLower(it.Priority), tok) {
		return true
	}
	for _, tag := range it.Tags {
		if strings.Contains(strings.ToLower(tag), tok) {
			return true
		}
	}
	for key, val := range it.Properties {
		if strings.Contains(strings.ToLower(key), tok) || strings.Contains(strings.ToLower(val), tok) {
			return true
		}
	}
	return false
}
