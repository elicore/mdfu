package model

import (
	"strings"
	"time"
)

// FormatKind identifies the frontmatter format a Document was parsed from.
type FormatKind string

const (
	FormatOKF     FormatKind = "okf"
	FormatPortent FormatKind = "portent"
	FormatGeneric FormatKind = "generic"
	FormatNone    FormatKind = "none"
)

// Attestation records who generated/verified a document and when.
type Attestation struct {
	By string
	At *time.Time
}

// Source is a single entry of the sources list.
type Source struct {
	ID       string
	Resource string
	Title    string
	Author   string
}

// Document is the unified normalized model shared by all tracks.
// Verbatim copy of the contract in PLAN.md. Do not evolve — integration will dedupe.
type Document struct {
	Path, Title, DocType, Description, Resource, Role string // Role: ""|index|log
	Tags                                             []string
	Body                                             string
	CreatedAt, UpdatedAt                             *time.Time
	Status                                           string
	Organized, Archived                              *bool
	BelongsTo, RelatedTo                             []string
	Sources                                          []Source
	Generated, Verified                              *Attestation
	Format                                           FormatKind
	Raw                                              map[string]any
	SearchBlob                                       string
	ParseError                                       error
}

// IsArchived reports whether the document is archived.
// True when the Archived bool pointer is non-nil and true,
// or when Status folds to "archived".
func (d *Document) IsArchived() bool {
	if d == nil {
		return false
	}
	if d.Archived != nil && *d.Archived {
		return true
	}
	if strings.EqualFold(strings.TrimSpace(d.Status), "archived") {
		return true
	}
	return false
}
