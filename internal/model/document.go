package model

import "time"

type FormatKind string // FormatOKF, FormatPortent, FormatGeneric, FormatNone

const (
	FormatOKF     FormatKind = "okf"
	FormatPortent FormatKind = "portent"
	FormatGeneric FormatKind = "generic"
	FormatNone    FormatKind = "none"
)

type Attestation struct {
	By string
	At *time.Time
}

type Source struct {
	ID, Resource, Title, Author string
}

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
