package task

import (
	"flag"
	"fmt"
	"sort"
	"strings"
)

// runValidate implements `mdfu task validate` per SPEC.md §F.8.
//
// It scans every file in scope, honours Config.ExcludePrefixes, and writes
// diagnostics to stderr only. A duplicate-ID group is an `error:` line and
// makes the command exit 1; a duplicate numeric part across prefixes, an empty
// tag, malformed metadata, and an unknown priority are `warning:` lines that
// never change the exit code. Nothing is written to stdout.
func runValidate(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	fs := flag.NewFlagSet("validate", flag.ContinueOnError)
	pos, done := parseFlags(env, fs, rest)
	if done != nil {
		return *done
	}
	if len(pos) > 0 {
		errf(env, "unexpected argument '%s'", pos[0])
		return Result{Code: 2}
	}

	tasks, _, err := LoadScope(scope)
	if err != nil {
		return fail(env, err)
	}

	var filtered []Task
	for _, t := range tasks {
		if prefix, ok := taskPrefix(t.ID); ok && prefixExcluded(scope.Config.ExcludePrefixes, prefix) {
			continue
		}
		filtered = append(filtered, t)
	}

	hadError := false
	hadError = validateDuplicateIDs(env, filtered) || hadError
	validateDuplicateNumbers(env, filtered)
	for _, t := range filtered {
		validateTaskMeta(env, t)
	}

	if hadError {
		return Result{Code: 1}
	}
	return Result{Code: 0}
}

// validateDuplicateIDs writes one `error:` line per duplicate-ID group, naming
// every location, and reports whether any group was found. Groups are emitted
// in identifier order.
func validateDuplicateIDs(env Env, tasks []Task) bool {
	byID := map[string][]Task{}
	for _, t := range tasks {
		byID[t.ID] = append(byID[t.ID], t)
	}
	ids := make([]string, 0)
	for id, group := range byID {
		if len(group) > 1 {
			ids = append(ids, id)
		}
	}
	sort.Strings(ids)

	for _, id := range ids {
		locs := make([]string, 0, len(byID[id]))
		for _, t := range byID[id] {
			locs = append(locs, fmt.Sprintf("%s:%d", t.File, t.Line))
		}
		fmt.Fprintf(env.Stderr, "error: duplicate ID '%s' in %s\n", id, strings.Join(locs, ", "))
	}
	return len(ids) > 0
}

// validateDuplicateNumbers writes the duplicate-numeric-part warning for every
// numeric part shared by identifiers under more than one prefix. Same-prefix
// repeats are the duplicate-ID error's job and are not warned twice.
func validateDuplicateNumbers(env Env, tasks []Task) {
	nums := DuplicateNumericParts(tasks)

	keys := make([]int, 0, len(nums))
	for n := range nums {
		keys = append(keys, n)
	}
	sort.Ints(keys)

	for _, n := range keys {
		prefixes := map[string]bool{}
		for _, id := range nums[n] {
			if prefix, ok := taskPrefix(id); ok {
				prefixes[prefix] = true
			}
		}
		if len(prefixes) < 2 {
			continue
		}
		fmt.Fprintf(env.Stderr, "warning: duplicate numeric part %d across prefixes: %s\n", n, strings.Join(nums[n], ", "))
	}
}

// validateTaskMeta reports the three per-task metadata warnings: an empty tag,
// malformed metadata (a bare `@key` with no `:value`), and an unknown
// priority.
func validateTaskMeta(env Env, t Task) {
	for _, word := range strings.Fields(validateHeaderMeta(t.HeaderRaw)) {
		switch {
		case word == "#":
			fmt.Fprintf(env.Stderr, "warning: empty tag in %s:%d\n", t.File, t.Line)
		case strings.HasPrefix(word, "@") && !propertyRe.MatchString(word):
			fmt.Fprintf(env.Stderr, "warning: malformed metadata '%s' in %s:%d\n", word, t.File, t.Line)
		case priorityRe.MatchString(word) && !validatePriorityKnown(word[1:]):
			fmt.Fprintf(env.Stderr, "warning: unknown priority '%s' in %s:%d\n", word, t.File, t.Line)
		}
	}
}

// validateHeaderMeta extracts the raw metadata portion of a header line so the
// dropped-token forms can be reported. It mirrors splitMetaSep's split without
// discarding unknown words.
func validateHeaderMeta(line string) string {
	m := headerRe.FindStringSubmatch(line)
	if m == nil {
		return ""
	}
	rest := m[3]
	if m[2] == "" {
		if sm := seedRe.FindStringSubmatch(rest); sm != nil {
			rest = sm[2]
		}
	}
	_, meta, _ := splitMetaSep(rest)
	return meta
}

// validatePriorityKnown reports whether p is one of the explicit priorities.
func validatePriorityKnown(p string) bool {
	switch p {
	case "crit", "high", "low":
		return true
	}
	return false
}

// prefixExcluded reports whether prefix is named in the excluded set.
func prefixExcluded(excluded []string, prefix string) bool {
	for _, e := range excluded {
		if e == prefix {
			return true
		}
	}
	return false
}

func init() {
	register(Subcommand{Name: "validate", Order: 80, Run: runValidate})
}
