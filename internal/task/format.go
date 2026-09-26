package task

import (
	"regexp"
	"strings"
	"unicode"
)

// Task is a single parsed task header plus its body.
//
// The struct is the frozen shape named by the work plan. Two fields are added
// beyond that shape, both required for byte-exact round-tripping:
//
//   - MetaSep records the exact whitespace that separated the title from the
//     metadata tokens in the source header. It is empty when the task has no
//     metadata. When metadata is present and MetaSep is empty, renderHeader
//     emits the canonical tab-tab separator.
//   - SeedPrefix records the bare prefix of a seed header (for example "PRJ"
//     for "- [ ] PRJ- Title") so the line can be rendered back verbatim.
type Task struct {
	ID            string
	Status        byte
	Title         string
	Tags          []string
	Priority      string
	Body          string
	BodyIndent    string
	File          string
	Line          int
	HeaderRaw     string
	Checked       bool
	Seeded        bool
	Unidentified  bool
	PropertyOrder []string
	Properties    map[string]string

	MetaSep    string
	SeedPrefix string
}

// Unidentified records a checkbox-shaped line that carries no ID. Seed lines
// are reported separately through Task.Seeded; this type is used for genuinely
// unidentified lines.
type Unidentified struct {
	File string
	Line int
	Raw  string
}

var (
	headerRe   = regexp.MustCompile(`^- \[([ x])\] ((?:[A-Z][A-Z0-9]*-\d+ )?)(.*)$`)
	seedRe     = regexp.MustCompile(`^([A-Z][A-Z0-9]*)- (.*)$`)
	tagRe      = regexp.MustCompile(`^#[A-Za-z][\w-]*$`)
	priorityRe = regexp.MustCompile(`^![A-Za-z]\w*$`)
	propertyRe = regexp.MustCompile(`^@([\w-]+):(\S+)$`)
)

// ParseHeader parses one header line. It strips a single trailing carriage
// return before matching, so CRLF input parses. An uppercase X checkbox, a
// leading space, or any line that is not a task header returns ok == false.
func ParseHeader(line string) (Task, bool) {
	line = strings.TrimSuffix(line, "\r")
	m := headerRe.FindStringSubmatch(line)
	if m == nil {
		return Task{}, false
	}

	t := Task{
		Status:    m[1][0],
		HeaderRaw: line,
	}
	t.Checked = t.Status == 'x'

	rest := m[3]
	if m[2] != "" {
		t.ID = strings.TrimSpace(m[2])
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

// splitMeta splits a header remainder into its title and metadata parts.
func splitMeta(rest string) (title, meta string) {
	title, meta, _ = splitMetaSep(rest)
	return title, meta
}

// splitMetaSep is splitMeta plus the exact separator whitespace, which
// renderHeader needs to reproduce the source line.
func splitMetaSep(rest string) (title, meta, sep string) {
	if idx := strings.Index(rest, "\t\t"); idx >= 0 {
		title = strings.TrimRight(rest[:idx], " \t")
		meta = strings.TrimLeft(rest[idx+2:], " \t")
		sep = rest[len(title) : len(rest)-len(meta)]
		return title, meta, sep
	}

	fields := strings.Fields(rest)
	if len(fields) == 0 {
		return "", "", ""
	}
	i := len(fields)
	for i > 0 && isMetaToken(fields[i-1]) {
		i--
	}
	if i == len(fields) {
		return strings.TrimRight(rest, " \t"), "", ""
	}
	meta = strings.Join(fields[i:], " ")
	start := fieldStart(rest, i)
	title = strings.TrimRight(rest[:start], " \t")
	sep = rest[len(title):start]
	return title, meta, sep
}

// parseTokens extracts the metadata tokens from meta. A word matching no token
// form is ignored here; reporting it is a validate concern. Priority is
// returned without its leading bang, and keeps an out-of-set value so that
// validate can warn about it. order lists property keys in first-seen order.
func parseTokens(meta string) (tags []string, priority string, props map[string]string, order []string) {
	props = map[string]string{}
	for _, word := range strings.Fields(meta) {
		switch {
		case tagRe.MatchString(word):
			tags = append(tags, word[1:])
		case priorityRe.MatchString(word):
			priority = word[1:]
		case propertyRe.MatchString(word):
			m := propertyRe.FindStringSubmatch(word)
			key, value := m[1], m[2]
			if _, seen := props[key]; !seen {
				order = append(order, key)
			}
			props[key] = value
		}
	}
	return tags, priority, props, order
}

// isMetaToken reports whether word is a tag, priority, or property token.
func isMetaToken(word string) bool {
	return tagRe.MatchString(word) || priorityRe.MatchString(word) || propertyRe.MatchString(word)
}

// fieldStart returns the byte offset at which the n-th (0-based) whitespace
// delimited field of s begins, matching strings.Fields.
func fieldStart(s string, n int) int {
	field := 0
	inField := false
	for i, r := range s {
		if unicode.IsSpace(r) {
			inField = false
			continue
		}
		if !inField {
			if field == n {
				return i
			}
			field++
			inField = true
		}
	}
	return len(s)
}

// renderHeader serializes a task back to a header line. For a task that came
// straight from ParseHeader and was not modified it is byte-exact. Metadata is
// emitted tags first, then priority, then properties; a task with metadata but
// no recorded separator uses the canonical tab-tab separator.
func renderHeader(t Task) string {
	var b strings.Builder
	b.WriteString("- [")
	if t.Checked {
		b.WriteByte('x')
	} else {
		b.WriteByte(' ')
	}
	b.WriteString("] ")
	switch {
	case t.ID != "":
		b.WriteString(t.ID)
		b.WriteByte(' ')
	case t.Seeded && t.SeedPrefix != "":
		b.WriteString(t.SeedPrefix)
		b.WriteString("- ")
	}
	b.WriteString(t.Title)

	meta := renderMeta(t)
	if meta != "" {
		sep := t.MetaSep
		if sep == "" {
			sep = "\t\t"
		}
		b.WriteString(sep)
		b.WriteString(meta)
	}
	return b.String()
}

// renderMeta rebuilds the metadata portion of a header.
func renderMeta(t Task) string {
	tokens := make([]string, 0, len(t.Tags)+1+len(t.PropertyOrder))
	for _, tag := range t.Tags {
		tokens = append(tokens, "#"+tag)
	}
	if t.Priority != "" {
		tokens = append(tokens, "!"+t.Priority)
	}
	for _, key := range t.PropertyOrder {
		if value, ok := t.Properties[key]; ok {
			tokens = append(tokens, "@"+key+":"+value)
		}
	}
	return strings.Join(tokens, " ")
}
