package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"strings"
	"time"

	"github.com/elicore/mdfu/internal/task"
	"github.com/elicore/mdfu/internal/tui"
)

// isTerminal reports whether stdout is an interactive terminal. It is a
// package-level seam so tests can force either branch; the default is
// stdlib-only TTY detection (no new dependency).
var isTerminal = defaultIsTerminal

// stdin is the reader handed to task.Env. It is a package-level seam so tests
// can script the `ids` prompt answer.
var stdin io.Reader = os.Stdin

// runTasksTUI is the tasks TUI entry point. routeTasks calls it only when
// stdout is a TTY, so this is the only place a bubbletea program is built; the
// non-TTY branch stays byte-identical to `mdfu task list`. It parses the same
// flags as the list subcommand, resolves the same scope through the task
// engine, and hands the selected rows to tui.RunTasks.
var runTasksTUI = func(stdout, stderr io.Writer, args []string, themePath string) int {
	env := newTaskEnv(stdout, stderr)

	scope, rest, err := task.ResolveScope(env, args)
	if err != nil {
		fmt.Fprintf(stderr, "mdfu task: %s\n", err.Error())
		return 1
	}

	fs := flag.NewFlagSet("tasks", flag.ContinueOnError)
	fs.SetOutput(stderr)
	all := fs.Bool("all", false, "include done tasks")
	blocked := fs.Bool("blocked", false, "include blocked tasks")
	sortBy := fs.String("sort", "", "sort field")
	var tagFlags, priorityFlags repeatableFlag
	fs.Var(&tagFlags, "tag", "filter by tag (repeatable; AND)")
	fs.Var(&priorityFlags, "priority", "filter by priority (repeatable; OR)")

	if err := fs.Parse(task.ReorderInterspersed(fs, rest)); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}

	var extraTags, extraPriorities []string
	for _, arg := range fs.Args() {
		switch {
		case strings.HasPrefix(arg, "#"):
			extraTags = append(extraTags, arg)
		case strings.HasPrefix(arg, "!"):
			extraPriorities = append(extraPriorities, arg)
		default:
			fmt.Fprintf(stderr, "mdfu tasks: unexpected argument '%s'\n", arg)
			return 2
		}
	}
	tags := append(append([]string(nil), tagFlags...), extraTags...)
	priorities := append(append([]string(nil), priorityFlags...), extraPriorities...)

	tasks, _, err := task.LoadScope(scope)
	if err != nil {
		fmt.Fprintf(stderr, "mdfu task: %s\n", err.Error())
		return 1
	}
	selected := task.SelectTasks(tasks, tags, priorities, *sortBy)
	items := make([]tui.TaskItem, 0, len(selected))
	for _, tk := range selected {
		items = append(items, tui.TaskItem{Task: tk})
	}

	th, err := loadTheme(themePath, stderr)
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 2
	}

	_, err = tui.RunTasks(items, tui.TaskConfig{
		Theme:       th,
		ShowBlocked: *blocked,
		ShowDone:    *all,
		Base:        scope.Base,
		Config:      scope.Config,
	})
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 1
	}
	return 0
}

// repeatableFlag collects a repeatable string flag, mirroring the task
// engine's --tag/--priority list options.
type repeatableFlag []string

func (r *repeatableFlag) String() string { return strings.Join(*r, ",") }

func (r *repeatableFlag) Set(value string) error {
	*r = append(*r, value)
	return nil
}

// defaultIsTerminal implements the repo's first TTY detection: stdout is a
// terminal when its file mode is a character device.
func defaultIsTerminal() bool {
	info, err := os.Stdout.Stat()
	if err != nil {
		return false
	}
	return info.Mode()&os.ModeCharDevice != 0
}

// noColor reports whether the user disabled ANSI colour through a non-empty
// $NO_COLOR.
func noColor() bool {
	return os.Getenv("NO_COLOR") != ""
}

// taskIDPattern mirrors internal/task's dispatch regex: a full PREFIX-<n> or a
// bare numeric part.
var taskIDPattern = regexp.MustCompile(`^(?:[A-Z][A-Z0-9]*-\d+|\d+)$`)

// maybeRouteTask intercepts the `mdfu task` / `mdfu tasks` argv surface before
// the root flagset parses it. Every task flag (--path, --all, --blocked,
// --json, --sort, --tag, --priority, --prefix) is unknown to the root flagset
// and would otherwise abort with "flag provided but not defined", so the
// router must run first.
//
// It scans raw args left to right:
//   - a bare `--` before the candidate bypasses the router, so `mdfu -- task`
//     searches for "task";
//   - before the candidate only `--config <path>` / `--config=<path>` is
//     accepted (remembered as the theme path);
//   - the first non-flag token is the subcommand candidate; known root flags
//     are skipped past their value so `mdfu --root <v> task list` still finds
//     its candidate;
//   - any other root flag before a `task`/`tasks` candidate is a usage error
//     (E2, exit 2). When the candidate is not task/tasks the router steps
//     aside and the existing path is untouched.
//
// The returned bool reports whether the router handled the invocation; a true
// bool means run() must return the code immediately.
func maybeRouteTask(args []string, stdout, stderr io.Writer) (bool, int) {
	themePath := ""
	pendingFlag := ""
	candidate := -1

	for i := 0; i < len(args); i++ {
		a := args[i]
		if a == "--" {
			return false, 0
		}
		if a == "" || a == "-" || a[0] != '-' {
			candidate = i
			break
		}
		name, hasValue, value := splitFlagToken(a)
		if name == "config" {
			if hasValue {
				themePath = value
			} else if i+1 < len(args) {
				i++
				themePath = args[i]
			}
			continue
		}
		if pendingFlag == "" {
			pendingFlag = name
		}
		if !hasValue && rootFlagTakesValue(name) && i+1 < len(args) {
			i++
		}
	}

	if candidate < 0 {
		return false, 0
	}
	cand := args[candidate]
	if cand != "task" && cand != "tasks" {
		return false, 0
	}
	if themePath == defaultConfigValue {
		return false, 0
	}
	if pendingFlag != "" {
		fmt.Fprintf(stderr, "mdfu task: --%s must come after the task subcommand\n", pendingFlag)
		return true, 2
	}

	rest, themePath := stripConfigFlag(args[candidate+1:], themePath)
	if cand == "tasks" {
		return true, routeTasks(stdout, stderr, rest, themePath)
	}
	return true, routeTask(stdout, stderr, rest)
}

// routeTask implements the `mdfu task` dispatch table.
func routeTask(stdout, stderr io.Writer, rest []string) int {
	if len(rest) == 0 {
		return dispatchTask(stdout, stderr, []string{"list"})
	}
	if rest[0] == "--" {
		return dispatchTask(stdout, stderr, rest[1:])
	}
	if isTaskSubcommand(rest[0]) {
		return dispatchTask(stdout, stderr, rest)
	}
	if taskIDPattern.MatchString(rest[0]) {
		return dispatchTask(stdout, stderr, append([]string{"view"}, rest...))
	}
	// D2 fall-through: an unknown token is a fuzzy search with the leading
	// `task` token consumed, so `mdfu task retention` searches for "retention"
	// and `mdfu task task` searches for "task".
	query := strings.TrimSpace(strings.Join(rest, " "))
	return runFilter(stdout, stderr, ".", false, true, query, false, defaultFilterLimit, "paths")
}

// routeTasks implements `mdfu tasks`. When stdout is not a TTY it is
// byte-identical to `mdfu task list` with the same flags and constructs no
// bubbletea program; that fallback runs through task.Dispatch so the two paths
// share one renderer. When stdout is a TTY the runTasksTUI hook owns the
// invocation (its default still falls back to `task list`).
func routeTasks(stdout, stderr io.Writer, args []string, themePath string) int {
	if !isTerminal() {
		return dispatchTask(stdout, stderr, append([]string{"list"}, args...))
	}
	return runTasksTUI(stdout, stderr, args, themePath)
}

// dispatchTask builds the task Env and routes argv through the task registry.
func dispatchTask(stdout, stderr io.Writer, argv []string) int {
	return task.Dispatch(newTaskEnv(stdout, stderr), argv).Code
}

// newTaskEnv assembles the execution environment for the task layer.
func newTaskEnv(stdout, stderr io.Writer) task.Env {
	cwd, err := os.Getwd()
	if err != nil {
		cwd = "."
	}
	return task.Env{
		Stdout:   stdout,
		Stderr:   stderr,
		Stdin:    stdin,
		Terminal: task.Terminal{IsTTY: isTerminal(), NoColor: noColor()},
		Cwd:      cwd,
		Now:      time.Now,
	}
}

// isTaskSubcommand reports whether name is a registered task subcommand or
// alias.
func isTaskSubcommand(name string) bool {
	for _, sc := range task.Subcommands() {
		if sc.Name == name {
			return true
		}
		for _, alias := range sc.Aliases {
			if alias == name {
				return true
			}
		}
	}
	return false
}

// splitFlagToken splits a flag token into its bare name, whether it carries an
// inline value (`--flag=value`), and that value.
func splitFlagToken(a string) (name string, hasValue bool, value string) {
	name = strings.TrimLeft(a, "-")
	if eq := strings.IndexByte(name, '='); eq >= 0 {
		return name[:eq], true, name[eq+1:]
	}
	return name, false, ""
}

// rootFlagTakesValue reports whether a root flag consumes the following token.
// It mirrors the value-typed flags in run()'s root flagset; --config is
// handled separately.
func rootFlagTakesValue(name string) bool {
	switch name {
	case "root", "limit", "format":
		return true
	default:
		return false
	}
}

// stripConfigFlag removes every `--config <path>` / `--config=<path>` from the
// task args and returns the theme path it named. The CLI subcommands do not
// use a theme, so --config must never reach them as an unknown flag.
func stripConfigFlag(args []string, themePath string) ([]string, string) {
	out := make([]string, 0, len(args))
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--config":
			if i+1 < len(args) {
				themePath = args[i+1]
				i++
			}
		case strings.HasPrefix(a, "--config="):
			themePath = a[len("--config="):]
		default:
			out = append(out, a)
		}
	}
	return out, themePath
}
