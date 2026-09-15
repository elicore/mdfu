// Package parse splits markdown frontmatter from body and normalizes it
// into a model.Document. Parsing never fails hard: malformed frontmatter is
// recorded on Document.ParseError while the body stays searchable.
package parse

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/elicore/mdfu/internal/model"
	"gopkg.in/yaml.v3"
)

// dateLayouts are tried in order when parsing date-like frontmatter values.
var dateLayouts = []string{
	time.RFC3339,
	"2006-01-02T15:04:05Z07:00",
	"2006-01-02",
	"2006-01",
}

// portentTypes are the `type` values that identify Portent/Tolaria documents.
// Any other `type` value identifies an OKF document.
var portentTypes = map[string]bool{
	"project":        true,
	"operation":      true,
	"responsibility": true,
	"task":           true,
	"event":          true,
	"note":           true,
	"topic":          true,
	"person":         true,
}

// SplitFrontmatter splits leading "---\n ... \n---" frontmatter from the body.
// Only a document that starts with an opening "---" line is treated as having
// frontmatter; without a closing "---" line the whole content is the body and
// hasFM is false.
func SplitFrontmatter(content string) (fm string, body string, hasFM bool) {
	content = strings.TrimPrefix(content, "\xef\xbb\xbf")
	if !strings.HasPrefix(content, "---") {
		return "", content, false
	}
	rest := content[len("---"):]
	if rest == "" || (rest[0] != '\n' && rest[0] != '\r') {
		return "", content, false
	}
	rest = strings.TrimLeft(rest, "\r\n")
	lines := strings.Split(rest, "\n")
	for i, line := range lines {
		if strings.TrimSpace(line) == "---" {
			fm = strings.Join(lines[:i], "\n")
			body = strings.Join(lines[i+1:], "\n")
			return fm, body, true
		}
	}
	return "", content, false
}

// ParseFile reads the file at path and parses it via ParseContent.
func ParseFile(path string) (*model.Document, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	return ParseContent(path, string(data))
}

// ParseContent parses markdown content into a normalized *model.Document.
//
// The frontmatter is YAML-unmarshalled into a generic map; a YAML error never
// fails hard — it is recorded on Document.ParseError and the returned
// document still carries the body (and any filename/H1-derived title) so it
// stays searchable. ParseContent itself therefore only returns a non-nil
// error for internal failures (currently always nil).
func ParseContent(path, content string) (*model.Document, error) {
	doc := &model.Document{
		Path: path,
		Raw:  map[string]any{},
	}
	fm, body, hasFM := SplitFrontmatter(content)
	doc.Body = body
	if hasFM {
		var raw map[string]any
		if err := yaml.Unmarshal([]byte(fm), &raw); err != nil {
			doc.ParseError = err
		} else if raw != nil {
			doc.Raw = raw
		}
	}
	normalize(doc, hasFM)
	doc.RebuildSearchBlob()
	return doc, nil
}

// normalize fills the normalized fields of doc from doc.Raw.
func normalize(doc *model.Document, hasFM bool) {
	raw := doc.Raw
	if raw == nil {
		raw = map[string]any{}
	}

	if v, ok := lookup(raw, "type"); ok {
		doc.DocType = strings.TrimSpace(asString(v))
	}
	if v, ok := lookup(raw, "title"); ok && strings.TrimSpace(asString(v)) != "" {
		doc.Title = strings.TrimSpace(asString(v))
	} else if h1 := extractH1(doc.Body); h1 != "" {
		doc.Title = h1
	} else {
		doc.Title = filenameBase(doc.Path)
	}
	if v, ok := lookup(raw, "description"); ok {
		doc.Description = strings.TrimSpace(asString(v))
	}
	if v, ok := lookup(raw, "resource"); ok {
		doc.Resource = strings.TrimSpace(asString(v))
	}
	if v, ok := lookup(raw, "tags"); ok {
		doc.Tags = parseStringList(v)
	}
	if v, ok := lookup(raw, "status"); ok {
		doc.Status = strings.ToLower(strings.TrimSpace(asString(v)))
	}
	if v, ok := lookup(raw, "organized"); ok {
		if b, ok := asBool(v); ok {
			doc.Organized = &b
		}
	}
	if v, ok := lookup(raw, "archived"); ok {
		if b, ok := asBool(v); ok {
			doc.Archived = &b
		}
	}
	if v, ok := lookup(raw, "belongs_to"); ok {
		doc.BelongsTo = parseStringList(v)
	} else if v, ok := lookup(raw, "belongs-to"); ok {
		doc.BelongsTo = parseStringList(v)
	}
	if v, ok := lookup(raw, "related_to"); ok {
		doc.RelatedTo = parseStringList(v)
	} else if v, ok := lookup(raw, "related-to"); ok {
		doc.RelatedTo = parseStringList(v)
	}
	if v, ok := lookup(raw, "sources"); ok {
		doc.Sources = parseSources(v)
	}
	if v, ok := lookup(raw, "generated"); ok {
		doc.Generated = parseAttestation(v)
	}
	if v, ok := lookup(raw, "verified"); ok {
		doc.Verified = parseAttestation(v)
	}

	doc.CreatedAt = firstDate(raw, []string{"created", "date", "timestamp", "generated"})
	doc.UpdatedAt = firstDate(raw, []string{"generated", "timestamp", "last_modified", "updated", "modified"})

	doc.Role = roleForPath(doc.Path)
	doc.Format = detectFormat(hasFM, doc.DocType, raw)
}

// firstDate returns the first parseable date found under keys, in order. The
// key "generated" resolves to its nested "at" value (i.e. generated.at).
func firstDate(raw map[string]any, keys []string) *time.Time {
	for _, k := range keys {
		v, ok := lookup(raw, k)
		if !ok {
			continue
		}
		if strings.EqualFold(k, "generated") {
			if m, ok := v.(map[string]any); ok {
				if at, ok := lookup(m, "at"); ok {
					if t := parseDate(at); t != nil {
						return t
					}
				}
				continue
			}
		}
		if t := parseDate(v); t != nil {
			return t
		}
	}
	return nil
}

// detectFormat classifies the document: a `type` in the Portent set (or
// Portent lifecycle keys without a type) → Portent; any other `type` → OKF;
// frontmatter without a type → Generic; no frontmatter → None.
func detectFormat(hasFM bool, docType string, raw map[string]any) model.FormatKind {
	if !hasFM {
		return model.FormatNone
	}
	if docType != "" {
		if portentTypes[strings.ToLower(strings.TrimSpace(docType))] {
			return model.FormatPortent
		}
		return model.FormatOKF
	}
	if _, ok := lookup(raw, "organized"); ok {
		return model.FormatPortent
	}
	if _, ok := lookup(raw, "archived"); ok {
		return model.FormatPortent
	}
	return model.FormatGeneric
}

// roleForPath returns "index" for index.md basenames, "log" for log.md
// basenames (case-insensitive), and "" otherwise.
func roleForPath(path string) string {
	base := strings.ToLower(filepath.Base(path))
	switch base {
	case "index.md":
		return "index"
	case "log.md":
		return "log"
	default:
		return ""
	}
}

// lookup finds a key in a frontmatter map case-insensitively.
func lookup(raw map[string]any, key string) (any, bool) {
	for k, v := range raw {
		if strings.EqualFold(k, key) {
			return v, true
		}
	}
	return nil, false
}

// asString renders a frontmatter scalar as a string.
func asString(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case time.Time:
		return t.Format(time.RFC3339)
	case bool:
		return strconv.FormatBool(t)
	case int:
		return strconv.Itoa(t)
	case int64:
		return strconv.FormatInt(t, 10)
	case float64:
		return strconv.FormatFloat(t, 'f', -1, 64)
	default:
		return fmt.Sprintf("%v", t)
	}
}

// asBool coerces a frontmatter value to a bool.
func asBool(v any) (bool, bool) {
	switch t := v.(type) {
	case bool:
		return t, true
	case string:
		b, err := strconv.ParseBool(strings.ToLower(strings.TrimSpace(t)))
		if err != nil {
			return false, false
		}
		return b, true
	case int:
		return t != 0, true
	case int64:
		return t != 0, true
	case float64:
		return t != 0, true
	default:
		return false, false
	}
}

// stripWikilink removes surrounding [[ ]] markers and whitespace.
func stripWikilink(s string) string {
	s = strings.TrimSpace(s)
	if strings.HasPrefix(s, "[[") && strings.HasSuffix(s, "]]") && len(s) >= 4 {
		s = s[2 : len(s)-2]
	}
	return strings.TrimSpace(s)
}

// parseStringList accepts a list, a single string, or a comma-separated
// string, and returns cleaned items with [[ ]] markers stripped.
func parseStringList(v any) []string {
	var out []string
	add := func(s string) {
		for _, part := range strings.Split(s, ",") {
			if cleaned := stripWikilink(part); cleaned != "" {
				out = append(out, cleaned)
			}
		}
	}
	switch t := v.(type) {
	case nil:
		return nil
	case string:
		add(t)
	case []any:
		for _, e := range t {
			if s, ok := e.(string); ok {
				add(s)
			} else if s := strings.TrimSpace(asString(e)); s != "" {
				out = append(out, stripWikilink(s))
			}
		}
	case []string:
		for _, e := range t {
			add(e)
		}
	default:
		if s := strings.TrimSpace(asString(v)); s != "" {
			out = append(out, stripWikilink(s))
		}
	}
	return out
}

// parseSources normalizes a sources list into []model.Source, looking up
// id/resource/title/author keys case-insensitively.
func parseSources(v any) []model.Source {
	items, ok := v.([]any)
	if !ok {
		return nil
	}
	var out []model.Source
	for _, item := range items {
		m, ok := item.(map[string]any)
		if !ok {
			continue
		}
		var s model.Source
		if id, ok := lookup(m, "id"); ok {
			s.ID = strings.TrimSpace(asString(id))
		}
		if r, ok := lookup(m, "resource"); ok {
			s.Resource = strings.TrimSpace(asString(r))
		}
		if t, ok := lookup(m, "title"); ok {
			s.Title = strings.TrimSpace(asString(t))
		}
		if a, ok := lookup(m, "author"); ok {
			s.Author = strings.TrimSpace(asString(a))
		}
		out = append(out, s)
	}
	return out
}

// parseAttestation normalizes a {by, at} map into a *model.Attestation,
// returning nil when the value is not a map.
func parseAttestation(v any) *model.Attestation {
	m, ok := v.(map[string]any)
	if !ok || m == nil {
		return nil
	}
	a := &model.Attestation{}
	if by, ok := lookup(m, "by"); ok {
		a.By = strings.TrimSpace(asString(by))
	}
	if at, ok := lookup(m, "at"); ok {
		a.At = parseDate(at)
	}
	return a
}

// parseDate parses a frontmatter date value: time.Time passes through,
// strings are tried against the known layouts, and integers are treated as
// unix timestamps. Returns nil when unparseable.
func parseDate(v any) *time.Time {
	switch t := v.(type) {
	case nil:
		return nil
	case time.Time:
		tt := t
		return &tt
	case *time.Time:
		return t
	case string:
		s := strings.TrimSpace(t)
		if s == "" {
			return nil
		}
		for _, layout := range dateLayouts {
			if parsed, err := time.Parse(layout, s); err == nil {
				return &parsed
			}
		}
		return nil
	case int:
		return parseDate(int64(t))
	case int64:
		tt := time.Unix(t, 0).UTC()
		return &tt
	case float64:
		tt := time.Unix(int64(t), 0).UTC()
		return &tt
	default:
		return nil
	}
}

// extractH1 returns the text of the first "# " heading in the body.
func extractH1(body string) string {
	for _, line := range strings.Split(body, "\n") {
		t := strings.TrimSpace(line)
		if strings.HasPrefix(t, "# ") {
			if title := strings.TrimSpace(strings.TrimPrefix(t, "# ")); title != "" {
				return title
			}
		}
	}
	return ""
}

// filenameBase returns the file base name without extension.
func filenameBase(path string) string {
	base := filepath.Base(path)
	if ext := filepath.Ext(base); ext != "" {
		base = strings.TrimSuffix(base, ext)
	}
	return base
}
