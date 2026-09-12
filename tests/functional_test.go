package tests

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/anomalyco/mdfu/internal/model"
	"github.com/anomalyco/mdfu/internal/output"
	"github.com/anomalyco/mdfu/internal/parse"
	"github.com/anomalyco/mdfu/internal/query"
	"github.com/anomalyco/mdfu/internal/scan"
	"github.com/anomalyco/mdfu/internal/search"
)

// buildVault creates a synthetic vault in t.TempDir from rel-path -> content
// mappings (slash-separated rel paths; subdirs created as needed) and returns
// the vault root.
func buildVault(t *testing.T, files map[string]string) string {
	t.Helper()
	root := t.TempDir()
	for rel, content := range files {
		full := filepath.Join(root, filepath.FromSlash(rel))
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatalf("MkdirAll %s: %v", filepath.Dir(full), err)
		}
		if err := os.WriteFile(full, []byte(content), 0o644); err != nil {
			t.Fatalf("WriteFile %s: %v", full, err)
		}
	}
	return root
}

// loadDocs runs the library pipeline stage scan -> parse over root.
func loadDocs(t *testing.T, root string, includeHidden bool) []*model.Document {
	t.Helper()
	paths, err := scan.WalkMarkdown(scan.Options{Root: root, IncludeHidden: includeHidden})
	if err != nil {
		t.Fatalf("WalkMarkdown(%s): %v", root, err)
	}
	var docs []*model.Document
	for _, p := range paths {
		d, err := parse.ParseFile(p)
		if err != nil {
			t.Fatalf("ParseFile(%s): %v", p, err)
		}
		if d == nil {
			t.Fatalf("ParseFile(%s) returned nil", p)
		}
		docs = append(docs, d)
	}
	return docs
}

func mustParseQuery(t *testing.T, input string) *query.Query {
	t.Helper()
	q, err := query.Parse(input)
	if err != nil {
		t.Fatalf("query.Parse(%q): %v", input, err)
	}
	return q
}

// searchVault runs query.Parse + search.Rank over docs.
func searchVault(t *testing.T, docs []*model.Document, input string) []*model.Document {
	t.Helper()
	q := mustParseQuery(t, input)
	return search.Rank(docs, q)
}

func pathSet(docs []*model.Document) map[string]bool {
	out := map[string]bool{}
	for _, d := range docs {
		out[filepath.Base(d.Path)] = true
	}
	return out
}

func containsBase(docs []*model.Document, base string) bool {
	for _, d := range docs {
		if filepath.Base(d.Path) == base {
			return true
		}
	}
	return false
}

func baseList(docs []*model.Document) []string {
	var out []string
	for _, d := range docs {
		out = append(out, filepath.Base(d.Path))
	}
	return out
}

// 1. Body free-text find via bare word.
func TestFunctionalBodyFreeText(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "# Alpha\n\nthe lighthouse beacon shines over the harbor\n",
		"b.md": "# Beta\n\nnothing but gardening notes about compost rotation\n",
	})
	docs := loadDocs(t, root, false)
	got := searchVault(t, docs, "lighthouse")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("bare 'lighthouse' = %v, want [a.md]", baseList(got))
	}
}

// 2. Frontmatter value (description) found via bare word.
func TestFunctionalFrontmatterValueAsFreeText(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "---\ntitle: Alpha\ndescription: Tracks monthly active users of the platform.\n---\n\n# Alpha\n\nunrelated body about baking\n",
		"b.md": "# Beta\n\nunrelated body about baking\n",
	})
	docs := loadDocs(t, root, false)
	got := searchVault(t, docs, "platform")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("bare 'platform' (description) = %v, want [a.md]", baseList(got))
	}
}

// 3. tag: filter + negation (value-prefix per query.Parse) + comma tags.
func TestFunctionalTagFilterNegationComma(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "---\ntags: [growth, kpi]\n---\n\n# A\n\nbody a\n",
		"b.md": "---\ntags: [growth, monthly]\n---\n\n# B\n\nbody b\n",
		"c.md": "---\ntags: [other]\n---\n\n# C\n\nbody c\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "tag:growth")
	if s := pathSet(got); !(s["a.md"] && s["b.md"] && !s["c.md"] && len(got) == 2) {
		t.Fatalf("tag:growth = %v, want [a.md b.md]", baseList(got))
	}

	// Comma means AND: both tags required.
	got = searchVault(t, docs, "tags:growth,kpi")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("tags:growth,kpi = %v, want [a.md]", baseList(got))
	}

	// Negation is a value prefix: tag:-kpi / tags:growth,-kpi.
	got = searchVault(t, docs, "tag:-kpi")
	if s := pathSet(got); !(s["b.md"] && s["c.md"] && !s["a.md"]) {
		t.Fatalf("tag:-kpi = %v, want docs without kpi", baseList(got))
	}
	got = searchVault(t, docs, "tags:growth,-kpi")
	if len(got) != 1 || filepath.Base(got[0].Path) != "b.md" {
		t.Fatalf("tags:growth,-kpi = %v, want [b.md]", baseList(got))
	}
}

// 4. type: filter (OKF Metric + Portent Task, case-insensitive).
func TestFunctionalTypeFilter(t *testing.T) {
	root := buildVault(t, map[string]string{
		"metric.md": "---\ntype: Metric\ntitle: Monthly Active Users\n---\n\n# Monthly Active Users\n\nmau body\n",
		"task.md":   "---\ntype: Task\ntitle: Ship mdfu MVP\n---\n\n# Ship mdfu MVP\n\ntask body\n",
		"note.md":   "---\ntype: Note\ntitle: Random\n---\n\n# Random\n\nnote body\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "type:Metric")
	if len(got) != 1 || filepath.Base(got[0].Path) != "metric.md" {
		t.Fatalf("type:Metric = %v, want [metric.md]", baseList(got))
	}
	// Case-insensitive value.
	got = searchVault(t, docs, "type:task")
	if len(got) != 1 || filepath.Base(got[0].Path) != "task.md" {
		t.Fatalf("type:task = %v, want [task.md]", baseList(got))
	}
}

// 5. title: fuzzy.
func TestFunctionalTitleFuzzy(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "---\ntitle: Ship mdfu MVP\n---\n\n# Ship mdfu MVP\n\nbody one\n",
		"b.md": "---\ntitle: Unrelated gardening notes\n---\n\n# Unrelated gardening notes\n\nbody two\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "title:mdfu")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("title:mdfu = %v, want [a.md]", baseList(got))
	}
	// Fuzzy deletion still matches (cf. query/search title fuzzy semantics).
	got = searchVault(t, docs, "title:mdu")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("title:mdu (fuzzy) = %v, want [a.md]", baseList(got))
	}
	if containsBase(searchVault(t, docs, "title:xyz"), "a.md") {
		t.Fatal("title:xyz should not match a.md")
	}
}

// 6. path: substring (case-insensitive).
func TestFunctionalPathSubstring(t *testing.T) {
	root := buildVault(t, map[string]string{
		"notes/deep/a.md": "# A\n\nbody a\n",
		"other/b.md":      "# B\n\nbody b\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "path:notes")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("path:notes = %v, want [a.md]", baseList(got))
	}
	got = searchVault(t, docs, "path:DEEP")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("path:DEEP (fold) = %v, want [a.md]", baseList(got))
	}
	if containsBase(searchVault(t, docs, "path:nosuchdir"), "a.md") {
		t.Fatal("path:nosuchdir should match nothing")
	}
}

// 7. Generic key:value on custom frontmatter key.
func TestFunctionalGenericKeyValue(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "---\nflavor: citrus\ntitle: A\n---\n\n# A\n\nbody a\n",
		"b.md": "---\nflavor: smoky\ntitle: B\n---\n\n# B\n\nbody b\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "flavor:citrus")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("flavor:citrus = %v, want [a.md]", baseList(got))
	}
	// Key lookup is case-insensitive, value fuzzy fold.
	got = searchVault(t, docs, "FLAVOR:cit")
	if len(got) != 1 || filepath.Base(got[0].Path) != "a.md" {
		t.Fatalf("FLAVOR:cit = %v, want [a.md]", baseList(got))
	}
	if len(searchVault(t, docs, "flavor:xyz")) != 0 {
		t.Fatal("flavor:xyz should match nothing")
	}
}

// 8. Date filters: created:/updated:/before:/after: with YYYY-MM-DD and A..B.
func TestFunctionalDateFilters(t *testing.T) {
	root := buildVault(t, map[string]string{
		"old.md": "---\ncreated: 2024-01-10\ntitle: Old\n---\n\n# Old\n\nold body\n",
		"mid.md": "---\ncreated: 2024-03-15\ntitle: Mid\n---\n\n# Mid\n\nmid body\n",
		"new.md": "---\ncreated: 2024-06-01\ntitle: New\nupdated: 2024-06-02\n---\n\n# New\n\nnew body\n",
	})
	docs := loadDocs(t, root, false)

	got := searchVault(t, docs, "created:2024-03-15")
	if len(got) != 1 || filepath.Base(got[0].Path) != "mid.md" {
		t.Fatalf("created:2024-03-15 = %v, want [mid.md]", baseList(got))
	}
	got = searchVault(t, docs, "created:2024-01-01..2024-04-01")
	if s := pathSet(got); !(s["old.md"] && s["mid.md"] && !s["new.md"]) {
		t.Fatalf("created range = %v, want [old.md mid.md]", baseList(got))
	}
	got = searchVault(t, docs, "before:2024-03-01")
	if len(got) != 1 || filepath.Base(got[0].Path) != "old.md" {
		t.Fatalf("before:2024-03-01 = %v, want [old.md]", baseList(got))
	}
	got = searchVault(t, docs, "after:2024-05-01")
	if len(got) != 1 || filepath.Base(got[0].Path) != "new.md" {
		t.Fatalf("after:2024-05-01 = %v, want [new.md]", baseList(got))
	}
	got = searchVault(t, docs, "updated:2024-06-02")
	if len(got) != 1 || filepath.Base(got[0].Path) != "new.md" {
		t.Fatalf("updated:2024-06-02 = %v, want [new.md]", baseList(got))
	}
}

// 9. status: + archived hidden-by-default via FilterArchived.
func TestFunctionalStatusAndArchived(t *testing.T) {
	root := buildVault(t, map[string]string{
		"active.md":    "---\nstatus: draft\ntitle: Active\n---\n\n# Active\n\nactive body\n",
		"flagged.md":   "---\nstatus: draft\narchived: true\ntitle: Flagged\n---\n\n# Flagged\n\nflagged body\n",
		"by_status.md": "---\nstatus: archived\ntitle: ByStatus\n---\n\n# ByStatus\n\nbystatus body\n",
	})
	docs := loadDocs(t, root, false)
	if len(docs) != 3 {
		t.Fatalf("loadDocs = %d, want 3", len(docs))
	}

	got := searchVault(t, docs, "status:draft")
	// MatchesDoc matches Status exactly (fold); the archived flag does not
	// exclude a status:draft hit — hiding is FilterArchived's job.
	if s := pathSet(got); !(s["active.md"] && s["flagged.md"] && len(got) == 2) {
		t.Fatalf("status:draft = %v, want [active.md flagged.md]", baseList(got))
	}

	// Rank itself does not hide archived; FilterArchived does.
	all := searchVault(t, docs, "")
	if len(all) != 3 {
		t.Fatalf("empty query rank = %v, want all 3 (no forced hiding)", baseList(all))
	}
	visible := search.FilterArchived(all, false)
	if len(visible) != 1 || filepath.Base(visible[0].Path) != "active.md" {
		t.Fatalf("FilterArchived(false) = %v, want [active.md]", baseList(visible))
	}
	kept := search.FilterArchived(all, true)
	if len(kept) != 3 {
		t.Fatalf("FilterArchived(true) = %v, want all 3", baseList(kept))
	}

	// Explicit status:archived still finds both archived docs
	// (bool flag via IsArchived + literal status).
	got = searchVault(t, docs, "status:archived")
	if s := pathSet(got); !(s["flagged.md"] && s["by_status.md"] && len(got) == 2) {
		t.Fatalf("status:archived = %v, want [flagged.md by_status.md]", baseList(got))
	}
}

// 10. Title-boost ranking: term in title outranks body-only match.
func TestFunctionalTitleBoostRanking(t *testing.T) {
	body := "lighthouse beacon shines over the harbor with detailed keeper notes"
	root := buildVault(t, map[string]string{
		"title_hit.md": "---\ntitle: lighthouse overview\n---\n\n# lighthouse overview\n\n" + body + "\n",
		"body_only.md": "---\ntitle: unrelated notes\n---\n\n# unrelated notes\n\n" + body + "\n",
	})
	docs := loadDocs(t, root, false)
	got := searchVault(t, docs, "lighthouse")
	if len(got) != 2 {
		t.Fatalf("lighthouse rank = %v, want 2 docs", baseList(got))
	}
	if filepath.Base(got[0].Path) != "title_hit.md" {
		t.Fatalf("title-boost failed: order = %v, want title_hit.md first", baseList(got))
	}
}

// 11. output.FormatJSON validity + FormatPaths line count + Snippet contains term.
func TestFunctionalOutputFormats(t *testing.T) {
	root := buildVault(t, map[string]string{
		"a.md": "# Alpha notes\n\nthe lighthouse beacon shines over the harbor with extra filler text to make the body long enough for snippets\n",
		"b.md": "# Beta notes\n\nthe lighthouse keeper logs daily events with extra filler text to make the body long enough for snippets\n",
	})
	docs := loadDocs(t, root, false)
	q := mustParseQuery(t, "lighthouse")
	ranked := search.Rank(docs, q)
	if len(ranked) != 2 {
		t.Fatalf("rank lighthouse = %v, want 2", baseList(ranked))
	}

	var results []output.Result
	for _, d := range ranked {
		results = append(results, output.Result{
			Path:    d.Path,
			Title:   d.Title,
			DocType: d.DocType,
			Snippet: output.Snippet(d.Body, q.Bare, 80),
		})
	}

	js, err := output.FormatJSON(results)
	if err != nil {
		t.Fatalf("FormatJSON: %v", err)
	}
	var back []output.Result
	if err := json.Unmarshal([]byte(js), &back); err != nil {
		t.Fatalf("FormatJSON output invalid: %v\n%s", err, js)
	}
	if len(back) != len(results) {
		t.Fatalf("JSON round-trip len = %d, want %d", len(back), len(results))
	}

	pathsOut := output.FormatPaths(results)
	lines := strings.Split(strings.TrimSuffix(pathsOut, "\n"), "\n")
	if len(results) == 0 || len(lines) != len(results) {
		t.Fatalf("FormatPaths lines = %d, want %d (%q)", len(lines), len(results), pathsOut)
	}

	for i, r := range results {
		if !strings.Contains(strings.ToLower(r.Snippet), "lighthouse") {
			t.Fatalf("result %d snippet missing term: %q", i, r.Snippet)
		}
	}
}
