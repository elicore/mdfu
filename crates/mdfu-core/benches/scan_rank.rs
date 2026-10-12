//! Timed performance harness for the PLAN 5k-file budget.
//!
//! Measures two indexless metrics over a deterministic synthetic vault:
//!
//! - **cold `scan+parse`**: a full [`scan::walk_markdown`] plus
//!   [`parse::parse_file`] of every discovered file, repeated once per run.
//! - **per-keystroke `query+rank`**: [`query::parse`] plus [`search::rank`]
//!   plus the picker's pointer-to-index map (the body of the CLI's injected
//!   filter), repeated per representative query.
//!
//! The harness exits non-zero when the corpus is empty or a run loses files;
//! a budget miss prints `MISS` but does **not** change the exit code
//! (recording, not gating: no index or daemon may be added to hit it).
//!
//! Run with `cargo bench -p mdfu-core --bench scan_rank` from the workspace root.
//! Environment overrides: `MDFU_BENCH_FILES`, `MDFU_BENCH_DIRS`,
//! `MDFU_BENCH_COLD_RUNS`, `MDFU_BENCH_KEYSTROKE_REPS`, `MDFU_BENCH_ROOT`,
//! `MDFU_BENCH_KEEP` (keep the vault), `MDFU_BENCH_JSON` (write machine JSON).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mdfu_core::model::Document;
use mdfu_core::parse;
use mdfu_core::query;
use mdfu_core::scan;
use mdfu_core::search;

/// Default synthetic corpus size (PLAN budget floor: 5k files).
const DEFAULT_FILES: usize = 5000;
/// Default vault fan-out (files are spread over this many directories).
const DEFAULT_DIRS: usize = 50;
/// Cold runs recorded for the p50/p95 percentiles.
const DEFAULT_COLD_RUNS: usize = 20;
/// Keystroke repetitions recorded per query.
const DEFAULT_KEYSTROKE_REPS: usize = 50;
/// Fixed PRNG seed (splitmix-style constant): the corpus is byte-identical
/// on every run.
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// `PLAN.md:20` cold `scan+parse` budget in milliseconds.
const COLD_BUDGET_MS: f64 = 500.0;
/// `PLAN.md:20` per-keystroke `query+rank` budget in milliseconds.
const KEYSTROKE_BUDGET_MS: f64 = 50.0;

/// Representative keystroke queries: a title-boosted bare word, a two-word
/// fuzzy AND, a hard `key:value` filter, a fuzzy-only subsequence, and a
/// mixed filter plus bare word.
const QUERY_CASES: &[&str] = &[
    "retention",
    "kumquat zebra",
    "type:metric",
    "rtntn",
    "status:draft perf",
];

/// Title vocabulary.
const TOPICS: &[&str] = &[
    "Launch",
    "Retention",
    "Cloud",
    "Metrics",
    "Pipeline",
    "Storage",
    "Review",
    "Runtime",
    "Quality",
    "Security",
    "Growth",
    "Support",
    "Design",
    "Roadmap",
    "Incident",
    "Costs",
];

/// Body vocabulary (lowercase prose words).
const WORDS: &[&str] = &[
    "the",
    "team",
    "reviewed",
    "the",
    "pipeline",
    "and",
    "recorded",
    "a",
    "short",
    "note",
    "about",
    "storage",
    "latency",
    "quality",
    "growth",
    "and",
    "costs",
    "for",
    "the",
    "next",
    "quarter",
    "with",
    "updates",
    "on",
    "runtime",
    "metrics",
    "security",
    "checks",
    "and",
    "a",
    "few",
    "open",
    "questions",
    "from",
    "support",
    "design",
    "reviews",
    "roadmap",
    "items",
    "incident",
    "follow",
    "ups",
    "and",
    "performance",
    "data",
    "collected",
    "during",
    "the",
    "week",
];

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("scan_rank bench: {error}");
            std::process::exit(1);
        }
    }
}

/// Runs generation, both measurements, and report emission.
fn run() -> Result<(), String> {
    let files = env_usize("MDFU_BENCH_FILES", DEFAULT_FILES);
    let dirs = env_usize("MDFU_BENCH_DIRS", DEFAULT_DIRS).max(1);
    let cold_runs = env_usize("MDFU_BENCH_COLD_RUNS", DEFAULT_COLD_RUNS).max(1);
    let reps = env_usize("MDFU_BENCH_KEYSTROKE_REPS", DEFAULT_KEYSTROKE_REPS).max(1);
    let keep = std::env::var_os("MDFU_BENCH_KEEP").is_some();
    let root = match std::env::var_os("MDFU_BENCH_ROOT") {
        Some(value) => PathBuf::from(value),
        None => std::env::temp_dir().join(format!("mdfu-bench-{}", std::process::id())),
    };
    let cleanup = Cleanup {
        path: root.clone(),
        active: !keep,
    };

    if files == 0 {
        return Err("empty corpus: MDFU_BENCH_FILES=0 produces no files".to_owned());
    }
    let corpus = generate_corpus(&root, files, dirs)?;
    let machine = machine_info();

    let scan_options = scan::Options {
        root: corpus.root.to_string_lossy().into_owned(),
        include_hidden: false,
        respect_gitignore: true,
        limit: 0,
    };
    let cold = measure_cold(&scan_options, corpus.files, cold_runs)?;
    let keystrokes = measure_keystrokes(&cold.docs, reps)?;

    let cold_verdict = verdict(cold.p95_ms, COLD_BUDGET_MS);
    let worst_keystroke = keystrokes
        .iter()
        .map(|stat| stat.p95_ms)
        .fold(0.0_f64, f64::max);
    let keystroke_verdict = verdict(worst_keystroke, KEYSTROKE_BUDGET_MS);

    print_report(&machine, &corpus, cold_runs, reps, &cold, &keystrokes);
    if let Some(path) = std::env::var_os("MDFU_BENCH_JSON") {
        let path = PathBuf::from(path);
        let json = json_document(
            &machine,
            &corpus,
            reps,
            &cold,
            &keystrokes,
            cold_verdict,
            keystroke_verdict,
        );
        fs::write(&path, json)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    drop(cleanup);
    Ok(())
}

/// Removes the generated vault on drop unless explicitly kept.
struct Cleanup {
    path: PathBuf,
    active: bool,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Deterministic xorshift64* generator (no external crates, fixed seed).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Any nonzero state works for xorshift; the constant is odd already.
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        let bound = u64::try_from(bound).unwrap_or(u64::MAX);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }
}

/// The generated vault plus its size accounting.
struct Corpus {
    root: PathBuf,
    files: usize,
    bytes: u64,
}

/// Writes `files` deterministic Markdown notes under `root`.
fn generate_corpus(root: &Path, files: usize, dirs: usize) -> Result<Corpus, String> {
    if root.exists() {
        fs::remove_dir_all(root)
            .map_err(|error| format!("cannot clean {}: {error}", root.display()))?;
    }
    for dir_index in 0..dirs.min(files) {
        let dir = root.join(format!("dir-{dir_index:02}"));
        fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    }
    let mut rng = Rng::new(SEED);
    let mut bytes = 0_u64;
    for index in 0..files {
        let dir = root.join(format!("dir-{:02}", index % dirs));
        let path = dir.join(format!("note-{index:05}.md"));
        let content = render_note(index, &mut rng);
        bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(0));
        fs::write(&path, content)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(Corpus {
        root: root.to_path_buf(),
        files,
        bytes,
    })
}

/// Renders one synthetic note: YAML frontmatter plus prose body.
fn render_note(index: usize, rng: &mut Rng) -> String {
    let topic = TOPICS[index % TOPICS.len()];
    let mut title = format!("{topic} notes {index:05}");
    if index.is_multiple_of(20) {
        title.push_str(" retention");
    }
    let doc_type = if index.is_multiple_of(10) {
        "metric"
    } else {
        "note"
    };
    let status = match index % 5 {
        0 => "draft",
        1 => "active",
        _ => "done",
    };
    let tag = if index.is_multiple_of(8) {
        "perf"
    } else {
        "notes"
    };
    let month = (index % 12) + 1;
    let day = (index % 27) + 1;
    let month_2 = ((index + 3) % 12) + 1;
    let day_2 = ((index + 11) % 27) + 1;

    let mut out = String::with_capacity(2400);
    let _ = writeln!(out, "---");
    let _ = writeln!(out, "type: {doc_type}");
    let _ = writeln!(out, "title: {title}");
    let _ = writeln!(out, "tags: [{tag}, vault]");
    let _ = writeln!(out, "status: {status}");
    let _ = writeln!(out, "created: 2026-{month:02}-{day:02}T09:30:00Z");
    let _ = writeln!(out, "updated: 2026-{month_2:02}-{day_2:02}T11:00:00Z");
    let _ = writeln!(
        out,
        "description: \"Synthetic {doc_type} {index:05} for the scan and rank harness.\""
    );
    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# {title}");
    let _ = writeln!(out);

    let paragraphs = 10 + rng.below(5);
    for _ in 0..paragraphs {
        let words = 18 + rng.below(18);
        for _ in 0..words {
            out.push_str(WORDS[rng.below(WORDS.len())]);
            out.push(' ');
        }
        out.push('\n');
    }
    if index.is_multiple_of(200) {
        out.push_str("\nkumquat rotates through the scan corpus.\n");
    }
    if index.is_multiple_of(173) {
        out.push_str("\nzebra stripes recorded during the walk.\n");
    }
    out
}

/// The cold-run samples plus the last loaded document set (reused for the
/// keystroke phase, so the corpus is parsed once for both metrics).
struct ColdRuns {
    docs: Vec<Document>,
    first_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

/// Measures full walk + parse of the vault `runs` times.
fn measure_cold(
    scan_options: &scan::Options,
    expected_files: usize,
    runs: usize,
) -> Result<ColdRuns, String> {
    let mut samples = Vec::with_capacity(runs);
    let mut first_ms = 0.0;
    let mut last_docs = Vec::new();
    for run_index in 0..runs {
        let start = Instant::now();
        let paths =
            scan::walk_markdown(scan_options).map_err(|error| format!("scan failed: {error}"))?;
        if paths.is_empty() {
            return Err(format!(
                "empty corpus: scan found 0 markdown files under {}",
                scan_options.root
            ));
        }
        let mut docs = Vec::with_capacity(paths.len());
        for path in &paths {
            if let Ok(doc) = parse::parse_file(path) {
                docs.push(doc);
            }
        }
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        if docs.is_empty() {
            return Err("empty corpus: parse loaded 0 documents".to_owned());
        }
        if docs.len() != expected_files {
            return Err(format!(
                "corpus mismatch: parsed {} of {expected_files} files",
                docs.len()
            ));
        }
        if run_index == 0 {
            first_ms = elapsed_ms;
        }
        samples.push(elapsed_ms);
        last_docs = docs;
    }
    samples.sort_by(f64::total_cmp);
    Ok(ColdRuns {
        docs: last_docs,
        first_ms,
        p50_ms: percentile_ms(&samples, 50, 100),
        p95_ms: percentile_ms(&samples, 95, 100),
        min_ms: samples.first().copied().unwrap_or(0.0),
        max_ms: samples.last().copied().unwrap_or(0.0),
    })
}

/// Per-query keystroke statistics.
struct QueryStat {
    query: String,
    hits: usize,
    p50_ms: f64,
    p95_ms: f64,
    parse_p50_us: f64,
    rank_p50_ms: f64,
}

/// Measures `query+rank` per keystroke for every [`QUERY_CASES`] entry.
fn measure_keystrokes(docs: &[Document], reps: usize) -> Result<Vec<QueryStat>, String> {
    let mut stats = Vec::with_capacity(QUERY_CASES.len());
    for query_text in QUERY_CASES {
        let probe = query::parse(query_text)
            .map_err(|error| format!("query {query_text:?} failed to parse: {error}"))?;
        let hits = rank_indices(docs, &probe).len();
        if hits == 0 {
            return Err(format!(
                "query {query_text:?} matched nothing; corpus and query set are out of sync"
            ));
        }
        let mut total_ms = Vec::with_capacity(reps);
        let mut parse_us = Vec::with_capacity(reps);
        let mut rank_ms = Vec::with_capacity(reps);
        for _ in 0..reps {
            let start = Instant::now();
            let parsed = query::parse(query_text)
                .map_err(|error| format!("query {query_text:?} failed to parse: {error}"))?;
            let after_parse = Instant::now();
            let ranked = search::rank(docs, Some(&parsed));
            let after_rank = Instant::now();
            let by_ptr: HashMap<*const Document, usize> = docs
                .iter()
                .enumerate()
                .map(|(index, doc)| (std::ptr::from_ref(doc), index))
                .collect();
            let indices: Vec<usize> = ranked
                .iter()
                .filter_map(|doc| by_ptr.get(&std::ptr::from_ref(*doc)).copied())
                .collect();
            let end = Instant::now();
            black_box(&indices);

            total_ms.push((end - start).as_secs_f64() * 1000.0);
            parse_us.push((after_parse - start).as_secs_f64() * 1_000_000.0);
            rank_ms.push((after_rank - after_parse).as_secs_f64() * 1000.0);
        }
        total_ms.sort_by(f64::total_cmp);
        parse_us.sort_by(f64::total_cmp);
        rank_ms.sort_by(f64::total_cmp);
        stats.push(QueryStat {
            query: (*query_text).to_owned(),
            hits,
            p50_ms: percentile_ms(&total_ms, 50, 100),
            p95_ms: percentile_ms(&total_ms, 95, 100),
            parse_p50_us: percentile_ms(&parse_us, 50, 100),
            rank_p50_ms: percentile_ms(&rank_ms, 50, 100),
        });
    }
    Ok(stats)
}

/// Mirrors the CLI's injected per-keystroke filter (parse + rank + index map).
fn rank_indices(docs: &[Document], parsed: &query::Query) -> Vec<usize> {
    let ranked = search::rank(docs, Some(parsed));
    let by_ptr: HashMap<*const Document, usize> = docs
        .iter()
        .enumerate()
        .map(|(index, doc)| (std::ptr::from_ref(doc), index))
        .collect();
    ranked
        .iter()
        .filter_map(|doc| by_ptr.get(&std::ptr::from_ref(*doc)).copied())
        .collect()
}

/// Nearest-rank percentile over a sorted sample slice.
fn percentile_ms(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    let len = sorted.len();
    if len == 0 {
        return 0.0;
    }
    let rank = numerator
        .saturating_mul(len)
        .saturating_add(denominator.saturating_sub(1))
        / denominator;
    sorted[rank.saturating_sub(1).min(len - 1)]
}

/// Budget verdict: `PASS` below the budget, `MISS` otherwise.
fn verdict(value_ms: f64, budget_ms: f64) -> &'static str {
    if value_ms < budget_ms {
        "PASS"
    } else {
        "MISS"
    }
}

/// Reads a positive `usize` environment override.
fn env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(value) => value.parse::<usize>().unwrap_or(default),
        Err(_) => default,
    }
}

/// Machine identification for the report.
struct Machine {
    cpu: String,
    threads: usize,
    mem_mib: u64,
    os: String,
    kernel: String,
    rustc: String,
}

/// Collects CPU, memory, OS, and toolchain identification.
fn machine_info() -> Machine {
    Machine {
        cpu: cpu_model(),
        threads: std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get),
        mem_mib: mem_total_mib().unwrap_or(0),
        os: std::env::consts::OS.to_owned(),
        kernel: command_first_line("uname", &["-r"]).unwrap_or_else(|| "unknown".to_owned()),
        rustc: command_first_line("rustc", &["-vV"]).unwrap_or_else(|| "unknown".to_owned()),
    }
}

/// First `model name` value of `/proc/cpuinfo`, or `unknown`.
fn cpu_model() -> String {
    let Ok(text) = fs::read_to_string("/proc/cpuinfo") else {
        return "unknown".to_owned();
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("model name") {
            if let Some((_, value)) = rest.split_once(':') {
                return value.trim().to_owned();
            }
        }
    }
    "unknown".to_owned()
}

/// `MemTotal` of `/proc/meminfo` in MiB, if readable.
fn mem_total_mib() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kib / 1024);
        }
    }
    None
}

/// First stdout line of a command, if it runs successfully.
fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}

/// Prints the human-readable report the `reports/` document is built from.
fn print_report(
    machine: &Machine,
    corpus: &Corpus,
    cold_runs: usize,
    reps: usize,
    cold: &ColdRuns,
    keystrokes: &[QueryStat],
) {
    let cold_verdict = verdict(cold.p95_ms, COLD_BUDGET_MS);
    let worst = keystrokes
        .iter()
        .map(|stat| stat.p95_ms)
        .fold(0.0_f64, f64::max);
    let keystroke_verdict = verdict(worst, KEYSTROKE_BUDGET_MS);
    let avg_kib = if corpus.files == 0 {
        0
    } else {
        corpus.bytes / u64::try_from(corpus.files).unwrap_or(1) / 1024
    };
    let mib = corpus.bytes / 1024 / 1024;

    println!("mdfu scan_rank perf harness");
    println!(
        "machine: {} | {} threads | {} MiB RAM | {} {} | {}",
        machine.cpu, machine.threads, machine.mem_mib, machine.os, machine.kernel, machine.rustc
    );
    println!(
        "corpus: {} files / {} bytes ({mib} MiB, avg {avg_kib} KiB) / seed 0x{SEED:016X} / root {}",
        corpus.files,
        corpus.bytes,
        corpus.root.display()
    );
    println!(
        "cold scan+parse: runs={cold_runs} first={:.3}ms p50={:.3}ms p95={:.3}ms min={:.3}ms max={:.3}ms",
        cold.first_ms, cold.p50_ms, cold.p95_ms, cold.min_ms, cold.max_ms
    );
    println!("  budget cold < {COLD_BUDGET_MS:.0}ms (p95): {cold_verdict}");
    println!("per-keystroke query+rank: reps={reps} (parse + rank + pointer-index map)");
    println!(
        "  {:<20} {:>7} {:>10} {:>10} {:>13} {:>12}",
        "query", "hits", "p50_ms", "p95_ms", "parse_p50_us", "rank_p50_ms"
    );
    for stat in keystrokes {
        println!(
            "  {:<20} {:>7} {:>10.3} {:>10.3} {:>13.2} {:>12.3}",
            stat.query, stat.hits, stat.p50_ms, stat.p95_ms, stat.parse_p50_us, stat.rank_p50_ms
        );
    }
    println!("  budget keystroke < {KEYSTROKE_BUDGET_MS:.0}ms (worst p95): {keystroke_verdict}");
    println!("verdict: cold {cold_verdict} | keystroke {keystroke_verdict}");
}

/// Machine-readable JSON twin of [`print_report`].
fn json_document(
    machine: &Machine,
    corpus: &Corpus,
    reps: usize,
    cold: &ColdRuns,
    keystrokes: &[QueryStat],
    cold_verdict: &str,
    keystroke_verdict: &str,
) -> String {
    let mut out = String::new();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"timestamp_unix\": {timestamp},");
    let _ = writeln!(out, "  \"machine\": {{");
    let _ = writeln!(out, "    \"cpu\": \"{}\",", json_escape(&machine.cpu));
    let _ = writeln!(out, "    \"threads\": {},", machine.threads);
    let _ = writeln!(out, "    \"mem_mib\": {},", machine.mem_mib);
    let _ = writeln!(out, "    \"os\": \"{}\",", json_escape(&machine.os));
    let _ = writeln!(out, "    \"kernel\": \"{}\",", json_escape(&machine.kernel));
    let _ = writeln!(out, "    \"rustc\": \"{}\"", json_escape(&machine.rustc));
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"corpus\": {{");
    let _ = writeln!(out, "    \"files\": {},", corpus.files);
    let _ = writeln!(out, "    \"bytes\": {},", corpus.bytes);
    let _ = writeln!(out, "    \"seed\": \"0x{SEED:016X}\",");
    let _ = writeln!(
        out,
        "    \"root\": \"{}\"",
        json_escape(&corpus.root.to_string_lossy())
    );
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"cold_scan_parse_ms\": {{");
    let _ = writeln!(out, "    \"first\": {:.3},", cold.first_ms);
    let _ = writeln!(out, "    \"p50\": {:.3},", cold.p50_ms);
    let _ = writeln!(out, "    \"p95\": {:.3},", cold.p95_ms);
    let _ = writeln!(out, "    \"min\": {:.3},", cold.min_ms);
    let _ = writeln!(out, "    \"max\": {:.3},", cold.max_ms);
    let _ = writeln!(out, "    \"budget_ms\": {COLD_BUDGET_MS:.0},");
    let _ = writeln!(out, "    \"verdict\": \"{cold_verdict}\"");
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"keystroke_query_rank\": {{");
    let _ = writeln!(out, "    \"reps\": {reps},");
    let _ = writeln!(out, "    \"budget_ms\": {KEYSTROKE_BUDGET_MS:.0},");
    let _ = writeln!(out, "    \"worst_p95_verdict\": \"{keystroke_verdict}\",");
    let _ = writeln!(out, "    \"queries\": [");
    for (index, stat) in keystrokes.iter().enumerate() {
        let comma = if index + 1 == keystrokes.len() {
            ""
        } else {
            ","
        };
        let _ = writeln!(
            out,
            "      {{\"query\": \"{}\", \"hits\": {}, \"p50_ms\": {:.3}, \"p95_ms\": {:.3}, \"parse_p50_us\": {:.2}, \"rank_p50_ms\": {:.3}}}{comma}",
            json_escape(&stat.query),
            stat.hits,
            stat.p50_ms,
            stat.p95_ms,
            stat.parse_p50_us,
            stat.rank_p50_ms
        );
    }
    let _ = writeln!(out, "    ]");
    let _ = writeln!(out, "  }}");
    let _ = writeln!(out, "}}");
    out
}

/// Escapes the characters JSON strings must not carry unescaped.
fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
