package task

import (
	"reflect"
	"strings"
	"testing"
)

func TestResolve(t *testing.T) {
	tasks := []Task{
		{ID: "EXMPL-1", Title: "one"},
		{ID: "EXMPL-2", Title: "two"},
		{ID: "A-1", Title: "a-one"},
	}

	t.Run("full id", func(t *testing.T) {
		got, err := Resolve("EXMPL-2", tasks)
		if err != nil {
			t.Fatalf("Resolve error = %v", err)
		}
		if got.Title != "two" {
			t.Errorf("Resolve title = %q, want two", got.Title)
		}
	})

	t.Run("bare number", func(t *testing.T) {
		got, err := Resolve("2", tasks)
		if err != nil {
			t.Fatalf("Resolve error = %v", err)
		}
		if got.ID != "EXMPL-2" {
			t.Errorf("Resolve ID = %q, want EXMPL-2", got.ID)
		}
	})

	errors := []struct {
		name  string
		input string
		tasks []Task
		want  string
	}{
		{"invalid", "not an id", tasks, "invalid task ID 'not an id'"},
		{"lowercase invalid", "exmpl-1", tasks, "invalid task ID 'exmpl-1'"},
		{"not found full id", "NOPE-1", tasks, "task 'NOPE-1' not found"},
		{"not found bare", "9", tasks, "task '9' not found"},
		{
			name:  "ambiguous numeric",
			input: "1",
			tasks: []Task{{ID: "B-1"}, {ID: "A-1"}},
			want:  "ambiguous numeric ID '1' matches: A-1, B-1",
		},
		{
			name:  "duplicate full id",
			input: "A-1",
			tasks: []Task{{ID: "A-1"}, {ID: "A-1"}},
			want:  "task 'A-1' appears multiple times; expected exactly one match",
		},
	}
	for _, tt := range errors {
		t.Run(tt.name, func(t *testing.T) {
			_, err := Resolve(tt.input, tt.tasks)
			if err == nil {
				t.Fatalf("Resolve(%q) error = nil, want %q", tt.input, tt.want)
			}
			if err.Error() != tt.want {
				t.Errorf("Resolve(%q) error = %q, want %q", tt.input, err.Error(), tt.want)
			}
		})
	}
}

func TestGlobalMax(t *testing.T) {
	tests := []struct {
		name  string
		tasks []Task
		want  int
	}{
		{"none", nil, 0},
		{"unidentified ignored", []Task{{Title: "no id"}}, 0},
		{"leading zeros numeric", []Task{{ID: "EXMPL-007"}, {ID: "OTHER-3"}}, 7},
		{"max wins", []Task{{ID: "EXMPL-2"}, {ID: "EXMPL-40"}}, 40},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := GlobalMax(tt.tasks); got != tt.want {
				t.Errorf("GlobalMax = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestPadWidth(t *testing.T) {
	tests := []struct {
		in   int
		want int
	}{
		{0, 3},
		{7, 3},
		{42, 3},
		{999, 3},
		{1000, 4},
		{12345, 5},
		{-5, 3},
	}
	for _, tt := range tests {
		if got := PadWidth(tt.in); got != tt.want {
			t.Errorf("PadWidth(%d) = %d, want %d", tt.in, got, tt.want)
		}
	}
}

func TestPrefixResolution(t *testing.T) {
	t.Run("valid prefix grammar", func(t *testing.T) {
		for _, s := range []string{"A", "EXMPL", "A1", "ABC123"} {
			if !ValidPrefix(s) {
				t.Errorf("ValidPrefix(%q) = false, want true", s)
			}
		}
		for _, s := range []string{"", "a", "1A", "A-B", "A B", "Ä"} {
			if ValidPrefix(s) {
				t.Errorf("ValidPrefix(%q) = true, want false", s)
			}
		}
	})

	t.Run("valid task id grammar", func(t *testing.T) {
		for _, s := range []string{"A-1", "EXMPL-42", "A1-007"} {
			if !ValidTaskID(s) {
				t.Errorf("ValidTaskID(%q) = false, want true", s)
			}
		}
		for _, s := range []string{"A", "A-", "-1", "a-1", "A-1-2", "A-x"} {
			if ValidTaskID(s) {
				t.Errorf("ValidTaskID(%q) = true, want false", s)
			}
		}
	})

	t.Run("numeric part", func(t *testing.T) {
		if n, ok := NumericPart("EXMPL-007"); !ok || n != 7 {
			t.Errorf("NumericPart(EXMPL-007) = (%d, %v), want (7, true)", n, ok)
		}
		if _, ok := NumericPart("not-an-id"); ok {
			t.Error("NumericPart(not-an-id) ok = true, want false")
		}
	})

	t.Run("most frequent prefix", func(t *testing.T) {
		tasks := []Task{{ID: "PRJ-1"}, {ID: "OTHER-2"}, {ID: "PRJ-3"}, {ID: "SEED"}}
		if got := MostFrequentPrefix(tasks); got != "PRJ" {
			t.Errorf("MostFrequentPrefix = %q, want PRJ", got)
		}
		if got := MostFrequentPrefix(nil); got != "" {
			t.Errorf("MostFrequentPrefix(nil) = %q, want empty", got)
		}
		// Tie broken by first appearance.
		tie := []Task{{ID: "BBB-1"}, {ID: "AAA-2"}}
		if got := MostFrequentPrefix(tie); got != "BBB" {
			t.Errorf("MostFrequentPrefix tie = %q, want BBB", got)
		}
	})

	t.Run("seed prefix", func(t *testing.T) {
		got, ok := SeedPrefix("- [ ] PRJ- Task title")
		if !ok || got != "PRJ" {
			t.Errorf("SeedPrefix = (%q, %v), want (PRJ, true)", got, ok)
		}
		if _, ok := SeedPrefix("- [ ] PRJ-001 Real task"); ok {
			t.Error("SeedPrefix identified line ok = true, want false")
		}
		if _, ok := SeedPrefix("- [ ] No ID here"); ok {
			t.Error("SeedPrefix unidentified line ok = true, want false")
		}
	})

	t.Run("validate prefix", func(t *testing.T) {
		if err := ValidatePrefix("PRJ"); err != nil {
			t.Errorf("ValidatePrefix(PRJ) = %v, want nil", err)
		}
		err := ValidatePrefix("bad")
		if err == nil || err.Error() != "invalid prefix 'bad'" {
			t.Errorf("ValidatePrefix(bad) = %v, want invalid prefix 'bad'", err)
		}
	})
}

func TestPlanIsAllOrNothing(t *testing.T) {
	t.Run("resolves sequential ids", func(t *testing.T) {
		assigns := []Assignment{
			{File: "a.md", Line: 3, RawLine: "- [ ] First task", FilePrefixes: []string{"PRJ"}},
			{File: "a.md", Line: 5, RawLine: "- [ ] SEED- Second task", Seed: "SEED"},
			{File: "b.md", Line: 2, RawLine: "- [ ] Third task", Prefix: "NEW"},
		}
		writes, err := Plan(assigns, 7)
		if err != nil {
			t.Fatalf("Plan error = %v", err)
		}
		want := []PlannedWrite{
			{File: "a.md", Line: 3, Header: "- [ ] PRJ-008 First task"},
			{File: "a.md", Line: 5, Header: "- [ ] SEED-009 Second task"},
			{File: "b.md", Line: 2, Header: "- [ ] NEW-010 Third task"},
		}
		if !reflect.DeepEqual(writes, want) {
			t.Errorf("Plan writes = %+v, want %+v", writes, want)
		}
	})

	t.Run("zero writes when a prefix is missing", func(t *testing.T) {
		assigns := []Assignment{
			{File: "a.md", Line: 3, RawLine: "- [ ] First task", FilePrefixes: []string{"PRJ"}},
			{File: "b.md", Line: 2, RawLine: "- [ ] No source here"},
		}
		writes, err := Plan(assigns, 0)
		if err == nil {
			t.Fatal("Plan error = nil, want error")
		}
		if len(writes) != 0 {
			t.Errorf("Plan returned %d writes, want zero", len(writes))
		}
		want := "b.md:2: no prefix found for task \"- [ ] No source here\" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ"
		if err.Error() != want {
			t.Errorf("Plan error = %q, want %q", err.Error(), want)
		}
	})

	t.Run("invalid explicit prefix", func(t *testing.T) {
		assigns := []Assignment{{File: "a.md", Line: 1, RawLine: "- [ ] Task", Prefix: "bad"}}
		if _, err := Plan(assigns, 0); err == nil || err.Error() != "invalid prefix 'bad'" {
			t.Errorf("Plan error = %v, want invalid prefix 'bad'", err)
		}
	})

	t.Run("padding follows global max", func(t *testing.T) {
		assigns := []Assignment{{File: "a.md", Line: 1, RawLine: "- [ ] Task", Prefix: "PRJ"}}
		writes, err := Plan(assigns, 999)
		if err != nil {
			t.Fatalf("Plan error = %v", err)
		}
		if got := writes[0].Header; got != "- [ ] PRJ-1000 Task" {
			t.Errorf("header = %q, want - [ ] PRJ-1000 Task", got)
		}
	})
}

func TestDuplicateNumericParts(t *testing.T) {
	tests := []struct {
		name  string
		tasks []Task
		want  map[int][]string
	}{
		{"distinct prefixes", []Task{{ID: "A-1"}, {ID: "B-1"}}, map[int][]string{1: {"A-1", "B-1"}}},
		{"duplicate task", []Task{{ID: "A-1"}, {ID: "A-1"}}, map[int][]string{1: {"A-1", "A-1"}}},
		{"unique", []Task{{ID: "A-1"}, {ID: "B-2"}}, map[int][]string{}},
		{"ignores unidentified", []Task{{Title: "none"}, {ID: "A-1"}}, map[int][]string{}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := DuplicateNumericParts(tt.tasks)
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("DuplicateNumericParts = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestResolveScopeErrorsAreBare(t *testing.T) {
	_, err := Resolve("bogus", nil)
	if err == nil {
		t.Fatal("expected error")
	}
	if strings.Contains(err.Error(), "mdfu task:") {
		t.Errorf("engine error must be bare, got %q", err.Error())
	}
}
