package query

import (
	"fmt"
	"strings"
	"time"
	"unicode"
)

// DateFilter represents a date range constraint on a document date field.
type DateFilter struct {
	Field string
	From, To *time.Time
	FromIncl, ToIncl bool
}

// Query is the parsed representation of a single-box search input.
type Query struct {
	Bare []string
	Tags []string
	NotTags []string
	DocType string
	Title string
	Status string
	Path string
	Generic map[string]string
	Created, Updated *DateFilter
	Raw string
}

// Parse parses the input string into a Query.
// Keys are case-insensitive; values preserve case.
func Parse(input string) (*Query, error) {
	q := &Query{
		Generic: map[string]string{},
		Raw:     input,
	}
	tokens := tokenize(input)
	for _, tok := range tokens {
		if tok == "" {
			continue
		}
		idx := strings.Index(tok, ":")
		if idx < 0 {
			bare := stripQuotes(tok)
			if bare != "" {
				q.Bare = append(q.Bare, bare)
			}
			continue
		}
		key := strings.ToLower(strings.TrimSpace(tok[:idx]))
		rawVal := tok[idx+1:]
		val := stripQuotes(strings.TrimSpace(rawVal))
		if key == "" {
			// No key, treat whole token as bare.
			bare := stripQuotes(tok)
			if bare != "" {
				q.Bare = append(q.Bare, bare)
			}
			continue
		}
		switch key {
		case "tag", "tags":
			if val == "" {
				continue
			}
			parts := strings.Split(val, ",")
			for _, p := range parts {
				p = stripQuotes(strings.TrimSpace(p))
				if p == "" {
					continue
				}
				if strings.HasPrefix(p, "-") || strings.HasPrefix(p, "!") {
					trimmed := strings.TrimSpace(p[1:])
					trimmed = stripQuotes(trimmed)
					if trimmed != "" {
						q.NotTags = append(q.NotTags, trimmed)
					}
				} else {
					q.Tags = append(q.Tags, p)
				}
			}
		case "type":
			q.DocType = val
		case "title":
			if val == "" {
				continue
			}
			if q.Title == "" {
				q.Title = val
			} else {
				q.Title += " " + val
			}
		case "status":
			q.Status = val
		case "path":
			q.Path = val
		case "created", "updated", "date", "before", "after":
			if strings.TrimSpace(val) == "" {
				continue
			}
			f, err := parseDateFilter(key, val)
			if err != nil {
				return nil, err
			}
			switch key {
			case "created":
				q.Created = f
			case "updated":
				q.Updated = f
			case "date", "before", "after":
				// "either" semantics: stored in Created slot with
				// Field marking OR behavior; MatchesDoc checks
				// either CreatedAt or UpdatedAt.
				// Keep Updated slot intact so an explicit
				// updated: filter can combine via AND.
				q.Created = f
			}
		default:
			q.Generic[key] = val
		}
	}
	if q.Generic == nil {
		q.Generic = map[string]string{}
	}
	return q, nil
}

// tokenize splits on whitespace respecting double quotes.
// Quotes are retained in tokens for later stripping.
func tokenize(input string) []string {
	var tokens []string
	var cur strings.Builder
	inQuotes := false
	flush := func() {
		if cur.Len() > 0 {
			tokens = append(tokens, cur.String())
			cur.Reset()
		}
	}
	for _, r := range input {
		switch {
		case r == '"':
			inQuotes = !inQuotes
			cur.WriteRune(r)
		case unicode.IsSpace(r) && !inQuotes:
			flush()
		default:
			cur.WriteRune(r)
		}
	}
	flush()
	return tokens
}

// stripQuotes removes one pair of surrounding double quotes if present.
func stripQuotes(s string) string {
	s = strings.TrimSpace(s)
	if len(s) >= 2 && strings.HasPrefix(s, `"`) && strings.HasSuffix(s, `"`) {
		return s[1 : len(s)-1]
	}
	return s
}

// parseDateFilter parses a date expression into a DateFilter.
// Supported forms: exact date, A..B range, >=D, >D, <=D, <D.
// Layouts: 2006-01-02, 2006-01, RFC3339.
func parseDateFilter(field, expr string) (*DateFilter, error) {
	expr = strings.TrimSpace(stripQuotes(strings.TrimSpace(expr)))
	if expr == "" {
		return nil, fmt.Errorf("empty date expression for %q", field)
	}
	f := &DateFilter{Field: field}
	if strings.Contains(expr, "..") {
		parts := strings.SplitN(expr, "..", 2)
		left := strings.TrimSpace(stripQuotes(strings.TrimSpace(parts[0])))
		right := strings.TrimSpace(stripQuotes(strings.TrimSpace(parts[1])))
		if left == "" && right == "" {
			return nil, fmt.Errorf("invalid date range %q", expr)
		}
		f.FromIncl = true
		f.ToIncl = true
		if left != "" {
			t, gran, err := parseOneDate(left)
			if err != nil {
				return nil, fmt.Errorf("invalid date %q: %w", left, err)
			}
			s := startOf(t, gran)
			f.From = &s
		}
		if right != "" {
			t, gran, err := parseOneDate(right)
			if err != nil {
				return nil, fmt.Errorf("invalid date %q: %w", right, err)
			}
			e := endOf(t, gran)
			f.To = &e
		}
		return f, nil
	}
	if strings.HasPrefix(expr, ">=") {
		rest := strings.TrimSpace(expr[2:])
		t, _, err := parseOneDate(rest)
		if err != nil {
			// retry with granularity-aware start
			return nil, fmt.Errorf("invalid date %q: %w", rest, err)
		}
		// >=D means from start of D inclusive.
		_, gran, _ := parseOneDate(rest)
		s := startOf(t, gran)
		f.From = &s
		f.FromIncl = true
		return f, nil
	}
	if strings.HasPrefix(expr, "<=") {
		rest := strings.TrimSpace(expr[2:])
		t, gran, err := parseOneDate(rest)
		if err != nil {
			return nil, fmt.Errorf("invalid date %q: %w", rest, err)
		}
		e := endOf(t, gran)
		f.To = &e
		f.ToIncl = true
		return f, nil
	}
	if strings.HasPrefix(expr, ">") {
		rest := strings.TrimSpace(expr[1:])
		t, gran, err := parseOneDate(rest)
		if err != nil {
			return nil, fmt.Errorf("invalid date %q: %w", rest, err)
		}
		e := endOf(t, gran)
		f.From = &e
		f.FromIncl = false
		return f, nil
	}
	if strings.HasPrefix(expr, "<") {
		rest := strings.TrimSpace(expr[1:])
		t, gran, err := parseOneDate(rest)
		if err != nil {
			return nil, fmt.Errorf("invalid date %q: %w", rest, err)
		}
		s := startOf(t, gran)
		f.To = &s
		f.ToIncl = false
		return f, nil
	}
	// No operator: exact date, or before:/after: shorthand.
	t, gran, err := parseOneDate(expr)
	if err != nil {
		return nil, fmt.Errorf("invalid date %q: %w", expr, err)
	}
	switch field {
	case "before":
		s := startOf(t, gran)
		f.To = &s
		f.ToIncl = false
		return f, nil
	case "after":
		e := endOf(t, gran)
		f.From = &e
		f.FromIncl = false
		return f, nil
	default:
		s := startOf(t, gran)
		e := endOf(t, gran)
		f.From = &s
		f.To = &e
		f.FromIncl = true
		f.ToIncl = true
		return f, nil
	}
}

// parseOneDate parses a single date string, returning the time and its granularity.
func parseOneDate(s string) (time.Time, string, error) {
	s = strings.TrimSpace(stripQuotes(strings.TrimSpace(s)))
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, "instant", nil
	}
	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t, "day", nil
	}
	if t, err := time.Parse("2006-01", s); err == nil {
		return t, "month", nil
	}
	// Extra robustness: datetime without timezone.
	if t, err := time.Parse("2006-01-02T15:04:05", s); err == nil {
		return t, "instant", nil
	}
	if t, err := time.Parse("2006-01-02 15:04:05", s); err == nil {
		return t, "instant", nil
	}
	return time.Time{}, "", fmt.Errorf("unrecognized date %q (want YYYY-MM-DD, YYYY-MM, or RFC3339)", s)
}

func startOf(t time.Time, gran string) time.Time {
	switch gran {
	case "month":
		return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
	case "day":
		return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
	default:
		return t
	}
}

func endOf(t time.Time, gran string) time.Time {
	switch gran {
	case "month":
		s := time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
		return s.AddDate(0, 1, 0).Add(-time.Nanosecond)
	case "day":
		s := time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
		return s.AddDate(0, 0, 1).Add(-time.Nanosecond)
	default:
		return t
	}
}
