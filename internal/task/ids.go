package task

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"sort"
	"strings"
)

// runIDs implements `mdfu task ids`: assign IDs to every seed and unidentified
// line in scope. Prefix resolution is all-or-nothing across files; no file is
// written until every file's prefix resolves. On success one rewritten header
// per assignment is written to stdout in assignment order.
func runIDs(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	fs := flag.NewFlagSet("ids", flag.ContinueOnError)
	prefixFlag := fs.String("prefix", "", "prefix for newly assigned IDs")
	rest, done := parseFlags(env, fs, rest)
	if done != nil {
		return *done
	}
	if len(rest) != 0 {
		errf(env, "ids takes no positional arguments")
		return Result{Code: 2}
	}

	prefix := strings.ToUpper(strings.TrimSpace(*prefixFlag))
	if prefix != "" {
		if err := ValidatePrefix(prefix); err != nil {
			return fail(env, err)
		}
	}

	tasks, unidentified, err := scopeTaskSet(scope)
	if err != nil {
		return fail(env, err)
	}
	if len(unidentified) == 0 {
		return Result{Code: 0}
	}

	byFileTasks := map[string][]Task{}
	var fileOrder []string
	for _, t := range tasks {
		if _, seen := byFileTasks[t.File]; !seen {
			fileOrder = append(fileOrder, t.File)
		}
		byFileTasks[t.File] = append(byFileTasks[t.File], t)
	}
	existing := map[string]string{}
	for _, f := range fileOrder {
		existing[f] = MostFrequentPrefix(byFileTasks[f])
	}

	seeds := map[string]string{}
	var assignFiles []string
	seenAssign := map[string]bool{}
	for _, u := range unidentified {
		if !seenAssign[u.File] {
			seenAssign[u.File] = true
			assignFiles = append(assignFiles, u.File)
		}
		if seeds[u.File] == "" {
			if p, ok := SeedPrefix(u.Raw); ok {
				seeds[u.File] = p
			}
		}
	}

	var reader *bufio.Reader
	if env.Stdin != nil {
		reader = bufio.NewReader(env.Stdin)
	}
	fallback := map[string]string{}
	for _, f := range assignFiles {
		switch {
		case existing[f] != "":
		case prefix != "":
			fallback[f] = prefix
		case seeds[f] != "":
			fallback[f] = seeds[f]
		default:
			answer, ok := promptPrefix(env, reader, f)
			if !ok {
				continue
			}
			if err := ValidatePrefix(answer); err != nil {
				return fail(env, err)
			}
			fallback[f] = answer
		}
	}

	assigns := make([]Assignment, 0, len(unidentified))
	for _, u := range unidentified {
		a := Assignment{
			File:    u.File,
			Line:    u.Line,
			RawLine: u.Raw,
			Prefix:  fallback[u.File],
		}
		if seeds[u.File] != "" {
			a.Seed = seeds[u.File]
		}
		if existing[u.File] != "" {
			a.FilePrefixes = []string{existing[u.File]}
		}
		assigns = append(assigns, a)
	}

	globalMax := GlobalMax(tasks)
	writes, err := Plan(assigns, globalMax)
	if err != nil {
		return fail(env, err)
	}
	if err := verifyAssignments(assigns); err != nil {
		return fail(env, err)
	}
	if err := writeAssignments(writes); err != nil {
		return fail(env, err)
	}

	for _, w := range writes {
		fmt.Fprintln(env.Stdout, w.Header)
	}
	emitDuplicateWarnings(env, tasks)
	return Result{Code: 0}
}

// promptPrefix writes the interactive prompt for file to stderr and reads one
// line from reader. It reports ok == false on EOF with no input.
func promptPrefix(env Env, reader *bufio.Reader, file string) (string, bool) {
	fmt.Fprintf(env.Stderr, "Enter prefix for %s: ", file)
	if reader == nil {
		return "", false
	}
	line, err := reader.ReadString('\n')
	line = strings.TrimSpace(line)
	if line == "" {
		return "", false
	}
	if err != nil && !errors.Is(err, io.EOF) {
		return "", false
	}
	return line, true
}

// verifyAssignments confirms every assignment's source line is still the raw
// line that was planned, before any file is written.
func verifyAssignments(assigns []Assignment) error {
	byFile := map[string][]Assignment{}
	var order []string
	for _, a := range assigns {
		if _, seen := byFile[a.File]; !seen {
			order = append(order, a.File)
		}
		byFile[a.File] = append(byFile[a.File], a)
	}
	for _, f := range order {
		data, err := os.ReadFile(f)
		if err != nil {
			return err
		}
		lines := strings.Split(string(data), "\n")
		for _, a := range byFile[f] {
			idx := a.Line - 1
			if idx < 0 || idx >= len(lines) || strings.TrimSuffix(lines[idx], "\r") != a.RawLine {
				return fmt.Errorf("file changed, task not at expected line: %s:%d", f, a.Line)
			}
		}
	}
	return nil
}

// writeAssignments applies the planned header rewrites, one FileEdit per file.
func writeAssignments(writes []PlannedWrite) error {
	byFile := map[string][]PlannedWrite{}
	var order []string
	for _, w := range writes {
		if _, seen := byFile[w.File]; !seen {
			order = append(order, w.File)
		}
		byFile[w.File] = append(byFile[w.File], w)
	}
	for _, f := range order {
		fe, err := Open(f)
		if err != nil {
			return err
		}
		for _, w := range byFile[f] {
			if err := fe.ReplaceLines(w.Line-1, w.Line, []string{w.Header}); err != nil {
				return err
			}
		}
		if err := fe.Commit(); err != nil {
			return err
		}
	}
	return nil
}

// emitDuplicateWarnings writes one warning line per already-duplicated
// numeric part to stderr. It never changes the exit code.
func emitDuplicateWarnings(env Env, tasks []Task) {
	dups := DuplicateNumericParts(tasks)
	if len(dups) == 0 {
		return
	}
	nums := make([]int, 0, len(dups))
	for n := range dups {
		nums = append(nums, n)
	}
	sort.Ints(nums)
	for _, n := range nums {
		fmt.Fprintf(env.Stderr, "warning: duplicate numeric part %d across prefixes: %s\n", n, strings.Join(dups[n], ", "))
	}
}

func init() {
	register(Subcommand{Name: "ids", Order: 60, Run: runIDs})
}
