package task

import (
	"flag"
	"fmt"
	"strings"
)

// stringList is a repeatable string flag (for --tag and --priority).
type stringList []string

func (s *stringList) String() string { return strings.Join(*s, ",") }

func (s *stringList) Set(value string) error {
	*s = append(*s, value)
	return nil
}

// runList implements `mdfu task list` (SPEC.md §F.1-F.3, §H).
func runList(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	fs := flag.NewFlagSet("list", flag.ContinueOnError)
	all := fs.Bool("all", false, "include done tasks")
	blocked := fs.Bool("blocked", false, "include blocked tasks")
	sortBy := fs.String("sort", "", "sort field")
	jsonOut := fs.Bool("json", false, "emit JSON")
	var tagFlags, priorityFlags stringList
	fs.Var(&tagFlags, "tag", "filter by tag (repeatable; AND)")
	fs.Var(&priorityFlags, "priority", "filter by priority (repeatable; OR)")

	pos, done := parseFlags(env, fs, rest)
	if done != nil {
		return *done
	}

	var extraTags, extraPriorities []string
	for _, arg := range pos {
		switch {
		case strings.HasPrefix(arg, "#"):
			extraTags = append(extraTags, arg)
		case strings.HasPrefix(arg, "!"):
			extraPriorities = append(extraPriorities, arg)
		default:
			fmt.Fprintf(env.Stderr, "mdfu tasks: unexpected argument '%s'\n", arg)
			return Result{Code: 2}
		}
	}

	tasks, unidentified, err := LoadScope(scope)
	if err != nil {
		return fail(env, err)
	}

	opts := ListOptions{
		All:            *all,
		Blocked:        *blocked,
		Sort:           *sortBy,
		TagFilter:      append(append([]string(nil), tagFlags...), extraTags...),
		PriorityFilter: append(append([]string(nil), priorityFlags...), extraPriorities...),
		JSON:           *jsonOut,
		Terminal:       env.Terminal,
		Unidentified:   unidentified,
	}
	for _, t := range tasks {
		if !opts.All && t.Checked {
			continue
		}
		if !opts.Blocked && HasUnresolvedBlockers(t, tasks) {
			opts.HiddenBlocked = append(opts.HiddenBlocked, t)
		}
	}

	if err := RenderList(env.Stdout, tasks, opts); err != nil {
		return fail(env, err)
	}
	return Result{Code: 0}
}

func init() {
	register(Subcommand{Name: "list", Order: 10, Run: runList})
}
