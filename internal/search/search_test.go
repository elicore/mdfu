package search

import (
	"testing"
	"time"

	"github.com/anomalyco/mdfu/internal/model"
	"github.com/anomalyco/mdfu/internal/query"
)

func timePtr(s string) *time.Time {
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		panic(err)
	}
	utc := time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
	return &utc
}

func timePtrRFC3339(s string) *time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		panic(err)
	}
	return &t
}

func boolPtr(b bool) *bool { return &b }

func mustParse(t *testing.T, input string) *query.Query {
	t.Helper()
	q, err := query.Parse(input)
	if err != nil {
		t.Fatalf("Parse(%q) error: %v", input, err)
	}
	return q
}

func TestMatchesTags(t *testing.T) {
	d := &model.Document{Path: "a.md", Tags: []string{"Foo", "Bar"}}
	if !MatchesDoc(d, mustParse(t, "tag:foo")) {
		t.Fatal("tag:foo should match (case-insensitive)")
	}
	if !MatchesDoc(d, mustParse(t, "tags:foo,bar")) {
		t.Fatal("tags:foo,bar should match")
	}
	if MatchesDoc(d, mustParse(t, "tag:baz")) {
		t.Fatal("tag:baz should not match")
	}
	if MatchesDoc(d, mustParse(t, "tag:-foo")) {
		t.Fatal("tag:-foo should exclude doc with foo")
	}
	if !MatchesDoc(d, mustParse(t, "tag:-baz")) {
		t.Fatal("tag:-baz should match (doc lacks baz)")
	}
}

func TestMatchesDocType(t *testing.T) {
	d := &model.Document{Path: "a.md", DocType: "Note"}
	if !MatchesDoc(d, mustParse(t, "type:note")) {
		t.Fatal("type should be case-insensitive exact")
	}
	if MatchesDoc(d, mustParse(t, "type:metric")) {
		t.Fatal("type:metric should not match Note")
	}
}

func TestMatchesTitle(t *testing.T) {
	d := &model.Document{Path: "a.md", Title: "Hello World"}
	if !MatchesDoc(d, mustParse(t, "title:hello")) {
		t.Fatal("title:hello should match")
	}
	if !MatchesDoc(d, mustParse(t, "title:hlo")) {
		t.Fatal("title:hlo should fuzzy-match Hello World")
	}
	if MatchesDoc(d, mustParse(t, "title:xyz")) {
		t.Fatal("title:xyz should not match")
	}
}

func TestMatchesStatusAndArchived(t *testing.T) {
	d := &model.Document{Path: "a.md", Status: "draft"}
	if !MatchesDoc(d, mustParse(t, "status:draft")) {
		t.Fatal("status:draft should match")
	}
	if !MatchesDoc(d, mustParse(t, "status:DRAFT")) {
		t.Fatal("status should fold case")
	}
	if MatchesDoc(d, mustParse(t, "status:published")) {
		t.Fatal("status:published should not match draft")
	}
	// status:archived matches Archived bool via IsArchived.
	arch := &model.Document{Path: "b.md", Status: "", Archived: boolPtr(true)}
	if !MatchesDoc(arch, mustParse(t, "status:archived")) {
		t.Fatal("status:archived should match Archived=true")
	}
	arch2 := &model.Document{Path: "c.md", Status: "archived"}
	if !MatchesDoc(arch2, mustParse(t, "status:archived")) {
		t.Fatal("status:archived should match Status=archived")
	}
	active := &model.Document{Path: "d.md", Status: "draft"}
	if MatchesDoc(active, mustParse(t, "status:archived")) {
		t.Fatal("status:archived should not match draft")
	}
}

func TestMatchesPath(t *testing.T) {
	d := &model.Document{Path: "notes/sub/dir.md"}
	if !MatchesDoc(d, mustParse(t, "path:sub/")) {
		t.Fatal("path:sub/ should match")
	}
	if !MatchesDoc(d, mustParse(t, "path:SUB")) {
		t.Fatal("path should fold case")
	}
	if MatchesDoc(d, mustParse(t, "path:other")) {
		t.Fatal("path:other should not match")
	}
}

func TestMatchesGenericRaw(t *testing.T) {
	d := &model.Document{
		Path: "a.md",
		Raw:  map[string]any{"author": "John Doe", "count": 42},
	}
	if !MatchesDoc(d, mustParse(t, "author:john")) {
		t.Fatal("generic author:john should match Raw")
	}
	if !MatchesDoc(d, mustParse(t, "AUTHOR:doe")) {
		t.Fatal("generic key lookup should be case-insensitive")
	}
	if MatchesDoc(d, mustParse(t, "author:xyz")) {
		t.Fatal("author:xyz should not match")
	}
	if MatchesDoc(d, mustParse(t, "missing:value")) {
		t.Fatal("missing key should not match")
	}
}

func TestMatchesGenericNormalized(t *testing.T) {
	d := &model.Document{Path: "a.md", DocType: "Note", Resource: "https://example.com", Organized: boolPtr(true)}
	if !MatchesDoc(d, mustParse(t, "resource:example")) {
		t.Fatal("generic resource should hit normalized field")
	}
	if !MatchesDoc(d, mustParse(t, "organized:true")) {
		t.Fatal("organized:true should match")
	}
	if MatchesDoc(d, mustParse(t, "organized:false")) {
		t.Fatal("organized:false should not match true")
	}
	arch := &model.Document{Path: "b.md", Archived: boolPtr(true)}
	if !MatchesDoc(arch, mustParse(t, "archived:true")) {
		t.Fatal("archived:true should match via IsArchived")
	}
}

func TestMatchesDates(t *testing.T) {
	d := &model.Document{Path: "a.md", CreatedAt: timePtr("2024-03-15"), UpdatedAt: timePtr("2024-06-01")}
	if !MatchesDoc(d, mustParse(t, "created:2024-03-15")) {
		t.Fatal("created exact should match")
	}
	if MatchesDoc(d, mustParse(t, "created:2024-01-01")) {
		t.Fatal("created other day should not match")
	}
	if !MatchesDoc(d, mustParse(t, "created:2024-01-01..2024-12-31")) {
		t.Fatal("created range should match")
	}
	if !MatchesDoc(d, mustParse(t, "updated:>=2024-01-01")) {
		t.Fatal("updated:>= should match")
	}
	if MatchesDoc(d, mustParse(t, "updated:<2024-01-01")) {
		t.Fatal("updated:< should not match")
	}
	// date: either semantics.
	if !MatchesDoc(d, mustParse(t, "date:2024-06-01")) {
		t.Fatal("date: matching UpdatedAt should match (either)")
	}
	if !MatchesDoc(d, mustParse(t, "date:2024-03-15")) {
		t.Fatal("date: matching CreatedAt should match (either)")
	}
	if MatchesDoc(d, mustParse(t, "date:2023-01-01")) {
		t.Fatal("date: neither should not match")
	}
	// Nil dates excluded only when filter present.
	nilDoc := &model.Document{Path: "b.md"}
	if MatchesDoc(nilDoc, mustParse(t, "created:2024-01-01")) {
		t.Fatal("nil CreatedAt with filter should not match")
	}
	if !MatchesDoc(nilDoc, mustParse(t, "hello")) {
		// No date filter; nil dates should not exclude.
		// (Bare words are Rank-only, MatchesDoc ignores them.)
		t.Fatal("nil dates without date filter should not exclude")
	}
	emptyQ, _ := query.Parse("")
	if !MatchesDoc(nilDoc, emptyQ) {
		t.Fatal("empty query should match nil-date doc")
	}
}

func TestRankTitleBoost(t *testing.T) {
	blob := "metric data here"
	a := &model.Document{Path: "a.md", Title: "metric overview", SearchBlob: blob}
	b := &model.Document{Path: "b.md", Title: "unrelated notes", SearchBlob: blob}
	q := mustParse(t, "metric")
	got := Rank([]*model.Document{b, a}, q)
	if len(got) != 2 {
		t.Fatalf("Rank len = %d, want 2", len(got))
	}
	if got[0].Path != "a.md" {
		t.Fatalf("title-boost failed: first = %s, want a.md", got[0].Path)
	}
}

func TestRankEmptyBareSortedByPath(t *testing.T) {
	b := &model.Document{Path: "b.md", Title: "B"}
	a := &model.Document{Path: "a.md", Title: "A"}
	q := mustParse(t, "tag:foo")
	// No tag filters match? Actually docs have no tags; tag:foo filters all out.
	// Use empty query instead.
	eq, _ := query.Parse("")
	got := Rank([]*model.Document{b, a}, eq)
	if len(got) != 2 || got[0].Path != "a.md" || got[1].Path != "b.md" {
		t.Fatalf("empty Bare should sort by path, got %v", paths(got))
	}
	_ = q
}

func TestRankTieBreakUpdatedThenPath(t *testing.T) {
	old := &model.Document{Path: "b.md", Title: "same", SearchBlob: "same blob", UpdatedAt: timePtr("2024-01-01")}
	recent := &model.Document{Path: "a.md", Title: "same", SearchBlob: "same blob", UpdatedAt: timePtr("2024-06-01")}
	q := mustParse(t, "same")
	got := Rank([]*model.Document{old, recent}, q)
	if len(got) != 2 {
		t.Fatalf("len = %d", len(got))
	}
	if got[0].Path != "a.md" {
		t.Fatalf("most-recent should win tie, got %v", paths(got))
	}
	// Both nil UpdatedAt -> path tie-break.
	p1 := &model.Document{Path: "b.md", Title: "same", SearchBlob: "same blob"}
	p2 := &model.Document{Path: "a.md", Title: "same", SearchBlob: "same blob"}
	got = Rank([]*model.Document{p1, p2}, q)
	if got[0].Path != "a.md" {
		t.Fatalf("path tie-break failed, got %v", paths(got))
	}
}

func TestRankFiltersHard(t *testing.T) {
	a := &model.Document{Path: "a.md", Title: "hello", Tags: []string{"x"}, SearchBlob: "hello"}
	b := &model.Document{Path: "b.md", Title: "hello", Tags: []string{"y"}, SearchBlob: "hello"}
	q := mustParse(t, "hello tag:x")
	got := Rank([]*model.Document{a, b}, q)
	if len(got) != 1 || got[0].Path != "a.md" {
		t.Fatalf("Rank should hard-filter tags, got %v", paths(got))
	}
}

func TestRankBlobFallback(t *testing.T) {
	d := &model.Document{Path: "a.md", Title: "hello world", Body: "some body text"}
	q := mustParse(t, "hello")
	got := Rank([]*model.Document{d}, q)
	if len(got) != 1 {
		t.Fatal("fallback to Title+Body should match")
	}
}

func TestFilterArchived(t *testing.T) {
	a := &model.Document{Path: "a.md"}
	b := &model.Document{Path: "b.md", Archived: boolPtr(true)}
	c := &model.Document{Path: "c.md", Status: "archived"}
	docs := []*model.Document{a, b, c}
	hidden := FilterArchived(docs, false)
	if len(hidden) != 1 || hidden[0].Path != "a.md" {
		t.Fatalf("FilterArchived(false) = %v", paths(hidden))
	}
	all := FilterArchived(docs, true)
	if len(all) != 3 {
		t.Fatalf("FilterArchived(true) = %v", paths(all))
	}
	// Rank does not force-hide.
	q, _ := query.Parse("")
	ranked := Rank(docs, q)
	if len(ranked) != 3 {
		t.Fatalf("Rank should not hide archived, got %v", paths(ranked))
	}
}

func TestMatchesRFC3339DocTimes(t *testing.T) {
	d := &model.Document{Path: "a.md", CreatedAt: timePtrRFC3339("2024-03-15T10:00:00Z")}
	if !MatchesDoc(d, mustParse(t, "created:2024-03-15")) {
		t.Fatal("RFC3339 instant should match day filter")
	}
	if MatchesDoc(d, mustParse(t, "created:2024-03-16")) {
		t.Fatal("other day should not match")
	}
}

func paths(docs []*model.Document) []string {
	out := make([]string, 0, len(docs))
	for _, d := range docs {
		out = append(out, d.Path)
	}
	return out
}
