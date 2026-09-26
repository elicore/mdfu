package tui

import (
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/theme"
)

// buildTaskModel resolves the markdown style from the configured theme while
// the terminal is still in cooked mode, then constructs the model through the
// explicit-injection constructor. Taking the filter as a parameter keeps the
// tasks TUI off the note picker's package-level global, and factoring it out
// makes the injection testable without a live program.
func buildTaskModel(items []TaskItem, cfg TaskConfig, f TaskFilterFunc) TaskModel {
	th := theme.Default()
	if cfg.Theme != nil {
		th = *cfg.Theme
	}
	if th.MarkdownStyle == "" {
		if lipgloss.HasDarkBackground() {
			th.MarkdownStyle = "dark"
		} else {
			th.MarkdownStyle = "light"
		}
	}
	cfg.Theme = &th
	return NewTaskModelWithFilter(items, cfg, f)
}

// RunTasks launches the tasks TUI and returns the final result. It mirrors the
// note picker's RunWithFilter: the theme's markdown style is resolved before
// the alt-screen program starts, an interrupt force-restores the terminal, and
// both value and pointer models returned by Run are handled. The model's
// injected filter defaults to FilterTaskItems; the global note-picker filter is
// never touched.
func RunTasks(items []TaskItem, cfg TaskConfig) (TaskResult, error) {
	m := buildTaskModel(items, cfg, nil)
	p := tea.NewProgram(m, tea.WithAltScreen())

	// BubbleTea only handles Ctrl-C between frames, so a slow render would
	// otherwise leave the process unquittable. Give the normal handler a
	// moment to quit cleanly, then restore the terminal and force-exit.
	interrupted := make(chan os.Signal, 1)
	signal.Notify(interrupted, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(interrupted)
	stopped := make(chan struct{})
	go func() {
		select {
		case <-interrupted:
		case <-stopped:
			return
		}
		select {
		case <-stopped:
		case <-time.After(500 * time.Millisecond):
			fmt.Fprint(os.Stderr, "\x1b[0m\x1b[?25h\x1b[?1049l")
			os.Exit(130)
		}
	}()

	final, err := p.Run()
	close(stopped)
	if err != nil {
		return TaskResult{}, err
	}
	fm, ok := final.(TaskModel)
	if !ok {
		// bubbletea may return a pointer in some versions; handle both.
		if fmp, ok2 := final.(*TaskModel); ok2 {
			fm = *fmp
		} else {
			return TaskResult{}, fmt.Errorf("tui: unexpected model type %T", final)
		}
	}
	return fm.Result(), nil
}
