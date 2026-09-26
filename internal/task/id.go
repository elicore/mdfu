package task

import (
	"fmt"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

var (
	prefixRe = regexp.MustCompile(`^[A-Z][A-Z0-9]*$`)
	idRe     = regexp.MustCompile(`^[A-Z][A-Z0-9]*-\d+$`)
	bareRe   = regexp.MustCompile(`^\d+$`)
)

// ValidPrefix reports whether s is a well-formed ID prefix.
func ValidPrefix(s string) bool {
	return prefixRe.MatchString(s)
}

// ValidTaskID reports whether s is a well-formed PREFIX-N identifier.
func ValidTaskID(s string) bool {
	return idRe.MatchString(s)
}

// NumericPart returns the decimal numeric part of an identifier.
func NumericPart(id string) (int, bool) {
	if !ValidTaskID(id) {
		return 0, false
	}
	i := strings.LastIndexByte(id, '-')
	n, err := strconv.Atoi(id[i+1:])
	if err != nil {
		return 0, false
	}
	return n, true
}

// Resolve maps a full identifier or a bare numeric part to exactly one task.
// The four failure strings are fixed by SPEC.md §J (E9-E12).
func Resolve(input string, tasks []Task) (Task, error) {
	if !ValidTaskID(input) && !bareRe.MatchString(input) {
		return Task{}, fmt.Errorf("invalid task ID '%s'", input)
	}

	var matches []Task
	if bareRe.MatchString(input) {
		want, _ := strconv.Atoi(input)
		for _, t := range tasks {
			if n, ok := NumericPart(t.ID); ok && n == want {
				matches = append(matches, t)
			}
		}
	} else {
		for _, t := range tasks {
			if t.ID == input {
				matches = append(matches, t)
			}
		}
	}

	switch len(matches) {
	case 0:
		return Task{}, fmt.Errorf("task '%s' not found", input)
	case 1:
		return matches[0], nil
	default:
		if bareRe.MatchString(input) {
			ids := make([]string, 0, len(matches))
			for _, t := range matches {
				ids = append(ids, t.ID)
			}
			sort.Strings(ids)
			return Task{}, fmt.Errorf("ambiguous numeric ID '%s' matches: %s", input, strings.Join(ids, ", "))
		}
		return Task{}, fmt.Errorf("task '%s' appears multiple times; expected exactly one match", input)
	}
}

// GlobalMax returns the largest numeric part across all identified tasks, or 0
// when there is none.
func GlobalMax(tasks []Task) int {
	max := 0
	for _, t := range tasks {
		if n, ok := NumericPart(t.ID); ok && n > max {
			max = n
		}
	}
	return max
}

// PadWidth returns the zero-padding width for new numeric parts.
func PadWidth(globalMax int) int {
	if globalMax < 0 {
		globalMax = 0
	}
	width := 3
	if d := len(strconv.Itoa(globalMax)); d > width {
		width = d
	}
	return width
}

// MostFrequentPrefix returns the prefix carried by the most tasks. Ties break
// toward the first prefix seen. It returns "" when there are no identified
// tasks.
func MostFrequentPrefix(tasks []Task) string {
	prefixes := make([]string, 0, len(tasks))
	for _, t := range tasks {
		if p, ok := taskPrefix(t.ID); ok {
			prefixes = append(prefixes, p)
		}
	}
	return mostFrequent(prefixes)
}

// SeedPrefix extracts the prefix from a seed header of the form
// "- [ ] PRJ- Title".
func SeedPrefix(line string) (string, bool) {
	line = strings.TrimSuffix(line, "\r")
	m := headerRe.FindStringSubmatch(line)
	if m == nil {
		return "", false
	}
	if m[2] != "" {
		return "", false
	}
	sm := seedRe.FindStringSubmatch(m[3])
	if sm == nil {
		return "", false
	}
	return sm[1], true
}

// ValidatePrefix returns an error when s is not a valid prefix.
func ValidatePrefix(s string) error {
	if !ValidPrefix(s) {
		return fmt.Errorf("invalid prefix '%s'", s)
	}
	return nil
}

// DuplicateNumericParts maps a numeric part to the sorted identifiers that
// share it, including only parts used by more than one task or prefix.
func DuplicateNumericParts(tasks []Task) map[int][]string {
	byNumber := map[int][]string{}
	prefixes := map[int]map[string]bool{}
	for _, t := range tasks {
		n, ok := NumericPart(t.ID)
		if !ok {
			continue
		}
		byNumber[n] = append(byNumber[n], t.ID)
		if prefixes[n] == nil {
			prefixes[n] = map[string]bool{}
		}
		if p, ok := taskPrefix(t.ID); ok {
			prefixes[n][p] = true
		}
	}
	out := map[int][]string{}
	for n, ids := range byNumber {
		if len(ids) > 1 || len(prefixes[n]) > 1 {
			sorted := append([]string(nil), ids...)
			sort.Strings(sorted)
			out[n] = sorted
		}
	}
	return out
}

// Assignment is one unidentified or seed line awaiting an ID.
type Assignment struct {
	File         string
	Line         int
	RawLine      string
	Prefix       string
	Seed         string
	FilePrefixes []string
}

// PlannedWrite is one header rewrite produced by Plan.
type PlannedWrite struct {
	File   string
	Line   int
	Header string
}

// Plan assigns sequential identifiers to every assignment. It is
// all-or-nothing: every assignment's prefix is resolved and validated before
// any write is planned, and an unresolvable assignment yields a zero-length
// write slice and an error. Numbers start at globalMax+1 and are zero-padded to
// PadWidth(globalMax).
func Plan(assigns []Assignment, globalMax int) ([]PlannedWrite, error) {
	width := PadWidth(globalMax)
	prefixes := make([]string, len(assigns))
	for i, a := range assigns {
		p, err := resolvePrefix(a)
		if err != nil {
			return nil, err
		}
		prefixes[i] = p
	}

	writes := make([]PlannedWrite, 0, len(assigns))
	n := globalMax
	for i, a := range assigns {
		n++
		id := fmt.Sprintf("%s-%0*d", prefixes[i], width, n)
		writes = append(writes, PlannedWrite{
			File:   a.File,
			Line:   a.Line,
			Header: headerWithID(a.RawLine, id),
		})
	}
	return writes, nil
}

// resolvePrefix picks the prefix for one assignment: the most frequent existing
// prefix in the file, then the seed prefix, then the caller-supplied prefix.
func resolvePrefix(a Assignment) (string, error) {
	candidates := []string{mostFrequent(a.FilePrefixes), a.Seed, a.Prefix}
	for _, candidate := range candidates {
		if candidate == "" {
			continue
		}
		if err := ValidatePrefix(candidate); err != nil {
			return "", err
		}
		return candidate, nil
	}
	return "", fmt.Errorf("%s:%d: no prefix found for task \"%s\" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ", a.File, a.Line, a.RawLine)
}

// headerWithID rewrites a source header line to carry id.
func headerWithID(raw, id string) string {
	t, ok := ParseHeader(raw)
	if !ok {
		return raw
	}
	t.ID = id
	t.Seeded = false
	t.SeedPrefix = ""
	return renderHeader(t)
}

// taskPrefix returns the prefix of a full identifier.
func taskPrefix(id string) (string, bool) {
	if !ValidTaskID(id) {
		return "", false
	}
	return id[:strings.IndexByte(id, '-')], true
}

// mostFrequent returns the most common non-empty string, preferring first
// appearance on ties.
func mostFrequent(values []string) string {
	counts := map[string]int{}
	order := make([]string, 0, len(values))
	for _, v := range values {
		if v == "" {
			continue
		}
		if _, seen := counts[v]; !seen {
			order = append(order, v)
		}
		counts[v]++
	}
	best := ""
	bestCount := 0
	for _, v := range order {
		if counts[v] > bestCount {
			bestCount = counts[v]
			best = v
		}
	}
	return best
}
