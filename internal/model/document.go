package model

import "time"

type FormatKind string

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
	ID       string
	Resource string
	Title    string
	Author   string
}

type Document struct {
	Path         string
	Title        string
	DocType      string
	Description  string
	Resource     string
	Role         string // Role: ""|index|log
	Tags         []string
	Body         string
	CreatedAt    *time.Time
	UpdatedAt    *time.Time
	Status       string
	Organized    *bool
	Archived     *bool
	BelongsTo    []string
	RelatedTo    []string
	Sources      []Source
	Generated    *Attestation
	Verified     *Attestation
	Format       FormatKind
	Raw          map[string]any
	SearchBlob   string
	ParseError   error
}
