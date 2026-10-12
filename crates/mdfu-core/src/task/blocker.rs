//! Task blocker resolution (SPEC.md §B).
//!
//! Port of `internal/task/blocker.go`. A blocker is *resolved* when the named
//! ID belongs to a done task in the same scope; a blocker naming an ID that no
//! task carries is *unresolved*. A done task never reports unresolved blockers,
//! even when it carries a `@blocked_by` value.
//!
//! The Go functions return a `map[string]bool` plus a `[]string`/keys+values
//! pair; the Rust ports return borrowed sets and owned `Vec`s with identical
//! iteration order (first-seen `property_order` for
//! [`display_properties`]).

use std::collections::HashSet;

use super::Task;

/// Returns the identifiers belonging to done tasks.
#[must_use]
pub fn done_ids(tasks: &[Task]) -> HashSet<&str> {
    tasks
        .iter()
        .filter(|task| task.checked && !task.id.is_empty())
        .map(|task| task.id.as_str())
        .collect()
}

/// Returns the identifiers named by `task`'s `blocked_by` property that do not
/// belong to a done task.
///
/// The value is split on commas; surrounding whitespace is trimmed and empty
/// parts are skipped. A done task always yields an empty vector.
#[must_use]
pub fn unresolved_blockers(task: &Task, tasks: &[Task]) -> Vec<String> {
    if task.checked {
        return Vec::new();
    }
    let Some(value) = task.properties.get("blocked_by") else {
        return Vec::new();
    };
    let done = done_ids(tasks);
    let mut unresolved = Vec::new();
    for part in value.split(',') {
        let id = part.trim();
        if id.is_empty() {
            continue;
        }
        if !done.contains(id) {
            unresolved.push(id.to_string());
        }
    }
    unresolved
}

/// Reports whether `task` is an open task with at least one unresolved blocker.
#[must_use]
pub fn has_unresolved_blockers(task: &Task, tasks: &[Task]) -> bool {
    !unresolved_blockers(task, tasks).is_empty()
}

/// Returns the properties to render for `task` as ordered `(key, value)` pairs:
/// every non-`blocked_by` property in original first-seen order, then
/// `blocked_by` last.
///
/// A fully resolved `blocked_by` is dropped; an unresolved one is kept with its
/// original value. Keys missing from the property map are skipped. The done
/// task set is resolved against `tasks` (the full scope), so a done blocker
/// hidden from the visible rows still resolves.
#[must_use]
pub fn display_properties(task: &Task, tasks: &[Task]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in &task.property_order {
        if key == "blocked_by" {
            continue;
        }
        if let Some(value) = task.properties.get(key) {
            out.push((key.clone(), value.clone()));
        }
    }
    if let Some(value) = task.properties.get("blocked_by") {
        if !unresolved_blockers(task, tasks).is_empty() {
            out.push(("blocked_by".to_string(), value.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::task::grammar::parse_header;

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn blocker_tasks() -> Vec<Task> {
        vec![
            Task {
                id: "BLK-1".to_string(),
                checked: true,
                status: b'x',
                title: "Ship the schema migration".to_string(),
                ..Task::default()
            },
            Task {
                id: "BLK-2".to_string(),
                status: b' ',
                title: "Backfill the analytics table".to_string(),
                property_order: vec!["blocked_by".to_string()],
                properties: props(&[("blocked_by", "BLK-1")]),
                ..Task::default()
            },
            Task {
                id: "BLK-3".to_string(),
                status: b' ',
                title: "Flip the read path".to_string(),
                property_order: vec!["blocked_by".to_string()],
                properties: props(&[("blocked_by", "BLK-404")]),
                ..Task::default()
            },
            Task {
                id: "BLK-4".to_string(),
                status: b' ',
                title: "Update the runbook".to_string(),
                ..Task::default()
            },
        ]
    }

    /// Parses every fence-unmasked strict header of `content` into a task,
    /// filling `file`/`line` so fixture-based blocker checks can run without
    /// the command layer.
    fn parse_tasks(file: &str, content: &str) -> Vec<Task> {
        use crate::task::body;

        let lines: Vec<String> = content.split('\n').map(str::to_string).collect();
        let mask = body::fence_mask(&lines);
        let mut tasks = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if mask[i] {
                continue;
            }
            let Some(mut task) = parse_header(line) else {
                continue;
            };
            task.file = file.to_string();
            task.line = i + 1;
            tasks.push(task);
        }
        tasks
    }

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/tasks"))
            .join(name)
    }

    fn by_id<'a>(tasks: &'a [Task], id: &str) -> &'a Task {
        tasks
            .iter()
            .find(|task| task.id == id)
            .unwrap_or_else(|| panic!("task {id} not found"))
    }

    #[test]
    fn done_task_never_reports_blockers() {
        let tasks = blocker_tasks();
        let done = Task {
            id: "DONE-1".to_string(),
            checked: true,
            status: b'x',
            property_order: vec!["blocked_by".to_string()],
            properties: props(&[("blocked_by", "NOPE-1")]),
            ..Task::default()
        };
        let mut all = tasks.clone();
        all.push(done.clone());

        assert!(unresolved_blockers(&done, &all).is_empty());
        assert!(!has_unresolved_blockers(&done, &all));
    }

    #[test]
    fn resolved_blocker_is_empty() {
        let tasks = blocker_tasks();
        assert!(unresolved_blockers(by_id(&tasks, "BLK-2"), &tasks).is_empty());
    }

    #[test]
    fn missing_blocker_is_unresolved() {
        let tasks = blocker_tasks();
        assert_eq!(
            unresolved_blockers(by_id(&tasks, "BLK-3"), &tasks),
            vec!["BLK-404".to_string()]
        );
        assert!(has_unresolved_blockers(by_id(&tasks, "BLK-3"), &tasks));
    }

    #[test]
    fn flipping_the_blocker_in_a_file_copy_re_enters_the_unresolved_set() {
        let path = fixture_path("spec-blockers.md");
        let data = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let flipped = data.replacen("- [x] BLK-1", "- [ ] BLK-1", 1);
        assert_ne!(flipped, data, "fixture copy must contain a done BLK-1");

        let tasks = parse_tasks("spec-blockers.md", &flipped);
        assert_eq!(
            unresolved_blockers(by_id(&tasks, "BLK-2"), &tasks),
            vec!["BLK-1".to_string()]
        );
    }

    #[test]
    fn display_properties_order_and_stripping() {
        let tasks = blocker_tasks();
        let mut task = Task {
            id: "X-1".to_string(),
            status: b' ',
            property_order: vec![
                "owner".to_string(),
                "blocked_by".to_string(),
                "status".to_string(),
            ],
            properties: props(&[
                ("owner", "alice"),
                ("blocked_by", "BLK-1"),
                ("status", "doing"),
            ]),
            ..Task::default()
        };

        // BLK-1 is done, so blocked_by is dropped.
        assert_eq!(
            display_properties(&task, &tasks),
            vec![
                ("owner".to_string(), "alice".to_string()),
                ("status".to_string(), "doing".to_string()),
            ]
        );

        // Partially unresolved keeps blocked_by last with its original value.
        task.properties
            .insert("blocked_by".to_string(), "BLK-1,BLK-404".to_string());
        assert_eq!(
            display_properties(&task, &tasks),
            vec![
                ("owner".to_string(), "alice".to_string()),
                ("status".to_string(), "doing".to_string()),
                ("blocked_by".to_string(), "BLK-1,BLK-404".to_string()),
            ]
        );
    }

    #[test]
    fn properties_still_hold_blocked_by() {
        let tasks = blocker_tasks();
        assert_eq!(
            by_id(&tasks, "BLK-2").properties.get("blocked_by"),
            Some(&"BLK-1".to_string())
        );
    }

    #[test]
    fn done_ids_skips_empty_ids_and_open_tasks() {
        let mut tasks = blocker_tasks();
        tasks.push(Task {
            id: String::new(),
            checked: true,
            ..Task::default()
        });
        let ids = done_ids(&tasks);
        assert_eq!(ids.len(), 1);
        assert!(ids.contains("BLK-1"));
    }
}
