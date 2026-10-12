//! Filter and interactive pipelines: scan → parse → query → rank → output.
//!
//! Ports `cmd/mdfu/pipeline.go` and `cmd/mdfu/filter.go`. [`load_documents`]
//! walks the root and parses every Markdown file (I/O failures are skipped so
//! one unreadable file never fails discovery), [`run_query_with_options`] is
//! the shared ranked-results pipeline used by filter mode and the TUI, and
//! [`build_filter_fn`] supplies the picker's injected per-keystroke filter.
//!
//! The dependency direction stays `mdfu-cli` → `mdfu-tui`: `mdfu-core` never
//! imports the note picker, and the picker receives its filter, reload, and
//! theme through [`notes::App::new`] configuration instead of globals.

use std::collections::HashMap;
use std::io::Write;

use mdfu_core::model::Document;
use mdfu_core::open;
use mdfu_core::output;
use mdfu_core::parse;
use mdfu_core::query;
use mdfu_core::scan;
use mdfu_core::search;
use mdfu_core::theme::Theme;
use mdfu_tui::notes::{self, Cmd, FilterFn, FilterHit, Msg};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

/// Root-directory options shared by filter and interactive modes; the Rust
/// stand-in for the Go root flags threaded through `runFilter`/`runTUI`.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// Directory to scan.
    pub root: &'a str,
    /// Include hidden files and directories.
    pub include_hidden: bool,
    /// Honor `.gitignore`/`.ignore` files (PLAN T6); `--no-ignore` maps to
    /// `false`. The Go reference accepts `RespectGitignore` but ignores it, so
    /// `false` is the parity behavior (see [`scan::walk_markdown`]).
    pub respect_gitignore: bool,
    /// Include archived documents.
    pub include_archived: bool,
    /// Result cap; `0` or negative means unlimited.
    pub limit: i64,
}

/// Walks `root` for Markdown files and parses each one.
///
/// Documents are kept even when frontmatter parsing fails (`Document` carries
/// `parse_error`); only files that fail I/O are skipped, exactly like Go
/// `loadDocuments`.
///
/// # Errors
///
/// Returns the walk error as a string when the root cannot be scanned.
pub fn load_documents(
    root: &str,
    include_hidden: bool,
    respect_gitignore: bool,
) -> Result<Vec<Document>, String> {
    let paths = scan::walk_markdown(&scan::Options {
        root: root.to_owned(),
        include_hidden,
        respect_gitignore,
        limit: 0, // never truncate discovery; --limit applies to final results
    })
    .map_err(|error| error.to_string())?;

    let mut docs = Vec::with_capacity(paths.len());
    for path in paths {
        // I/O error: skip the file but keep discovery going (Go parity).
        if let Ok(doc) = parse::parse_file(&path) {
            docs.push(doc);
        }
    }
    Ok(docs)
}

/// Shared scan → parse → query → rank → results pipeline.
///
/// `--limit` applies after ranking; `0` or negative means unlimited. Unless
/// `include_archived` is set, archived documents are dropped from the ranked
/// list (Go `search.FilterArchived`). Every result carries `score = 0.0`:
/// rank order is the score, exactly like Go.
///
/// # Errors
///
/// Returns the walk error string, or the query parse error string for a
/// malformed date filter.
pub fn run_query_with_options(
    root: &str,
    include_hidden: bool,
    respect_gitignore: bool,
    query_str: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<output::Result>, String> {
    let docs = load_documents(root, include_hidden, respect_gitignore)?;
    let parsed = query::parse(query_str).map_err(|error| error.to_string())?;
    let ranked = search::rank(&docs, Some(&parsed));
    let ranked: Vec<&Document> = if include_archived {
        ranked
    } else {
        ranked
            .into_iter()
            .filter(|doc| !doc.is_archived())
            .collect()
    };

    let mut results: Vec<output::Result> = ranked
        .iter()
        .map(|doc| output::Result {
            path: doc.path.clone(),
            score: 0.0,
            title: doc.title.clone(),
            doc_type: doc.doc_type.clone(),
            snippet: output::snippet(&doc.body, &parsed.bare, 120),
        })
        .collect();
    // Negative caps mean unlimited; `unwrap_or(0)` collapses "negative" and
    // "does not fit in usize" to the unlimited sentinel.
    let cap = usize::try_from(limit).unwrap_or(0);
    if cap > 0 {
        results.truncate(cap);
    }
    Ok(results)
}

/// Runs filter mode: the shared pipeline, formatted to `stdout`.
///
/// Returns the process exit code: `0` with results, `1` when empty or on
/// error (Go `runFilter`). Empty results print nothing.
#[must_use]
pub fn run_filter(
    options: &Options<'_>,
    query_str: &str,
    format: &str,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let results = match run_query_with_options(
        options.root,
        options.include_hidden,
        options.respect_gitignore,
        query_str,
        options.include_archived,
        options.limit,
    ) {
        Ok(results) => results,
        Err(error) => {
            let _ = writeln!(stderr, "mdfu: {error}");
            return 1;
        }
    };
    if results.is_empty() {
        return 1;
    }

    let rendered = match format {
        "json" => match output::format_json(&results) {
            Ok(json) => json,
            Err(error) => {
                let _ = writeln!(stderr, "format json: {error}");
                return 1;
            }
        },
        "vimgrep" => output::format_vimgrep(&results),
        _ => output::format_paths(&results),
    };
    let _ = write!(stdout, "{rendered}");
    0
}

/// Returns the picker's injected filter: re-parses the query string and
/// re-ranks on every keystroke.
///
/// Empty queries and query parse errors return every index in original order
/// (Go `buildFilterFunc`). Archived visibility is left to the picker's own
/// `show_archived` handling. The closure filters the App's *current* item
/// slice, so a re-parsed document after `$EDITOR` is visible to the next call.
#[must_use]
pub fn build_filter_fn() -> FilterFn {
    let hits = |index: usize| FilterHit {
        index,
        matched_indexes: Vec::new(),
    };
    Box::new(
        move |query_str: &str, items: &[Document]| -> Vec<FilterHit> {
            if query_str.trim().is_empty() {
                return (0..items.len()).map(hits).collect();
            }
            let Ok(parsed) = query::parse(query_str) else {
                return (0..items.len()).map(hits).collect();
            };
            let ranked = search::rank_with_matches(items, Some(&parsed));
            // Go keys its items by *Document pointer; the picker's items are
            // inline values with stable addresses, so a pointer map is the exact
            // equivalent.
            let by_ptr: HashMap<*const Document, usize> = items
                .iter()
                .enumerate()
                .map(|(index, doc)| (std::ptr::from_ref(doc), index))
                .collect();
            ranked
                .iter()
                .filter_map(|hit| {
                    by_ptr
                        .get(&std::ptr::from_ref(hit.doc))
                        .map(|&index| FilterHit {
                            index,
                            matched_indexes: hit.matched_indexes.clone(),
                        })
                })
                .collect()
        },
    )
}

/// Interactive picker mode: loads documents, injects the live filter plus the
/// editor/reload hooks, runs the picker, and prints the confirmed paths one
/// per line.
///
/// On abort Go's picker returns no selection, so nothing is printed and the
/// exit code stays `0`; load or terminal failures print `mdfu: ...` and
/// return `1`.
///
/// # Divergences
///
/// Go's `RunWithFilter` installs a SIGINT/SIGTERM handler that force-restores
/// the terminal and exits 130 when bubbletea does not quit within 500ms. The
/// Rust loop relies on the crossterm ctrl-c key event (which the picker maps
/// to abort) and ratatui's panic hook for restoration; an external SIGTERM is
/// not trapped. The `$EDITOR` value is read from the process environment by
/// the caller, keeping the picker crate env-agnostic.
#[must_use]
pub fn run_tui(
    options: &Options<'_>,
    theme: &Theme,
    no_hyperlinks: bool,
    editor: Option<&str>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(stderr, "mdfu: {error}");
            return 1;
        }
    };
    let outcome = drive_picker(&mut terminal, options, theme, no_hyperlinks, editor);
    ratatui::restore();

    match outcome {
        Ok(paths) => {
            for path in paths {
                let _ = writeln!(stdout, "{path}");
            }
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "mdfu: {error}");
            1
        }
    }
}

/// Builds the picker and pumps its event loop until it quits.
///
/// Returns the confirmed document paths (`[]` on abort), or an error string
/// for a load, terminal, or event failure.
fn drive_picker(
    terminal: &mut DefaultTerminal,
    options: &Options<'_>,
    theme: &Theme,
    no_hyperlinks: bool,
    editor: Option<&str>,
) -> Result<Vec<String>, String> {
    let docs = load_documents(
        options.root,
        options.include_hidden,
        options.respect_gitignore,
    )?;
    let docs: Vec<Document> = if options.include_archived {
        docs
    } else {
        docs.into_iter().filter(|doc| !doc.is_archived()).collect()
    };

    let config = notes::Config {
        limit: usize::try_from(options.limit).unwrap_or(0),
        show_archived: options.include_archived,
        preview: true,
        no_hyperlinks,
        theme: Some(theme.clone()),
        show_frontmatter: None,
        reload: Some(Box::new(|path| {
            parse::parse_file(path).map_err(|error| error.to_string())
        })),
        editor: editor.map(str::to_owned),
    };
    let mut app = notes::App::new(docs, config, build_filter_fn());

    let size = terminal.size().map_err(|error| error.to_string())?;
    let _ = app.update(Msg::Resize {
        width: size.width,
        height: size.height,
    });

    loop {
        terminal
            .draw(|frame| frame.render_widget(&mut app, frame.area()))
            .map_err(|error| error.to_string())?;
        let event = event::read().map_err(|error| error.to_string())?;
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                let Some(command) = app.update(Msg::Key(key)) else {
                    continue;
                };
                match command {
                    Cmd::Quit => break,
                    Cmd::OpenUrl(url) => {
                        // Go ignores the browser-launch error (`open.Open`).
                        let _ = open::open_url(&url);
                    }
                    Cmd::ExecEditor(argv) => {
                        let message = notes::exec_editor(terminal, &argv);
                        let _ = app.update(message);
                    }
                }
            }
            Event::Resize(width, height) => {
                let _ = app.update(Msg::Resize { width, height });
            }
            _ => {}
        }
    }

    if app.is_aborted() {
        return Ok(Vec::new());
    }
    Ok(app.result().iter().map(|doc| doc.path.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TESTDATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../testdata");

    fn options(root: &str) -> Options<'_> {
        Options {
            root,
            include_hidden: false,
            respect_gitignore: true,
            include_archived: false,
            limit: 20,
        }
    }

    fn doc(path: &str, content: &str) -> Document {
        parse::parse_content(path, content)
    }

    #[test]
    fn pipeline_load_documents_parses_testdata() {
        let docs = load_documents(TESTDATA, false, true).expect("walk testdata");
        assert!(docs.len() >= 7, "expected the fixture vault, got {docs:?}");
        assert!(
            docs.iter().any(|doc| doc.path.ends_with("bad-yaml.md")),
            "broken-YAML file must stay loaded: {docs:?}"
        );
    }

    #[test]
    fn pipeline_run_query_kumquat_zebra_matches_bad_yaml_with_snippet() {
        let results = run_query_with_options(TESTDATA, false, true, "kumquat zebra", false, 20)
            .expect("pipeline");
        assert_eq!(results.len(), 1, "results: {results:?}");
        let hit = &results[0];
        assert!(hit.path.ends_with("bad-yaml.md"), "path: {}", hit.path);
        assert!(hit.snippet.to_lowercase().contains("kumquat"));
        assert_eq!(hit.score.to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn pipeline_run_query_empty_query_returns_all_sorted_by_path() {
        let all = run_query_with_options(TESTDATA, false, true, "", false, 0).expect("pipeline");
        assert!(all.len() >= 7, "empty query returns all: {all:?}");
        assert!(
            all.windows(2).all(|pair| pair[0].path <= pair[1].path),
            "empty query sorts by path"
        );
    }

    #[test]
    fn pipeline_run_query_limit_caps_results() {
        let capped = run_query_with_options(TESTDATA, false, true, "", false, 2).expect("pipeline");
        assert_eq!(capped.len(), 2, "limit must truncate: {capped:?}");
    }

    #[test]
    fn pipeline_run_filter_exits_0_on_match_and_1_on_empty() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_filter(
            &options(TESTDATA),
            "kumquat zebra",
            "paths",
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
        let out = String::from_utf8(stdout).expect("utf8");
        assert!(out.contains("bad-yaml.md"), "stdout: {out}");
        assert!(stderr.is_empty());

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_filter(
            &options(TESTDATA),
            "zzzznotathing",
            "paths",
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn pipeline_run_filter_json_emits_snippet() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_filter(
            &options(TESTDATA),
            "kumquat zebra",
            "json",
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 0);
        let out = String::from_utf8(stdout).expect("utf8");
        assert!(out.contains("\"snippet\""), "stdout: {out}");
        assert!(out.contains("\"score\": 0.0"), "stdout: {out}");
    }

    #[test]
    fn pipeline_run_filter_reports_query_parse_error() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_filter(
            &options(TESTDATA),
            "created:notadate",
            "paths",
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        let err = String::from_utf8(stderr).expect("utf8");
        assert!(err.starts_with("mdfu: "), "stderr: {err}");
    }

    fn hit_indices(hits: Vec<FilterHit>) -> Vec<usize> {
        hits.into_iter().map(|hit| hit.index).collect()
    }

    #[test]
    fn pipeline_build_filter_fn_empty_query_returns_all_indices() {
        let items = vec![doc("a.md", "alpha"), doc("b.md", "beta")];
        let filter = build_filter_fn();
        assert_eq!(hit_indices(filter("", &items)), vec![0, 1]);
        assert_eq!(hit_indices(filter("   ", &items)), vec![0, 1]);
    }

    #[test]
    fn pipeline_build_filter_fn_parse_error_returns_all_indices() {
        let items = vec![doc("a.md", "alpha"), doc("b.md", "beta")];
        let filter = build_filter_fn();
        assert_eq!(hit_indices(filter("created:notadate", &items)), vec![0, 1]);
    }

    #[test]
    fn pipeline_build_filter_fn_ranks_matching_docs() {
        let items = vec![
            doc("a.md", "# Alpha\n\nkumquat zebra xylophone\n"),
            doc("b.md", "# Beta\n\nonly beta here\n"),
        ];
        let filter = build_filter_fn();
        let kumquat = filter("kumquat", &items);
        assert_eq!(hit_indices(kumquat.clone()), vec![0]);
        assert!(
            kumquat[0].matched_indexes.is_empty(),
            "body-only match carries no title positions"
        );
        let beta = filter("beta", &items);
        assert_eq!(hit_indices(beta.clone()), vec![1]);
        assert!(
            !beta[0].matched_indexes.is_empty(),
            "title match carries title positions"
        );
        assert_eq!(
            hit_indices(filter("kumquat beta", &items)),
            Vec::<usize>::new()
        );
    }
}
