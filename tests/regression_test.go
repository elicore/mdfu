package tests

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/elicore/mdfu/internal/parse"
	"github.com/elicore/mdfu/internal/query"
	"github.com/elicore/mdfu/internal/scan"
	"github.com/elicore/mdfu/internal/search"
)

// 12. Bad-YAML file still searchable by body.
func TestRegressionBadYAMLSearchable(t *testing.T) {
	root := buildVault(t, map[string]string{
		"bad.md": "---\ntype: [unclosed\n  title: \"missing quote\ntags: [a, b\n---\n\n# Recovered Title\n\nBody still searchable: kumquat zebra xylophone.\n",
		"ok.md":  "# OK\n\nplain body about gardening\n",
	})
	docs := loadDocs(t, root, false)
	if len(docs) != 2 {
		t.Fatalf("loadDocs = %d, want 2", len(docs))
	}
	for _, d := range docs {
		if filepath.Base(d.Path) == "bad.md" && d.ParseError == nil {
			t.Fatal("bad.md ParseError = nil, want non-nil")
		}
	}
	got := searchVault(t, docs, "kumquat")
	if len(got) != 1 || filepath.Base(got[0].Path) != "bad.md" {
		t.Fatalf("bare 'kumquat' = %v, want [bad.md]", baseList(got))
	}
}

// 13. No-frontmatter file searchable (title from H1, fallback to filename).
func TestRegressionNoFrontmatter(t *testing.T) {
	root := buildVault(t, map[string]string{
		"lone.md":    "# Lone Note\n\nJust body, no frontmatter at all. Mentions honeycrisp apples.\n",
		"nofname.md": "just plain text mentioning durian fruit with no heading at all\n",
	})
	docs := loadDocs(t, root, false)
	byBase := map[string]string{}
	for _, d := range docs {
		byBase[filepath.Base(d.Path)] = d.Title
	}
	if byBase["lone.md"] != "Lone Note" {
		t.Fatalf("lone.md Title = %q, want H1 %q", byBase["lone.md"], "Lone Note")
	}
	if byBase["nofname.md"] != "nofname" {
		t.Fatalf("nofname.md Title = %q, want filename fallback %q", byBase["nofname.md"], "nofname")
	}
	got := searchVault(t, docs, "honeycrisp")
	if len(got) != 1 || filepath.Base(got[0].Path) != "lone.md" {
		t.Fatalf("honeycrisp = %v, want [lone.md]", baseList(got))
	}
	got = searchVault(t, docs, "durian")
	if len(got) != 1 || filepath.Base(got[0].Path) != "nofname.md" {
		t.Fatalf("durian = %v, want [nofname.md]", baseList(got))
	}
}

// 14. index.md / log.md get Role set and are not dropped.
func TestRegressionIndexLogRoles(t *testing.T) {
	root := buildVault(t, map[string]string{
		"index.md":   "---\ntitle: Vault Index\n---\n\n# Vault Index\n\nsharedterm index body\n",
		"sub/log.md": "---\ntitle: Daily Log\n---\n\n# Daily Log\n\nsharedterm log body\n",
		"note.md":    "# Plain\n\nsharedterm plain body\n",
	})
	docs := loadDocs(t, root, false)
	roles := map[string]string{}
	for _, d := range docs {
		roles[filepath.Base(d.Path)] = d.Role
	}
	if roles["index.md"] != "index" {
		t.Fatalf("index.md Role = %q, want %q", roles["index.md"], "index")
	}
	if roles["log.md"] != "log" {
		t.Fatalf("log.md Role = %q, want %q", roles["log.md"], "log")
	}
	if roles["note.md"] != "" {
		t.Fatalf("note.md Role = %q, want empty", roles["note.md"])
	}
	// Roles deprioritize but never drop: all docs rank.
	got := searchVault(t, docs, "sharedterm")
	if len(got) != 3 {
		t.Fatalf("sharedterm = %v, want all 3 incl. index/log", baseList(got))
	}
	emptyQ, _ := query.Parse("")
	if got := search.Rank(docs, emptyQ); len(got) != 3 {
		t.Fatalf("empty query rank = %v, want all 3", baseList(got))
	}
}

// 15. OKF v0.1 legacy `timestamp` populates a date.
func TestRegressionLegacyTimestamp(t *testing.T) {
	root := buildVault(t, map[string]string{
		"legacy.md": "---\ntype: Claim\ntimestamp: 2023-11-15\nstatus: Draft\ntags: retention, cohort\n---\n\n# Legacy Retention Claim\n\nCohort retention improved.\n",
	})
	docs := loadDocs(t, root, false)
	if len(docs) != 1 {
		t.Fatalf("loadDocs = %d, want 1", len(docs))
	}
	if docs[0].CreatedAt == nil {
		t.Fatal("legacy timestamp: CreatedAt = nil, want non-nil")
	}
	got := searchVault(t, docs, "created:2023-11-15")
	if len(got) != 1 {
		t.Fatalf("created:2023-11-15 over legacy timestamp = %v, want [legacy.md]", baseList(got))
	}
}

// 16. Portent `tags: "a, b, [[Project X]]"` splits/strips correctly.
func TestRegressionPortentTagsSplit(t *testing.T) {
	root := buildVault(t, map[string]string{
		"task.md": "---\ntype: Task\ntitle: T\ntags: \"a, b, [[Project X]]\"\n---\n\n# T\n\nbody\n",
	})
	docs := loadDocs(t, root, false)
	if len(docs) != 1 {
		t.Fatalf("loadDocs = %d, want 1", len(docs))
	}
	want := []string{"a", "b", "Project X"}
	if len(docs[0].Tags) != len(want) {
		t.Fatalf("Tags = %q, want %q", docs[0].Tags, want)
	}
	for i := range want {
		if docs[0].Tags[i] != want[i] {
			t.Fatalf("Tags = %q, want %q", docs[0].Tags, want)
		}
	}
	// Stripped tags are filterable (exact, case-insensitive per MatchesDoc).
	if got := searchVault(t, docs, "tag:a"); len(got) != 1 {
		t.Fatalf("tag:a = %v, want [task.md]", baseList(got))
	}
	if got := searchVault(t, docs, `tag:"Project X"`); len(got) != 1 || filepath.Base(got[0].Path) != "task.md" {
		t.Fatalf("tag:\"Project X\" = %v, want [task.md]", baseList(got))
	}
}

// 17. [[wikilink]] in belongs_to / related_to searchable.
func TestRegressionWikilinkRelations(t *testing.T) {
	root := buildVault(t, map[string]string{
		"task.md":  "---\ntype: Task\ntitle: Ship mdfu MVP\nbelongs_to: \"[[Project Atlas]]\"\nrelated_to:\n  - \"[[Launch Plan]]\"\n  - Retro Notes\n---\n\n# Ship mdfu MVP\n\nbody\n",
		"other.md": "# Other\n\nunrelated gardening body\n",
	})
	docs := loadDocs(t, root, false)
	var found bool
	for _, d := range docs {
		if filepath.Base(d.Path) == "task.md" {
			found = true
			if len(d.BelongsTo) != 1 || d.BelongsTo[0] != "Project Atlas" {
				t.Fatalf("BelongsTo = %q, want [Project Atlas]", d.BelongsTo)
			}
			if len(d.RelatedTo) != 2 || d.RelatedTo[0] != "Launch Plan" || d.RelatedTo[1] != "Retro Notes" {
				t.Fatalf("RelatedTo = %q, want [Launch Plan Retro Notes]", d.RelatedTo)
			}
		}
	}
	if !found {
		t.Fatal("task.md not loaded")
	}
	// Generic lookup on the relation field (brackets stripped or substring).
	if got := searchVault(t, docs, "belongs_to:Atlas"); len(got) != 1 || filepath.Base(got[0].Path) != "task.md" {
		t.Fatalf("belongs_to:Atlas = %v, want [task.md]", baseList(got))
	}
	// Bare word hits the wikilink value via SearchBlob.
	if got := searchVault(t, docs, "Atlas"); len(got) != 1 || filepath.Base(got[0].Path) != "task.md" {
		t.Fatalf("bare 'Atlas' = %v, want [task.md]", baseList(got))
	}
}

// 18. Hidden files excluded by default, included with IncludeHidden.
func TestRegressionHiddenFiles(t *testing.T) {
	root := buildVault(t, map[string]string{
		"visible.md":       "# V\n\nvisible body\n",
		".secret.md":       "# S\n\nsecret body\n",
		".hiddendir/in.md": "# H\n\nhidden dir body\n",
	})
	def, err := scan.WalkMarkdown(scan.Options{Root: root})
	if err != nil {
		t.Fatalf("WalkMarkdown default: %v", err)
	}
	for _, p := range def {
		if strings.Contains(filepath.ToSlash(p), ".secret") || strings.Contains(filepath.ToSlash(p), ".hiddendir") {
			t.Fatalf("default walk should exclude hidden, got %v", def)
		}
	}
	if len(def) != 1 {
		t.Fatalf("default walk = %v, want [visible.md]", def)
	}
	withHidden, err := scan.WalkMarkdown(scan.Options{Root: root, IncludeHidden: true})
	if err != nil {
		t.Fatalf("WalkMarkdown IncludeHidden: %v", err)
	}
	if len(withHidden) != 3 {
		t.Fatalf("IncludeHidden walk = %v, want 3 files", withHidden)
	}
	// End-to-end: hidden doc parses and ranks only when included.
	docsHidden := loadDocs(t, root, true)
	if len(docsHidden) != 3 {
		t.Fatalf("loadDocs hidden = %d, want 3", len(docsHidden))
	}
}

// 19. .git directory always skipped.
func TestRegressionGitSkipped(t *testing.T) {
	root := buildVault(t, map[string]string{
		"note.md":      "# N\n\nnormal body\n",
		".git/evil.md": "# Evil\n\nshould never appear\n",
	})
	for _, includeHidden := range []bool{false, true} {
		got, err := scan.WalkMarkdown(scan.Options{Root: root, IncludeHidden: includeHidden})
		if err != nil {
			t.Fatalf("WalkMarkdown(hidden=%v): %v", includeHidden, err)
		}
		for _, p := range got {
			if strings.Contains(filepath.ToSlash(p), ".git") {
				t.Fatalf("walk(hidden=%v) included .git file: %v", includeHidden, got)
			}
		}
		if len(got) != 1 {
			t.Fatalf("walk(hidden=%v) = %v, want [note.md]", includeHidden, got)
		}
	}
}

// 20. Empty vault -> Rank empty, no panic.
func TestRegressionEmptyVault(t *testing.T) {
	root := t.TempDir()
	paths, err := scan.WalkMarkdown(scan.Options{Root: root})
	if err != nil {
		t.Fatalf("WalkMarkdown empty: %v", err)
	}
	if len(paths) != 0 {
		t.Fatalf("empty walk = %v, want []", paths)
	}
	// Full pipeline on zero docs.
	docs := loadDocs(t, root, false)
	if len(docs) != 0 {
		t.Fatalf("loadDocs empty = %d, want 0", len(docs))
	}
	q, err := query.Parse("lighthouse")
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	// Rank over nil/empty must not panic and must be empty.
	if got := search.Rank(docs, q); len(got) != 0 {
		t.Fatalf("Rank(empty, q) = %v, want empty", got)
	}
	if got := search.Rank(nil, q); len(got) != 0 {
		t.Fatalf("Rank(nil) = %v, want empty", got)
	}
	emptyQ, _ := query.Parse("")
	if got := search.Rank(docs, emptyQ); len(got) != 0 {
		t.Fatalf("Rank(empty, emptyQ) = %v, want empty", got)
	}
	if got := search.Rank(nil, emptyQ); len(got) != 0 {
		t.Fatalf("Rank(nil, empty) = %v, want empty", got)
	}
}

// 21. Date layout variants YYYY-MM and RFC3339 parse.
func TestRegressionDateLayouts(t *testing.T) {
	root := buildVault(t, map[string]string{
		"month.md": "---\ntitle: M\ncreated: 2026-01\n---\n\n# M\n\nmonth body\n",
		"rfc.md":   "---\ntitle: R\ncreated: 2024-06-01T10:00:00Z\n---\n\n# R\n\nrfc body\n",
	})
	// Direct parse checks (public API) for layout coverage.
	monthDoc, err := parse.ParseFile(filepath.Join(root, "month.md"))
	if err != nil {
		t.Fatalf("ParseFile month.md: %v", err)
	}
	if monthDoc.CreatedAt == nil {
		t.Fatal("YYYY-MM created: CreatedAt nil")
	}
	rfcDoc, err := parse.ParseFile(filepath.Join(root, "rfc.md"))
	if err != nil {
		t.Fatalf("ParseFile rfc.md: %v", err)
	}
	if rfcDoc.CreatedAt == nil {
		t.Fatal("RFC3339 created: CreatedAt nil")
	}

	docs := loadDocs(t, root, false)
	if got := searchVault(t, docs, "created:2026-01"); len(got) != 1 || filepath.Base(got[0].Path) != "month.md" {
		t.Fatalf("created:2026-01 = %v, want [month.md]", baseList(got))
	}
	if got := searchVault(t, docs, "created:2024-06-01"); len(got) != 1 || filepath.Base(got[0].Path) != "rfc.md" {
		t.Fatalf("created:2024-06-01 over RFC3339 instant = %v, want [rfc.md]", baseList(got))
	}
	// RFC3339 query form itself parses.
	if got := searchVault(t, docs, "created:2024-06-01T10:00:00Z"); len(got) != 1 {
		t.Fatalf("created:RFC3339 = %v, want [rfc.md]", baseList(got))
	}
}
