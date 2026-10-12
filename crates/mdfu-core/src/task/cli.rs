//! The `mdfu task` command layer: the static subcommand registry, dispatch, and
//! the nine subcommands (SPEC.md §F/§I/§J).
//!
//! Port of `internal/task/commands.go` and every subcommand file. The registry
//! is a `static` table ordered 10..90 (Go registers through `init()`, which a
//! static table replaces without any mutable state). All host access is
//! injected through [`Env`]: stdout/stderr/stdin, the terminal flags, the
//! working directory, the clock, and the environment lookup. The one host
//! contact left is spawning `$EDITOR` for `open`, which by definition runs a
//! child process and inherits the real process stdio.
//!
//! The TTY table is rendered here from the structured [`TableData`] of
//! [`super::render`] with the SPEC §F.3 glyphs; the port plan relaxes exact
//! lipgloss box bytes, and no SGR is emitted so piped output stays ANSI-free.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use super::blocker::has_unresolved_blockers;
use super::config::join_go;
use super::edit;
use super::grammar;
use super::id;
use super::render::{
    render_list, render_view_human, render_view_json, ListBody, ListOptions, TableData, Terminal,
};
use super::scope;
use super::{Task, Unidentified};

/// The execution environment a subcommand runs against.
///
/// Mirrors the Go `Env` struct: tests inject buffers, a fixed working
/// directory, and an environment lookup so the command layer never touches the
/// real host. `now` is carried for parity with Go even though none of the nine
/// commands consumes it yet.
pub struct Env<'a> {
    /// Standard output sink.
    pub stdout: &'a mut dyn Write,
    /// Standard error sink.
    pub stderr: &'a mut dyn Write,
    /// Standard input source for the `ids` prefix prompt; `None` disables it.
    pub stdin: Option<&'a mut dyn BufRead>,
    /// Terminal capability flags.
    pub terminal: Terminal,
    /// Working directory the scope resolves from.
    pub cwd: String,
    /// Injected clock (Unix seconds), mirroring Go `Env.Now`.
    pub now: fn() -> i64,
    /// Injected environment lookup (`EDITOR`, `MDTASK_PATH`, ...).
    pub get_env: &'a dyn Fn(&str) -> Option<String>,
}

/// The process exit code a subcommand resolves to (Go `Result`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 0 success, 1 runtime/task/IO/config error, 2 usage error (SPEC.md §I).
    pub code: i32,
}

impl Outcome {
    const fn new(code: i32) -> Self {
        Self { code }
    }
}

/// One entry of the `mdfu task` registry.
pub struct Subcommand {
    /// Canonical command name.
    pub name: &'static str,
    /// Alternate names accepted by dispatch.
    pub aliases: &'static [&'static str],
    /// Canonical display order; lower sorts first.
    pub order: u8,
    run: fn(&mut Env<'_>, &[String]) -> Outcome,
}

/// The static subcommand registry in canonical order (10..90).
static SUBCOMMANDS: &[Subcommand] = &[
    Subcommand {
        name: "list",
        aliases: &[],
        order: 10,
        run: run_list,
    },
    Subcommand {
        name: "view",
        aliases: &["show"],
        order: 20,
        run: run_view,
    },
    Subcommand {
        name: "open",
        aliases: &[],
        order: 30,
        run: run_open,
    },
    Subcommand {
        name: "move",
        aliases: &[],
        order: 40,
        run: run_move,
    },
    Subcommand {
        name: "set",
        aliases: &[],
        order: 50,
        run: run_set,
    },
    Subcommand {
        name: "ids",
        aliases: &[],
        order: 60,
        run: run_ids,
    },
    Subcommand {
        name: "archive",
        aliases: &[],
        order: 70,
        run: run_archive,
    },
    Subcommand {
        name: "validate",
        aliases: &[],
        order: 80,
        run: run_validate,
    },
    Subcommand {
        name: "install-skills",
        aliases: &[],
        order: 90,
        run: run_install_skills,
    },
];

/// Returns the registry in canonical order.
#[must_use]
pub fn subcommands() -> &'static [Subcommand] {
    SUBCOMMANDS
}

/// Finds a subcommand by exact name or alias.
fn lookup_subcommand(name: &str) -> Option<&'static Subcommand> {
    SUBCOMMANDS
        .iter()
        .find(|subcommand| subcommand.name == name || subcommand.aliases.contains(&name))
}

/// Routes `argv` to a registered subcommand and returns its result.
///
/// An empty `argv` runs `list` with no arguments; a first token matching a
/// subcommand name or alias runs it with the remaining arguments; a task-ID
/// shaped token runs `view` with all of `argv`; anything else writes E1 plus
/// the help text to stderr and returns code 1.
#[must_use = "the dispatch outcome is the process exit code"]
pub fn dispatch(env: &mut Env<'_>, argv: &[String]) -> Outcome {
    if argv.is_empty() {
        return run_subcommand(env, "list", &[]);
    }
    if let Some(subcommand) = lookup_subcommand(&argv[0]) {
        return (subcommand.run)(env, &argv[1..]);
    }
    if is_task_id_shaped(&argv[0]) {
        return run_subcommand(env, "view", argv);
    }
    errf(env, &format!("unknown command '{}'", argv[0]));
    let _ = env.stderr.write_all(help_text().as_bytes());
    Outcome::new(1)
}

/// Resolves `name` and invokes it, returning code 1 when the name is somehow
/// not registered.
fn run_subcommand(env: &mut Env<'_>, name: &str, args: &[String]) -> Outcome {
    if let Some(subcommand) = lookup_subcommand(name) {
        (subcommand.run)(env, args)
    } else {
        errf(env, &format!("unknown command '{name}'"));
        Outcome::new(1)
    }
}

/// Writes the D1-prefixed message plus a newline to stderr.
fn errf(env: &mut Env<'_>, message: &str) {
    let _ = writeln!(env.stderr, "mdfu task: {message}");
}

/// Reports `err` on stderr and returns exit code 1.
fn fail(env: &mut Env<'_>, err: &dyn fmt::Display) -> Outcome {
    let _ = writeln!(env.stderr, "mdfu task: {err}");
    Outcome::new(1)
}

/// Writes `text` to stdout, ignoring the I/O result like Go's `fmt.Fprint`.
fn write_out(env: &mut Env<'_>, text: &str) {
    let _ = env.stdout.write_all(text.as_bytes());
}

/// Reports whether `token` is ID-shaped (a full `PREFIX-<n>` or bare digits).
fn is_task_id_shaped(token: &str) -> bool {
    id::valid_task_id(token)
        || (!token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The mdfu-branded usage block (divergence D4); deterministic and ends with a
/// newline.
#[must_use]
pub fn help_text() -> &'static str {
    "Usage: mdfu task <command> [options] [args]\n\
     \n\
     Commands:\n\
     \x20 list            List tasks\n\
     \x20 view, show      Show a single task\n\
     \x20 open            Open a task in $EDITOR\n\
     \x20 move            Move a task block to another file\n\
     \x20 set             Set metadata on tasks\n\
     \x20 ids             Assign IDs to tasks without one\n\
     \x20 archive         Archive done tasks\n\
     \x20 validate        Validate the task scope\n\
     \x20 install-skills  Install the mdfu task skills\n\
     \n\
     Global options:\n\
     \x20 --path <dir|file>  Base directory or file for the task scope\n\
     \n\
     Run 'mdfu task <command> --help' for command-specific help.\n"
}

/// Kind of value a flag accepts.
#[derive(Clone, Copy)]
enum FlagKind {
    /// A boolean flag; `-flag` alone means true.
    Bool,
    /// A single string value.
    Value,
    /// A repeatable string value.
    List,
}

/// One flag specification for [`parse_flags`].
#[derive(Clone, Copy)]
struct FlagDef {
    name: &'static str,
    kind: FlagKind,
    usage: &'static str,
}

const LIST_FLAGS: &[FlagDef] = &[
    FlagDef {
        name: "all",
        kind: FlagKind::Bool,
        usage: "include done tasks",
    },
    FlagDef {
        name: "blocked",
        kind: FlagKind::Bool,
        usage: "include blocked tasks",
    },
    FlagDef {
        name: "sort",
        kind: FlagKind::Value,
        usage: "sort field",
    },
    FlagDef {
        name: "json",
        kind: FlagKind::Bool,
        usage: "emit JSON",
    },
    FlagDef {
        name: "tag",
        kind: FlagKind::List,
        usage: "filter by tag (repeatable; AND)",
    },
    FlagDef {
        name: "priority",
        kind: FlagKind::List,
        usage: "filter by priority (repeatable; OR)",
    },
];

const VIEW_FLAGS: &[FlagDef] = &[FlagDef {
    name: "json",
    kind: FlagKind::Bool,
    usage: "emit JSON",
}];

const IDS_FLAGS: &[FlagDef] = &[FlagDef {
    name: "prefix",
    kind: FlagKind::Value,
    usage: "prefix for newly assigned IDs",
}];

const NO_FLAGS: &[FlagDef] = &[];

/// Values parsed for one invocation.
#[derive(Default)]
struct ParsedFlags {
    bools: BTreeMap<&'static str, bool>,
    values: BTreeMap<&'static str, String>,
    lists: BTreeMap<&'static str, Vec<String>>,
    positionals: Vec<String>,
}

impl ParsedFlags {
    fn bool_flag(&self, name: &str) -> bool {
        self.bools.get(name).copied().unwrap_or(false)
    }

    fn text_flag(&self, name: &str) -> &str {
        self.values.get(name).map_or("", String::as_str)
    }

    fn list_flag(&self, name: &str) -> &[String] {
        self.lists.get(name).map_or(&[], Vec::as_slice)
    }
}

/// Parses `args` with GNU-style interspersed flags (the observable behavior of
/// Go's `ReorderInterspersed` + `flag.Parse`).
///
/// Returns `Err` with code 0 for `-h`/`-help` and code 2 for any parse error,
/// mirroring Go `parseFlags` (SPEC.md §I, §K D4).
fn parse_flags(
    env: &mut Env<'_>,
    name: &str,
    defs: &[FlagDef],
    args: &[String],
) -> Result<ParsedFlags, Outcome> {
    let mut parsed = ParsedFlags::default();
    let mut index = 0;
    let mut positional_only = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if positional_only || arg.is_empty() || arg == "-" || !arg.starts_with('-') {
            parsed.positionals.push(arg.clone());
            continue;
        }
        if arg == "--" {
            positional_only = true;
            continue;
        }
        let body = arg.trim_start_matches('-');
        let (flag_name, inline_value) = match body.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_string())),
            None => (body, None),
        };
        let Some(def) = defs.iter().find(|def| def.name == flag_name) else {
            if flag_name == "h" || flag_name == "help" {
                write_usage(env, name, defs);
                return Err(Outcome::new(0));
            }
            let _ = writeln!(env.stderr, "flag provided but not defined: -{flag_name}");
            write_usage(env, name, defs);
            return Err(Outcome::new(2));
        };
        parse_one_flag(
            env,
            name,
            defs,
            def,
            inline_value,
            args,
            &mut index,
            &mut parsed,
        )?;
    }
    Ok(parsed)
}

/// Parses one recognized flag into `parsed`, consuming a following value when
/// needed.
#[allow(clippy::too_many_arguments)]
fn parse_one_flag(
    env: &mut Env<'_>,
    name: &str,
    defs: &[FlagDef],
    def: &FlagDef,
    inline_value: Option<String>,
    args: &[String],
    index: &mut usize,
    parsed: &mut ParsedFlags,
) -> Result<(), Outcome> {
    match def.kind {
        FlagKind::Bool => {
            let value = if let Some(text) = inline_value {
                if let Some(parsed_bool) = parse_go_bool(&text) {
                    parsed_bool
                } else {
                    let _ = writeln!(
                        env.stderr,
                        "invalid boolean value {text:?} for -{}: parse error",
                        def.name
                    );
                    write_usage(env, name, defs);
                    return Err(Outcome::new(2));
                }
            } else {
                true
            };
            parsed.bools.insert(def.name, value);
        }
        FlagKind::Value | FlagKind::List => {
            let value = if let Some(text) = inline_value {
                text
            } else if *index < args.len() {
                let value = args[*index].clone();
                *index += 1;
                value
            } else {
                let _ = writeln!(env.stderr, "flag needs an argument: -{}", def.name);
                write_usage(env, name, defs);
                return Err(Outcome::new(2));
            };
            if matches!(def.kind, FlagKind::List) {
                parsed.lists.entry(def.name).or_default().push(value);
            } else {
                parsed.values.insert(def.name, value);
            }
        }
    }
    Ok(())
}

/// Parses a Go `strconv.ParseBool` spelling.
fn parse_go_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// Writes the Go `flag.PrintDefaults` usage block for a parse error or `-h`:
/// flags sorted by name, the value-flag type suffix, and the indented usage.
fn write_usage(env: &mut Env<'_>, name: &str, defs: &[FlagDef]) {
    let _ = writeln!(env.stderr, "Usage of {name}:");
    let mut sorted: Vec<&FlagDef> = defs.iter().collect();
    sorted.sort_by_key(|def| def.name);
    for def in sorted {
        let type_name = match def.kind {
            FlagKind::Bool => "",
            FlagKind::Value => " string",
            FlagKind::List => " value",
        };
        let _ = writeln!(env.stderr, "  -{}{type_name}", def.name);
        let _ = writeln!(env.stderr, "    \t{}", def.usage);
    }
}

/// Implements `mdfu task list` (SPEC.md §F.1-F.3, §H).
fn run_list(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let parsed = match parse_flags(env, "list", LIST_FLAGS, &rest) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };

    let mut extra_tags = Vec::new();
    let mut extra_priorities = Vec::new();
    for arg in &parsed.positionals {
        if arg.starts_with('#') {
            extra_tags.push(arg.clone());
        } else if arg.starts_with('!') {
            extra_priorities.push(arg.clone());
        } else {
            let _ = writeln!(env.stderr, "mdfu tasks: unexpected argument '{arg}'");
            return Outcome::new(2);
        }
    }

    let (tasks, unidentified) = match scope::load_scope(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };

    let mut opts = ListOptions {
        all: parsed.bool_flag("all"),
        blocked: parsed.bool_flag("blocked"),
        sort: parsed.text_flag("sort").to_string(),
        tag_filter: parsed
            .list_flag("tag")
            .iter()
            .cloned()
            .chain(extra_tags)
            .collect(),
        priority_filter: parsed
            .list_flag("priority")
            .iter()
            .cloned()
            .chain(extra_priorities)
            .collect(),
        json: parsed.bool_flag("json"),
        terminal: env.terminal,
        hidden_blocked: Vec::new(),
        unidentified,
    };
    for task in &tasks {
        if !opts.all && task.checked {
            continue;
        }
        if !opts.blocked && has_unresolved_blockers(task, &tasks) {
            opts.hidden_blocked.push(task.clone());
        }
    }

    let rendered = match render_list(&tasks, &opts) {
        Ok(rendered) => rendered,
        Err(err) => return fail(env, &err),
    };
    write_list_body(env, &rendered.body);
    write_out(env, &rendered.notes);
    Outcome::new(0)
}

/// Writes the selected `list` payload: JSON/lines verbatim, or the box table
/// (skipped entirely when no row is visible, like Go `RenderList`).
fn write_list_body(env: &mut Env<'_>, body: &ListBody) {
    match body {
        ListBody::Json(text) | ListBody::Lines(text) => write_out(env, text),
        ListBody::Table(data) => {
            if !data.rows.is_empty() {
                write_out(env, &render_table(data));
            }
        }
    }
}

/// Implements `mdfu task view` (alias `show`) per SPEC.md §F.4.
fn run_view(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let parsed = match parse_flags(env, "view", VIEW_FLAGS, &rest) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };
    if parsed.positionals.len() != 1 {
        errf(env, "view requires exactly one task ID");
        return Outcome::new(2);
    }

    let (tasks, _) = match scope::scope_task_set(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    let task = match id::resolve(&parsed.positionals[0], &tasks) {
        Ok(task) => task,
        Err(err) => return fail(env, &err),
    };

    if parsed.bool_flag("json") {
        return match render_view_json(task) {
            Ok(text) => {
                write_out(env, &text);
                Outcome::new(0)
            }
            Err(err) => fail(env, &err),
        };
    }
    write_out(env, &render_view_human(task, env.terminal));
    Outcome::new(0)
}

/// Implements `mdfu task open <id>` (SPEC.md §F.5).
fn run_open(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    if rest.len() != 1 {
        errf(env, "open requires exactly one task ID");
        return Outcome::new(2);
    }

    let (tasks, _) = match scope::scope_task_set(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    let task = match id::resolve(&rest[0], &tasks) {
        Ok(task) => task,
        Err(err) => return fail(env, &err),
    };

    let editor = (env.get_env)("EDITOR").unwrap_or_default();
    if editor.trim().is_empty() {
        errf(env, "$EDITOR is not set");
        return Outcome::new(1);
    }
    let fields: Vec<&str> = editor.split_whitespace().collect();
    let mut command = Command::new(fields[0]);
    command.args(&fields[1..]);
    command.arg(format!("+{}", task.line));
    command.arg(&task.file);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    match command.status() {
        Ok(status) => Outcome::new(status.code().unwrap_or(1)),
        Err(err) => fail(env, &err),
    }
}

/// Implements `mdfu task move <id> <file>` (SPEC.md §F.5).
fn run_move(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    if rest.len() != 2 {
        errf(env, "move requires a task ID and a destination file");
        return Outcome::new(2);
    }

    let (tasks, _) = match scope::scope_task_set(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    let task = match id::resolve(&rest[0], &tasks) {
        Ok(task) => task,
        Err(err) => return fail(env, &err),
    };
    match edit::move_task(task, &rest[1]) {
        Ok(()) => Outcome::new(0),
        Err(err) => fail(env, &err),
    }
}

/// Implements `mdfu task set <id…> <token…>` (SPEC.md §F.6).
fn run_set(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let (ids, tokens) = split_set_args(&rest);
    if ids.is_empty() {
        return fail(env, &"no task IDs provided");
    }
    if tokens.is_empty() {
        return fail(env, &"no metadata tokens provided");
    }

    let (tasks, _) = match scope::scope_task_set(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    let mut resolved: Vec<(Task, String)> = Vec::with_capacity(ids.len());
    for id in &ids {
        let task = match id::resolve(id, &tasks) {
            Ok(task) => task,
            Err(err) => return fail(env, &err),
        };
        let (header, _) = set_header_tokens(&task.header_raw, &tokens);
        resolved.push((task.clone(), header));
    }

    let mut files: Vec<(String, Vec<(Task, String)>)> = Vec::new();
    for (task, header) in resolved {
        if let Some(index) = files.iter().position(|(file, _)| *file == task.file) {
            files[index].1.push((task, header));
        } else {
            let file = task.file.clone();
            files.push((file, vec![(task, header)]));
        }
    }

    for (file, group) in files {
        let mut edit = match edit::open(&file) {
            Ok(edit) => edit,
            Err(err) => return fail(env, &err),
        };
        for (task, header) in group {
            if header == task.header_raw {
                continue;
            }
            if let Err(err) = edit.verify(&task.id, task.line) {
                return fail(env, &err);
            }
            if let Err(err) = edit.replace_header_line(task.line, header.as_bytes()) {
                return fail(env, &err);
            }
        }
        if let Err(err) = edit.commit() {
            return fail(env, &err);
        }
    }
    Outcome::new(0)
}

/// Splits `args` on commas and whitespace and classifies each piece: an
/// ID-shaped token is an ID, everything else is a metadata token.
fn split_set_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut tokens = Vec::new();
    for arg in args {
        for piece in arg
            .split([',', ' ', '\t', '\n', '\r'])
            .filter(|piece| !piece.is_empty())
        {
            if is_task_id_shaped(piece) {
                ids.push(piece.to_string());
            } else {
                tokens.push(piece.to_string());
            }
        }
    }
    (ids, tokens)
}

/// Rewrites `raw`'s metadata portion to include `tokens` (Go `setHeaderTokens`).
///
/// An existing tag is skipped, a new priority replaces the existing one in
/// place, and a metadata-free header receives the literal two-tab separator.
/// Returns the rewritten header and whether it differs from `raw`.
fn set_header_tokens(raw: &str, tokens: &[String]) -> (String, bool) {
    let trimmed = raw.strip_suffix('\r').unwrap_or(raw);
    let Some((status, id_part, rest)) = grammar::header_parts(trimmed) else {
        return (raw.to_string(), false);
    };
    let (title, meta, mut sep) = grammar::split_meta_sep(&rest);
    let mut out: Vec<String> = meta.split_whitespace().map(str::to_string).collect();

    for token in tokens {
        if grammar::is_tag_token(token) {
            if !out.iter().any(|existing| existing == token) {
                out.push(token.clone());
            }
        } else if grammar::is_priority_token(token) {
            match out
                .iter()
                .position(|existing| grammar::is_priority_token(existing))
            {
                Some(index) => out[index].clone_from(token),
                None => out.push(token.clone()),
            }
        } else if let Some(key) = grammar::property_key_of(token) {
            match out
                .iter()
                .position(|existing| grammar::property_key_of(existing).as_deref() == Some(&key))
            {
                Some(index) => out[index].clone_from(token),
                None => out.push(token.clone()),
            }
        } else if !out.iter().any(|existing| existing == token) {
            out.push(token.clone());
        }
    }

    if out.is_empty() {
        return (raw.to_string(), false);
    }
    if sep.is_empty() {
        sep = "\t\t".to_string();
    }
    let header = format!("- [{status}] {id_part}{title}{sep}{}", out.join(" "));
    let changed = header != trimmed;
    (header, changed)
}

/// Implements `mdfu task ids` (SPEC.md §F.6).
fn run_ids(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let parsed = match parse_flags(env, "ids", IDS_FLAGS, &rest) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };
    if !parsed.positionals.is_empty() {
        errf(env, "ids takes no positional arguments");
        return Outcome::new(2);
    }

    let prefix = parsed.text_flag("prefix").trim().to_uppercase();
    if !prefix.is_empty() {
        if let Err(err) = id::validate_prefix(&prefix) {
            return fail(env, &err);
        }
    }

    let (tasks, unidentified) = match scope::scope_task_set(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    if unidentified.is_empty() {
        return Outcome::new(0);
    }

    let existing = ids_existing_prefixes(&tasks);
    let (seeds, assign_files) = ids_seed_prefixes(&unidentified);
    let fallback = match ids_fallback_prefixes(env, &prefix, &existing, &seeds, &assign_files) {
        Ok(fallback) => fallback,
        Err(outcome) => return outcome,
    };
    let assigns = ids_assignments(&unidentified, &existing, &seeds, &fallback);

    let writes = match id::plan(&assigns, id::global_max(&tasks)) {
        Ok(writes) => writes,
        Err(err) => return fail(env, &err),
    };
    if let Err(err) = verify_assignments(&assigns) {
        return fail(env, &err);
    }
    if let Err(err) = edit::apply(&writes) {
        return fail(env, &err);
    }

    for write in &writes {
        let _ = writeln!(env.stdout, "{}", write.header);
    }
    emit_duplicate_warnings(env, &tasks);
    Outcome::new(0)
}

/// Returns the most frequent existing prefix of every file that has tasks.
fn ids_existing_prefixes(tasks: &[Task]) -> BTreeMap<String, String> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_file: BTreeMap<&str, Vec<Task>> = BTreeMap::new();
    for task in tasks {
        if !by_file.contains_key(task.file.as_str()) {
            order.push(&task.file);
        }
        by_file
            .entry(task.file.as_str())
            .or_default()
            .push(task.clone());
    }
    let mut existing = BTreeMap::new();
    for file in order {
        if let Some(group) = by_file.get(file) {
            existing.insert(file.to_string(), id::most_frequent_prefix(group));
        }
    }
    existing
}

/// Returns the seed prefix per file (first seed wins) plus the assignment
/// files in first-seen order.
fn ids_seed_prefixes(unidentified: &[Unidentified]) -> (BTreeMap<String, String>, Vec<String>) {
    let mut seeds: BTreeMap<String, String> = BTreeMap::new();
    let mut assign_files = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for line in unidentified {
        if seen.insert(line.file.clone()) {
            assign_files.push(line.file.clone());
        }
        if seeds.get(&line.file).is_none_or(String::is_empty) {
            if let Some(prefix) = id::seed_prefix(&line.raw) {
                seeds.insert(line.file.clone(), prefix);
            }
        }
    }
    (seeds, assign_files)
}

/// Resolves the fallback prefix per assignment file: existing prefix, then
/// `--prefix`, then the seed prefix, then the interactive prompt.
fn ids_fallback_prefixes(
    env: &mut Env<'_>,
    prefix: &str,
    existing: &BTreeMap<String, String>,
    seeds: &BTreeMap<String, String>,
    assign_files: &[String],
) -> Result<BTreeMap<String, String>, Outcome> {
    let mut fallback = BTreeMap::new();
    for file in assign_files {
        let existing_prefix = existing.get(file).map_or("", String::as_str);
        if !existing_prefix.is_empty() {
            continue;
        }
        if !prefix.is_empty() {
            fallback.insert(file.clone(), prefix.to_string());
            continue;
        }
        let seed_prefix = seeds.get(file).map_or("", String::as_str);
        if !seed_prefix.is_empty() {
            fallback.insert(file.clone(), seed_prefix.to_string());
            continue;
        }
        let Some(answer) = prompt_prefix(env, file) else {
            continue;
        };
        if let Err(err) = id::validate_prefix(&answer) {
            return Err(fail(env, &err));
        }
        fallback.insert(file.clone(), answer);
    }
    Ok(fallback)
}

/// Writes the interactive prefix prompt to stderr and reads one line from the
/// injected stdin. Reports `None` on EOF with no input.
fn prompt_prefix(env: &mut Env<'_>, file: &str) -> Option<String> {
    let _ = write!(env.stderr, "Enter prefix for {file}: ");
    let reader = env.stdin.as_deref_mut()?;
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(_) => {
            let answer = line.trim().to_string();
            if answer.is_empty() {
                None
            } else {
                Some(answer)
            }
        }
        Err(_) => None,
    }
}

/// Builds one [`id::Assignment`] per ID-less line.
fn ids_assignments(
    unidentified: &[Unidentified],
    existing: &BTreeMap<String, String>,
    seeds: &BTreeMap<String, String>,
    fallback: &BTreeMap<String, String>,
) -> Vec<id::Assignment> {
    unidentified
        .iter()
        .map(|line| {
            let file_prefixes = existing
                .get(&line.file)
                .filter(|prefix| !prefix.is_empty())
                .map_or(Vec::new(), |prefix| vec![prefix.clone()]);
            id::Assignment {
                file: line.file.clone(),
                line: line.line,
                raw_line: line.raw.clone(),
                prefix: fallback.get(&line.file).cloned().unwrap_or_default(),
                seed: seeds.get(&line.file).cloned().unwrap_or_default(),
                file_prefixes,
            }
        })
        .collect()
}

/// Confirms every assignment's source line is still the raw line that was
/// planned, before any file is written (Go `verifyAssignments`).
fn verify_assignments(assigns: &[id::Assignment]) -> Result<(), String> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_file: BTreeMap<&str, Vec<&id::Assignment>> = BTreeMap::new();
    for assignment in assigns {
        if !by_file.contains_key(assignment.file.as_str()) {
            order.push(&assignment.file);
        }
        by_file
            .entry(assignment.file.as_str())
            .or_default()
            .push(assignment);
    }
    for file in order {
        let data = fs::read(file).map_err(|err| err.to_string())?;
        let text = String::from_utf8_lossy(&data);
        let lines: Vec<&str> = text.split('\n').collect();
        for assignment in &by_file[file] {
            let found = assignment
                .line
                .checked_sub(1)
                .and_then(|index| lines.get(index));
            let matches = found
                .is_some_and(|line| line.strip_suffix('\r').unwrap_or(line) == assignment.raw_line);
            if !matches {
                return Err(format!(
                    "file changed, task not at expected line: {file}:{}",
                    assignment.line
                ));
            }
        }
    }
    Ok(())
}

/// Writes one warning line per already-duplicated numeric part to stderr. It
/// never changes the exit code.
fn emit_duplicate_warnings(env: &mut Env<'_>, tasks: &[Task]) {
    for (number, ids) in id::duplicate_numeric_parts(tasks) {
        let _ = writeln!(
            env.stderr,
            "warning: duplicate numeric part {number} across prefixes: {}",
            ids.join(", ")
        );
    }
}

/// Implements `mdfu task archive [ids…]` per SPEC.md §F.7.
fn run_archive(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let parsed = match parse_flags(env, "archive", NO_FLAGS, &rest) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };
    let (tasks, _) = match scope::load_scope(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };

    let targets = match archive_targets(env, &tasks, &parsed.positionals) {
        Ok(targets) => targets,
        Err(outcome) => return outcome,
    };
    if targets.is_empty() {
        return Outcome::new(0);
    }

    let base_dir = if scope.is_file {
        scope::go_dir(&scope.file)
    } else {
        scope.base.clone()
    };
    match edit::archive_tasks(&targets, &base_dir, &scope.config) {
        Ok(()) => Outcome::new(0),
        Err(err) => fail(env, &err),
    }
}

/// Resolves the tasks to archive: every done task without IDs, or exactly the
/// named done tasks (E18 for a named open task), in scope order.
fn archive_targets(
    env: &mut Env<'_>,
    tasks: &[Task],
    ids: &[String],
) -> Result<Vec<Task>, Outcome> {
    if ids.is_empty() {
        return Ok(tasks.iter().filter(|task| task.checked).cloned().collect());
    }
    let mut want: BTreeSet<String> = BTreeSet::new();
    for id in ids {
        let task = match id::resolve(id, tasks) {
            Ok(task) => task,
            Err(err) => return Err(fail(env, &err)),
        };
        if !task.checked {
            errf(env, &format!("task '{id}' is not done"));
            return Err(Outcome::new(1));
        }
        want.insert(task.id.clone());
    }
    Ok(tasks
        .iter()
        .filter(|task| want.contains(&task.id))
        .cloned()
        .collect())
}

/// Implements `mdfu task validate` per SPEC.md §F.8.
fn run_validate(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let (scope, rest) = match scope::resolve_scope(&env.cwd, env.get_env, args) {
        Ok(parts) => parts,
        Err(err) => return fail(env, &err),
    };
    let parsed = match parse_flags(env, "validate", NO_FLAGS, &rest) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };
    if let Some(arg) = parsed.positionals.first() {
        errf(env, &format!("unexpected argument '{arg}'"));
        return Outcome::new(2);
    }

    let (tasks, _) = match scope::load_scope(&scope, env.get_env) {
        Ok(loaded) => loaded,
        Err(err) => return fail(env, &err),
    };
    let filtered: Vec<Task> = tasks
        .iter()
        .filter(|task| !prefix_excluded(&scope.config.exclude_prefixes, &task.id))
        .cloned()
        .collect();

    let had_error = validate_duplicate_ids(env, &filtered);
    validate_duplicate_numbers(env, &filtered);
    for task in &filtered {
        validate_task_meta(env, task);
    }
    if had_error {
        Outcome::new(1)
    } else {
        Outcome::new(0)
    }
}

/// Returns the prefix of a full identifier.
fn task_prefix(id: &str) -> Option<&str> {
    if !id::valid_task_id(id) {
        return None;
    }
    id.split_once('-').map(|(prefix, _)| prefix)
}

/// Reports whether `id`'s prefix is named in the excluded set.
fn prefix_excluded(excluded: &[String], id: &str) -> bool {
    match task_prefix(id) {
        Some(prefix) => excluded.iter().any(|entry| entry == prefix),
        None => false,
    }
}

/// Writes one `error:` line per duplicate-ID group, naming every location.
fn validate_duplicate_ids(env: &mut Env<'_>, tasks: &[Task]) -> bool {
    let mut by_id: BTreeMap<&str, Vec<&Task>> = BTreeMap::new();
    for task in tasks {
        by_id.entry(task.id.as_str()).or_default().push(task);
    }
    let mut found = false;
    for (id, group) in by_id.iter().filter(|(_, group)| group.len() > 1) {
        let locations: Vec<String> = group
            .iter()
            .map(|task| format!("{}:{}", task.file, task.line))
            .collect();
        let _ = writeln!(
            env.stderr,
            "error: duplicate ID '{id}' in {}",
            locations.join(", ")
        );
        found = true;
    }
    found
}

/// Writes the duplicate-numeric-part warning for every numeric part shared by
/// identifiers under more than one prefix.
fn validate_duplicate_numbers(env: &mut Env<'_>, tasks: &[Task]) {
    for (number, ids) in id::duplicate_numeric_parts(tasks) {
        let prefixes: BTreeSet<&str> = ids.iter().filter_map(|id| task_prefix(id)).collect();
        if prefixes.len() < 2 {
            continue;
        }
        let _ = writeln!(
            env.stderr,
            "warning: duplicate numeric part {number} across prefixes: {}",
            ids.join(", ")
        );
    }
}

/// Writes the three per-task metadata warnings: an empty tag, malformed
/// metadata (a bare `@key` with no `:value`), and an unknown priority.
fn validate_task_meta(env: &mut Env<'_>, task: &Task) {
    for word in grammar::header_meta(&task.header_raw).split_whitespace() {
        if word == "#" {
            let _ = writeln!(
                env.stderr,
                "warning: empty tag in {}:{}",
                task.file, task.line
            );
        } else if word.starts_with('@') && grammar::property_key_of(word).is_none() {
            let _ = writeln!(
                env.stderr,
                "warning: malformed metadata '{word}' in {}:{}",
                task.file, task.line
            );
        } else if grammar::is_priority_token(word) && !validate_priority_known(&word[1..]) {
            let _ = writeln!(
                env.stderr,
                "warning: unknown priority '{word}' in {}:{}",
                task.file, task.line
            );
        }
    }
}

/// Reports whether `priority` is one of the explicit ranks.
fn validate_priority_known(priority: &str) -> bool {
    matches!(priority, "crit" | "high" | "low")
}

/// Embedded skill directory names in install order (divergence D3).
const SKILL_NAMES: [&str; 3] = ["mdfu-task", "mdfu-task-add", "mdfu-task-do"];

/// The three embedded skill documents, copied byte-for-byte from the Go tree.
const SKILL_ASSETS: [&str; 3] = [
    include_str!("../../assets/skills/mdfu-task/SKILL.md"),
    include_str!("../../assets/skills/mdfu-task-add/SKILL.md"),
    include_str!("../../assets/skills/mdfu-task-do/SKILL.md"),
];

/// Implements `mdfu task install-skills <dir>` per SPEC.md §F.9 (divergence D3).
fn run_install_skills(env: &mut Env<'_>, args: &[String]) -> Outcome {
    let parsed = match parse_flags(env, "install-skills", NO_FLAGS, args) {
        Ok(parsed) => parsed,
        Err(outcome) => return outcome,
    };
    if parsed.positionals.is_empty() {
        errf(env, "missing directory argument");
        return Outcome::new(2);
    }
    if parsed.positionals.len() > 1 {
        errf(
            env,
            &format!("unexpected argument '{}'", parsed.positionals[1]),
        );
        return Outcome::new(2);
    }
    let dir = &parsed.positionals[0];

    for (name, data) in SKILL_NAMES.iter().zip(SKILL_ASSETS) {
        let parent = join_go(dir, name);
        let target = join_go(&parent, "SKILL.md");
        if let Err(err) = create_skill_dir(&parent) {
            return fail(env, &skill_write_error(&target, &err));
        }
        if let Err(err) = write_skill(&target, data.as_bytes()) {
            return fail(env, &skill_write_error(&target, &err));
        }
        let _ = writeln!(env.stdout, "Installed {name} into {dir}");
    }
    Outcome::new(0)
}

/// Creates `path` and its parents with the Go `os.MkdirAll(…, 0o755)` mode.
fn create_skill_dir(path: &str) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o755);
    }
    builder.create(path)
}

/// Writes `data` to `path` with the Go `os.WriteFile(…, 0o644)` mode.
fn write_skill(path: &str, data: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o644);
    }
    options.open(path)?.write_all(data)
}

/// Maps a filesystem failure to the SPEC §F.9 message, keeping the exact
/// permission-denied spelling.
fn skill_write_error(path: &str, err: &io::Error) -> String {
    if err.kind() == io::ErrorKind::PermissionDenied {
        format!("cannot write to '{path}': permission denied")
    } else {
        format!("cannot write to '{path}': {err}")
    }
}

/// Renders structured [`TableData`] as the SPEC §F.3 rounded box.
///
/// Exact lipgloss box bytes are relaxed by the port plan; this reproduces the
/// documented glyphs and column padding for ASCII content and emits no SGR.
fn render_table(data: &TableData) -> String {
    let mut widths: Vec<usize> = data.headers.iter().map(|h| h.chars().count()).collect();
    for row in &data.rows {
        for (index, cell) in row.iter().enumerate() {
            if index < widths.len() {
                widths[index] = widths[index].max(cell.text.chars().count());
            }
        }
    }

    let mut out = String::new();
    push_border(&mut out, '╭', '┬', '╮', &widths);
    let headers: Vec<String> = data.headers.iter().map(|h| (*h).to_string()).collect();
    push_row(&mut out, &headers, &widths);
    push_border(&mut out, '├', '┼', '┤', &widths);
    for row in &data.rows {
        let cells: Vec<String> = row.iter().map(|cell| cell.text.clone()).collect();
        push_row(&mut out, &cells, &widths);
    }
    push_border(&mut out, '╰', '┴', '╯', &widths);
    out
}

/// Pushes one border line: `left`, per-column `─` runs joined by `joint`, and
/// `right`.
fn push_border(out: &mut String, left: char, joint: char, right: char, widths: &[usize]) {
    out.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            out.push(joint);
        }
        for _ in 0..width + 2 {
            out.push('─');
        }
    }
    out.push(right);
    out.push('\n');
}

/// Pushes one table row: `│` + space + padded cell + space + `│` per column.
fn push_row(out: &mut String, cells: &[String], widths: &[usize]) {
    out.push('│');
    for (index, width) in widths.iter().enumerate() {
        let text = cells.get(index).map_or("", String::as_str);
        out.push(' ');
        out.push_str(text);
        for _ in text.chars().count()..*width {
            out.push(' ');
        }
        out.push_str(" │");
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{BufRead, Cursor};
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/tasks");

    const LIST_VAULT: &str = "- [ ] LIST-1 Alpha #one !high\n\
                              - [ ] LIST-2 Beta #one #two !low\n\
                              - [ ] LIST-3 Gamma #two\n\
                              - [x] LIST-4 Delta #one\n";

    const LIST_BLOCKED_VAULT: &str = "- [x] BLK-1 Done\n\
                                      - [ ] BLK-2 Blocked by open @blocked_by:BLK-3\n\
                                      - [ ] BLK-3 Open @blocked_by:BLK-404\n\
                                      - [ ] BLK-4 Free\n";

    const WANT_SPEC_TABLE: &str = "╭───────────┬───────────────┬─────────┬───────┬───────────────╮\n\
                                   │ ID        │ TITLE         │ TAGS    │ PRI   │ PROPS         │\n\
                                   ├───────────┼───────────────┼─────────┼───────┼───────────────┤\n\
                                   │ EXMPL-001 │ Fix the thing │ #launch │ !high │ @status:doing │\n\
                                   │ EXMPL-002 │ Ship it       │ #launch │       │               │\n\
                                   ╰───────────┴───────────────┴─────────┴───────┴───────────────╯\n";

    /// Serializes the process-wide chdir that mirrors Go's `t.Chdir`; the
    /// command layer keeps Go's process-cwd path semantics.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    /// A temp vault plus the environment values a test injects.
    struct Harness {
        dir: TempDir,
        vars: BTreeMap<String, String>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                dir: TempDir::new().expect("tempdir"),
                vars: BTreeMap::new(),
            }
        }

        fn path(&self, name: &str) -> String {
            self.dir.path().join(name).to_string_lossy().into_owned()
        }

        fn write(&self, name: &str, content: &str) {
            let path = self.dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("mkdir");
            }
            fs::write(path, content).expect("write");
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.dir.path().join(name)).expect("read")
        }
    }

    fn run_strings(
        harness: &Harness,
        terminal: Terminal,
        stdin_data: Option<&str>,
        argv: &[String],
    ) -> (Outcome, String, String) {
        let _guard = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::env::set_current_dir(&harness.dir).expect("chdir to vault");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut cursor = Cursor::new(stdin_data.unwrap_or("").as_bytes().to_vec());
        let stdin: Option<&mut dyn BufRead> = if stdin_data.is_some() {
            Some(&mut cursor as &mut dyn BufRead)
        } else {
            None
        };
        let get_env = |key: &str| harness.vars.get(key).cloned();
        let mut env = Env {
            stdout: &mut stdout,
            stderr: &mut stderr,
            stdin,
            terminal,
            cwd: harness.dir.path().to_string_lossy().into_owned(),
            now: || 0,
            get_env: &get_env,
        };
        let outcome = dispatch(&mut env, argv);
        (
            outcome,
            String::from_utf8(stdout).expect("utf8 stdout"),
            String::from_utf8(stderr).expect("utf8 stderr"),
        )
    }

    fn run(harness: &Harness, argv: &[&str]) -> (Outcome, String, String) {
        run_strings(harness, Terminal::default(), None, &strings(argv))
    }

    /// Runs `list` scoped to `file` with the extra args appended.
    fn run_list(harness: &Harness, file: &str, extra: &[&str]) -> (Outcome, String, String) {
        let mut argv = strings(&["list", "--path", file]);
        argv.extend(strings(extra));
        run_strings(harness, Terminal::default(), None, &argv)
    }

    fn fixture_paths() -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = fs::read_dir(FIXTURE_DIR)
            .expect("read fixtures")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        paths.sort();
        paths
    }

    fn assert_key_order(text: &str, keys: &[&str]) {
        let mut last = 0;
        for key in keys {
            let needle = format!("\"{key}\"");
            let position = text
                .find(&needle)
                .unwrap_or_else(|| panic!("missing {needle} in {text}"));
            assert!(position >= last, "key {needle} out of order in {text}");
            last = position;
        }
    }

    #[test]
    fn subcommands_are_the_nine_in_order() {
        let names: Vec<&str> = subcommands().iter().map(|sub| sub.name).collect();
        assert_eq!(
            names,
            [
                "list",
                "view",
                "open",
                "move",
                "set",
                "ids",
                "archive",
                "validate",
                "install-skills"
            ]
        );
        let orders: Vec<u8> = subcommands().iter().map(|sub| sub.order).collect();
        assert_eq!(orders, [10, 20, 30, 40, 50, 60, 70, 80, 90]);
        assert_eq!(subcommands()[1].aliases, ["show"]);
    }

    #[test]
    fn dispatch_routes_to_every_subcommand() {
        let harness = Harness::new();
        let cases: &[(&[&str], i32, &str)] = &[
            (&[], 0, ""),
            (&["list"], 0, ""),
            (&["ids"], 0, ""),
            (
                &["view"],
                2,
                "mdfu task: view requires exactly one task ID\n",
            ),
            (
                &["show"],
                2,
                "mdfu task: view requires exactly one task ID\n",
            ),
            (
                &["open"],
                2,
                "mdfu task: open requires exactly one task ID\n",
            ),
            (
                &["move"],
                2,
                "mdfu task: move requires a task ID and a destination file\n",
            ),
            (&["set"], 1, "mdfu task: no task IDs provided\n"),
            (&["archive"], 0, ""),
            (&["validate"], 0, ""),
            (
                &["install-skills"],
                2,
                "mdfu task: missing directory argument\n",
            ),
        ];
        for (argv, code, want_stderr) in cases {
            let (outcome, _, stderr) = run(&harness, argv);
            assert_eq!(outcome.code, *code, "argv={argv:?}");
            assert_eq!(stderr, *want_stderr, "argv={argv:?}");
        }
    }

    #[test]
    fn id_shaped_tokens_route_to_view() {
        let harness = Harness::new();
        for token in ["42", "EXMPL-42"] {
            let (outcome, stdout, stderr) = run(&harness, &[token]);
            assert_eq!(outcome.code, 1, "token={token}");
            assert!(stdout.is_empty(), "token={token}");
            assert_eq!(
                stderr,
                format!("mdfu task: task '{token}' not found\n"),
                "token={token}"
            );
        }
    }

    #[test]
    fn unknown_command_is_e1_with_help() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run(&harness, &["bogus"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            format!("mdfu task: unknown command 'bogus'\n{}", help_text())
        );
        assert!(!stderr.contains("mdtask"));
    }

    #[test]
    fn help_text_matches_go_byte_for_byte() {
        let want = "Usage: mdfu task <command> [options] [args]\n\nCommands:\n  list            List tasks\n  view, show      Show a single task\n  open            Open a task in $EDITOR\n  move            Move a task block to another file\n  set             Set metadata on tasks\n  ids             Assign IDs to tasks without one\n  archive         Archive done tasks\n  validate        Validate the task scope\n  install-skills  Install the mdfu task skills\n\nGlobal options:\n  --path <dir|file>  Base directory or file for the task scope\n\nRun 'mdfu task <command> --help' for command-specific help.\n";
        assert_eq!(help_text(), want);
    }

    #[test]
    fn flag_usage_matches_go_print_defaults() {
        let harness = Harness::new();
        let want = "Usage of list:\n  -all\n    \tinclude done tasks\n  -blocked\n    \tinclude blocked tasks\n  -json\n    \temit JSON\n  -priority value\n    \tfilter by priority (repeatable; OR)\n  -sort string\n    \tsort field\n  -tag value\n    \tfilter by tag (repeatable; AND)\n";
        let (outcome, stdout, stderr) = run(&harness, &["list", "--help"]);
        assert_eq!(outcome.code, 0);
        assert!(stdout.is_empty());
        assert_eq!(stderr, want);

        let want = "flag provided but not defined: -bogus\nUsage of list:\n  -all\n    \tinclude done tasks\n  -blocked\n    \tinclude blocked tasks\n  -json\n    \temit JSON\n  -priority value\n    \tfilter by priority (repeatable; OR)\n  -sort string\n    \tsort field\n  -tag value\n    \tfilter by tag (repeatable; AND)\n";
        let (outcome, _, stderr) = run(&harness, &["list", "--bogus"]);
        assert_eq!(outcome.code, 2);
        assert_eq!(stderr, want);
    }

    #[test]
    fn exit_codes_follow_the_parity_table() {
        let harness = Harness::new();
        let (outcome, stdout, _) = run(&harness, &["list", "--bogus"]);
        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());

        let (outcome, stdout, _) = run(&harness, &["list", "--tag"]);
        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());

        let (outcome, stdout, stderr) = run(&harness, &["list", "--help"]);
        assert_eq!(outcome.code, 0);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("Usage of list:"));

        let missing = harness.path("nope");
        let (outcome, stdout, stderr) = run(&harness, &["list", "--path", &missing]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            format!("mdfu task: --path '{missing}' does not exist or is not a file/directory\n")
        );

        let bad = Harness::new();
        bad.write(".mdtaskrc", "{");
        let (outcome, stdout, stderr) = run(&bad, &["list"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert!(
            stderr.starts_with("mdfu task: Invalid JSON in "),
            "{stderr}"
        );
    }

    #[test]
    fn list_line_modes_match_go() {
        let cases: &[(&str, &[&str], &str)] = &[
            (
                "default",
                &[],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n\
                 [ ] LIST-3  Gamma  #two\n",
            ),
            (
                "all",
                &["--all"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n\
                 [ ] LIST-3  Gamma  #two\n\
                 [x] LIST-4  Delta  #one\n",
            ),
            (
                "blocked",
                &["--blocked"],
                "[ ] BLK-2  Blocked by open  @blocked_by:BLK-3\n\
                 [ ] BLK-3  Open  @blocked_by:BLK-404\n\
                 [ ] BLK-4  Free\n",
            ),
            (
                "sort priority",
                &["--sort=priority"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-3  Gamma  #two\n\
                 [ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "one tag flag",
                &["--tag", "one"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "two tags AND",
                &["--tag", "one", "--tag", "two"],
                "[ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "two positional tags AND",
                &["#one", "#two"],
                "[ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "priority OR",
                &["--priority", "high", "--priority", "low"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "positional tag plus tag flag",
                &["#one", "--tag", "two"],
                "[ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "positional priority plus priority flag OR",
                &["!low", "--priority", "high"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n",
            ),
            (
                "unknown sort keeps file order",
                &["--sort=bogus"],
                "[ ] LIST-1  Alpha  #one  !high\n\
                 [ ] LIST-2  Beta  #one #two  !low\n\
                 [ ] LIST-3  Gamma  #two\n",
            ),
            (
                "all blocked",
                &["--all", "--blocked"],
                "[x] BLK-1  Done\n\
                 [ ] BLK-2  Blocked by open  @blocked_by:BLK-3\n\
                 [ ] BLK-3  Open  @blocked_by:BLK-404\n\
                 [ ] BLK-4  Free\n",
            ),
            (
                "single priority OR",
                &["--priority", "low"],
                "[ ] LIST-2  Beta  #one #two  !low\n",
            ),
        ];
        for (name, args, want) in cases {
            let harness = Harness::new();
            let content = if name.contains("blocked") {
                LIST_BLOCKED_VAULT
            } else {
                LIST_VAULT
            };
            harness.write("tasks.md", content);
            let file = harness.path("tasks.md");
            let (outcome, stdout, stderr) = run_list(&harness, &file, args);
            assert_eq!(outcome.code, 0, "{name}: {stderr}");
            assert_eq!(stdout, *want, "{name}");
        }
    }

    #[test]
    fn list_json_is_byte_exact() {
        let harness = Harness::new();
        harness.write("tasks.md", "- [ ] JSON-1 Simple @owner:alice #docs !high\n");
        let file = harness.path("tasks.md");
        let (outcome, stdout, stderr) = run_list(&harness, &file, &["--json"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        let want = "[\n  {\n    \"id\": \"JSON-1\",\n    \"title\": \"Simple\",\n    \
                    \"status\": \"open\",\n    \"priority\": \"high\",\n    \"tags\": [\n      \
                    \"docs\"\n    ],\n    \"properties\": {\n      \"owner\": \"alice\"\n    },\n    \
                    \"file\": \"tasks.md\",\n    \"line\": 1\n  }\n]\n";
        assert_eq!(stdout, want);
    }

    #[test]
    fn list_json_key_order_and_note_suppression() {
        let harness = Harness::new();
        harness.write("tasks.md", LIST_BLOCKED_VAULT);
        let file = harness.path("tasks.md");
        let (outcome, stdout, stderr) = run_list(&harness, &file, &["--all", "--json"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_key_order(
            &stdout,
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
        for needle in [
            "\"id\": \"BLK-1\"",
            "\"status\": \"done\"",
            "\"priority\": \"medium\"",
            "\"tags\": []",
            "\"properties\": {}",
        ] {
            assert!(stdout.contains(needle), "missing {needle}: {stdout}");
        }
        assert!(!stdout.contains("Warning:"));
        assert!(!stdout.contains("have no ID"));
        assert!(!stdout.contains('\u{1b}'));
    }

    #[test]
    fn list_hidden_blocked_note_is_aligned() {
        let harness = Harness::new();
        harness.write("tasks.md", LIST_BLOCKED_VAULT);
        let file = harness.path("tasks.md");
        let (outcome, stdout, stderr) = run_list(&harness, &file, &[]);
        assert_eq!(outcome.code, 0, "{stderr}");
        let raw1 = "- [ ] BLK-2 Blocked by open @blocked_by:BLK-3";
        let raw2 = "- [ ] BLK-3 Open @blocked_by:BLK-404";
        let widest = raw1.len();
        let want = format!(
            "[ ] BLK-4  Free\n\
             Warning: 2 blocked task(s) hidden (use `mdfu task list --blocked` to show them):\n\
             {raw1}{pad1}tasks.md:2\n\
             {raw2}{pad2}tasks.md:3\n",
            pad1 = " ".repeat(widest - raw1.len() + 2),
            pad2 = " ".repeat(widest - raw2.len() + 2),
        );
        assert_eq!(stdout, want);
    }

    #[test]
    fn list_piped_output_has_no_ansi_on_fixtures() {
        for path in fixture_paths() {
            let harness = Harness::new();
            let file = path.to_string_lossy().into_owned();
            let (outcome, stdout, stderr) = run_list(&harness, &file, &["--all", "--blocked"]);
            assert_eq!(outcome.code, 0, "{file}: {stderr}");
            assert!(!stdout.contains('\u{1b}'), "{file}: {stdout}");
        }
    }

    #[test]
    fn list_tty_table_matches_spec_glyphs() {
        let harness = Harness::new();
        harness.write(
            "tasks.md",
            "- [ ] EXMPL-001 Fix the thing\t\t#launch !high @status:doing\n\
             - [x] EXMPL-002 Ship it\t\t#launch\n",
        );
        let file = harness.path("tasks.md");
        let argv = strings(&["list", "--path", &file, "--all"]);
        let (outcome, stdout, stderr) = run_strings(
            &harness,
            Terminal {
                is_tty: true,
                no_color: true,
            },
            None,
            &argv,
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, WANT_SPEC_TABLE);
    }

    #[test]
    fn list_rejects_unknown_positional_with_e3() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run(&harness, &["list", "bogus"]);
        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu tasks: unexpected argument 'bogus'\n");
    }

    const VIEW_VAULT: &str = "- [ ] VIEW-1 First task\n\
                              \x20 body line one\n\
                              \x20 body line two\n\
                              - [x] VIEW-2 Done dependency\n\
                              - [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n\
                              - [ ] VIEW-4 No body\n";

    fn run_view_file(
        harness: &Harness,
        terminal: Terminal,
        name: &str,
        content: &str,
        extra: &[&str],
    ) -> (Outcome, String, String) {
        harness.write(name, content);
        let file = harness.path(name);
        let mut argv = strings(&["view", "--path", &file]);
        argv.extend(strings(extra));
        run_strings(harness, terminal, None, &argv)
    }

    #[test]
    fn view_human_body_and_no_body() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["VIEW-1"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(
            stdout,
            "- [ ] VIEW-1 First task\n      body line one\n      body line two\n"
        );

        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["VIEW-4"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] VIEW-4 No body\n");
    }

    #[test]
    fn view_resolved_blocker_stays_verbatim_and_bare_number_resolves() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["VIEW-3"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n");

        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["1"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(
            stdout,
            "- [ ] VIEW-1 First task\n      body line one\n      body line two\n"
        );
    }

    #[test]
    fn view_json_key_order_and_body_last() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["VIEW-1", "--json"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_key_order(
            &stdout,
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
        for needle in [
            "\"id\": \"VIEW-1\"",
            "\"priority\": \"medium\"",
            "\"tags\": []",
            "\"properties\": {}",
            "\"body\": \"body line one\\nbody line two\"",
        ] {
            assert!(stdout.contains(needle), "missing {needle}: {stdout}");
        }
        assert!(!stdout.contains("Warning:"));
    }

    #[test]
    fn view_json_keeps_a_resolved_blocker() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal::default(),
            "tasks.md",
            VIEW_VAULT,
            &["VIEW-3", "--json"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert!(stdout.contains("\"blocked_by\": \"VIEW-2\""), "{stdout}");
    }

    #[test]
    fn view_tty_location_line_and_no_color() {
        let vault = "- [ ] HUMAN-1 Title here\n\
                     \x20 first body line\n\
                     \x20 second body line\n\
                     \n\
                     \x20 after blank\n\
                     - [ ] HUMAN-2 No body\n";
        let want_body = "- [ ] HUMAN-1 Title here\n\
                         \x20     first body line\n\
                         \x20     second body line\n\
                         \n\
                         \x20     after blank\n";

        let harness = Harness::new();
        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal {
                is_tty: true,
                no_color: false,
            },
            "tasks.md",
            vault,
            &["HUMAN-1"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(
            stdout,
            format!("\u{1b}[38;5;245mtasks.md:1\u{1b}[0m\n{want_body}")
        );

        let (outcome, stdout, stderr) = run_view_file(
            &harness,
            Terminal {
                is_tty: true,
                no_color: true,
            },
            "tasks.md",
            vault,
            &["HUMAN-2"],
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "tasks.md:6\n- [ ] HUMAN-2 No body\n");
    }

    #[test]
    fn view_error_paths_propagate_e9_e10_e11_e12() {
        let harness = Harness::new();
        let cases: &[(&str, &str, &str)] = &[
            (VIEW_VAULT, "BOGUS", "mdfu task: invalid task ID 'BOGUS'\n"),
            (
                VIEW_VAULT,
                "NOPE-001",
                "mdfu task: task 'NOPE-001' not found\n",
            ),
            (
                "- [ ] DUP-1 One\n- [ ] DUP-1 Two\n",
                "DUP-1",
                "mdfu task: task 'DUP-1' appears multiple times; expected exactly one match\n",
            ),
            (
                "- [ ] ALPHA-1 A\n- [ ] BETA-1 B\n",
                "1",
                "mdfu task: ambiguous numeric ID '1' matches: ALPHA-1, BETA-1\n",
            ),
        ];
        for (content, id, want_stderr) in cases {
            let (outcome, stdout, stderr) =
                run_view_file(&harness, Terminal::default(), "tasks.md", content, &[id]);
            assert_eq!(outcome.code, 1, "id={id}");
            assert!(stdout.is_empty(), "id={id}: {stdout}");
            assert_eq!(stderr, *want_stderr, "id={id}");
        }
    }

    #[test]
    fn show_alias_routes_identically() {
        let harness = Harness::new();
        harness.write("tasks.md", VIEW_VAULT);
        let file = harness.path("tasks.md");
        let (outcome, stdout, stderr) = run(&harness, &["show", "VIEW-4", "--path", &file]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] VIEW-4 No body\n");
    }

    #[test]
    fn validate_duplicate_id_is_an_error() {
        let harness = Harness::new();
        harness.write("a.md", "- [ ] A-1 One\n- [x] A-1 Two\n");
        let (outcome, stdout, stderr) = run(&harness, &["validate"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "error: duplicate ID 'A-1' in a.md:1, a.md:2\n");
    }

    #[test]
    fn validate_warnings_never_change_the_exit_code() {
        let cases: &[(&str, &str)] = &[
            (
                "- [ ] A-1 One\n- [ ] B-1 Two\n",
                "warning: duplicate numeric part 1 across prefixes: A-1, B-1\n",
            ),
            ("- [ ] A-1 Title\t\t#\n", "warning: empty tag in a.md:1\n"),
            (
                "- [ ] A-1 Title\t\t@key\n",
                "warning: malformed metadata '@key' in a.md:1\n",
            ),
            (
                "- [ ] A-1 Title\t\t!urgent\n",
                "warning: unknown priority '!urgent' in a.md:1\n",
            ),
        ];
        for (content, want_stderr) in cases {
            let harness = Harness::new();
            harness.write("a.md", content);
            let (outcome, stdout, stderr) = run(&harness, &["validate"]);
            assert_eq!(outcome.code, 0, "content={content:?}: {stderr}");
            assert!(stdout.is_empty());
            assert_eq!(stderr, *want_stderr, "content={content:?}");
        }
    }

    #[test]
    fn validate_excluded_prefixes_are_not_reported() {
        let harness = Harness::new();
        harness.write(".mdfurc", r#"{"excludePrefixes":["ARCH"]}"#);
        harness.write(
            "a.md",
            "- [ ] ARCH-1 One\n- [x] ARCH-1 Two\n- [ ] EXMPL-1 One\n- [x] EXMPL-1 Two\n",
        );
        let (outcome, stdout, stderr) = run(&harness, &["validate"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "error: duplicate ID 'EXMPL-1' in a.md:3, a.md:4\n");
        assert!(!stderr.contains("ARCH"));
    }

    #[test]
    fn validate_rejects_unexpected_positionals() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run(&harness, &["validate", "x"]);
        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu task: unexpected argument 'x'\n");
    }

    #[test]
    fn ids_starts_at_global_max_plus_one_padded_to_three() {
        let harness = Harness::new();
        harness.write("a.md", "- [ ] A-1 First\n- [ ] Do me\n");
        let (outcome, stdout, stderr) = run(&harness, &["ids"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] A-002 Do me\n");
        assert_eq!(harness.read("a.md"), "- [ ] A-1 First\n- [ ] A-002 Do me\n");
        assert!(stderr.is_empty(), "{stderr}");
    }

    #[test]
    fn ids_uses_the_most_frequent_existing_prefix() {
        let harness = Harness::new();
        harness.write(
            "m.md",
            "- [ ] A-1 One\n- [ ] A-2 Two\n- [ ] B-3 Three\n- [ ] Do me\n",
        );
        let (outcome, stdout, stderr) = run(&harness, &["ids"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] A-004 Do me\n");
        assert_eq!(
            harness.read("m.md"),
            "- [ ] A-1 One\n- [ ] A-2 Two\n- [ ] B-3 Three\n- [ ] A-004 Do me\n"
        );
    }

    #[test]
    fn ids_seed_line_supplies_the_prefix() {
        let harness = Harness::new();
        harness.write("s.md", "- [ ] SEED- A seed\n- [ ] No ID\n");
        let (outcome, stdout, stderr) = run(&harness, &["ids"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] SEED-001 A seed\n- [ ] SEED-002 No ID\n");
        assert_eq!(
            harness.read("s.md"),
            "- [ ] SEED-001 A seed\n- [ ] SEED-002 No ID\n"
        );
    }

    #[test]
    fn ids_prefix_flag_is_uppercased_before_validation() {
        let harness = Harness::new();
        harness.write("p.md", "- [ ] Do me\n");
        let (outcome, stdout, stderr) = run(&harness, &["ids", "--prefix", "prj"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] PRJ-001 Do me\n");
        assert_eq!(harness.read("p.md"), "- [ ] PRJ-001 Do me\n");
    }

    #[test]
    fn ids_invalid_prefix_flag_fails_with_e13() {
        let harness = Harness::new();
        harness.write("p.md", "- [ ] Do me\n");
        let (outcome, stdout, stderr) = run(&harness, &["ids", "--prefix", "1bad"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu task: invalid prefix '1BAD'\n");
    }

    #[test]
    fn ids_all_or_nothing_writes_nothing_when_a_prefix_is_missing() {
        let harness = Harness::new();
        harness.write("a.md", "- [ ] A-1 Keep\n- [ ] Do me\n");
        harness.write("b.md", "- [ ] No source\n");
        let before_a = harness.read("a.md");
        let before_b = harness.read("b.md");

        let (outcome, stdout, _) = run(&harness, &["ids"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(harness.read("a.md"), before_a);
        assert_eq!(harness.read("b.md"), before_b);
    }

    #[test]
    fn ids_duplicate_numeric_part_warns_and_exits_zero() {
        let harness = Harness::new();
        harness.write("d.md", "- [ ] A-1 First\n- [ ] B-1 Second\n- [ ] Do me\n");
        let (outcome, stdout, stderr) = run(&harness, &["ids"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stdout, "- [ ] A-002 Do me\n");
        assert_eq!(
            stderr,
            "warning: duplicate numeric part 1 across prefixes: A-1, B-1\n"
        );
        assert_eq!(
            harness.read("d.md"),
            "- [ ] A-1 First\n- [ ] B-1 Second\n- [ ] A-002 Do me\n"
        );
    }

    #[test]
    fn ids_reads_the_prompt_answer_from_injected_stdin() {
        let harness = Harness::new();
        harness.write("b.md", "- [ ] No source\n");
        let (outcome, stdout, stderr) = run_strings(
            &harness,
            Terminal::default(),
            Some("PRJ\n"),
            &strings(&["ids"]),
        );
        assert_eq!(outcome.code, 0, "{stderr}");
        assert_eq!(stderr, "Enter prefix for b.md: ");
        assert_eq!(stdout, "- [ ] PRJ-001 No source\n");
        assert_eq!(harness.read("b.md"), "- [ ] PRJ-001 No source\n");
    }

    #[test]
    fn set_tag_noop_priority_replace_tabs_and_hash_title() {
        let cases: &[(&str, &[&str], &str)] = &[
            (
                "- [ ] A-1 Task\t\t#tag\n",
                &["A-1", "#tag"],
                "- [ ] A-1 Task\t\t#tag\n",
            ),
            (
                "- [ ] A-1 Task\t\t!low #tag\n",
                &["A-1", "!high"],
                "- [ ] A-1 Task\t\t!high #tag\n",
            ),
            (
                "- [ ] A-1 Task\n",
                &["A-1", "#new"],
                "- [ ] A-1 Task\t\t#new\n",
            ),
            (
                "- [ ] A-1 Fix #123 now\n",
                &["A-1", "#tag"],
                "- [ ] A-1 Fix #123 now\t\t#tag\n",
            ),
        ];
        for (content, args, want) in cases {
            let harness = Harness::new();
            harness.write("tasks.md", content);
            let mut argv = strings(&["set"]);
            argv.extend(strings(args));
            let (outcome, stdout, stderr) = run_strings(&harness, Terminal::default(), None, &argv);
            assert_eq!(outcome.code, 0, "content={content:?}: {stderr}");
            assert!(stdout.is_empty(), "content={content:?}");
            assert!(stderr.is_empty(), "content={content:?}: {stderr}");
            assert_eq!(harness.read("tasks.md"), *want, "content={content:?}");
        }
    }

    #[test]
    fn set_comma_separated_ids_in_one_file() {
        let harness = Harness::new();
        harness.write("tasks.md", "- [ ] A-1 One\n- [ ] A-2 Two\n");
        let (outcome, stdout, stderr) = run(&harness, &["set", "A-1,A-2", "#tag"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert!(stdout.is_empty());
        assert_eq!(
            harness.read("tasks.md"),
            "- [ ] A-1 One\t\t#tag\n- [ ] A-2 Two\t\t#tag\n"
        );
    }

    #[test]
    fn set_requires_ids_and_tokens() {
        let harness = Harness::new();
        let cases: &[(&[&str], &str)] = &[
            (&["set", "#tag"], "mdfu task: no task IDs provided\n"),
            (&["set", "A-1"], "mdfu task: no metadata tokens provided\n"),
            (&["set"], "mdfu task: no task IDs provided\n"),
        ];
        for (argv, want_stderr) in cases {
            let (outcome, stdout, stderr) = run(&harness, argv);
            assert_eq!(outcome.code, 1, "argv={argv:?}");
            assert!(stdout.is_empty(), "argv={argv:?}");
            assert_eq!(stderr, *want_stderr, "argv={argv:?}");
        }
    }

    #[test]
    fn archive_without_ids_archives_every_done_task() {
        let harness = Harness::new();
        harness.write(
            "a.md",
            "- [x] A-1 Done one\n  body of one\n- [ ] A-2 Open two\n- [x] A-3 Done three\n",
        );
        let (outcome, stdout, stderr) = run(&harness, &["archive"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert_eq!(harness.read("a.md"), "- [ ] A-2 Open two\n");
        assert_eq!(
            harness.read("_archive.md"),
            "- [x] A-1 Done one\n  body of one\n\n- [x] A-3 Done three\n"
        );
    }

    #[test]
    fn archive_named_open_task_is_e18_and_writes_nothing() {
        let harness = Harness::new();
        harness.write("a.md", "- [ ] A-1 Open\n- [x] A-2 Done\n");
        let (outcome, stdout, stderr) = run(&harness, &["archive", "A-1"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu task: task 'A-1' is not done\n");
        assert_eq!(harness.read("a.md"), "- [ ] A-1 Open\n- [x] A-2 Done\n");
        assert!(!harness.dir.path().join("_archive.md").exists());
    }

    #[test]
    fn archive_escaping_path_is_e19_and_writes_nothing() {
        let harness = Harness::new();
        harness.write(".mdfurc", r#"{"archivePath":"../escaped-archive.md"}"#);
        harness.write("a.md", "- [x] A-1 Done\n");
        let (outcome, stdout, stderr) = run(&harness, &["archive"]);
        assert_eq!(outcome.code, 1, "{stderr}");
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("is outside the base directory"),
            "stderr={stderr}"
        );
        assert_eq!(harness.read("a.md"), "- [x] A-1 Done\n");
        assert!(!harness
            .dir
            .path()
            .parent()
            .expect("parent")
            .join("escaped-archive.md")
            .exists());
    }

    #[test]
    fn archive_empty_scope_is_a_silent_exit_zero() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run(&harness, &["archive"]);
        assert_eq!(outcome.code, 0);
        assert!(stdout.is_empty() && stderr.is_empty());
        assert!(!harness.dir.path().join("_archive.md").exists());
    }

    #[test]
    fn move_relocates_a_block_and_keeps_the_separator() {
        let harness = Harness::new();
        harness.write(
            "a.md",
            "# Notes\n\n- [ ] A-1 First\n\n- [ ] A-2 Second\n  second body\n",
        );
        harness.write("b.md", "# Target\n");
        let (outcome, stdout, stderr) = run(&harness, &["move", "A-2", "b.md"]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert!(stdout.is_empty() && stderr.is_empty());
        assert_eq!(
            harness.read("b.md"),
            "# Target\n- [ ] A-2 Second\n  second body\n"
        );
        assert_eq!(harness.read("a.md"), "# Notes\n\n- [ ] A-1 First\n\n");
    }

    #[cfg(unix)]
    #[test]
    fn open_runs_the_editor_with_line_and_file_and_propagates_exit() {
        use std::os::unix::fs::PermissionsExt;

        let mut harness = Harness::new();
        harness.write("tasks.md", "# Notes\n\n- [ ] A-1 First\n");
        let argv_file = harness.path("argv.txt");
        let script = harness.path("editor.sh");
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{argv_file}'\nexit 3\n"),
        )
        .expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        harness.vars.insert("EDITOR".to_string(), script);

        let (outcome, stdout, stderr) = run(&harness, &["open", "A-1"]);
        assert_eq!(outcome.code, 3);
        assert!(stdout.is_empty() && stderr.is_empty());
        assert_eq!(
            fs::read_to_string(&argv_file).expect("argv"),
            "+3\ntasks.md\n"
        );
    }

    #[test]
    fn open_without_editor_is_e8() {
        let harness = Harness::new();
        harness.write("tasks.md", "- [ ] A-1 First\n");
        let (outcome, stdout, stderr) = run(&harness, &["open", "A-1"]);
        assert_eq!(outcome.code, 1);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu task: $EDITOR is not set\n");
    }

    #[test]
    fn install_skills_writes_three_files_and_overwrites() {
        let harness = Harness::new();
        let target = harness.path("out");
        fs::create_dir_all(harness.path("out/mdfu-task")).expect("mkdir");
        fs::write(harness.path("out/UNRELATED.md"), "keep me\n").expect("sentinel");
        fs::write(harness.path("out/mdfu-task/SKILL.md"), "STALE\n").expect("stale");

        let (outcome, stdout, stderr) = run(&harness, &["install-skills", &target]);
        assert_eq!(outcome.code, 0, "{stderr}");
        let lines: Vec<&str> = stdout.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 3, "stdout={stdout:?}");
        for (index, name) in SKILL_NAMES.iter().enumerate() {
            assert_eq!(lines[index], format!("Installed {name} into {target}"));
            let data = fs::read(harness.path(&format!("out/{name}/SKILL.md"))).expect("installed");
            assert!(!data.is_empty(), "{name} is empty");
        }
        assert_ne!(harness.read("out/mdfu-task/SKILL.md"), "STALE\n");
        assert_eq!(harness.read("out/UNRELATED.md"), "keep me\n");
    }

    #[test]
    fn install_skills_missing_directory_is_a_usage_error() {
        let harness = Harness::new();
        let (outcome, stdout, stderr) = run(&harness, &["install-skills"]);
        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "mdfu task: missing directory argument\n");
    }

    #[test]
    fn embedded_skills_name_no_foreign_skill() {
        for (name, data) in SKILL_NAMES.iter().zip(SKILL_ASSETS) {
            assert!(!data.is_empty(), "{name} is empty");
            for foreign in ["mdtask", "mdtask-add", "mdtask-do"] {
                assert!(!data.contains(foreign), "{name} contains {foreign}");
            }
        }

        let harness = Harness::new();
        let target = harness.path("out");
        let (outcome, stdout, stderr) = run(&harness, &["install-skills", &target]);
        assert_eq!(outcome.code, 0, "{stderr}");
        assert!(!stdout.contains("mdtask"));
    }
}
