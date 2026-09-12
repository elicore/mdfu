// Package model defines the unified normalized Document contract shared by
// all tracks of mdfu. It covers OKF v0.1/v0.2 and Portent/Tolaria
// frontmatter via a single normalized struct (see PLAN.md).
package model

import (
	"fmt"
	"sort"
	"strings"
	"time"
)

// FormatKind identifies which frontmatter family a Document was parsed from.
type FormatKind string

const (
	FormatOKF     FormatKind = "okf"
	FormatPortent FormatKind = "portent"
	FormatGeneric FormatKind = "generic"
	FormatNone    FormatKind = "none"
)

// Attestation records who generated or verified a document and when.
type Attestation struct {
	By string
	At *time.Time
}

// Source records a provenance entry for a document.
type Source struct {
	ID, Resource, Title, Author string
}

// Document is the unified normalized model for a single markdown file.
// Contract from PLAN.md — do not change without agreement across tracks.
type Document struct {
	Path, Title, DocType, Description, Resource, Role string // Role: ""|index|log
	Tags                                              []string
	Body                                              string
	CreatedAt, UpdatedAt                              *time.Time
	Status                                            string
	Organized, Archived                               *bool
	BelongsTo, RelatedTo                              []string
	Sources                                           []Source
	Generated, Verified                               *Attestation
	Format                                            FormatKind
	Raw                                               map[string]any
	SearchBlob                                        string
	ParseError                                        error
}

// IsArchived reports whether the document is archived, either explicitly via
// the archived flag or implicitly via status "archived".
func (d *Document) IsArchived() bool {
	if d == nil {
		return false
	}
	if d.Archived != nil && *d.Archived {
		return true
	}
	return d.Status == "archived"
}

// BuildSearchBlob concatenates title + description + tags + flattened scalars
// of the raw frontmatter + body into a single searchable blob. The blob is
// stored as-is; lowercasing happens at match time.
func BuildSearchBlob(title, description string, tags []string, raw map[string]any, body string) string {
	var sb strings.Builder
	write := func(s string) {
		s = strings.TrimSpace(s)
		if s == "" {
			return
		}
		if sb.Len() > 0 {
			sb.WriteString("\n")
		}
		sb.WriteString(s)
	}
	write(title)
	write(description)
	for _, t := range tags {
		write(t)
	}
	for _, s := range FlattenScalars(raw) {
		write(s)
	}
	write(body)
	return sb.String()
}

// RebuildSearchBlob recomputes d.SearchBlob from the document's normalized
// fields and returns it.
func (d *Document) RebuildSearchBlob() string {
	d.SearchBlob = BuildSearchBlob(d.Title, d.Description, d.Tags, d.Raw, d.Body)
	return d.SearchBlob
}

// FlattenScalars walks a raw frontmatter map and returns every scalar leaf
// value as a string, in deterministic (sorted-key) order. Nested maps and
// slices are recursed into; keys themselves and nils are skipped.
func FlattenScalars(raw map[string]any) []string {
	if len(raw) == 0 {
		return nil
	}
	keys := make([]string, 0, len(raw))
	for k := range raw {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	var out []string
	for _, k := range keys {
		out = append(out, flattenValue(raw[k])...)
	}
	return out
}

func flattenValue(v any) []string {
	switch t := v.(type) {
	case nil:
		return nil
	case string:
		if strings.TrimSpace(t) == "" {
			return nil
		}
		return []string{t}
	case time.Time:
		return []string{t.Format(time.RFC3339)}
	case bool,
		int, int8, int16, int32, int64,
		uint, uint8, uint16, uint32, uint64,
		float32, float64:
		return []string{fmt.Sprintf("%v", t)}
	case []any:
		var out []string
		for _, e := range t {
			out = append(out, flattenValue(e)...)
		}
		return out
	case []string:
		var out []string
		for _, e := range t {
			if strings.TrimSpace(e) != "" {
				out = append(out, e)
			}
		}
		return out
	case map[string]any:
		return FlattenScalars(t)
	case map[string]string:
		keys := make([]string, 0, len(t))
		for k := range t {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		var out []string
		for _, k := range keys {
			if strings.TrimSpace(t[k]) != "" {
				out = append(out, t[k])
			}
		}
		return out
	default:
		if s, ok := t.(fmt.Stringer); ok {
			return []string{s.String()}
		}
		return []string{fmt.Sprintf("%v", t)}
	}
}
