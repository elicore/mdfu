// Frontmatter row model and rendering for the document preview. Normalized
// document fields render as labeled rows (lists as pills); remaining raw
// frontmatter keys render with JSON-object values flattened inline as
// "k: v, k: v" and scalar lists as pills.
package tui

import (
	"encoding/json"
	"fmt"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/model"
	"github.com/elicore/mdfu/internal/theme"
)

// fmKind classifies how a frontmatter field's value is rendered.
type fmKind int

const (
	fmScalar fmKind = iota
	fmPills
	fmObject
)

// fmPair is one key/value entry of a flattened object value. A pair with an
// empty key and an fmObject field is one element of a list-of-objects value.
type fmPair struct {
	key   string
	field fmField
}

// fmField is one ordered frontmatter row: a label plus a scalar, a pill
// list, or a flattened object value.
type fmField struct {
	label  string
	kind   fmKind
	scalar string
	pills  []string
	obj    []fmPair
}

// fmConsumedKeys are the raw frontmatter keys already surfaced through the
// normalized document fields; they never render again as raw rows. Matching
// is case-insensitive.
var fmConsumedKeys = map[string]struct{}{
	"type": {}, "title": {}, "description": {}, "resource": {}, "tags": {}, "status": {}, "organized": {}, "archived": {},
	"belongs_to": {}, "belongs-to": {}, "related_to": {}, "related-to": {},
	"created": {}, "date": {}, "timestamp": {}, "updated": {}, "modified": {}, "last_modified": {},
}

// maxObjectDepth caps dotted-key flattening of nested maps; deeper values
// fall back to %v.
const maxObjectDepth = 3

// buildFrontmatterRows converts a parsed document into the ordered
// frontmatter row model: normalized rows first (Type, Status, Description,
// Resource, Tags, Belongs to, Related to, Created, Updated), then every
// remaining raw key sorted alphabetically. Empty values are skipped.
func buildFrontmatterRows(doc *model.Document) []fmField {
	if doc == nil {
		return nil
	}
	var rows []fmField
	add := func(f fmField) {
		if f.scalar != "" || len(f.pills) > 0 {
			rows = append(rows, f)
		}
	}
	add(fmField{label: "Type", kind: fmScalar, scalar: doc.DocType})
	add(fmField{label: "Status", kind: fmScalar, scalar: doc.Status})
	add(fmField{label: "Description", kind: fmScalar, scalar: doc.Description})
	add(fmField{label: "Resource", kind: fmScalar, scalar: doc.Resource})
	// Pills come from the normalized lists (already wikilink-stripped and
	// comma-split by the parser), never from the raw tags/belongs_to strings.
	add(fmField{label: "Tags", kind: fmPills, pills: doc.Tags})
	add(fmField{label: "Belongs to", kind: fmPills, pills: doc.BelongsTo})
	add(fmField{label: "Related to", kind: fmPills, pills: doc.RelatedTo})
	if doc.CreatedAt != nil {
		add(fmField{label: "Created", kind: fmScalar, scalar: doc.CreatedAt.Format(time.RFC3339)})
	}
	if doc.UpdatedAt != nil {
		add(fmField{label: "Updated", kind: fmScalar, scalar: doc.UpdatedAt.Format(time.RFC3339)})
	}
	keys := make([]string, 0, len(doc.Raw))
	for k := range doc.Raw {
		if _, consumed := fmConsumedKeys[strings.ToLower(k)]; !consumed {
			keys = append(keys, k)
		}
	}
	slices.Sort(keys)
	for _, k := range keys {
		if f, ok := classifyRawValue(k, doc.Raw[k]); ok {
			rows = append(rows, f)
		}
	}
	return rows
}

// classifyRawValue maps one raw frontmatter value to a row: maps and
// JSON-object strings become objects, lists of maps become object lists,
// scalar lists become pills, and anything else becomes a scalar.
func classifyRawValue(label string, v any) (fmField, bool) {
	row := func(f fmField) (fmField, bool) { f.label = label; return f, true }
	switch t := v.(type) {
	case map[string]any:
		if pairs := buildPairs(t, 1); len(pairs) > 0 {
			return row(fmField{kind: fmObject, obj: pairs})
		}
	case []any:
		return classifySlice(label, t)
	case string:
		if strings.TrimSpace(t) == "" {
			return fmField{}, false
		}
		if m, ok := parseJSONObject(t); ok {
			if pairs := buildPairs(m, 1); len(pairs) > 0 {
				return row(fmField{kind: fmObject, obj: pairs})
			}
		}
		return row(fmField{kind: fmScalar, scalar: t})
	default:
		if f, ok := pairField(v); ok {
			return row(f)
		}
	}
	return fmField{}, false
}

// classifySlice maps a list value: every element a map → object list (one
// anonymous group per element); otherwise it behaves like a nested pair
// value (scalar list → pills, mixed list → %v scalar).
func classifySlice(label string, t []any) (fmField, bool) {
	pairs := make([]fmPair, 0, len(t))
	objectList := true
	for _, e := range t {
		m, ok := e.(map[string]any)
		if !ok {
			objectList = false
			break
		}
		if sub := buildPairs(m, 1); len(sub) > 0 {
			pairs = append(pairs, fmPair{field: fmField{kind: fmObject, obj: sub}})
		}
	}
	if objectList {
		if len(pairs) > 0 {
			return fmField{label: label, kind: fmObject, obj: pairs}, true
		}
		return fmField{}, false
	}
	if f, ok := pairField(t); ok {
		f.label = label
		return f, true
	}
	return fmField{}, false
}

// buildPairs flattens a map into ordered pairs. Keys sort descending so
// attestation maps read naturally ("by: …, at: …") and source maps lead with
// their title; the order is deterministic. Nested maps recurse into dotted
// keys up to maxObjectDepth, then fall back to %v.
func buildPairs(m map[string]any, depth int) []fmPair {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	slices.Reverse(keys)
	var pairs []fmPair
	for _, k := range keys {
		if sub, isMap := m[k].(map[string]any); isMap && depth < maxObjectDepth {
			for _, p := range buildPairs(sub, depth+1) {
				p.key = k + "." + p.key
				pairs = append(pairs, p)
			}
			continue
		}
		if f, ok := pairField(m[k]); ok {
			pairs = append(pairs, fmPair{key: k, field: f})
		}
	}
	return pairs
}

// pairField maps one nested object value to a field: a map (only reachable
// past the depth cap) or a non-scalar list falls back to %v, a scalar list
// becomes pills, and scalars are stringified. Empty values report false.
func pairField(v any) (fmField, bool) {
	switch t := v.(type) {
	case map[string]any:
		return fmField{kind: fmScalar, scalar: fmt.Sprintf("%v", t)}, true
	case []any:
		if pills, ok := slicePills(t); ok {
			return fmField{kind: fmPills, pills: pills}, true
		}
		return fmField{kind: fmScalar, scalar: fmt.Sprintf("%v", t)}, true
	case []string:
		if pills, ok := slicePills(t); ok {
			return fmField{kind: fmPills, pills: pills}, true
		}
	case string:
		return fmField{kind: fmScalar, scalar: t}, strings.TrimSpace(t) != ""
	default:
		if s := scalarString(v); s != "" {
			return fmField{kind: fmScalar, scalar: s}, true
		}
	}
	return fmField{}, false
}

// parseJSONObject flattens a JSON-encoded object string into a map; anything
// that is not a JSON object reports false.
func parseJSONObject(s string) (map[string]any, bool) {
	var m map[string]any
	if err := json.Unmarshal([]byte(strings.TrimSpace(s)), &m); err != nil || m == nil {
		return nil, false
	}
	return m, true
}

// slicePills stringifies a list when every element is a scalar, dropping
// empty items; it reports false for lists containing maps or sub-lists.
func slicePills[S ~[]E, E any](t S) ([]string, bool) {
	pills := make([]string, 0, len(t))
	for _, e := range t {
		switch any(e).(type) {
		case map[string]any, []any:
			return nil, false
		}
		if s := scalarString(e); strings.TrimSpace(s) != "" {
			pills = append(pills, s)
		}
	}
	return pills, len(pills) > 0
}

// scalarString renders a frontmatter scalar as a string: nil is empty,
// strings pass through, time.Time formats as RFC3339, and bools/ints/floats
// go through %v.
func scalarString(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case time.Time:
		return t.Format(time.RFC3339)
	default:
		return fmt.Sprintf("%v", t)
	}
}

// renderField renders one frontmatter row: the label prefix styled with
// th.FrontmatterKey, then the value per its kind, with query matches
// highlighted over the final string.
func renderField(th theme.Theme, f fmField, re *regexp.Regexp) string {
	var value string
	switch f.kind {
	case fmScalar:
		value = f.scalar
	case fmPills:
		value = renderPillList(th, f.pills)
	case fmObject:
		value = renderObject(th, f.obj)
	}
	return highlightRe(th.FrontmatterKey.Render(f.label+": ")+value, re)
}

// renderObject renders a flattened object value: a list-of-objects (every
// pair an anonymous fmObject group) joins its element groups with "; ", a
// plain object joins its pairs with ", ".
func renderObject(th theme.Theme, pairs []fmPair) string {
	groups := make([]string, 0, len(pairs))
	for _, p := range pairs {
		if p.key != "" || p.field.kind != fmObject {
			return renderPairs(th, pairs)
		}
		groups = append(groups, renderPairs(th, p.field.obj))
	}
	return strings.Join(groups, "; ")
}

// renderPairs joins flattened object pairs with ", ": scalars as "k: v",
// scalar lists as pills, sub-objects recursively.
func renderPairs(th theme.Theme, pairs []fmPair) string {
	out := make([]string, 0, len(pairs))
	for _, p := range pairs {
		switch p.field.kind {
		case fmPills:
			out = append(out, p.key+": "+renderPillList(th, p.field.pills))
		case fmObject:
			out = append(out, p.key+": "+renderPairs(th, p.field.obj))
		default:
			out = append(out, p.key+": "+p.field.scalar)
		}
	}
	return strings.Join(out, ", ")
}

// renderPillList joins pills with a single space.
func renderPillList(th theme.Theme, pills []string) string {
	out := make([]string, 0, len(pills))
	for _, p := range pills {
		out = append(out, renderPill(th, p))
	}
	return strings.Join(out, " ")
}

// renderPill renders one pill badge from the theme's composed pill style,
// adding rounded side borders in the pill's background color when the theme
// selects the round shape.
func renderPill(th theme.Theme, s string) string {
	st := th.Pill
	if th.PillShape == "round" {
		st = st.Border(lipgloss.RoundedBorder(), false, true, false, true).
			BorderForeground(th.Pill.GetBackground())
	}
	return st.Render(s)
}
