package task

import (
	"encoding/json"
	"flag"
	"fmt"
	"strings"
)

// viewJSON is the SPEC §F.4 `view --json` object: the §F.1 keys in order via
// the embedded jsonTask, plus body last.
type viewJSON struct {
	jsonTask
	Body string `json:"body"`
}

// runView implements `mdfu task view` (alias `show`) per SPEC.md §F.4.
func runView(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	fs := flag.NewFlagSet("view", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit JSON")

	pos, done := parseFlags(env, fs, rest)
	if done != nil {
		return *done
	}
	if len(pos) != 1 {
		errf(env, "view requires exactly one task ID")
		return Result{Code: 2}
	}

	tasks, _, err := LoadScope(scope)
	if err != nil {
		return fail(env, err)
	}
	t, err := Resolve(pos[0], tasks)
	if err != nil {
		return fail(env, err)
	}

	if *jsonOut {
		return renderViewJSON(env, t)
	}
	return renderViewHuman(env, t)
}

// renderViewHuman writes the location line (TTY only), the verbatim header, and
// the six-space-indented body of SPEC.md §F.4.
func renderViewHuman(env Env, t Task) Result {
	if env.Terminal.IsTTY {
		loc := fmt.Sprintf("%s:%d", t.File, t.Line)
		if !env.Terminal.NoColor {
			loc = "\x1b[38;5;245m" + loc + "\x1b[0m"
		}
		fmt.Fprintln(env.Stdout, loc)
	}
	fmt.Fprintln(env.Stdout, t.HeaderRaw)
	if t.Body != "" {
		for _, line := range strings.Split(t.Body, "\n") {
			if line == "" {
				fmt.Fprintln(env.Stdout)
				continue
			}
			fmt.Fprintf(env.Stdout, "      %s\n", line)
		}
	}
	return Result{Code: 0}
}

// renderViewJSON writes the single §F.4 object. Unlike list, view never hides a
// resolved blocker, so properties come from the task's own property map.
func renderViewJSON(env Env, t Task) Result {
	tags := make([]string, 0, len(t.Tags))
	for _, tag := range t.Tags {
		tags = append(tags, sanitize(tag))
	}
	props := make(map[string]string, len(t.Properties))
	for key, value := range t.Properties {
		props[sanitize(key)] = sanitize(value)
	}
	status := "open"
	if t.Checked {
		status = "done"
	}

	enc := json.NewEncoder(env.Stdout)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(viewJSON{
		jsonTask: jsonTask{
			ID:         t.ID,
			Title:      sanitize(t.Title),
			Status:     status,
			Priority:   effectivePriority(t.Priority),
			Tags:       tags,
			Properties: props,
			File:       t.File,
			Line:       t.Line,
		},
		Body: t.Body,
	}); err != nil {
		return fail(env, err)
	}
	return Result{Code: 0}
}

func init() {
	register(Subcommand{Name: "view", Aliases: []string{"show"}, Order: 20, Run: runView})
}
