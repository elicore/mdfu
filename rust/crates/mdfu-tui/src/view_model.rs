//! View-model boundary between `mdfu-core` content types and ratatui widgets.
//!
//! Widgets render *only* from the plain-data types defined here. Every
//! conversion ([`NoteRow::from`], [`PreviewBlock::from`], [`TaskRow::from`],
//! [`TaskDetail::from`]) snapshots the values a widget needs at conversion
//! time, so no frame ever reaches back into [`Document`] or [`Task`].
//!
//! The module depends on nothing beyond `mdfu-core` and `std`: raw frontmatter
//! values are flattened into this module's own `RawValue` reader through their
//! JSON encoding, so no direct `serde_json` dependency is required. The
//! frontmatter row model mirrors `internal/tui/frontmatter.go` (`fmKind`,
//! `buildFrontmatterRows`, `classifyRawValue`, `buildPairs`, `renderObject`).

use mdfu_core::model::Document;
use mdfu_core::task::{blocker, Task};

/// One row of the note picker's result list.
///
/// `snippet` is query-dependent and cannot be derived from a [`Document`]
/// alone; [`From`] leaves it empty and the caller attaches it with
/// [`NoteRow::with_snippet`] once the active query (and row width) is known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteRow {
    /// Document title; empty when the document has no title.
    pub title: String,
    /// Document path as loaded.
    pub path: String,
    /// Frontmatter `type` value.
    pub doc_type: String,
    /// Body-only match excerpt for the active query; empty when unused.
    pub snippet: String,
    /// Archived state, following [`Document::is_archived`] (explicit flag or
    /// `status: archived`).
    pub archived: bool,
}

impl From<&Document> for NoteRow {
    fn from(doc: &Document) -> Self {
        Self {
            title: doc.title.clone(),
            path: doc.path.clone(),
            doc_type: doc.doc_type.clone(),
            snippet: String::new(),
            archived: doc.is_archived(),
        }
    }
}

impl NoteRow {
    /// Attaches the query snippet computed by the picker.
    #[must_use]
    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = snippet.into();
        self
    }

    /// Title to display, mirroring the Go picker: title, else path, else
    /// `"(untitled)"`.
    #[must_use]
    pub fn display_title(&self) -> &str {
        if !self.title.is_empty() {
            &self.title
        } else if !self.path.is_empty() {
            &self.path
        } else {
            "(untitled)"
        }
    }
}

/// Snapshot of the document preview pane: filename bar, title block,
/// frontmatter rows, markdown body, and hyperlink preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBlock {
    /// Basename of the document path (empty when the path is empty).
    pub filename: String,
    /// Document title; empty when the document has no title.
    pub title: String,
    /// Ordered frontmatter rows; `Path` first when a path exists.
    pub fields: Vec<Field>,
    /// Raw markdown body, exactly as parsed.
    pub body_markdown: String,
    /// Whether web links should render as OSC 8 hyperlinks.
    pub hyperlinks: bool,
}

impl From<&Document> for PreviewBlock {
    fn from(doc: &Document) -> Self {
        Self {
            filename: preview_filename(&doc.path),
            title: doc.title.clone(),
            fields: frontmatter_fields(doc),
            body_markdown: doc.body.clone(),
            hyperlinks: true,
        }
    }
}

impl PreviewBlock {
    /// Sets the hyperlink preference (`true` by default, like the picker's
    /// `--no-hyperlinks` default).
    #[must_use]
    pub fn with_hyperlinks(mut self, hyperlinks: bool) -> Self {
        self.hyperlinks = hyperlinks;
        self
    }

    /// Title to display, mirroring the Go title block: title or
    /// `"(untitled)"`.
    #[must_use]
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "(untitled)"
        } else {
            &self.title
        }
    }
}

/// One labelled frontmatter row of the document preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Normalized row label (`"Path"`, `"Tags"`, ...) or raw frontmatter key.
    pub label: String,
    /// Structured row value.
    pub value: FieldValue,
}

/// A row value in the shape the preview renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// A single scalar rendered as `label: value`.
    Scalar(String),
    /// Scalar badges, rendered space-separated.
    Pills(Vec<String>),
    /// A flattened object value; pairs render joined with `", "`.
    Pairs(Vec<FieldPair>),
    /// A list of object values; each inner list is one element and elements
    /// render joined with `"; "`.
    PairGroups(Vec<Vec<FieldPair>>),
}

/// One `key: value` entry of a flattened object [`FieldValue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPair {
    /// Pair key; dotted (`a.b`) for nested maps within the depth cap.
    pub key: String,
    /// Pair value.
    pub value: FieldValue,
}

/// One row of the task list.
///
/// Blocker state is scope-relative: [`From`] resolves it against the task
/// itself, while [`TaskRow::from_scoped`] resolves it against the full task
/// scope so done blockers can resolve and resolved `blocked_by` entries can be
/// dropped, mirroring Go `DisplayProperties` / `HasUnresolvedBlockers`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskRow {
    /// Checkbox state.
    pub checked: bool,
    /// Full task ID; empty for unidentified rows.
    pub id: String,
    /// Title text with metadata stripped.
    pub title: String,
    /// Tags without their leading `#`, in source order.
    pub tags: Vec<String>,
    /// Priority without its leading `!`; empty when absent.
    pub priority: String,
    /// Display properties in render order: non-`blocked_by` first-seen order,
    /// then an unresolved `blocked_by` last.
    pub properties: Vec<(String, String)>,
    /// Whether at least one `blocked_by` ID in scope is unresolved.
    pub blocked: bool,
}

impl From<&Task> for TaskRow {
    fn from(task: &Task) -> Self {
        Self::from_scoped(task, std::slice::from_ref(task))
    }
}

impl TaskRow {
    /// Snapshots `task` with blocker properties resolved against `scope`.
    #[must_use]
    pub fn from_scoped(task: &Task, scope: &[Task]) -> Self {
        Self {
            checked: task.checked,
            id: task.id.clone(),
            title: task.title.clone(),
            tags: task.tags.clone(),
            priority: task.priority.clone(),
            properties: blocker::display_properties(task, scope),
            blocked: blocker::has_unresolved_blockers(task, scope),
        }
    }
}

/// Snapshot of the task detail pane: header, flattened metadata, body, and
/// blocker state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskDetail {
    /// Exact header line, after a single trailing `\r` is stripped.
    pub header_raw: String,
    /// Full task ID; empty for unidentified rows.
    pub id: String,
    /// Checkbox state.
    pub checked: bool,
    /// Title text with metadata stripped.
    pub title: String,
    /// Priority without its leading `!`; empty when absent.
    pub priority: String,
    /// Tags without their leading `#`, in source order.
    pub tags: Vec<String>,
    /// Display properties in render order (see [`TaskRow::properties`]).
    pub properties: Vec<(String, String)>,
    /// Slash-separated path relative to the invocation directory.
    pub file: String,
    /// 1-based line number; 0 when unknown.
    pub line: usize,
    /// Dedented body text.
    pub body: String,
    /// Whether at least one `blocked_by` ID in scope is unresolved.
    pub blocked: bool,
}

impl From<&Task> for TaskDetail {
    fn from(task: &Task) -> Self {
        Self::from_scoped(task, std::slice::from_ref(task))
    }
}

impl TaskDetail {
    /// Snapshots `task` with blocker properties resolved against `scope`.
    #[must_use]
    pub fn from_scoped(task: &Task, scope: &[Task]) -> Self {
        Self {
            header_raw: task.header_raw.clone(),
            id: task.id.clone(),
            checked: task.checked,
            title: task.title.clone(),
            priority: task.priority.clone(),
            tags: task.tags.clone(),
            properties: blocker::display_properties(task, scope),
            file: task.file.clone(),
            line: task.line,
            body: task.body.clone(),
            blocked: blocker::has_unresolved_blockers(task, scope),
        }
    }

    /// Renders the checkbox state as the Go `statusWord` does.
    #[must_use]
    pub fn status_word(&self) -> &'static str {
        if self.checked {
            "done"
        } else {
            "open"
        }
    }
}

/// Snapshot of the task editor frame: the editor buffer plus the metadata the
/// side panel shows.
///
/// The header is parsed by the command layer, never by a widget: `meta` holds
/// the already-parsed `(label, value)` preview rows, `body_indent` the raw
/// indent the body editor reports, and `is_body` selects the body-editor
/// frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskEditor {
    /// Editor buffer content, newline-separated.
    pub text: String,
    /// Parsed header preview rows (`ID`, `Status`, ...); empty for the body
    /// editor.
    pub meta: Vec<(String, String)>,
    /// Whether the frame is the body editor.
    pub is_body: bool,
    /// Raw body indent shown in the body editor's status, e.g. `"  "`.
    pub body_indent: String,
    /// Transient status message appended to the hints; empty hides it.
    pub status: String,
}

/// Plain-data status line for both TUIs.
///
/// `line()` reproduces the Go `renderStatus` / `renderTaskFooter` text; a
/// widget may instead style `visible`/`total`, `toggles`, `hints`, and
/// `message` separately.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusBar {
    /// Rows currently visible.
    pub visible: usize,
    /// Rows in the unfiltered set.
    pub total: usize,
    /// Toggle labels such as `"archived:shown"` or `"blocked:hidden"`.
    pub toggles: Vec<String>,
    /// Key hints for the current mode.
    pub hints: String,
    /// Transient status message; empty hides it.
    pub message: String,
}

impl StatusBar {
    /// Builds the note picker's status bar.
    #[must_use]
    pub fn notes(
        visible: usize,
        total: usize,
        show_archived: bool,
        show_frontmatter: bool,
        searching: bool,
        message: &str,
    ) -> Self {
        let hints = if searching {
            "esc:back • tab:multi • enter:select"
        } else {
            "/:search • e:edit • tab:multi • enter:select"
        };
        Self {
            visible,
            total,
            toggles: vec![
                format!("archived:{}", shown(show_archived)),
                format!("fm:{}", shown(show_frontmatter)),
            ],
            hints: hints.to_owned(),
            message: message.to_owned(),
        }
    }

    /// Builds the task TUI's status bar.
    #[must_use]
    pub fn tasks(visible: usize, total: usize, show_blocked: bool, message: &str) -> Self {
        Self {
            visible,
            total,
            toggles: vec![format!("blocked:{}", shown(show_blocked))],
            hints: "x:done • t:edit • ?:help".to_owned(),
            message: message.to_owned(),
        }
    }

    /// Renders the plain-text line (matches the Go status strings).
    #[must_use]
    pub fn line(&self) -> String {
        let mut line = format!("{}/{}", self.visible, self.total);
        for toggle in &self.toggles {
            line.push_str(" • ");
            line.push_str(toggle);
        }
        line.push_str(" • ");
        line.push_str(&self.hints);
        if !self.message.is_empty() {
            line.push_str(" • ");
            line.push_str(&self.message);
        }
        line
    }
}

/// Renders a toggle flag the way both Go status lines do.
fn shown(flag: bool) -> &'static str {
    if flag {
        "shown"
    } else {
        "hidden"
    }
}

/// Basename of a preview path; empty for an empty path (Go `filenameBar`).
fn preview_filename(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        filepath_base(path).to_owned()
    }
}

/// `filepath.Base` equivalent: trailing `/` separators are dropped.
fn filepath_base(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "." } else { "/" };
    }
    match trimmed.rfind('/') {
        Some(index) => &trimmed[index + 1..],
        None => trimmed,
    }
}

/// Maximum nesting depth of dotted-key flattening; deeper maps render as
/// Go-style `map[...]` scalars (mirrors `maxObjectDepth`).
const MAX_OBJECT_DEPTH: usize = 3;

/// Maximum nesting depth the JSON reader accepts, so a pathological raw value
/// cannot exhaust the stack.
const MAX_JSON_DEPTH: usize = 100;

/// Raw frontmatter keys already surfaced through normalized document fields;
/// matching is case-insensitive (mirrors `fmConsumedKeys`).
const CONSUMED_KEYS: &[&str] = &[
    "type",
    "title",
    "description",
    "resource",
    "tags",
    "status",
    "organized",
    "archived",
    "belongs_to",
    "belongs-to",
    "related_to",
    "related-to",
    "created",
    "date",
    "timestamp",
    "updated",
    "modified",
    "last_modified",
];

/// Builds the ordered preview rows: `Path` first (when a path exists), then
/// the normalized rows, then every remaining raw key sorted alphabetically.
fn frontmatter_fields(doc: &Document) -> Vec<Field> {
    let mut fields = Vec::new();
    if !doc.path.is_empty() {
        fields.push(scalar_field("Path", &doc.path));
    }
    if !doc.doc_type.is_empty() {
        fields.push(scalar_field("Type", &doc.doc_type));
    }
    if !doc.status.is_empty() {
        fields.push(scalar_field("Status", &doc.status));
    }
    if !doc.description.is_empty() {
        fields.push(scalar_field("Description", &doc.description));
    }
    if !doc.resource.is_empty() {
        fields.push(scalar_field("Resource", &doc.resource));
    }
    if !doc.tags.is_empty() {
        fields.push(pills_field("Tags", &doc.tags));
    }
    if !doc.belongs_to.is_empty() {
        fields.push(pills_field("Belongs to", &doc.belongs_to));
    }
    if !doc.related_to.is_empty() {
        fields.push(pills_field("Related to", &doc.related_to));
    }
    if let Some(created) = doc.created_at.as_deref().filter(|text| !text.is_empty()) {
        fields.push(scalar_field("Created", created));
    }
    if let Some(updated) = doc.updated_at.as_deref().filter(|text| !text.is_empty()) {
        fields.push(scalar_field("Updated", updated));
    }
    let mut keys: Vec<&String> = doc.raw.keys().filter(|key| !is_consumed_key(key)).collect();
    keys.sort_unstable();
    for key in keys {
        let Some(value) = doc.raw.get(key) else {
            continue;
        };
        // `serde_json` values round-trip through their compact JSON encoding,
        // which this module's reader accepts without naming `serde_json`
        // types (it is not a direct dependency).
        let Some(raw) = parse_json(&value.to_string()) else {
            continue;
        };
        if let Some(value) = classify_raw_value(&raw) {
            fields.push(Field {
                label: key.clone(),
                value,
            });
        }
    }
    fields
}

/// Reports whether a raw key is already surfaced by a normalized row.
fn is_consumed_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    CONSUMED_KEYS.contains(&lower.as_str())
}

/// Builds one scalar row.
fn scalar_field(label: &str, value: &str) -> Field {
    Field {
        label: label.to_owned(),
        value: FieldValue::Scalar(value.to_owned()),
    }
}

/// Builds one pill row.
fn pills_field(label: &str, values: &[String]) -> Field {
    Field {
        label: label.to_owned(),
        value: FieldValue::Pills(values.to_vec()),
    }
}

/// Maps one raw value to a row value: maps and JSON-object strings become
/// objects, lists of maps become object lists, scalar lists become pills, and
/// anything else becomes a scalar (mirrors `classifyRawValue`).
fn classify_raw_value(value: &RawValue) -> Option<FieldValue> {
    match value {
        RawValue::Object(entries) => {
            let pairs = build_pairs(entries, 1);
            (!pairs.is_empty()).then_some(FieldValue::Pairs(pairs))
        }
        RawValue::Array(items) => classify_slice(items),
        RawValue::String(text) => classify_string(text),
        other => pair_field(other),
    }
}

/// Classifies a raw string: a JSON object flattens into pairs, anything else
/// stays a scalar; blank strings yield nothing (mirrors `parseJSONObject`).
fn classify_string(text: &str) -> Option<FieldValue> {
    if text.trim().is_empty() {
        return None;
    }
    if let Some(RawValue::Object(entries)) = parse_json(text.trim()) {
        let pairs = build_pairs(&entries, 1);
        if !pairs.is_empty() {
            return Some(FieldValue::Pairs(pairs));
        }
    }
    Some(FieldValue::Scalar(text.to_owned()))
}

/// Maps a list value: every element a map yields object groups; otherwise it
/// falls back to nested pair behavior (mirrors `classifySlice`).
fn classify_slice(items: &[RawValue]) -> Option<FieldValue> {
    let mut groups: Vec<Vec<FieldPair>> = Vec::new();
    let mut object_list = true;
    for item in items {
        let RawValue::Object(entries) = item else {
            object_list = false;
            break;
        };
        let pairs = build_pairs(entries, 1);
        if !pairs.is_empty() {
            groups.push(pairs);
        }
    }
    if object_list {
        return (!groups.is_empty()).then_some(FieldValue::PairGroups(groups));
    }
    pair_field(&RawValue::Array(items.to_vec()))
}

/// Maps one nested value the way `pairField` does: objects and non-scalar
/// lists fall back to `%v`, scalar lists become pills, scalars stringify.
fn pair_field(value: &RawValue) -> Option<FieldValue> {
    match value {
        RawValue::Null => None,
        RawValue::String(text) => {
            (!text.trim().is_empty()).then(|| FieldValue::Scalar(text.clone()))
        }
        RawValue::Array(items) => Some(slice_field(items)),
        RawValue::Bool(_) | RawValue::Number(_) | RawValue::Object(_) => {
            Some(FieldValue::Scalar(go_value_string(value)))
        }
    }
}

/// Maps a list fallback: all-scalar lists become pills, anything else renders
/// as a Go `%v` scalar.
fn slice_field(items: &[RawValue]) -> FieldValue {
    if let Some(pills) = slice_pills(items) {
        return FieldValue::Pills(pills);
    }
    FieldValue::Scalar(go_value_string(&RawValue::Array(items.to_vec())))
}

/// Stringifies a list when every element is a scalar, dropping empty items
/// and reporting `None` for lists containing maps or sub-lists (mirrors
/// `slicePills`).
fn slice_pills(items: &[RawValue]) -> Option<Vec<String>> {
    let mut pills = Vec::new();
    for item in items {
        if matches!(item, RawValue::Object(_) | RawValue::Array(_)) {
            return None;
        }
        let text = scalar_string(item);
        if !text.trim().is_empty() {
            pills.push(text);
        }
    }
    (!pills.is_empty()).then_some(pills)
}

/// Renders a scalar the way `scalarString` does: `null` is empty, strings pass
/// through, anything else goes through the `%v` renderer.
fn scalar_string(value: &RawValue) -> String {
    match value {
        RawValue::Null => String::new(),
        RawValue::String(text) => text.clone(),
        other => go_value_string(other),
    }
}

/// Flattens an object into ordered pairs. Keys sort descending so attestation
/// maps read naturally and source maps lead with their title; nested maps
/// recurse into dotted keys up to [`MAX_OBJECT_DEPTH`], then fall back to
/// `%v` (mirrors `buildPairs`).
fn build_pairs(entries: &[(String, RawValue)], depth: usize) -> Vec<FieldPair> {
    let mut ordered: Vec<&(String, RawValue)> = entries.iter().collect();
    ordered.sort_by(|left, right| right.0.cmp(&left.0));
    let mut pairs = Vec::new();
    for (key, value) in ordered {
        if let RawValue::Object(sub) = value {
            if depth < MAX_OBJECT_DEPTH {
                for pair in build_pairs(sub, depth + 1) {
                    pairs.push(FieldPair {
                        key: format!("{key}.{}", pair.key),
                        value: pair.value,
                    });
                }
                continue;
            }
        }
        if let Some(field) = pair_field(value) {
            pairs.push(FieldPair {
                key: key.clone(),
                value: field,
            });
        }
    }
    pairs
}

/// Renders a value the way Go `fmt.Sprintf("%v")` does: strings bare, arrays
/// space-joined in brackets, maps sorted by key as `map[k:v ...]`.
fn go_value_string(value: &RawValue) -> String {
    match value {
        RawValue::Null => "<nil>".to_owned(),
        RawValue::Bool(flag) => flag.to_string(),
        RawValue::Number(number) => go_number_string(number),
        RawValue::String(text) => text.clone(),
        RawValue::Array(items) => {
            let parts: Vec<String> = items.iter().map(go_value_string).collect();
            format!("[{}]", parts.join(" "))
        }
        RawValue::Object(entries) => {
            let mut ordered: Vec<&(String, RawValue)> = entries.iter().collect();
            ordered.sort_by(|left, right| left.0.cmp(&right.0));
            let parts: Vec<String> = ordered
                .iter()
                .map(|entry| format!("{}:{}", entry.0, go_value_string(&entry.1)))
                .collect();
            format!("map[{}]", parts.join(" "))
        }
    }
}

/// Renders a parsed number like Go's `%v`: integers exactly, floats through
/// the `'g'`-style formatter.
fn go_number_string(number: &Number) -> String {
    match number {
        Number::Int(value) => value.to_string(),
        Number::Uint(value) => value.to_string(),
        Number::Float(value) => go_float_string(*value),
    }
}

/// Formats an `f64` like Go's `fmt.Sprintf("%v", f)`:
/// `strconv.FormatFloat` with `'g'` and shortest precision, which uses plain
/// decimal notation for exponents in `[-4, 6)` and Go-style scientific
/// notation otherwise. Mirrors `mdfu-core`'s private helper of the same name.
#[allow(clippy::float_cmp)] // exact zero/sign checks are intentional here
fn go_float_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf"
        } else {
            "+Inf"
        }
        .to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent_text.parse().unwrap_or(0);
    if (-4..6).contains(&exponent) {
        format!("{value}")
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
    }
}

/// JSON-shaped value produced by this module's own reader.
#[derive(Debug, Clone, PartialEq)]
enum RawValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<RawValue>),
    Object(Vec<(String, RawValue)>),
}

/// A JSON number: integers keep exact text; floats format like Go `%v`.
#[derive(Debug, Clone, PartialEq)]
enum Number {
    Int(i64),
    Uint(u64),
    Float(f64),
}

/// Reads one JSON value from `text`; `None` when the text is not a single
/// well-formed JSON value.
fn parse_json(text: &str) -> Option<RawValue> {
    let mut parser = JsonParser { text, pos: 0 };
    let value = parser.value(0)?;
    parser.skip_ws();
    (parser.pos == text.len()).then_some(value)
}

/// Minimal recursive-descent JSON reader (RFC 8259). It only exists to turn
/// `serde_json` values and JSON object *strings* into this module's own
/// `RawValue`; it is not a general-purpose parser.
struct JsonParser<'a> {
    text: &'a str,
    pos: usize,
}

impl JsonParser<'_> {
    /// Reads one value, rejecting nesting beyond [`MAX_JSON_DEPTH`].
    fn value(&mut self, depth: usize) -> Option<RawValue> {
        if depth > MAX_JSON_DEPTH {
            return None;
        }
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(RawValue::String),
            b't' => self.literal("true").map(|()| RawValue::Bool(true)),
            b'f' => self.literal("false").map(|()| RawValue::Bool(false)),
            b'n' => self.literal("null").map(|()| RawValue::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    /// Reads an object; duplicate keys are preserved in source order.
    fn object(&mut self, depth: usize) -> Option<RawValue> {
        self.pos += 1; // consume '{'
        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(RawValue::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return None;
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(RawValue::Object(entries));
                }
                _ => return None,
            }
        }
    }

    /// Reads an array.
    fn array(&mut self, depth: usize) -> Option<RawValue> {
        self.pos += 1; // consume '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(RawValue::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(RawValue::Array(items));
                }
                _ => return None,
            }
        }
    }

    /// Reads a quoted string, decoding escapes.
    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek()? {
                b'"' => {
                    self.pos += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                0x00..=0x1f => return None,
                _ => {
                    let ch = self.text[self.pos..].chars().next()?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// Decodes one escape sequence into `out`.
    fn escape(&mut self, out: &mut String) -> Option<()> {
        let decoded = match self.peek()? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{0008}',
            b'f' => '\u{000c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                self.pos += 1;
                return self.unicode_escape(out);
            }
            _ => return None,
        };
        out.push(decoded);
        self.pos += 1;
        Some(())
    }

    /// Decodes one `\uXXXX` escape, combining surrogate pairs.
    fn unicode_escape(&mut self, out: &mut String) -> Option<()> {
        let code = self.hex4()?;
        let combined = if (0xd800..=0xdbff).contains(&code) {
            if self.peek() != Some(b'\\') {
                return None;
            }
            self.pos += 1;
            if self.peek() != Some(b'u') {
                return None;
            }
            self.pos += 1;
            let low = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return None;
            }
            0x1_0000 + ((code - 0xd800) << 10) + (low - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&code) {
            return None;
        } else {
            code
        };
        out.push(char::from_u32(combined)?);
        Some(())
    }

    /// Reads four hexadecimal digits.
    fn hex4(&mut self) -> Option<u32> {
        let end = self.pos + 4;
        let digits = self.text.get(self.pos..end)?;
        let value = u32::from_str_radix(digits, 16).ok()?;
        self.pos = end;
        Some(value)
    }

    /// Reads a JSON number.
    fn number(&mut self) -> Option<RawValue> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => self.skip_digits(),
            _ => return None,
        }
        let mut floating = false;
        if self.peek() == Some(b'.') {
            floating = true;
            self.pos += 1;
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return None;
            }
            self.skip_digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            floating = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return None;
            }
            self.skip_digits();
        }
        let text = self.text.get(start..self.pos)?;
        if floating {
            let value: f64 = text.parse().ok()?;
            return Some(RawValue::Number(Number::Float(value)));
        }
        if text.starts_with('-') {
            let value: i64 = text.parse().ok()?;
            return Some(RawValue::Number(Number::Int(value)));
        }
        let value: u64 = text.parse().ok()?;
        Some(RawValue::Number(Number::Uint(value)))
    }

    /// Consumes a run of ASCII digits.
    fn skip_digits(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    /// Consumes one exact literal word.
    fn literal(&mut self, word: &str) -> Option<()> {
        if self.text.get(self.pos..self.pos + word.len()) == Some(word) {
            self.pos += word.len();
            return Some(());
        }
        None
    }

    /// Skips JSON whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Peeks the next byte.
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Builds a property map for task fixtures.
    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// Builds a task with the given id, title, and checkbox state.
    fn task(id: &str, title: &str, checked: bool) -> Task {
        Task {
            id: id.to_owned(),
            title: title.to_owned(),
            checked,
            status: if checked { b'x' } else { b' ' },
            ..Task::default()
        }
    }

    /// Flattens field labels for compact assertions.
    fn labels(fields: &[Field]) -> Vec<&str> {
        fields.iter().map(|field| field.label.as_str()).collect()
    }

    #[test]
    fn json_reader_parses_objects_arrays_and_scalars() {
        assert_eq!(parse_json(" null "), Some(RawValue::Null));
        assert_eq!(parse_json("true"), Some(RawValue::Bool(true)));
        assert_eq!(parse_json("-12"), Some(RawValue::Number(Number::Int(-12))));
        assert_eq!(parse_json("12"), Some(RawValue::Number(Number::Uint(12))));
        assert_eq!(
            parse_json("2.5e3"),
            Some(RawValue::Number(Number::Float(2500.0)))
        );
        assert_eq!(
            parse_json(r#""a\nb""#),
            Some(RawValue::String("a\nb".to_owned()))
        );
        assert_eq!(
            parse_json(r#""\ud83d\ude00""#),
            Some(RawValue::String("😀".to_owned()))
        );
        assert_eq!(
            parse_json(r#"{"a": [1, "x"], "b": {"c": null}}"#),
            Some(RawValue::Object(vec![
                (
                    "a".to_owned(),
                    RawValue::Array(vec![
                        RawValue::Number(Number::Uint(1)),
                        RawValue::String("x".to_owned()),
                    ]),
                ),
                (
                    "b".to_owned(),
                    RawValue::Object(vec![("c".to_owned(), RawValue::Null)]),
                ),
            ]))
        );
    }

    #[test]
    fn json_reader_rejects_malformed_and_overdeep_input() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":}",
            "01",
            "1.",
            "\"abc",
            "tru",
            "{}x",
            "\"\\ud800\"",
        ] {
            assert_eq!(parse_json(bad), None, "must reject {bad:?}");
        }
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert_eq!(parse_json(&deep), None, "nesting cap must reject");
        let shallow = format!("{}{}", "[".repeat(50), "]".repeat(50));
        assert!(parse_json(&shallow).is_some());
    }

    #[test]
    fn note_row_from_document_snapshots_fields() {
        let doc = Document {
            title: "Ship mdfu MVP".to_owned(),
            path: "notes/ship.md".to_owned(),
            doc_type: "Task".to_owned(),
            archived: Some(true),
            ..Document::default()
        };
        let row = NoteRow::from(&doc);
        assert_eq!(row.title, "Ship mdfu MVP");
        assert_eq!(row.path, "notes/ship.md");
        assert_eq!(row.doc_type, "Task");
        assert_eq!(row.snippet, "");
        assert!(row.archived);

        let status_archived = Document {
            status: "archived".to_owned(),
            ..Document::default()
        };
        assert!(NoteRow::from(&status_archived).archived);
    }

    #[test]
    fn note_row_empty_document_is_empty_without_panic() {
        let row = NoteRow::from(&Document::default());
        assert_eq!(
            row,
            NoteRow {
                title: String::new(),
                path: String::new(),
                doc_type: String::new(),
                snippet: String::new(),
                archived: false,
            }
        );
        assert_eq!(row.display_title(), "(untitled)");
    }

    #[test]
    fn note_row_display_title_falls_back_to_path_then_untitled() {
        let by_path = NoteRow {
            title: String::new(),
            path: "a/b.md".to_owned(),
            ..NoteRow::default()
        };
        assert_eq!(by_path.display_title(), "a/b.md");
        let titled = NoteRow {
            title: "Title".to_owned(),
            path: "a/b.md".to_owned(),
            ..NoteRow::default()
        };
        assert_eq!(titled.display_title(), "Title");
    }

    #[test]
    fn note_row_snippet_attaches_with_with_snippet() {
        let row = NoteRow::from(&Document {
            title: "T".to_owned(),
            ..Document::default()
        })
        .with_snippet("…match…");
        assert_eq!(row.snippet, "…match…");
    }

    #[test]
    fn note_row_and_preview_build_for_every_fixture() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata"));
        let mut count = 0;
        for entry in std::fs::read_dir(dir).expect("testdata reads") {
            let path = entry.expect("entry reads").path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
                continue;
            }
            let path_text = path.to_string_lossy().into_owned();
            let doc = mdfu_core::parse::parse_file(&path_text).expect("fixture parses");
            let row = NoteRow::from(&doc);
            assert_eq!(row.path, path_text);
            assert!(!row.title.is_empty(), "fixture has a title: {path_text}");
            let preview = PreviewBlock::from(&doc);
            assert_eq!(
                preview.filename,
                path.file_name().unwrap().to_string_lossy()
            );
            assert_eq!(preview.body_markdown, doc.body);
            assert!(preview.hyperlinks);
            count += 1;
        }
        assert_eq!(count, 7, "expected the seven testdata fixtures");
    }

    #[test]
    fn preview_block_snapshots_filename_title_body_and_hyperlinks() {
        let doc = Document {
            path: "vault/Notes/portent-task.md".to_owned(),
            title: "Ship mdfu MVP".to_owned(),
            body: "# Old Working Title\n\nbody".to_owned(),
            ..Document::default()
        };
        let block = PreviewBlock::from(&doc);
        assert_eq!(block.filename, "portent-task.md");
        assert_eq!(block.title, "Ship mdfu MVP");
        assert_eq!(block.body_markdown, "# Old Working Title\n\nbody");
        assert!(block.hyperlinks);
        assert_eq!(block.display_title(), "Ship mdfu MVP");
        assert!(!block.with_hyperlinks(false).hyperlinks);

        let empty = PreviewBlock::from(&Document::default());
        assert_eq!(empty.filename, "");
        assert!(empty.fields.is_empty());
        assert_eq!(empty.display_title(), "(untitled)");
    }

    #[test]
    fn preview_block_builds_normalized_rows_in_order() {
        let doc = mdfu_core::parse::parse_content(
            "n.md",
            "---\ntype: Metric\nstatus: verified\ndescription: Tracks latency.\n\
             resource: https://example.com/x\ntags: [hermes, ingest]\n\
             belongs_to: Project Atlas\nrelated_to: [Launch Plan]\n\
             created: 2026-01-02\nupdated: 2026-03-04\nauthor: bob\nviews: 42\n---\n# H1\n\nbody\n",
        );
        let fields = PreviewBlock::from(&doc).fields;
        assert_eq!(
            labels(&fields),
            [
                "Path",
                "Type",
                "Status",
                "Description",
                "Resource",
                "Tags",
                "Belongs to",
                "Related to",
                "Created",
                "Updated",
                "author",
                "views",
            ]
        );
        assert_eq!(fields[0].value, FieldValue::Scalar("n.md".to_owned()));
        assert_eq!(
            fields[5].value,
            FieldValue::Pills(vec!["hermes".to_owned(), "ingest".to_owned()])
        );
        assert_eq!(
            fields[7].value,
            FieldValue::Pills(vec!["Launch Plan".to_owned()])
        );
        assert_eq!(
            fields[9].value,
            FieldValue::Scalar("2026-03-04T00:00:00Z".to_owned())
        );
        assert_eq!(fields[10].value, FieldValue::Scalar("bob".to_owned()));
        assert_eq!(fields[11].value, FieldValue::Scalar("42".to_owned()));
    }

    #[test]
    fn preview_block_classifies_objects_lists_and_json_strings() {
        let doc = mdfu_core::parse::parse_content(
            "",
            concat!(
                "---\ngenerated:\n  by: hermes-pipeline\n  at: \"2026-09-01T10:05:00Z\"\n",
                "sources:\n  - id: dash\n    resource: https://example.com/dash\n    title: Dash\n",
                "  - id: ingest\n",
                "meta: '{\"created_by\":\"Hermes\",\"created_at\":\"2026-09-01T10:00:00Z\"}'\n",
                "flag: true\nempty_list: []\n---\n",
            ),
        );
        let fields = PreviewBlock::from(&doc).fields;
        // `generated.at` feeds both normalized timestamps in `mdfu-core`
        // (`first_date` keys), so Created and Updated lead the raw rows.
        assert_eq!(
            labels(&fields),
            ["Created", "Updated", "flag", "generated", "meta", "sources"]
        );
        assert_eq!(
            fields[0].value,
            FieldValue::Scalar("2026-09-01T10:05:00Z".to_owned())
        );
        assert_eq!(fields[2].value, FieldValue::Scalar("true".to_owned()));
        assert_eq!(
            fields[3].value,
            FieldValue::Pairs(vec![
                FieldPair {
                    key: "by".to_owned(),
                    value: FieldValue::Scalar("hermes-pipeline".to_owned()),
                },
                FieldPair {
                    key: "at".to_owned(),
                    value: FieldValue::Scalar("2026-09-01T10:05:00Z".to_owned()),
                },
            ])
        );
        assert_eq!(
            fields[4].value,
            FieldValue::Pairs(vec![
                FieldPair {
                    key: "created_by".to_owned(),
                    value: FieldValue::Scalar("Hermes".to_owned()),
                },
                FieldPair {
                    key: "created_at".to_owned(),
                    value: FieldValue::Scalar("2026-09-01T10:00:00Z".to_owned()),
                },
            ])
        );
        assert_eq!(
            fields[5].value,
            FieldValue::PairGroups(vec![
                vec![
                    FieldPair {
                        key: "title".to_owned(),
                        value: FieldValue::Scalar("Dash".to_owned()),
                    },
                    FieldPair {
                        key: "resource".to_owned(),
                        value: FieldValue::Scalar("https://example.com/dash".to_owned()),
                    },
                    FieldPair {
                        key: "id".to_owned(),
                        value: FieldValue::Scalar("dash".to_owned()),
                    },
                ],
                vec![FieldPair {
                    key: "id".to_owned(),
                    value: FieldValue::Scalar("ingest".to_owned()),
                }],
            ])
        );
    }

    #[test]
    fn preview_block_skips_empty_raw_values() {
        let doc = mdfu_core::parse::parse_content(
            "",
            "---\nnothing:\nblank: \"  \"\nempty: {}\nflag: true\n---\n",
        );
        let fields = PreviewBlock::from(&doc).fields;
        assert_eq!(labels(&fields), ["flag"]);
        assert_eq!(fields[0].value, FieldValue::Scalar("true".to_owned()));

        let empty = PreviewBlock::from(&Document::default());
        assert!(empty.fields.is_empty());
    }

    #[test]
    fn task_row_snapshots_task_fields_and_self_scope_blockers() {
        let done = task("BLK-1", "Ship the schema migration", true);
        let blocked = Task {
            property_order: vec!["owner".to_owned(), "blocked_by".to_owned()],
            properties: props(&[("owner", "alice"), ("blocked_by", "BLK-1")]),
            tags: vec!["launch".to_owned()],
            priority: "high".to_owned(),
            ..task("BLK-2", "Backfill the analytics table", false)
        };
        let scope = vec![done, blocked.clone()];

        let scoped = TaskRow::from_scoped(&blocked, &scope);
        assert!(!scoped.checked);
        assert_eq!(scoped.id, "BLK-2");
        assert_eq!(scoped.title, "Backfill the analytics table");
        assert_eq!(scoped.tags, ["launch"]);
        assert_eq!(scoped.priority, "high");
        assert_eq!(
            scoped.properties,
            [("owner".to_owned(), "alice".to_owned())]
        );
        assert!(!scoped.blocked);

        // Without the done task in scope the same blocker is unresolved.
        let standalone = TaskRow::from(&blocked);
        assert!(standalone.blocked);
        assert_eq!(
            standalone.properties,
            [
                ("owner".to_owned(), "alice".to_owned()),
                ("blocked_by".to_owned(), "BLK-1".to_owned()),
            ]
        );

        let done_row = TaskRow::from_scoped(&scope[0], &scope);
        assert!(done_row.checked);
        assert!(!done_row.blocked);

        let empty = TaskRow::from_scoped(&Task::default(), &[]);
        assert_eq!(empty, TaskRow::default());
    }

    #[test]
    fn task_row_from_scoped_keeps_unresolved_blocked_by_last() {
        let missing = Task {
            property_order: vec!["status".to_owned(), "blocked_by".to_owned()],
            properties: props(&[("status", "doing"), ("blocked_by", "BLK-404")]),
            ..task("BLK-3", "Flip the read path", false)
        };
        let row = TaskRow::from_scoped(&missing, std::slice::from_ref(&missing));
        assert!(row.blocked);
        assert_eq!(
            row.properties,
            [
                ("status".to_owned(), "doing".to_owned()),
                ("blocked_by".to_owned(), "BLK-404".to_owned()),
            ]
        );
    }

    #[test]
    fn task_detail_snapshots_and_resolves_blockers() {
        let blocker_task = task("BLK-1", "done", true);
        let missing = Task {
            header_raw: "- [ ] BLK-3 Flip the read path".to_owned(),
            file: "tasks.md".to_owned(),
            line: 7,
            body: "body line".to_owned(),
            property_order: vec!["blocked_by".to_owned()],
            properties: props(&[("blocked_by", "BLK-404")]),
            ..task("BLK-3", "Flip the read path", false)
        };
        let scope = vec![blocker_task, missing.clone()];

        let detail = TaskDetail::from_scoped(&missing, &scope);
        assert_eq!(detail.header_raw, "- [ ] BLK-3 Flip the read path");
        assert_eq!(detail.id, "BLK-3");
        assert!(!detail.checked);
        assert_eq!(detail.title, "Flip the read path");
        assert_eq!(detail.file, "tasks.md");
        assert_eq!(detail.line, 7);
        assert_eq!(detail.body, "body line");
        assert!(detail.blocked);
        assert_eq!(
            detail.properties,
            [("blocked_by".to_owned(), "BLK-404".to_owned())]
        );
        assert_eq!(detail.status_word(), "open");

        let standalone = TaskDetail::from(&missing);
        assert!(standalone.blocked);

        let empty = TaskDetail::from_scoped(&Task::default(), &[]);
        assert_eq!(empty, TaskDetail::default());
        assert_eq!(empty.status_word(), "open");
    }

    #[test]
    fn status_bar_notes_line_matches_go_strings() {
        let browse = StatusBar::notes(2, 5, false, true, false, "");
        assert_eq!(
            browse.line(),
            "2/5 • archived:hidden • fm:shown • /:search • e:edit • tab:multi • enter:select"
        );

        let searching = StatusBar::notes(1, 1, true, false, true, "reloaded");
        assert_eq!(
            searching.line(),
            "1/1 • archived:shown • fm:hidden • esc:back • tab:multi • enter:select • reloaded"
        );
    }

    #[test]
    fn status_bar_tasks_line_matches_go_strings() {
        let hidden = StatusBar::tasks(3, 7, false, "");
        assert_eq!(
            hidden.line(),
            "3/7 • blocked:hidden • x:done • t:edit • ?:help"
        );

        let shown = StatusBar::tasks(0, 0, true, "cannot write");
        assert_eq!(
            shown.line(),
            "0/0 • blocked:shown • x:done • t:edit • ?:help • cannot write"
        );
    }

    #[test]
    fn mdfu_core_manifest_declares_no_ratatui() {
        let manifest = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../mdfu-core/Cargo.toml"
        ));
        let text = std::fs::read_to_string(manifest).expect("mdfu-core manifest reads");
        assert!(
            !text.to_lowercase().contains("ratatui"),
            "mdfu-core must stay renderer-free"
        );
    }
}
