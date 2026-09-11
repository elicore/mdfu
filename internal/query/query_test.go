package query

import (
	"testing"
	"time"
)

func TestParseBare(t *testing.T) {
	q, err := Parse("hello world")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if len(q.Bare) != 2 || q.Bare[0] != "hello" || q.Bare[1] != "world" {
		t.Fatalf("Bare = %v, want [hello world]", q.Bare)
	}
	if q.Raw != "hello world" {
		t.Fatalf("Raw = %q", q.Raw)
	}
}

func TestParseTags(t *testing.T) {
	q, err := Parse("tag:foo")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if len(q.Tags) != 1 || q.Tags[0] != "foo" {
		t.Fatalf("Tags = %v", q.Tags)
	}

	q, _ = Parse("tags:a,b")
	if len(q.Tags) != 2 || q.Tags[0] != "a" || q.Tags[1] != "b" {
		t.Fatalf("Tags = %v, want [a b]", q.Tags)
	}

	// Case-insensitive key, preserve value case.
	q, _ = Parse("TAG:Foo")
	if len(q.Tags) != 1 || q.Tags[0] != "Foo" {
		t.Fatalf("Tags = %v, want [Foo]", q.Tags)
	}
}

func TestParseTagNegation(t *testing.T) {
	q, err := Parse("tag:-foo")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if len(q.NotTags) != 1 || q.NotTags[0] != "foo" {
		t.Fatalf("NotTags = %v", q.NotTags)
	}
	if len(q.Tags) != 0 {
		t.Fatalf("Tags = %v, want empty", q.Tags)
	}

	q, _ = Parse("tags:a,-b,!c")
	if len(q.Tags) != 1 || q.Tags[0] != "a" {
		t.Fatalf("Tags = %v", q.Tags)
	}
	if len(q.NotTags) != 2 || q.NotTags[0] != "b" || q.NotTags[1] != "c" {
		t.Fatalf("NotTags = %v, want [b c]", q.NotTags)
	}

	q, _ = Parse("tag:foo tag:-bar")
	if len(q.Tags) != 1 || len(q.NotTags) != 1 {
		t.Fatalf("Tags=%v NotTags=%v", q.Tags, q.NotTags)
	}
}

func TestParseTypeTitleStatusPath(t *testing.T) {
	q, err := Parse("type:Note title:hello status:draft path:sub/dir")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.DocType != "Note" {
		t.Fatalf("DocType = %q", q.DocType)
	}
	if q.Title != "hello" {
		t.Fatalf("Title = %q", q.Title)
	}
	if q.Status != "draft" {
		t.Fatalf("Status = %q", q.Status)
	}
	if q.Path != "sub/dir" {
		t.Fatalf("Path = %q", q.Path)
	}

	// Case-insensitive keys.
	q, _ = Parse("TYPE:Note TITLE:Hello STATUS:Draft PATH:Sub/")
	if q.DocType != "Note" || q.Title != "Hello" || q.Status != "Draft" || q.Path != "Sub/" {
		t.Fatalf("case-insensitive parse failed: %+v", q)
	}
}

func TestParseQuotes(t *testing.T) {
	q, err := Parse(`title:"my title" foo`)
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Title != "my title" {
		t.Fatalf("Title = %q, want %q", q.Title, "my title")
	}
	if len(q.Bare) != 1 || q.Bare[0] != "foo" {
		t.Fatalf("Bare = %v", q.Bare)
	}

	q, _ = Parse(`"foo bar" baz`)
	if len(q.Bare) != 2 || q.Bare[0] != "foo bar" || q.Bare[1] != "baz" {
		t.Fatalf("Bare = %v, want [foo bar baz]", q.Bare)
	}

	q, _ = Parse(`author:"John Doe"`)
	if q.Generic["author"] != "John Doe" {
		t.Fatalf("Generic = %v", q.Generic)
	}
}

func TestParseGeneric(t *testing.T) {
	q, err := Parse("author:John custom:value")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Generic["author"] != "John" {
		t.Fatalf("Generic[author] = %q", q.Generic["author"])
	}
	if q.Generic["custom"] != "value" {
		t.Fatalf("Generic[custom] = %q", q.Generic["custom"])
	}
	// Keys lowercased.
	q, _ = Parse("CUSTOM:Value")
	if q.Generic["custom"] != "Value" {
		t.Fatalf("Generic = %v, want key custom", q.Generic)
	}
	// Value case preserved.
	q, _ = Parse("author:JoHn")
	if q.Generic["author"] != "JoHn" {
		t.Fatalf("Generic[author] = %q, want case preserved", q.Generic["author"])
	}
}

func TestParseDateExact(t *testing.T) {
	q, err := Parse("created:2024-03-15")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created == nil {
		t.Fatal("Created nil")
	}
	if q.Created.From == nil || q.Created.To == nil {
		t.Fatal("From/To nil for exact date")
	}
	if !q.Created.FromIncl || !q.Created.ToIncl {
		t.Fatal("exact date should be inclusive")
	}
	// Whole day.
	wantFrom := time.Date(2024, 3, 15, 0, 0, 0, 0, time.UTC)
	if !q.Created.From.Equal(wantFrom) {
		t.Fatalf("From = %v, want %v", q.Created.From, wantFrom)
	}
	mid := time.Date(2024, 3, 15, 12, 0, 0, 0, time.UTC)
	if mid.Before(*q.Created.From) || mid.After(*q.Created.To) {
		t.Fatalf("mid-day %v not in range %v..%v", mid, q.Created.From, q.Created.To)
	}
}

func TestParseDateRange(t *testing.T) {
	q, err := Parse("created:2024-01-01..2024-02-01")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created == nil || q.Created.From == nil || q.Created.To == nil {
		t.Fatal("range From/To nil")
	}
	wantFrom := time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)
	if !q.Created.From.Equal(wantFrom) {
		t.Fatalf("From = %v", q.Created.From)
	}
	// To should be end of Feb 1.
	if q.Created.To.Day() != 1 || q.Created.To.Month() != 2 {
		t.Fatalf("To = %v", q.Created.To)
	}
}

func TestParseDateComparisons(t *testing.T) {
	q, _ := Parse("updated:>=2024-01-01")
	if q.Updated == nil || q.Updated.From == nil || q.Updated.To != nil || !q.Updated.FromIncl {
		t.Fatalf(">= parse failed: %+v", q.Updated)
	}
	q, _ = Parse("updated:>2024-01-01")
	if q.Updated == nil || q.Updated.From == nil || q.Updated.FromIncl {
		t.Fatalf("> should be exclusive: %+v", q.Updated)
	}
	q, _ = Parse("updated:<=2024-01-01")
	if q.Updated == nil || q.Updated.To == nil || !q.Updated.ToIncl {
		t.Fatalf("<= parse failed: %+v", q.Updated)
	}
	q, _ = Parse("updated:<2024-01-01")
	if q.Updated == nil || q.Updated.To == nil || q.Updated.ToIncl {
		t.Fatalf("< should be exclusive: %+v", q.Updated)
	}
}

func TestParseDateMonthAndRFC3339(t *testing.T) {
	q, err := Parse("created:2024-01")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created.From.Month() != 1 || q.Created.To.Month() != 1 {
		t.Fatalf("month range failed: %v..%v", q.Created.From, q.Created.To)
	}
	// Whole month: Jan 15 inside, Feb 1 outside.
	mid := time.Date(2024, 1, 15, 0, 0, 0, 0, time.UTC)
	if mid.Before(*q.Created.From) || mid.After(*q.Created.To) {
		t.Fatalf("mid-month not in range")
	}

	q, err = Parse("created:2024-03-15T10:00:00Z")
	if err != nil {
		t.Fatalf("RFC3339 parse error: %v", err)
	}
	if q.Created.From == nil || q.Created.To == nil {
		t.Fatal("RFC3339 From/To nil")
	}
	if !q.Created.From.Equal(*q.Created.To) {
		t.Fatalf("RFC3339 exact should have From==To, got %v vs %v", q.Created.From, q.Created.To)
	}
}

func TestParseBeforeAfterDate(t *testing.T) {
	q, err := Parse("before:2024-01-01")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created == nil || q.Created.To == nil {
		t.Fatalf("before should set Created.To, got %+v", q.Created)
	}
	if q.Created.ToIncl {
		t.Fatal("before should be exclusive")
	}

	q, err = Parse("after:2024-06-01")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created == nil || q.Created.From == nil {
		t.Fatalf("after should set Created.From, got %+v", q.Created)
	}
	if q.Created.FromIncl {
		t.Fatal("after should be exclusive")
	}

	q, err = Parse("date:2024-03-15")
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if q.Created == nil {
		t.Fatal("date: should set Created filter (either semantics)")
	}
}

func TestParseBadDate(t *testing.T) {
	if _, err := Parse("created:not-a-date"); err == nil {
		t.Fatal("expected error for bad date")
	}
}

func TestParseMixed(t *testing.T) {
	q, err := Parse(`fuzzy words tag:foo,-bar type:Note created:2024-01-01..2024-12-31 author:John`)
	if err != nil {
		t.Fatalf("Parse error: %v", err)
	}
	if len(q.Bare) != 2 {
		t.Fatalf("Bare = %v", q.Bare)
	}
	if len(q.Tags) != 1 || len(q.NotTags) != 1 {
		t.Fatalf("Tags=%v NotTags=%v", q.Tags, q.NotTags)
	}
	if q.DocType != "Note" {
		t.Fatalf("DocType = %q", q.DocType)
	}
	if q.Created == nil {
		t.Fatal("Created nil")
	}
	if q.Generic["author"] != "John" {
		t.Fatalf("Generic = %v", q.Generic)
	}
}
