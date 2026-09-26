package task

import (
	"fmt"
	"os"
	"strings"
)

// LoadScope loads every identified task and every ID-less (seed or
// unidentified) line in scope.
//
// When scope.IsFile is true the named file is read as the whole scope without a
// walk: every fence-unmasked header is parsed, body and body indent are filled
// from blockRange and collectBody, Task.File is the cwd-relative slash path, and
// Task.Line is the 1-based header line. Otherwise LoadAll performs a full walk
// of scope.Base. Seed and unidentified lines are returned in unidentified.
func LoadScope(scope Scope) ([]Task, []Unidentified, error) {
	if !scope.IsFile {
		return LoadAll(scope.Base, scope.Config)
	}

	data, err := os.ReadFile(scope.File)
	if err != nil {
		return nil, nil, fmt.Errorf("cannot read %s: %w", scope.File, err)
	}

	lines := strings.Split(string(data), "\n")
	mask := fenceMask(lines)
	rel := relToCwd(scope.Cwd, scope.File)

	var tasks []Task
	var unidentified []Unidentified
	for i, raw := range lines {
		if mask[i] {
			continue
		}
		parsed, ok := ParseHeader(raw)
		if !ok {
			continue
		}
		if parsed.ID == "" {
			unidentified = append(unidentified, Unidentified{
				File: rel,
				Line: i + 1,
				Raw:  strings.TrimSuffix(raw, "\r"),
			})
			continue
		}
		if end, ok := blockRange(lines, i); ok {
			parsed.Body, parsed.BodyIndent = collectBody(lines, i, end)
		}
		parsed.File = rel
		parsed.Line = i + 1
		tasks = append(tasks, parsed)
	}
	return tasks, unidentified, nil
}
