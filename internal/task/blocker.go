package task

import "strings"

// DoneIDs returns the set of identifiers belonging to done tasks.
func DoneIDs(tasks []Task) map[string]bool {
	done := map[string]bool{}
	for _, t := range tasks {
		if t.Checked && t.ID != "" {
			done[t.ID] = true
		}
	}
	return done
}

// UnresolvedBlockers returns the identifiers named by t's blocked_by property
// that do not belong to a done task. A done task has no unresolved blockers.
func UnresolvedBlockers(t Task, tasks []Task) []string {
	if t.Checked {
		return nil
	}
	value, ok := t.Properties["blocked_by"]
	if !ok {
		return nil
	}
	done := DoneIDs(tasks)
	var unresolved []string
	for _, part := range strings.Split(value, ",") {
		id := strings.TrimSpace(part)
		if id == "" {
			continue
		}
		if !done[id] {
			unresolved = append(unresolved, id)
		}
	}
	return unresolved
}

// HasUnresolvedBlockers reports whether t is an open task with at least one
// unresolved blocker.
func HasUnresolvedBlockers(t Task, tasks []Task) bool {
	return len(UnresolvedBlockers(t, tasks)) > 0
}

// DisplayProperties returns the properties to render for t: every non-blocked_by
// property in original order, then blocked_by last. A fully resolved blocked_by
// is dropped; an unresolved one is kept with its original value.
func DisplayProperties(t Task, tasks []Task) (keys []string, values map[string]string) {
	values = map[string]string{}
	for _, key := range t.PropertyOrder {
		if key == "blocked_by" {
			continue
		}
		value, ok := t.Properties[key]
		if !ok {
			continue
		}
		keys = append(keys, key)
		values[key] = value
	}
	if value, ok := t.Properties["blocked_by"]; ok && len(UnresolvedBlockers(t, tasks)) > 0 {
		keys = append(keys, "blocked_by")
		values["blocked_by"] = value
	}
	return keys, values
}
