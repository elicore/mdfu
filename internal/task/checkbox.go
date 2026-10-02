package task

import (
	"fmt"
	"os"
	"regexp"
	"strings"
)

// checkboxRe is the broadened header grammar: optional indent, any of the
// three markdown bullets, [ ]/[x]/[X], then the remainder. It is a superset of
// the strict headerRe, so every mdtask header parses here too.
var checkboxRe = regexp.MustCompile(`^([ \t]*)([-*+]) \[([ xX])\] (.*)$`)

// idPrefixRe recognises the optional `PREFIX-<n> ` that follows a checkbox.
var idPrefixRe = regexp.MustCompile(`^([A-Z][A-Z0-9]*-\d+ )`)

// ParseCheckbox parses one broadened checkbox line. It is a superset of
// ParseHeader: every strict header parses here too, with Broad set. A checkbox
// must still be followed by a space, so `- [ ]x` is not an item.
func ParseCheckbox(line string) (Task, bool) {
	line = strings.TrimSuffix(line, "\r")
	m := checkboxRe.FindStringSubmatch(line)
	if m == nil {
		return Task{}, false
	}

	t := Task{
		Status:    m[3][0],
		HeaderRaw: line,
		Broad:     true,
	}
	t.Checked = t.Status == 'x' || t.Status == 'X'

	rest := m[4]
	if idm := idPrefixRe.FindStringSubmatch(rest); idm != nil {
		t.ID = strings.TrimSpace(idm[1])
		rest = rest[len(idm[0]):]
	} else if sm := seedRe.FindStringSubmatch(rest); sm != nil {
		t.Seeded = true
		t.SeedPrefix = sm[1]
		rest = sm[2]
	} else {
		t.Unidentified = true
	}

	title, meta, sep := splitMetaSep(rest)
	t.Title = title
	t.MetaSep = sep
	t.Tags, t.Priority, t.Properties, t.PropertyOrder = parseTokens(meta)
	return t, true
}

// FlipCheckboxAny returns line with its checkbox flipped between unchecked and
// checked, preserving the original indent, bullet, and trailing text. It
// reports ok == false for a line that is not a checkbox item.
func FlipCheckboxAny(line string) (string, bool) {
	line = strings.TrimSuffix(line, "\r")
	m := checkboxRe.FindStringSubmatch(line)
	if m == nil {
		return "", false
	}
	flip := "x"
	if m[3][0] == 'x' || m[3][0] == 'X' {
		flip = " "
	}
	return m[1] + m[2] + " [" + flip + "] " + m[4], true
}

// scopeFiles returns every markdown file the scope covers, applying the same
// walk, include/exclude globs, and archive exclusion as LoadAll. LoadCheckboxes
// uses it; the strict LoadScope path is left untouched.
func scopeFiles(scope Scope) ([]string, error) {
	if scope.IsFile {
		return []string{scope.File}, nil
	}
	paths, err := Walk(scope.Base, DiscoverOptions{
		FollowSymlinks: true,
		ExcludeDirs:    excludeDirsFromEnv(),
	})
	if err != nil {
		return nil, err
	}
	paths = Filter(paths, scope.Base, scope.Config.Include, scope.Config.Exclude)

	archive := resolveArchive(scope.Base, scope.Config.ArchivePath)
	out := make([]string, 0, len(paths))
	for _, path := range paths {
		if archive != "" && samePath(path, archive) {
			continue
		}
		out = append(out, path)
	}
	return out, nil
}

// LoadCheckboxes loads every markdown checkbox in scope through the broadened
// grammar: identified, seed, and unidentified items alike, with any bullet and
// any indentation. It is the tasks TUI's loader; the CLI never calls it, so the
// frozen list/view output is unaffected.
//
// Body and BodyIndent are filled with blockRangeAny, which ends a block at the
// next checkbox line so a nested checklist item is its own item rather than
// body text of its parent.
func LoadCheckboxes(scope Scope) ([]Task, error) {
	files, err := scopeFiles(scope)
	if err != nil {
		return nil, err
	}
	cwd := scope.Cwd
	if cwd == "" {
		cwd, _ = os.Getwd()
	}

	var tasks []Task
	for _, path := range files {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("cannot read %s: %w", path, err)
		}
		lines := strings.Split(string(data), "\n")
		mask := fenceMask(lines)
		rel := relToCwd(cwd, path)
		for i, raw := range lines {
			if mask[i] {
				continue
			}
			parsed, ok := ParseCheckbox(raw)
			if !ok {
				continue
			}
			if end, ok := blockRangeAny(lines, i); ok {
				parsed.Body, parsed.BodyIndent = collectBody(lines, i, end)
			}
			parsed.File = rel
			parsed.Line = i + 1
			tasks = append(tasks, parsed)
		}
	}
	return tasks, nil
}

// blockRangeAny is blockRange for the broadened view: it ends the block at the
// next checkbox line at any indent, so a nested checklist item is not absorbed
// into its parent's body. Non-checkbox indented lines remain body, and fenced
// code is masked exactly as in blockRange.
func blockRangeAny(lines []string, start int) (int, bool) {
	if start < 0 || start >= len(lines) {
		return 0, false
	}
	mask := fenceMask(lines)
	if mask[start] {
		return 0, false
	}
	if _, ok := ParseCheckbox(lines[start]); !ok {
		return 0, false
	}

	lastIndented := -1
	for i := start + 1; i < len(lines); i++ {
		line := strings.TrimSuffix(lines[i], "\r")
		if blank(line) {
			continue
		}
		if !mask[i] {
			if _, ok := ParseCheckbox(line); ok {
				break
			}
		}
		if line[0] == ' ' || line[0] == '\t' {
			lastIndented = i
			continue
		}
		break
	}
	if lastIndented < 0 {
		return start + 1, true
	}
	return lastIndented + 1, true
}
