package task

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func blockerTasks() []Task {
	return []Task{
		{ID: "BLK-1", Checked: true, Status: 'x', Title: "Ship the schema migration"},
		{
			ID: "BLK-2", Status: ' ', Title: "Backfill the analytics table",
			PropertyOrder: []string{"blocked_by"}, Properties: map[string]string{"blocked_by": "BLK-1"},
		},
		{
			ID: "BLK-3", Status: ' ', Title: "Flip the read path",
			PropertyOrder: []string{"blocked_by"}, Properties: map[string]string{"blocked_by": "BLK-404"},
		},
		{ID: "BLK-4", Status: ' ', Title: "Update the runbook"},
	}
}

func TestBlockers(t *testing.T) {
	tasks := blockerTasks()
	byID := map[string]Task{}
	for _, task := range tasks {
		byID[task.ID] = task
	}

	t.Run("done task never reports blockers", func(t *testing.T) {
		done := Task{
			ID: "DONE-1", Checked: true, Status: 'x',
			PropertyOrder: []string{"blocked_by"}, Properties: map[string]string{"blocked_by": "NOPE-1"},
		}
		all := append(append([]Task(nil), tasks...), done)
		if got := UnresolvedBlockers(done, all); got != nil {
			t.Errorf("UnresolvedBlockers(done) = %v, want nil", got)
		}
		if HasUnresolvedBlockers(done, all) {
			t.Error("HasUnresolvedBlockers(done) = true, want false")
		}
	})

	t.Run("resolved blocker is empty", func(t *testing.T) {
		got := UnresolvedBlockers(byID["BLK-2"], tasks)
		if len(got) != 0 {
			t.Errorf("UnresolvedBlockers(BLK-2) = %v, want empty", got)
		}
	})

	t.Run("missing blocker is unresolved", func(t *testing.T) {
		got := UnresolvedBlockers(byID["BLK-3"], tasks)
		if !reflect.DeepEqual(got, []string{"BLK-404"}) {
			t.Errorf("UnresolvedBlockers(BLK-3) = %v, want [BLK-404]", got)
		}
		if !HasUnresolvedBlockers(byID["BLK-3"], tasks) {
			t.Error("HasUnresolvedBlockers(BLK-3) = false, want true")
		}
	})

	t.Run("flipping the blocker in a file copy re-enters the unresolved set", func(t *testing.T) {
		src := filepath.Join("..", "..", "testdata", "tasks", "spec-blockers.md")
		data, err := os.ReadFile(src)
		if err != nil {
			t.Fatalf("read fixture: %v", err)
		}
		flipped := strings.Replace(string(data), "- [x] BLK-1", "- [ ] BLK-1", 1)
		if flipped == string(data) {
			t.Fatal("fixture copy did not contain a done BLK-1 to flip")
		}
		path := filepath.Join(t.TempDir(), "spec-blockers.md")
		if err := os.WriteFile(path, []byte(flipped), 0o644); err != nil {
			t.Fatalf("write flipped fixture: %v", err)
		}
		_, mutated := parseTaskFile(t, path)
		byID := map[string]Task{}
		for _, task := range mutated {
			byID[task.ID] = task
		}
		got := UnresolvedBlockers(byID["BLK-2"], mutated)
		if !reflect.DeepEqual(got, []string{"BLK-1"}) {
			t.Errorf("after flipping BLK-1 open, UnresolvedBlockers(BLK-2) = %v, want [BLK-1]", got)
		}
	})

	t.Run("display properties order and stripping", func(t *testing.T) {
		task := Task{
			ID: "X-1", Status: ' ',
			PropertyOrder: []string{"owner", "blocked_by", "status"},
			Properties:    map[string]string{"owner": "alice", "blocked_by": "BLK-1", "status": "doing"},
		}
		// BLK-1 is done, so blocked_by is dropped.
		keys, values := DisplayProperties(task, tasks)
		if !reflect.DeepEqual(keys, []string{"owner", "status"}) {
			t.Errorf("keys = %v, want [owner status]", keys)
		}
		if !reflect.DeepEqual(values, map[string]string{"owner": "alice", "status": "doing"}) {
			t.Errorf("values = %v", values)
		}

		// Partially unresolved keeps blocked_by last with its original value.
		task.Properties["blocked_by"] = "BLK-1,BLK-404"
		keys, values = DisplayProperties(task, tasks)
		if !reflect.DeepEqual(keys, []string{"owner", "status", "blocked_by"}) {
			t.Errorf("keys with unresolved = %v, want [owner status blocked_by]", keys)
		}
		if values["blocked_by"] != "BLK-1,BLK-404" {
			t.Errorf("blocked_by value = %q", values["blocked_by"])
		}
	})

	t.Run("properties still hold blocked_by", func(t *testing.T) {
		if byID["BLK-2"].Properties["blocked_by"] != "BLK-1" {
			t.Error("Properties must retain blocked_by even when display strips it")
		}
	})
}
