//! `list` and `view` rendering for the task engine (SPEC.md §F, §G, §H).
//!
//! Ports `internal/task/render.go` plus the two render helpers of
//! `internal/task/view.go`. JSON and the non-TTY line format are byte-exact;
//! the TTY box table is returned as structured row/cell data for a ratatui
//! consumer instead of lipgloss box bytes (per the port plan), so exact
//! box/SGR bytes are intentionally not reproduced. Column text, ordering, and
//! semantic colors match Go.
//!
//! Go `encoding/json` runs with `SetEscapeHTML(false)` for both `list` and
//! `view`, so `<`, `>`, and `&` stay literal (SPEC §F.1); only Go's
//! unconditional U+2028/U+2029 escaping is applied by `escape_go_json`.
//! `output::format_json` is the filter-mode counterpart that *does* escape
//! HTML, which is why it is not reused here.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::blocker::{display_properties, has_unresolved_blockers};
use super::{Task, Unidentified};

/// Terminal capability flags. Deliberately has no width: the Go table sizes its
/// columns to content and never reads the terminal width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Terminal {
    /// Whether stdout is a terminal.
    pub is_tty: bool,
    /// Whether color output is disabled.
    pub no_color: bool,
}

/// Controls [`render_list`].
///
/// `hidden_blocked` holds the tasks hidden by the blocker gate rather than a
/// count because SPEC §H note 1 prints one detail line per hidden task,
/// reading each task's `file`, `line`, and `header_raw`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListOptions {
    /// Include done tasks.
    pub all: bool,
    /// Include blocked tasks instead of hiding them.
    pub blocked: bool,
    /// `"priority"` sorts crit/high/medium/low; any other value is file order.
    pub sort: String,
    /// Required tags (AND); a leading `#` is ignored.
    pub tag_filter: Vec<String>,
    /// Accepted priorities (OR); a leading `!` is ignored.
    pub priority_filter: Vec<String>,
    /// Emit the JSON array and suppress the notes.
    pub json: bool,
    /// Terminal capability flags.
    pub terminal: Terminal,
    /// Open tasks hidden by the blocker gate, for note 1.
    pub hidden_blocked: Vec<Task>,
    /// Seed/unidentified lines, for note 2.
    pub unidentified: Vec<Unidentified>,
}

/// One rendered `list` invocation.
///
/// `notes` is empty in JSON mode; otherwise the caller writes `body` first and
/// then `notes` (both to stdout), mirroring Go `RenderList`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRender {
    /// The selected mode's payload.
    pub body: ListBody,
    /// The two trailing notes (SPEC §H) or empty in JSON mode.
    pub notes: String,
}

/// The rendered `list` payload for the three mutually exclusive modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListBody {
    /// JSON array (SPEC §F.1).
    Json(String),
    /// Non-TTY line format (SPEC §F.2).
    Lines(String),
    /// Structured TTY table (SPEC §F.3) for a ratatui consumer.
    Table(TableData),
}

/// Structured TTY table: headers plus per-cell text and color role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableData {
    /// Column headers in order: `ID`, `TITLE`, `TAGS`, `PRI`, `PROPS`.
    pub headers: Vec<&'static str>,
    /// One row per visible task, in final (possibly sorted) order.
    pub rows: Vec<Vec<TableCell>>,
    /// True when ANSI color would be enabled (`is_tty && !no_color`).
    pub color: bool,
}

/// One TTY table cell: display text plus its semantic color role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    /// Cell text, never containing ANSI.
    pub text: String,
    /// Semantic color role; the consumer maps it to ratatui/SGR.
    pub color: CellColor,
}

/// Semantic color roles from SPEC §G; the consumer maps them to ratatui styles
/// or SGR when [`TableData::color`] is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellColor {
    /// Default foreground.
    Default,
    /// Open task ID: SGR `38;5;245`.
    Id,
    /// Done task ID and title: SGR `38;5;240`.
    Done,
    /// Tag column: SGR `38;5;62`.
    Tag,
    /// Priority column: SGR `38;5;212`.
    Priority,
    /// PROPS column carrying an unresolved blocker: SGR `38;5;203`.
    Blocked,
}

/// Renders visible tasks in one of the three mutually exclusive modes from
/// SPEC §F: JSON, the non-TTY line format, or the structured TTY table.
///
/// The two §H notes are returned in [`ListRender::notes`] (empty in JSON mode).
/// Zero visible rows produce an empty [`ListBody::Lines`] or a table with zero
/// rows, though the notes still apply. `tasks` is the full scope, so blockers
/// resolve even when a done blocker is not itself visible.
///
/// # Errors
///
/// Returns the [`serde_json::Error`] from JSON mode; serializing the plain
/// string/number fields of `JsonTask` cannot realistically fail.
#[must_use = "the rendered list must be inspected"]
pub fn render_list(tasks: &[Task], opts: &ListOptions) -> Result<ListRender, serde_json::Error> {
    let mut visible = select_visible(tasks, opts);
    if opts.sort == "priority" {
        sort_by_priority(&mut visible);
    }

    if opts.json {
        return Ok(ListRender {
            body: ListBody::Json(render_json(tasks, &visible)?),
            notes: String::new(),
        });
    }

    let body = if opts.terminal.is_tty {
        ListBody::Table(table_data(tasks, &visible, !opts.terminal.no_color))
    } else {
        ListBody::Lines(render_lines(tasks, &visible))
    };
    Ok(ListRender {
        body,
        notes: render_notes(opts),
    })
}

/// Applies the default selection (open, unblocked) plus the tag and priority
/// filters, cloning the surviving tasks in input order.
#[must_use]
pub fn select_visible(tasks: &[Task], opts: &ListOptions) -> Vec<Task> {
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        if !opts.all && task.checked {
            continue;
        }
        if !opts.blocked && has_unresolved_blockers(task, tasks) {
            continue;
        }
        if !matches_tags(task, &opts.tag_filter) {
            continue;
        }
        if !matches_priority(task, &opts.priority_filter) {
            continue;
        }
        out.push(task.clone());
    }
    out
}

/// Reports whether `task` carries every requested tag (AND). A leading `#` on a
/// filter is ignored, exactly one occurrence like Go `strings.TrimPrefix`.
fn matches_tags(task: &Task, filters: &[String]) -> bool {
    for filter in filters {
        let filter = filter.strip_prefix('#').unwrap_or(filter);
        if !task.tags.iter().any(|tag| tag == filter) {
            return false;
        }
    }
    true
}

/// Reports whether `task`'s effective priority is any of the requested
/// priorities (OR). A leading `!` on a filter is ignored.
fn matches_priority(task: &Task, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let priority = effective_priority(&task.priority);
    filters.iter().any(|filter| {
        let filter = filter.strip_prefix('!').unwrap_or(filter);
        effective_priority(filter) == priority
    })
}

/// Maps an absent or unknown priority to `medium` and keeps the three explicit
/// ranks.
#[must_use]
pub fn effective_priority(priority: &str) -> &str {
    match priority {
        "crit" | "high" | "low" => priority,
        _ => "medium",
    }
}

/// Stably orders `crit`, `high`, `medium`, `low`; unknown values sort as
/// `medium`.
pub fn sort_by_priority(tasks: &mut [Task]) {
    tasks.sort_by_key(|task| priority_rank(effective_priority(&task.priority)));
}

/// Rank of an effective priority; only `medium` is reachable as the fallback.
fn priority_rank(priority: &str) -> u8 {
    match priority {
        "crit" => 0,
        "high" => 1,
        "low" => 3,
        _ => 2,
    }
}

/// The SPEC §F.1 JSON object. Field order is the required key order.
#[derive(Serialize)]
struct JsonTask {
    id: String,
    title: String,
    status: &'static str,
    priority: String,
    tags: Vec<String>,
    properties: BTreeMap<String, String>,
    file: String,
    line: usize,
}

/// The SPEC §F.4 `view --json` object: the eight §F.1 keys in order plus
/// `body` last. Declared flat (not `#[serde(flatten)]`) because a flatten map
/// would drop the required key order.
#[derive(Serialize)]
struct ViewJson {
    id: String,
    title: String,
    status: &'static str,
    priority: String,
    tags: Vec<String>,
    properties: BTreeMap<String, String>,
    file: String,
    line: usize,
    body: String,
}

/// Writes the indented JSON array of SPEC §F.1: two-space indent, one trailing
/// newline, Go-compatible escaping. Resolved blockers are stripped through
/// [`display_properties`].
///
/// # Errors
///
/// Returns the [`serde_json::Error`] if serialization fails.
#[must_use = "the serialized JSON must be inspected"]
pub fn render_json(all: &[Task], visible: &[Task]) -> Result<String, serde_json::Error> {
    let out: Vec<JsonTask> = visible
        .iter()
        .map(|task| {
            let properties = display_properties(task, all).into_iter().collect();
            json_task(task, properties)
        })
        .collect();
    encode_pretty(&out)
}

/// Builds the shared eight-key object. `status` and `priority` are derived;
/// tags and properties are always present (`[]` / `{}` when empty).
fn json_task(task: &Task, properties: BTreeMap<String, String>) -> JsonTask {
    JsonTask {
        id: task.id.clone(),
        title: task.title.clone(),
        status: if task.checked { "done" } else { "open" },
        priority: effective_priority(&task.priority).to_string(),
        tags: task.tags.clone(),
        properties,
        file: task.file.clone(),
        line: task.line,
    }
}

/// Serializes with `serde_json`'s two-space pretty printer, appends Go's
/// encoder newline, and applies `escape_go_json`.
fn encode_pretty<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(value)?;
    json.push('\n');
    Ok(escape_go_json(&json))
}

/// Applies the escaping Go `encoding/json` still performs with
/// `SetEscapeHTML(false)`: U+2028 and U+2029 are always emitted as `\u2028`
/// and `\u2029`. `<`, `>`, and `&` stay literal (SPEC §F.1) so
/// `output::escape_go_html` must not be used here.
fn escape_go_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        match ch {
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

/// Writes the non-TTY line format of SPEC §F.2: fields joined by exactly two
/// spaces, omitted entirely when empty, zero ANSI bytes.
#[must_use]
pub fn render_lines(all: &[Task], visible: &[Task]) -> String {
    let mut out = String::new();
    for task in visible {
        out.push_str(if task.checked { "[x]" } else { "[ ]" });
        out.push(' ');
        out.push_str(&task.id);
        out.push_str("  ");
        out.push_str(&task.title);

        if !task.tags.is_empty() {
            out.push_str("  ");
            for (i, tag) in task.tags.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push('#');
                out.push_str(tag);
            }
        }
        if !task.priority.is_empty() {
            out.push_str("  !");
            out.push_str(&task.priority);
        }
        for (key, value) in display_properties(task, all) {
            out.push_str("  @");
            out.push_str(&key);
            out.push(':');
            out.push_str(&value);
        }
        out.push('\n');
    }
    out
}

/// Column headers of the TTY table in SPEC §F.3 order.
const TABLE_HEADERS: [&str; 5] = ["ID", "TITLE", "TAGS", "PRI", "PROPS"];

/// Builds the structured TTY table: one row per visible task with semantic
/// color roles. `color` is the §G enable flag (`is_tty && !no_color`); the
/// consumer decides the actual styling.
#[must_use]
pub fn table_data(all: &[Task], visible: &[Task], color: bool) -> TableData {
    TableData {
        headers: TABLE_HEADERS.to_vec(),
        rows: visible.iter().map(|task| table_row(task, all)).collect(),
        color,
    }
}

/// Renders one task's raw (unstyled) cells plus their semantic colors.
fn table_row(task: &Task, all: &[Task]) -> Vec<TableCell> {
    let mut tags = String::new();
    for (i, tag) in task.tags.iter().enumerate() {
        if i > 0 {
            tags.push(' ');
        }
        tags.push('#');
        tags.push_str(tag);
    }

    let priority = if task.priority.is_empty() {
        String::new()
    } else {
        format!("!{}", task.priority)
    };

    let mut props = String::new();
    for (i, (key, value)) in display_properties(task, all).iter().enumerate() {
        if i > 0 {
            props.push(' ');
        }
        props.push('@');
        props.push_str(key);
        props.push(':');
        props.push_str(value);
    }

    let id_color = if task.checked {
        CellColor::Done
    } else {
        CellColor::Id
    };
    let title_color = if task.checked {
        CellColor::Done
    } else {
        CellColor::Default
    };
    let tags_color = if tags.is_empty() {
        CellColor::Default
    } else {
        CellColor::Tag
    };
    let priority_color = if priority.is_empty() {
        CellColor::Default
    } else {
        CellColor::Priority
    };
    let props_color = if props.contains("@blocked_by:") {
        CellColor::Blocked
    } else {
        CellColor::Default
    };

    vec![
        TableCell {
            text: truncate_cells(&task.id, 120),
            color: id_color,
        },
        TableCell {
            text: truncate_cells(&task.title, 120),
            color: title_color,
        },
        TableCell {
            text: tags,
            color: tags_color,
        },
        TableCell {
            text: priority,
            color: priority_color,
        },
        TableCell {
            text: props,
            color: props_color,
        },
    ]
}

/// Truncates `s` to at most `max` display cells: the first `max - 1` characters
/// plus `…` (U+2026).
///
/// Width is approximated by Unicode scalar count; SPEC §F.3's reference
/// truncation is 120 cells and the port plan explicitly does not require
/// lipgloss-width-exact bytes.
fn truncate_cells(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut out: String = chars[..max.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}

/// Writes the two SPEC §H notes to stdout, unprefixed, each paragraph ending
/// with a newline. Note 1 aligns every `<file>:<line>` location after the
/// widest raw header plus two spaces.
#[must_use]
pub fn render_notes(opts: &ListOptions) -> String {
    let mut out = String::new();
    if !opts.hidden_blocked.is_empty() {
        let max_raw = opts
            .hidden_blocked
            .iter()
            .map(|task| display_width(&task.header_raw))
            .max()
            .unwrap_or(0);
        let _ = writeln!(
            out,
            "Warning: {} blocked task(s) hidden (use `mdfu task list --blocked` to show them):",
            opts.hidden_blocked.len()
        );
        for task in &opts.hidden_blocked {
            let pad = max_raw.saturating_sub(display_width(&task.header_raw)) + 2;
            let _ = writeln!(
                out,
                "{}{}{}:{}",
                task.header_raw,
                " ".repeat(pad),
                task.file,
                task.line
            );
        }
    }
    if !opts.unidentified.is_empty() {
        let _ = writeln!(
            out,
            "{} task(s) have no ID (run `mdfu task ids` to assign one).",
            opts.unidentified.len()
        );
    }
    out
}

/// Approximates `lipgloss.Width` by counting Unicode scalar values. Exact
/// display width is deliberately out of scope for this port; ASCII fixtures,
/// where one scalar is one cell, align byte-identically.
fn display_width(s: &str) -> usize {
    s.chars().count()
}

/// Writes the `view` human output of SPEC §F.4: the TTY location line (styled
/// `38;5;245`, or plain with `no_color`), the verbatim original header line
/// (resolved `@blocked_by` tokens stay visible), and the body indented by
/// exactly six spaces with blank body lines emitted as empty lines.
#[must_use]
pub fn render_view_human(task: &Task, terminal: Terminal) -> String {
    let mut out = String::new();
    if terminal.is_tty {
        let location = format!("{}:{}", task.file, task.line);
        if terminal.no_color {
            out.push_str(&location);
        } else {
            out.push_str(&sgr("38;5;245", &location));
        }
        out.push('\n');
    }
    out.push_str(&task.header_raw);
    out.push('\n');
    if !task.body.is_empty() {
        for line in task.body.split('\n') {
            if line.is_empty() {
                out.push('\n');
                continue;
            }
            out.push_str("      ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Wraps `text` in one SGR span: `ESC[<params>m` … `ESC[0m`.
fn sgr(params: &str, text: &str) -> String {
    format!("\u{1b}[{params}m{text}\u{1b}[0m")
}

/// Writes the single `view --json` object of SPEC §F.4: the eight §F.1 keys in
/// order plus `body` last. Unlike `list`, view keeps every property (a resolved
/// blocker stays in `properties` because the verbatim header already shows it).
///
/// # Errors
///
/// Returns the [`serde_json::Error`] if serialization fails.
#[must_use = "the serialized JSON must be inspected"]
pub fn render_view_json(task: &Task) -> Result<String, serde_json::Error> {
    encode_pretty(&ViewJson {
        id: task.id.clone(),
        title: task.title.clone(),
        status: if task.checked { "done" } else { "open" },
        priority: effective_priority(&task.priority).to_string(),
        tags: task.tags.clone(),
        properties: task.properties.clone(),
        file: task.file.clone(),
        line: task.line,
        body: task.body.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::task::{body, grammar};

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata/tasks");

    const WANT_LINES: &str = "[ ] EXMPL-001  Fix the thing  #launch  !high  @status:doing\n\
                              [x] EXMPL-002  Ship it  #launch\n";

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn spec_tasks() -> Vec<Task> {
        vec![
            Task {
                id: "EXMPL-001".to_string(),
                status: b' ',
                title: "Fix the thing".to_string(),
                tags: vec!["launch".to_string()],
                priority: "high".to_string(),
                property_order: vec!["status".to_string()],
                properties: props(&[("status", "doing")]),
                file: "tasks.md".to_string(),
                line: 1,
                ..Task::default()
            },
            Task {
                id: "EXMPL-002".to_string(),
                status: b'x',
                checked: true,
                title: "Ship it".to_string(),
                tags: vec!["launch".to_string()],
                file: "tasks.md".to_string(),
                line: 2,
                ..Task::default()
            },
        ]
    }

    /// Parses every fence-unmasked strict header of `content`, filling body and
    /// `file`/`line` like `LoadAll` so rendering can be exercised without the
    /// command layer.
    fn parse_tasks(file: &str, content: &str) -> Vec<Task> {
        let lines: Vec<String> = content.split('\n').map(str::to_string).collect();
        let mask = body::fence_mask(&lines);
        let mut tasks = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if mask[i] {
                continue;
            }
            let Some(mut task) = grammar::parse_header(line) else {
                continue;
            };
            if task.id.is_empty() {
                continue;
            }
            if let Some(end) = body::block_range(&lines, i) {
                let (text, indent) = body::collect_body(&lines, i, end);
                task.body = text;
                task.body_indent = indent;
            }
            task.file = file.to_string();
            task.line = i + 1;
            tasks.push(task);
        }
        tasks
    }

    fn fixture(name: &str) -> String {
        let path = format!("{FIXTURE_DIR}/{name}");
        let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("read {path}: {err}"));
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn task_by_id(tasks: &[Task], id: &str) -> Task {
        tasks
            .iter()
            .find(|task| task.id == id)
            .unwrap_or_else(|| panic!("task {id} not found"))
            .clone()
    }

    fn assert_key_order(json: &str, keys: &[&str]) {
        let mut previous: Option<usize> = None;
        for key in keys {
            let idx = json
                .find(&format!("\"{key}\""))
                .unwrap_or_else(|| panic!("key {key} missing from:\n{json}"));
            if let Some(previous) = previous {
                assert!(idx >= previous, "key {key} out of order in:\n{json}");
            }
            previous = Some(idx);
        }
    }

    fn cell_texts(table: &TableData, row: usize) -> Vec<&str> {
        table.rows[row]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect()
    }

    #[test]
    fn table_returns_structured_rows_with_semantic_colors() {
        let tasks = spec_tasks();
        let opts = ListOptions {
            all: true,
            terminal: Terminal {
                is_tty: true,
                no_color: true,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Table(table) = render.body else {
            panic!("expected table body");
        };
        assert_eq!(table.headers, ["ID", "TITLE", "TAGS", "PRI", "PROPS"]);
        assert!(!table.color);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(
            cell_texts(&table, 0),
            [
                "EXMPL-001",
                "Fix the thing",
                "#launch",
                "!high",
                "@status:doing"
            ]
        );
        assert_eq!(
            cell_texts(&table, 1),
            ["EXMPL-002", "Ship it", "#launch", "", ""]
        );

        assert_eq!(table.rows[0][0].color, CellColor::Id);
        assert_eq!(table.rows[0][1].color, CellColor::Default);
        assert_eq!(table.rows[0][2].color, CellColor::Tag);
        assert_eq!(table.rows[0][3].color, CellColor::Priority);
        assert_eq!(table.rows[0][4].color, CellColor::Default);
        assert_eq!(table.rows[1][0].color, CellColor::Done);
        assert_eq!(table.rows[1][1].color, CellColor::Done);

        for row in &table.rows {
            for cell in row {
                assert!(!cell.text.contains('\u{1b}'));
            }
        }
    }

    #[test]
    fn table_reports_the_color_enable_flag() {
        let tasks = spec_tasks();
        let opts = ListOptions {
            all: true,
            terminal: Terminal {
                is_tty: true,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Table(table) = render.body else {
            panic!("expected table body");
        };
        assert!(table.color);
    }

    #[test]
    fn empty_selection_prints_no_rows_or_lines() {
        let table_opts = ListOptions {
            all: true,
            terminal: Terminal {
                is_tty: true,
                no_color: true,
            },
            ..ListOptions::default()
        };
        let render = render_list(&[], &table_opts).unwrap();
        let ListBody::Table(table) = render.body else {
            panic!("expected table body");
        };
        assert!(table.rows.is_empty());
        assert!(render.notes.is_empty());

        let lines_opts = ListOptions {
            all: true,
            ..ListOptions::default()
        };
        let render = render_list(&[], &lines_opts).unwrap();
        let ListBody::Lines(lines) = render.body else {
            panic!("expected line body");
        };
        assert_eq!(lines, "");
        assert!(render.notes.is_empty());
    }

    #[test]
    fn lines_match_the_spec_literal_and_contain_no_ansi() {
        let tasks = spec_tasks();
        let opts = ListOptions {
            all: true,
            terminal: Terminal {
                is_tty: false,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Lines(lines) = render.body else {
            panic!("expected line body");
        };
        assert_eq!(lines, WANT_LINES);
        assert!(!lines.contains('\u{1b}'));
        assert!(render.notes.is_empty());
    }

    #[test]
    fn json_does_not_escape_html_and_keeps_defaults_and_key_order() {
        let tasks = vec![Task {
            id: "X-1".to_string(),
            status: b' ',
            title: "<b>&</b>".to_string(),
            file: "f.md".to_string(),
            line: 3,
            ..Task::default()
        }];
        let opts = ListOptions {
            all: true,
            json: true,
            terminal: Terminal {
                is_tty: true,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert!(!json.contains("\\u003c"));
        assert!(!json.contains("\\u0026"));
        assert!(json.contains("<b>&</b>"));
        assert!(json.contains(r#""tags": []"#));
        assert!(json.contains(r#""properties": {}"#));
        assert!(json.contains(r#""priority": "medium""#));
        assert!(json.contains(r#""status": "open""#));
        assert_key_order(
            &json,
            &[
                "id",
                "title",
                "status",
                "priority",
                "tags",
                "properties",
                "file",
                "line",
            ],
        );
    }

    #[test]
    fn json_exact_bytes_match_the_go_encoder() {
        let tasks = parse_tasks("tasks.md", "- [ ] JSON-1 Simple @owner:alice #docs !high\n");
        let visible = select_visible(&tasks, &ListOptions::default());
        let json = render_json(&tasks, &visible).unwrap();
        let want = "[\n  {\n    \"id\": \"JSON-1\",\n    \"title\": \"Simple\",\n    \"status\": \"open\",\n    \"priority\": \"high\",\n    \"tags\": [\n      \"docs\"\n    ],\n    \"properties\": {\n      \"owner\": \"alice\"\n    },\n    \"file\": \"tasks.md\",\n    \"line\": 1\n  }\n]\n";
        assert_eq!(json, want);
    }

    #[test]
    fn json_empty_result_is_an_empty_array() {
        let opts = ListOptions {
            json: true,
            terminal: Terminal {
                is_tty: true,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&[], &opts).unwrap();
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert_eq!(json, "[]\n");
    }

    #[test]
    fn json_suppresses_notes() {
        let tasks = spec_tasks();
        let opts = ListOptions {
            all: true,
            json: true,
            terminal: Terminal {
                is_tty: true,
                no_color: false,
            },
            hidden_blocked: vec![Task {
                id: "B-1".to_string(),
                header_raw: "- [ ] B-1 Blocked".to_string(),
                file: "f.md".to_string(),
                line: 9,
                ..Task::default()
            }],
            unidentified: vec![Unidentified {
                file: "f.md".to_string(),
                line: 10,
                raw: "- [ ] No ID".to_string(),
            }],
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        assert!(render.notes.is_empty());
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert!(!json.contains("Warning:"));
        assert!(!json.contains("have no ID"));
    }

    #[test]
    fn json_escapes_line_separators_like_go_without_html_escaping() {
        let task = Task {
            id: "U-1".to_string(),
            status: b' ',
            title: "a\u{2028}b\u{2029}c".to_string(),
            file: "f.md".to_string(),
            line: 1,
            ..Task::default()
        };
        let json = render_json(std::slice::from_ref(&task), std::slice::from_ref(&task)).unwrap();
        assert!(json.contains("a\\u2028b\\u2029c"));
        assert!(!json.contains('\u{2028}'));
        assert!(!json.contains('\u{2029}'));
    }

    #[test]
    fn sort_priority_orders_crit_high_medium_low_stably() {
        let tasks = vec![
            Task {
                id: "A-1".to_string(),
                priority: "low".to_string(),
                ..Task::default()
            },
            Task {
                id: "B-1".to_string(),
                priority: "crit".to_string(),
                ..Task::default()
            },
            Task {
                id: "C-1".to_string(),
                ..Task::default()
            },
            Task {
                id: "D-1".to_string(),
                priority: "high".to_string(),
                ..Task::default()
            },
            Task {
                id: "E-1".to_string(),
                priority: "crit".to_string(),
                ..Task::default()
            },
        ];
        let opts = ListOptions {
            all: true,
            json: true,
            sort: "priority".to_string(),
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert_key_order(&json, &["B-1", "E-1", "D-1", "C-1", "A-1"]);
    }

    #[test]
    fn unknown_sort_leaves_file_order_untouched() {
        let tasks = vec![
            Task {
                id: "A-1".to_string(),
                priority: "low".to_string(),
                ..Task::default()
            },
            Task {
                id: "B-1".to_string(),
                priority: "crit".to_string(),
                ..Task::default()
            },
            Task {
                id: "C-1".to_string(),
                ..Task::default()
            },
            Task {
                id: "D-1".to_string(),
                priority: "high".to_string(),
                ..Task::default()
            },
            Task {
                id: "E-1".to_string(),
                priority: "crit".to_string(),
                ..Task::default()
            },
        ];
        let opts = ListOptions {
            all: true,
            json: true,
            sort: "bogus".to_string(),
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert_key_order(&json, &["A-1", "B-1", "C-1", "D-1", "E-1"]);
    }

    #[test]
    fn notes_match_the_go_literal_and_alignment() {
        let hidden = vec![
            Task {
                id: "B-1".to_string(),
                header_raw: "- [ ] B-1 Short".to_string(),
                file: "a.md".to_string(),
                line: 4,
                ..Task::default()
            },
            Task {
                id: "B-2".to_string(),
                header_raw: "- [ ] B-2 A much longer line".to_string(),
                file: "b.md".to_string(),
                line: 12,
                ..Task::default()
            },
        ];
        let unidentified = vec![Unidentified {
            file: "a.md".to_string(),
            line: 9,
            raw: "- [ ] No ID".to_string(),
        }];
        let opts = ListOptions {
            hidden_blocked: hidden.clone(),
            unidentified,
            ..ListOptions::default()
        };
        let notes = render_notes(&opts);
        assert!(notes.contains(
            "Warning: 2 blocked task(s) hidden (use `mdfu task list --blocked` to show them):\n"
        ));
        assert!(notes.contains("1 task(s) have no ID (run `mdfu task ids` to assign one).\n"));
        let widest = hidden
            .iter()
            .map(|task| task.header_raw.chars().count())
            .max()
            .unwrap();
        for task in &hidden {
            let pad = widest - task.header_raw.chars().count() + 2;
            let want = format!(
                "{}{}{}:{}\n",
                task.header_raw,
                " ".repeat(pad),
                task.file,
                task.line
            );
            assert!(
                notes.contains(&want),
                "missing detail line {want:?} in:\n{notes}"
            );
        }
    }

    #[test]
    fn table_truncates_long_titles_to_one_hundred_twenty_cells() {
        let task = Task {
            id: "T-1".to_string(),
            title: "x".repeat(130),
            ..Task::default()
        };
        let table = table_data(
            std::slice::from_ref(&task),
            std::slice::from_ref(&task),
            false,
        );
        let title = &table.rows[0][1].text;
        assert_eq!(title.chars().count(), 120);
        assert!(title.ends_with('…'));
        assert!(title.starts_with(&"x".repeat(119)));
    }

    #[test]
    fn spec_basic_fixture_renders_lines_and_json() {
        let tasks = parse_tasks("spec-basic.md", &fixture("spec-basic.md"));

        let opts = ListOptions {
            terminal: Terminal {
                is_tty: false,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Lines(lines) = render.body else {
            panic!("expected line body");
        };
        let want = "[ ] BASIC-1  Draft the launch announcement  #tag  !high  @status:doing\n\
                    [ ] BASIC-2  Gather screenshots for the changelog\n";
        assert_eq!(lines, want);
        assert!(!lines.contains('\u{1b}'));

        let json_opts = ListOptions {
            json: true,
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &json_opts).unwrap();
        let ListBody::Json(json) = render.body else {
            panic!("expected JSON body");
        };
        assert_key_order(
            &json,
            &[
                "id",
                "title",
                "status",
                "priority",
                "tags",
                "properties",
                "file",
                "line",
            ],
        );
        assert!(json.contains(r#""id": "BASIC-1""#));
        assert!(json.contains(r#""file": "spec-basic.md""#));
        assert!(json.contains(r#""line": 8"#));
        assert!(!json.contains('\u{1b}'));
    }

    #[test]
    fn blocked_tasks_are_hidden_and_resolved_blockers_stripped() {
        let tasks = parse_tasks("spec-blockers.md", &fixture("spec-blockers.md"));

        let opts = ListOptions {
            hidden_blocked: vec![task_by_id(&tasks, "BLK-3")],
            terminal: Terminal {
                is_tty: false,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &opts).unwrap();
        let ListBody::Lines(lines) = render.body else {
            panic!("expected line body");
        };
        assert_eq!(
            lines,
            "[ ] BLK-2  Backfill the analytics table\n[ ] BLK-4  Update the runbook\n"
        );
        assert!(render.notes.contains("Warning: 1 blocked task(s) hidden"));
        assert!(
            render
                .notes
                .contains("- [ ] BLK-3 Flip the read path @blocked_by:BLK-404  spec-blockers.md:7"),
            "notes = {:?}",
            render.notes
        );

        let all_opts = ListOptions {
            all: true,
            blocked: true,
            terminal: Terminal {
                is_tty: false,
                no_color: false,
            },
            ..ListOptions::default()
        };
        let render = render_list(&tasks, &all_opts).unwrap();
        let ListBody::Lines(lines) = render.body else {
            panic!("expected line body");
        };
        assert!(lines.contains("[x] BLK-1  Ship the schema migration\n"));
        assert!(lines.contains("[ ] BLK-2  Backfill the analytics table\n"));
        assert!(lines.contains("[ ] BLK-3  Flip the read path  @blocked_by:BLK-404\n"));
    }

    #[test]
    fn done_task_with_blocked_by_reports_no_unresolved_blocker() {
        let done = Task {
            id: "D-1".to_string(),
            status: b'x',
            checked: true,
            property_order: vec!["blocked_by".to_string()],
            properties: props(&[("blocked_by", "NOPE-1")]),
            ..Task::default()
        };
        let all = vec![done.clone()];
        assert!(!has_unresolved_blockers(&done, &all));
        assert!(display_properties(&done, &all).is_empty());
    }

    #[test]
    fn zero_ansi_across_all_task_fixtures() {
        let mut names: Vec<String> = std::fs::read_dir(FIXTURE_DIR)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert!(!names.is_empty(), "no task fixtures found");
        for name in names {
            let tasks = parse_tasks(&name, &fixture(&name));
            let opts = ListOptions {
                all: true,
                blocked: true,
                terminal: Terminal {
                    is_tty: false,
                    no_color: false,
                },
                ..ListOptions::default()
            };
            let render = render_list(&tasks, &opts).unwrap();
            let ListBody::Lines(lines) = render.body else {
                panic!("expected line body");
            };
            assert!(!lines.contains('\u{1b}'), "{name} emitted ANSI");
            assert!(
                !render.notes.contains('\u{1b}'),
                "{name} notes emitted ANSI"
            );
        }
    }

    const VIEW_VAULT: &str = "- [ ] VIEW-1 First task\n  body line one\n  body line two\n\
                              - [x] VIEW-2 Done dependency\n\
                              - [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n\
                              - [ ] VIEW-4 No body\n";

    #[test]
    fn view_human_body_is_indented_by_six_spaces() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-1");
        let out = render_view_human(&task, Terminal::default());
        assert_eq!(
            out,
            "- [ ] VIEW-1 First task\n      body line one\n      body line two\n"
        );
    }

    #[test]
    fn view_human_no_body_prints_only_the_header() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-4");
        let out = render_view_human(&task, Terminal::default());
        assert_eq!(out, "- [ ] VIEW-4 No body\n");
    }

    #[test]
    fn view_human_keeps_a_resolved_blocker_in_the_verbatim_header() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-3");
        let out = render_view_human(&task, Terminal::default());
        assert_eq!(out, "- [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n");
    }

    #[test]
    fn view_human_blank_body_lines_stay_empty() {
        let vault = "- [ ] HUMAN-1 Title here\n  first body line\n  second body line\n\n  after blank\n- [ ] HUMAN-2 No body\n";
        let task = task_by_id(&parse_tasks("tasks.md", vault), "HUMAN-1");
        let out = render_view_human(&task, Terminal::default());
        assert_eq!(
            out,
            "- [ ] HUMAN-1 Title here\n      first body line\n      second body line\n\n      after blank\n"
        );
    }

    #[test]
    fn view_human_tty_prepends_the_gray_location_line() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-1");
        let out = render_view_human(
            &task,
            Terminal {
                is_tty: true,
                no_color: false,
            },
        );
        assert_eq!(
            out,
            "\u{1b}[38;5;245mtasks.md:1\u{1b}[0m\n- [ ] VIEW-1 First task\n      body line one\n      body line two\n"
        );
    }

    #[test]
    fn view_human_tty_no_color_prints_a_plain_location_line() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-4");
        let out = render_view_human(
            &task,
            Terminal {
                is_tty: true,
                no_color: true,
            },
        );
        assert_eq!(out, "tasks.md:6\n- [ ] VIEW-4 No body\n");
    }

    #[test]
    fn view_json_has_body_last_and_keeps_a_resolved_blocker() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-3");
        let json = render_view_json(&task).unwrap();
        assert_key_order(
            &json,
            &[
                "id",
                "title",
                "status",
                "priority",
                "tags",
                "properties",
                "file",
                "line",
                "body",
            ],
        );
        assert!(json.contains(r#""blocked_by": "VIEW-2""#));
        assert!(json.contains(r#""id": "VIEW-3""#));
        assert!(json.ends_with("\"body\": \"\"\n}\n"));
    }

    #[test]
    fn view_json_body_is_the_dedented_text() {
        let task = task_by_id(&parse_tasks("tasks.md", VIEW_VAULT), "VIEW-1");
        let json = render_view_json(&task).unwrap();
        assert!(json.contains(r#""body": "body line one\nbody line two""#));
        assert!(!json.contains("Warning:"));
    }
}
