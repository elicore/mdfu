package parse

import (
	"strings"
	"testing"
	"time"

	"github.com/anomalyco/mdfu/internal/model"
)

func mustParseFile(t *testing.T, path string) *model.Document {
	t.Helper()
	doc, err := ParseFile(path)
	if err != nil {
		t.Fatalf("ParseFile(%s) returned error: %v", path, err)
	}
	if doc == nil {
		t.Fatalf("ParseFile(%s) returned nil document", path)
	}
	return doc
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func checkTime(t *testing.T, field string, got *time.Time, want string) {
	t.Helper()
	if want == "" {
		if got != nil {
			t.Errorf("%s = %v, want nil", field, got)
		}
		return
	}
	if got == nil {
		t.Errorf("%s = nil, want %s", field, want)
		return
	}
	wantT, err := time.Parse(time.RFC3339, want)
	if err != nil {
		t.Fatalf("bad want time %q: %v", want, err)
	}
	if !got.Equal(wantT) {
		t.Errorf("%s = %v, want %v", field, got, wantT)
	}
}

func TestParseFixtures(t *testing.T) {
	tests := []struct {
		file        string
		title       string
		docType     string
		tags        []string
		createdAt   string // RFC3339 or "" for nil
		updatedAt   string // RFC3339 or "" for nil
		status      string
		format      model.FormatKind
		role        string
		blobContain []string
		wantErr     bool
	}{
		{
			file:      "../../testdata/okf-v02-metric.md",
			title:     "Monthly Active Users",
			docType:   "Metric",
			tags:      []string{"growth", "kpi", "monthly"},
			createdAt: "2024-05-01T00:00:00Z",
			updatedAt: "2024-06-01T10:00:00Z",
			status:    "verified",
			format:    model.FormatOKF,
			blobContain: []string{
				"Monthly Active Users",
				"monthly active users of the platform",
				"growth",
				"data-pipeline",
				"alice",
				"snowflake://analytics/mau",
				"12% month over month",
			},
		},
		{
			file:      "../../testdata/okf-v01-legacy.md",
			title:     "Legacy Retention Claim", // H1 fallback, no fm title
			docType:   "Claim",
			tags:      []string{"retention", "cohort"},
			createdAt: "2023-11-15T00:00:00Z", // via timestamp key
			updatedAt: "2023-11-15T00:00:00Z",
			status:    "draft", // lowercased
			format:    model.FormatOKF,
			blobContain: []string{
				"Legacy Retention Claim",
				"Citations",
			},
		},
		{
			file:      "../../testdata/portent-task.md",
			title:     "Ship mdfu MVP", // fm title wins over body H1
			docType:   "Task",
			tags:      []string{"tolaria", "launch", "Project Atlas"},
			createdAt: "2026-01-01T00:00:00Z", // YYYY-MM layout
			updatedAt: "",
			status:    "draft",
			format:    model.FormatPortent,
			blobContain: []string{
				"Ship mdfu MVP",
				"Project Atlas",
				"Launch Plan",
			},
		},
		{
			file:      "../../testdata/generic.md",
			title:     "Generic Note", // H1 fallback
			docType:   "",
			tags:      []string{"misc"},
			createdAt: "",
			updatedAt: "",
			status:    "",
			format:    model.FormatGeneric,
			blobContain: []string{
				"Generic Note",
				"plain note without a type",
				"bob",
				"42",
				"compost rotation",
			},
		},
		{
			file:      "../../testdata/nofrontmatter.md",
			title:     "Lone Note",
			docType:   "",
			tags:      nil,
			createdAt: "",
			updatedAt: "",
			status:    "",
			format:    model.FormatNone,
			blobContain: []string{
				"Lone Note",
				"honeycrisp apples",
			},
		},
		{
			file:      "../../testdata/bad-yaml.md",
			title:     "Recovered Title", // H1 fallback despite YAML error
			createdAt: "",
			updatedAt: "",
			format:    model.FormatGeneric,
			blobContain: []string{
				"kumquat zebra",
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.file, func(t *testing.T) {
			doc := mustParseFile(t, tt.file)
			if tt.title != "" && doc.Title != tt.title {
				t.Errorf("Title = %q, want %q", doc.Title, tt.title)
			}
			if tt.docType != "" && doc.DocType != tt.docType {
				t.Errorf("DocType = %q, want %q", doc.DocType, tt.docType)
			}
			if tt.tags != nil && !equalStrings(doc.Tags, tt.tags) {
				t.Errorf("Tags = %q, want %q", doc.Tags, tt.tags)
			}
			checkTime(t, "CreatedAt", doc.CreatedAt, tt.createdAt)
			checkTime(t, "UpdatedAt", doc.UpdatedAt, tt.updatedAt)
			if tt.status != "" && doc.Status != tt.status {
				t.Errorf("Status = %q, want %q", doc.Status, tt.status)
			}
			if doc.Format != tt.format {
				t.Errorf("Format = %q, want %q", doc.Format, tt.format)
			}
			if doc.Role != tt.role {
				t.Errorf("Role = %q, want %q", doc.Role, tt.role)
			}
			if doc.SearchBlob == "" {
				t.Errorf("SearchBlob is empty, want non-empty")
			}
			for _, want := range tt.blobContain {
				if !strings.Contains(doc.SearchBlob, want) {
					t.Errorf("SearchBlob missing %q\nblob:\n%s", want, doc.SearchBlob)
				}
			}
			if tt.wantErr && doc.ParseError == nil {
				t.Errorf("ParseError = nil, want non-nil")
			}
			if !tt.wantErr && doc.ParseError != nil {
				t.Errorf("ParseError = %v, want nil", doc.ParseError)
			}
		})
	}
}

func TestParseOKFv02AttestationsAndSources(t *testing.T) {
	doc := mustParseFile(t, "../../testdata/okf-v02-metric.md")

	if doc.Generated == nil || doc.Generated.By != "data-pipeline" {
		t.Errorf("Generated = %+v, want By=data-pipeline", doc.Generated)
	} else {
		checkTime(t, "Generated.At", doc.Generated.At, "2024-06-01T10:00:00Z")
	}
	if doc.Verified == nil || doc.Verified.By != "alice" {
		t.Errorf("Verified = %+v, want By=alice", doc.Verified)
	} else {
		checkTime(t, "Verified.At", doc.Verified.At, "2024-06-02T12:30:00Z")
	}
	if len(doc.Sources) != 2 {
		t.Fatalf("len(Sources) = %d, want 2", len(doc.Sources))
	}
	if doc.Sources[0].ID != "warehouse" || doc.Sources[0].Author != "data-team" {
		t.Errorf("Sources[0] = %+v, want ID=warehouse Author=data-team", doc.Sources[0])
	}
	if doc.Sources[1].Resource != "https://dash.example.com/mau" {
		t.Errorf("Sources[1].Resource = %q", doc.Sources[1].Resource)
	}
	if doc.Description == "" {
		t.Errorf("Description is empty, want non-empty")
	}
	if doc.IsArchived() {
		t.Errorf("IsArchived() = true, want false")
	}
}

func TestParsePortentRelationsAndFlags(t *testing.T) {
	doc := mustParseFile(t, "../../testdata/portent-task.md")

	if !equalStrings(doc.BelongsTo, []string{"Project Atlas"}) {
		t.Errorf("BelongsTo = %q, want [Project Atlas]", doc.BelongsTo)
	}
	if !equalStrings(doc.RelatedTo, []string{"Launch Plan", "Retro Notes"}) {
		t.Errorf("RelatedTo = %q, want [Launch Plan Retro Notes]", doc.RelatedTo)
	}
	if doc.Organized == nil || !*doc.Organized {
		t.Errorf("Organized = %v, want true pointer", doc.Organized)
	}
	if doc.Archived == nil || *doc.Archived {
		t.Errorf("Archived = %v, want false pointer", doc.Archived)
	}
	if doc.IsArchived() {
		t.Errorf("IsArchived() = true, want false")
	}
}

func TestParseRoleFromBasename(t *testing.T) {
	for path, want := range map[string]string{
		"vault/index.md": "index",
		"vault/INDEX.MD": "index",
		"notes/log.md":   "log",
		"notes/note.md":  "",
	} {
		doc, err := ParseContent(path, "# Hey\n\nbody\n")
		if err != nil {
			t.Fatalf("ParseContent(%s) error: %v", path, err)
		}
		if doc.Role != want {
			t.Errorf("Role for %s = %q, want %q", path, want, doc.Role)
		}
	}
}

func TestIsArchived(t *testing.T) {
	yes := true
	no := false
	tests := []struct {
		name string
		doc  model.Document
		want bool
	}{
		{"nil archived, plain status", model.Document{Status: "draft"}, false},
		{"archived flag true", model.Document{Archived: &yes}, true},
		{"archived flag false", model.Document{Archived: &no}, false},
		{"status archived", model.Document{Status: "archived"}, true},
		{"both", model.Document{Archived: &yes, Status: "draft"}, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.doc.IsArchived(); got != tt.want {
				t.Errorf("IsArchived() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestSplitFrontmatter(t *testing.T) {
	tests := []struct {
		name      string
		content   string
		wantFM    string
		wantBody  string
		wantHasFM bool
	}{
		{"basic", "---\ntitle: x\n---\nbody\n", "title: x", "body\n", true},
		{"no fm", "# Title\n\nbody\n", "", "# Title\n\nbody\n", false},
		{"unclosed", "---\ntitle: x\nbody\n", "", "---\ntitle: x\nbody\n", false},
		{"not leading", "\n---\ntitle: x\n---\nbody\n", "", "\n---\ntitle: x\n---\nbody\n", false},
		{"no newline after dashes", "---title: x\n---\n", "", "---title: x\n---\n", false},
		{"empty fm", "---\n---\nbody\n", "", "body\n", true},
		{"hr in body untouched", "hello\n\n---\n\nworld\n", "", "hello\n\n---\n\nworld\n", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fm, body, hasFM := SplitFrontmatter(tt.content)
			if fm != tt.wantFM || body != tt.wantBody || hasFM != tt.wantHasFM {
				t.Errorf("SplitFrontmatter() = (%q, %q, %v), want (%q, %q, %v)",
					fm, body, hasFM, tt.wantFM, tt.wantBody, tt.wantHasFM)
			}
		})
	}
}

func TestTitleFallbackToFilename(t *testing.T) {
	doc, err := ParseContent("notes/my-idea.md", "no heading here\n")
	if err != nil {
		t.Fatalf("ParseContent error: %v", err)
	}
	if doc.Title != "my-idea" {
		t.Errorf("Title = %q, want %q", doc.Title, "my-idea")
	}
	if doc.Format != model.FormatNone {
		t.Errorf("Format = %q, want none", doc.Format)
	}
}
