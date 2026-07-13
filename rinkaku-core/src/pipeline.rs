//! Wiring the diff parser, language registry, and symbol extractor into a
//! single pure pipeline.
//!
//! [`analyze_diff`] takes a diff's text and a `read_file` port for fetching
//! a changed file's new-side content, and produces a [`crate::render::Report`].
//! File reads are injected rather than performed here so this module stays
//! pure and testable: `main.rs` supplies a closure that reads the working
//! tree, tests supply a closure backed by an in-memory map.

use crate::deps::{Resolver, is_generated_content, resolve_dependencies};
use crate::diff::{ChangeKind, parse_unified_diff};
use crate::extract::{
    ExtractedSymbol, RemovedSymbol, classify_symbols, extract_all_symbols, extract_changed_symbols,
};
use crate::file_size::{compute_file_size_bands, compute_file_size_warnings};
use crate::graph::{build_graph, compute_fan_ins, stamp_ids};
use crate::language::{LanguageSupport, language_for_path};
use crate::progress::{OnProgress, should_report_progress};
use crate::render::{FileReport, Report, ReportOrigin, SkipReason, SkippedFile, TestFileSummary};
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

/// A `read_file`-shaped port for fetching a changed file's *base*-side
/// content (ADR 0014) — see `analyze_diff`'s `read_base_file` parameter.
/// Named so the parameter's type doesn't trip clippy's `type_complexity`
/// lint at the call site; the shape itself intentionally mirrors
/// `read_file`'s own `impl Fn(&str) -> std::io::Result<String>>`, just as a
/// trait object (`&dyn Fn`) so it can be threaded through as `Option<_>`,
/// which `impl Trait` cannot be.
pub type ReadBaseFile<'a> = &'a dyn Fn(&str) -> std::io::Result<String>;

/// Errors that can occur while running the pipeline.
#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error("failed to parse diff: {0}")]
    Diff(#[from] crate::diff::ParseError),
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Parses `diff_text` and extracts changed symbols from every file it can,
/// reading each file's new-side content through `read_file`.
///
/// Deleted files are skipped (there is no new-side content to read).
/// Binary files are skipped (no line-level diff to extract from). Files
/// with no registered [`crate::language::LanguageSupport`] for their
/// extension are skipped. All skips are recorded in the returned
/// [`Report`], never silently dropped.
///
/// Files with no changed line ranges (a pure rename or a mode-change-only
/// diff — no hunks) are *not* skipped, since they are supported and were
/// looked at; they are reported as a [`crate::render::FileReport`] with an
/// empty `symbols` list, and — unlike every other case above — `read_file`
/// is never called for them, since there is no content change to extract
/// symbols from.
///
/// `resolver`, when `Some`, is used to populate each extracted symbol's
/// `dependencies` (1-hop expansion, ADR 0003) via
/// [`crate::deps::resolve_dependencies`]. `None` skips dependency
/// resolution entirely — no `Resolver::resolve` calls are made — which is
/// how the CLI's `--deps 0` is wired (`main.rs`).
///
/// `include_tests` controls ADR 0009's test-symbol exclusion mechanism
/// (kept intact, though ADR 0025 flipped the CLI-facing default to
/// include tests and renamed the flag to `--exclude-tests`): `false`
/// (the CLI's `--exclude-tests`) drops every symbol a file's
/// [`crate::language::LanguageSupport`] considers a test — by path
/// ([`LanguageSupport::is_test_path`], the whole file) or by AST context
/// ([`ExtractedSymbol::is_test`], set per-definition during extraction) —
/// from `files` before dependency resolution and graph-building run, and
/// summarizes the excluded counts per file in the returned `Report`'s
/// `tests`. `true` (the CLI's new default) keeps every symbol in `files`
/// and leaves `tests` empty. Filtering happens before
/// `resolve_dependencies`/`build_graph` rather than at render time so test
/// symbols are excluded from the dependency graph and 1-hop resolution too,
/// not just hidden from the rendered "Change graph"/"Definitions" sections.
///
/// `generated_paths` (ADR 0010) is the set of changed paths `main.rs`
/// resolved as `-diff`/`linguist-generated` via `git check-attr` at the
/// process boundary — this module stays pure and never runs `git` itself,
/// so the set is computed by the caller and passed in as plain data, same
/// as `read_file`. A path in this set is reported as `SkipReason::Generated`
/// unless it was also deleted, in which case `SkipReason::Deleted` wins
/// (checked first): the fact that a file was removed is more important
/// information for a reviewer than an attribute the file no longer carries
/// any content for, and `read_file` is never called either way.
///
/// `include_generated` gates both `generated_paths` (the caller passes an
/// empty set when it's `false`, so this parameter does not duplicate that
/// gating — see `main.rs`'s `resolve_generated_paths`) and, newly, content
/// marker detection (ADR 0011): once a file's source is read (only reached
/// when neither `generated_paths` nor any earlier check already skipped
/// it), `false` runs [`is_generated_content`] over it before parsing and
/// reports `SkipReason::Generated` on a match instead of calling
/// `extract_changed_symbols`. `true` (`--include-generated`) skips this
/// check entirely, matching attribute-based skipping's own opt-out. No
/// local repository being available for `main.rs` to resolve
/// `generated_paths` against does not affect this check, since it only
/// needs file content, not `git check-attr`.
///
/// Known inefficiency: a changed file is parsed here (via
/// `extract_changed_symbols`) and, when `resolver` is `TagsResolver`,
/// parsed *again* while building that resolver's index
/// (`TagsResolver::new` calls `extract_all_symbols` over every tracked
/// file, changed files included). Measured as a minor contributor next to
/// the per-file `git show`/`git ls-files` subprocess cost `--base` mode
/// pays for indexing (see the performance note at the top of `deps.rs`),
/// so left unaddressed for now rather than adding a cache purely on
/// suspicion.
///
/// `read_base_file` (ADR 0014), when `Some`, is used the same way as
/// `read_file` but for a changed file's *base*-side content — mirroring
/// `read_file`'s own shape (a plain closure/fn port, not a trait object) so
/// the two ports read the same way at every call site. See
/// [`classify_against_base`]'s doc comment for the exact rules: a brand-new
/// file (`ChangeKind::Added`) classifies every symbol `Added` directly from
/// the diff's own knowledge, without ever calling `read_base_file` (there is
/// no base side to read); a renamed/copied file reads base content from
/// `old_path`, since that is where it actually lived on the base side, while
/// still reporting any `removed` symbols under the new-side `path`; every
/// other kind reads `path` itself. `None` — the pure-stdin-pipe case, where
/// no base commit is known — leaves every symbol's `classification` at its
/// default `None` ("not attempted") and `removed` empty. Beyond the
/// diff-attested `Added` case, this function never guesses a classification
/// from partial information: a `read_base_file` call failing (`Err`, e.g. a
/// transient git failure) is treated as "no base content available" for
/// that one file rather than propagated as an [`AnalyzeError`] or guessed
/// at.
///
/// `on_progress` (ADR 0033, amended), when `Some`, is called with
/// `(files_done, changed_files.len())` as this function's sequential
/// per-file loop below works through the diff's changed files —
/// approximately every [`crate::progress::PROGRESS_REPORT_STRIDE`] files
/// ([`crate::progress::should_report_progress`]), always including a final
/// `(total, total)` call. Unlike [`analyze_repo`]'s parallel loop, this
/// loop is already sequential, so a plain `usize` counter is incremented
/// in place — no `AtomicUsize` is needed. "Files done" counts every file
/// the loop looks at, including ones it skips (deleted/generated/binary/
/// unsupported-language), matching `analyze_repo`'s own "looked at" —not
/// "produced a report for"— convention so a caller watching the callback
/// sees the same meaning regardless of which pipeline entry point produced
/// it. `None` (every caller before this parameter existed) skips all
/// counting overhead, leaving behavior and output byte-for-byte unchanged.
#[allow(clippy::too_many_arguments)]
pub fn analyze_diff(
    diff_text: &str,
    read_file: impl Fn(&str) -> std::io::Result<String>,
    read_base_file: Option<ReadBaseFile>,
    resolver: Option<&dyn Resolver>,
    include_tests: bool,
    generated_paths: &std::collections::HashSet<String>,
    include_generated: bool,
    on_progress: Option<OnProgress>,
) -> Result<Report, AnalyzeError> {
    let changed_files = parse_unified_diff(diff_text)?;
    let total = changed_files.len();
    let mut done = 0usize;

    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut removed = Vec::new();
    // ADR 0028: `(path, line_count)` for every file the pipeline actually
    // reads content for, collected here rather than at the render layer so
    // skipped files (binary/generated/deleted/unsupported-language) are
    // excluded by construction — they have no content to measure, or are
    // explicitly outside rinkaku's concern.
    let mut sized_files: Vec<(String, usize)> = Vec::new();

    for changed_file in changed_files {
        // ADR 0033 (amended): the per-file body is wrapped in a labeled
        // block so every early exit below (`break 'file`, replacing what
        // used to be a bare `continue`) still falls through to the single
        // progress report at the bottom of the loop — "files done" counts
        // every changed file the loop looks at, including skipped ones,
        // matching `analyze_repo`'s own "looked at" convention (see this
        // function's doc comment).
        'file: {
            if changed_file.kind == ChangeKind::Deleted {
                skipped.push(SkippedFile {
                    path: changed_file.path,
                    reason: SkipReason::Deleted,
                });
                break 'file;
            }
            if generated_paths.contains(&changed_file.path) {
                skipped.push(SkippedFile {
                    path: changed_file.path,
                    reason: SkipReason::Generated,
                });
                break 'file;
            }
            if changed_file.is_binary {
                skipped.push(SkippedFile {
                    path: changed_file.path,
                    reason: SkipReason::Binary,
                });
                break 'file;
            }
            let Some(lang) = language_for_path(&changed_file.path) else {
                skipped.push(SkippedFile {
                    path: changed_file.path,
                    reason: SkipReason::UnsupportedLanguage,
                });
                break 'file;
            };

            // Base-side content lives at `old_path` for a rename/copy (the
            // pre-rename path — the new-side `path` never existed on the base
            // side under a rename, so reading it there would always fail), and
            // at `path` itself for every other kind.
            let read_path = changed_file
                .old_path
                .as_deref()
                .unwrap_or(&changed_file.path);

            // No new-side hunks means no new-side content change (a pure
            // rename, a mode-change-only diff, or — ADR 0014's case — a hunk
            // that only *removes* lines with nothing added back):
            // extract_changed_symbols would return no symbols for an empty
            // changed_ranges anyway, so the head-side read is skipped entirely
            // rather than paying IO for a result already known to be empty.
            // `old_changed_ranges` can still be non-empty in the removal case,
            // though, so classification against the base side still runs when
            // a base reader is available — a whole-function deletion is
            // exactly the case ADR 0014's `removed` classification exists for.
            if changed_file.changed_ranges.is_empty() {
                let mut no_head_symbols: Vec<ExtractedSymbol> = Vec::new();
                removed.extend(classify_against_base(
                    &mut no_head_symbols,
                    read_base_file,
                    lang,
                    changed_file.kind,
                    read_path,
                    &changed_file.path,
                    &changed_file.old_changed_ranges,
                ));
                files.push(FileReport {
                    path: changed_file.path,
                    symbols: Vec::new(),
                });
                break 'file;
            }

            let source =
                read_file(&changed_file.path).map_err(|source| AnalyzeError::ReadFile {
                    path: changed_file.path.clone(),
                    source,
                })?;
            // ADR 0011: content-marker detection, checked after the read but
            // before parsing — a file already excluded by an attribute
            // (generated_paths, above) never reaches here, so this only ever
            // adds coverage on top of ADR 0010, never duplicates it.
            if !include_generated && is_generated_content(&source) {
                skipped.push(SkippedFile {
                    path: changed_file.path,
                    reason: SkipReason::Generated,
                });
                break 'file;
            }
            // ADR 0028: measure line count once the file's content has cleared
            // every skip check above. `str::lines()` returns a sensible count
            // whether or not the final line ends in a newline.
            sized_files.push((changed_file.path.clone(), source.lines().count()));
            let mut symbols = extract_changed_symbols(&source, lang, &changed_file.changed_ranges);

            // ADR 0014: classify each symbol's contract impact against the
            // base side. `ChangeKind::Added` classifies every symbol `Added`
            // directly (see `classify_against_base`'s doc comment); every other
            // kind is left at `None`/empty (classify_symbols never runs) when
            // `read_base_file` is absent or its call fails for this file.
            removed.extend(classify_against_base(
                &mut symbols,
                read_base_file,
                lang,
                changed_file.kind,
                read_path,
                &changed_file.path,
                &changed_file.old_changed_ranges,
            ));

            files.push(FileReport {
                path: changed_file.path,
                symbols,
            });
        }

        if let Some(on_progress) = on_progress {
            done += 1;
            if should_report_progress(done, total) {
                on_progress(done, total);
            }
        }
    }

    let mut tests = Vec::new();
    if !include_tests {
        (files, tests) = partition_test_symbols(files);
    }

    let mut files = match resolver {
        Some(resolver) => resolve_dependencies(files, resolver),
        None => files,
    };

    // Built last, over the final `files`: the graph's node IDs must match
    // whatever symbols actually end up in the report (dependency
    // resolution does not add/remove/reorder symbols, but building the
    // graph from the post-resolution list rather than an intermediate one
    // avoids relying on that invariant holding forever).
    let graph = build_graph(&files);
    stamp_ids(&mut files, &graph);
    // Computed from the same final `graph` as everything else above, so a
    // fan-in entry's `used_by` names always match the stamped ids/nodes
    // (ADR 0013).
    let fan_ins = compute_fan_ins(&graph);
    // ADR 0028: file-size warnings from the `(path, line_count)` pairs
    // collected inline above during the per-file read loop.
    let file_size_warnings = compute_file_size_warnings(&sized_files);
    // ADR 0028 amendment: every file's band, from the same pairs.
    let file_size_bands = compute_file_size_bands(&sized_files);

    Ok(Report {
        origin: ReportOrigin::Diff,
        files,
        skipped,
        graph,
        tests,
        fan_ins,
        file_size_warnings,
        file_size_bands,
        removed,
    })
}

/// Builds a whole-repository outline [`Report`] directly from file
/// contents, bypassing the diff pipeline entirely (ADR 0017): every symbol
/// in every supported, non-test, non-generated file is reported, rather
/// than only the symbols touching a diff's changed lines.
///
/// `paths` is the file list (`main.rs` supplies `git ls-files`'s output;
/// tests supply a fixed list), and `read_file` fetches each path's content
/// — the same read-file-port shape `analyze_diff` uses, keeping this
/// module IO-free. A `read_file` failure for one path is treated as "skip
/// this file" (best-effort, matching `build_resolver`'s working-tree
/// branch in `main.rs`) rather than failing the whole run: a whole-repo
/// listing is a best-effort aid, and one unreadable path (e.g. a submodule
/// gitlink entry `git ls-files` still lists) should not blank out the
/// entire outline.
///
/// No `resolver`/dependency-resolution parameter: ADR 0017 explicitly
/// skips 1-hop expansion here, since every symbol in the repository is
/// already in scope — there is no "elsewhere in the repo" left to expand
/// into. `classification` is left `None` on every symbol, the same value
/// stdin mode already uses when no base commit is known (`analyze_diff`'s
/// own `read_base_file: None` case): nothing changed, so there is no base
/// side to classify against, and `Added` would misrepresent every symbol
/// in the repository as new.
///
/// Per-file filtering mirrors [`crate::deps::TagsResolver::new`]'s
/// pre-filter exactly, applied in the same order, so a symbol excluded
/// from the whole-repo outline is excluded from the dependency index for
/// the same reason a reviewer would expect: unsupported language (no
/// [`crate::language::LanguageSupport`] for the extension) skips the file
/// silently — not reported as [`SkippedFile`] at all, since an outline of
/// "the whole repo" naturally excludes languages rinkaku doesn't parse,
/// same as `analyze_diff`'s `SkipReason::UnsupportedLanguage` records a
/// diff's unsupported files but nothing here would gain from repeating
/// that per-file for every unrelated non-source file in a repository (a
/// `SkippedFile` entry only exists in `analyze_diff` because the file was
/// *touched by the diff*, which has no analogue here); a whole-file test
/// path ([`crate::language::LanguageSupport::is_test_path`]) or a
/// generated file (`generated_paths` or [`is_generated_content`], ADR
/// 0010/0011) is skipped from `files` entirely, gated by `include_tests`/
/// `include_generated` respectively, same as `analyze_diff`. Within a
/// file that passes those checks, individual AST-detected test symbols
/// (`ExtractedSymbol::is_test`) are additionally dropped, gated by
/// `include_tests` — matching `partition_test_symbols`'s per-symbol
/// filtering exactly, just applied before a `FileReport` is built rather
/// than after (there is no test-only file case to summarize into
/// `Report::tests` beyond what per-file skipping already handles, so
/// `tests` stays empty; a repo-wide "how many test symbols exist" summary
/// was judged non-essential for v1's outline use case — see ADR 0017). A
/// file left with zero symbols after that per-symbol filtering (every
/// definition in it was test code, or it had no definitions at all) is
/// dropped from `files` entirely rather than kept as an empty
/// `FileReport` — unlike `analyze_diff`, there is no "pure rename with
/// nothing to report but still worth noting" case here (ADR 0017: this
/// mode has no diff, so no rename), so an empty entry would only ever
/// mean "nothing here", simplest left out of the outline.
///
/// Uses [`extract_all_symbols`] (the same function
/// `crate::deps::TagsResolver::new` uses to build its repo-wide index)
/// rather than `extract_changed_symbols`, since there is no changed-range
/// concept to filter by. Unlike `extract_changed_symbols`, this does not
/// suppress a nested definition in favor of its narrowest enclosing one
/// (e.g. a Rust `impl` block containing a touched method) — an outline
/// wants every definition, matching how `TagsResolver`'s index already
/// treats nesting (both a container and its members are independently
/// indexable), and `container` on each symbol already records the nesting
/// relationship for renderers/the TUI to use.
///
/// `files`/`graph`/`fan_ins` are built the same way `analyze_diff` builds
/// them (`build_graph`, `stamp_ids`, `compute_fan_ins`), so every
/// downstream renderer (Markdown, JSON, TUI) sees the same `Report` shape
/// regardless of which pipeline entry point produced it.
///
/// `on_progress` (ADR 0033), when `Some`, is called with `(files_done,
/// paths.len())` as files finish being processed by the parallel loop
/// below — approximately every [`crate::progress::PROGRESS_REPORT_STRIDE`]
/// files (`crate::progress::should_report_progress`), always including a
/// final `(paths.len(), paths.len())` call. `None` (every caller except
/// `--tui` mode's `main.rs`) skips all counting overhead: the atomic
/// counter is only incremented when a callback is actually present, so
/// existing callers pay nothing for this parameter.
pub fn analyze_repo(
    paths: &[String],
    read_file: impl Fn(&str) -> std::io::Result<String> + Sync + Send,
    include_tests: bool,
    generated_paths: &std::collections::HashSet<String>,
    include_generated: bool,
    on_progress: Option<OnProgress>,
) -> Report {
    // ADR 0031: the per-file body below is embarrassingly parallel —
    // `extract_all_symbols` builds a fresh `tree_sitter::Parser` per call
    // (see `extract::with_definition_nodes`), the `read_file` port is
    // `Sync + Send` (bound tightened above), and every filter reads
    // borrowed state (`generated_paths`, `include_*` flags) without
    // mutation, so rayon can fan the work across CPU cores without any
    // shared-mutable-state hazard. `par_iter().collect::<Vec<_>>()`
    // preserves source order, so `files`/`sized_files` end up in the same
    // order the sequential loop produced (locked in by
    // `should_produce_deterministic_output_on_repeated_calls`).
    //
    // Each per-file body returns `Option<PerFileOutcome>`: the outer
    // `None` covers every "skip this path" branch (unsupported language,
    // test path, generated attribute, unreadable file, generated content
    // marker); `PerFileOutcome::report` is `None` when the file's content
    // was successfully read (so its size entry is kept) but every
    // extracted symbol was filtered out (so no report is emitted).
    // Splitting the two lets `sized_files` include size-warning
    // candidates whose symbols were all tests without adding a phantom
    // `FileReport` for them — matching the sequential loop's
    // `sized_files.push` then `if symbols.is_empty() { continue; }` order
    // exactly.
    struct PerFileOutcome {
        sized: (String, usize),
        report: Option<FileReport>,
    }
    // ADR 0033: counts files as they finish, regardless of which branch
    // below a given file took (skipped, filtered to empty, or reported) —
    // "progress" here means "files the loop has looked at", matching what
    // a reviewer watching a `done/total` bar expects it to track, not just
    // the subset that ended up producing a `FileReport`. `AtomicUsize`
    // rather than a per-thread-local counter: rayon's worker threads share
    // this one counter, and `fetch_add`'s return value (the count *before*
    // this increment) is turned into a 1-indexed "files done" count with
    // `+ 1` so `should_report_progress` sees the same 1-indexed convention
    // `TagsResolver::new`'s sequential loop below also uses.
    let completed = AtomicUsize::new(0);
    let total = paths.len();
    let per_file: Vec<Option<PerFileOutcome>> = paths
        .par_iter()
        .map(|path| {
            let outcome = (|| {
                let lang = language_for_path(path)?;
                if !include_tests && lang.is_test_path(path) {
                    return None;
                }
                if !include_generated && generated_paths.contains(path) {
                    return None;
                }
                // A path `git ls-files` lists can still fail to read (e.g. a
                // submodule gitlink entry, or a file deleted in the working
                // tree but not yet staged) — skipped rather than aborting the
                // whole outline, same best-effort stance `main.rs`'s
                // `build_resolver` already takes for its own working-tree read
                // loop.
                let content = read_file(path).ok()?;
                if !include_generated && is_generated_content(&content) {
                    return None;
                }
                let sized = (path.clone(), content.lines().count());

                let symbols: Vec<ExtractedSymbol> = extract_all_symbols(&content, lang)
                    .into_iter()
                    .filter(|symbol| include_tests || !symbol.is_test)
                    .collect();
                let report = if symbols.is_empty() {
                    None
                } else {
                    Some(FileReport {
                        path: path.clone(),
                        symbols,
                    })
                };
                Some(PerFileOutcome { sized, report })
            })();

            if let Some(on_progress) = on_progress {
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if should_report_progress(done, total) {
                    on_progress(done, total);
                }
            }

            outcome
        })
        .collect();

    // ADR 0028: same collection strategy as `analyze_diff` — record every
    // file whose content actually got read past the per-file filters, so
    // filtered-out files (unsupported language, test path, generated) are
    // never measured.
    let mut sized_files: Vec<(String, usize)> = Vec::with_capacity(per_file.len());
    let mut files: Vec<FileReport> = Vec::with_capacity(per_file.len());
    for outcome in per_file.into_iter().flatten() {
        sized_files.push(outcome.sized);
        if let Some(report) = outcome.report {
            files.push(report);
        }
    }

    let graph = build_graph(&files);
    stamp_ids(&mut files, &graph);
    let fan_ins = compute_fan_ins(&graph);
    let file_size_warnings = compute_file_size_warnings(&sized_files);
    let file_size_bands = compute_file_size_bands(&sized_files);

    Report {
        origin: ReportOrigin::RepoOutline,
        files,
        skipped: Vec::new(),
        graph,
        tests: Vec::new(),
        fan_ins,
        file_size_warnings,
        file_size_bands,
        removed: Vec::new(),
    }
}

/// Shared by both places in `analyze_diff`'s loop that need ADR 0014
/// classification for one file: the ordinary case (`head_symbols` already
/// extracted from a non-empty `changed_ranges`) and the "removal-only hunk"
/// case (`head_symbols` empty by construction, since there was no new-side
/// content to extract from — see `analyze_diff`'s doc comment).
///
/// `ChangeKind::Added` is special-cased using the diff's own knowledge
/// rather than an IO outcome: an added file has no base side *by
/// construction* (git itself says so via the `new file mode`/`+++ b/...`
/// header this parsed from), so every one of `head_symbols` is classified
/// `Added` directly, without ever calling `read_base_file` — there is
/// nothing to read, and a base commit that happened to independently
/// contain a same-path file (e.g. a re-add after an unrelated delete)
/// must not be confused for this file's own history. Every other kind
/// (`Modified`, `Renamed`, `Copied`) reads `read_path`'s content via
/// `read_base_file`, extracts every base symbol via
/// [`extract_all_symbols`], and runs [`classify_symbols`] to set
/// `head_symbols`' classification fields in place and collect this file's
/// removed symbols. `read_path` and `report_path` are split because a
/// rename/copy's base content lives at the pre-rename path
/// (`changed_file.old_path`) while a removed symbol should still be
/// reported under the file's current, new-side path — the one path a
/// reviewer looking at this diff actually has open — not the path history
/// happens to read the comparison content from.
///
/// For every non-`Added` kind, a `read_base_file` call failing (`None` port,
/// or an `Err` result — e.g. a transient git failure, or a rename/copy
/// resolving to a base path this repository never actually had) leaves
/// `head_symbols` untouched (`classification` stays `None`, "not
/// attempted") rather than guessing — ADR 0014's "never guess" contract
/// applies to every case except the diff-attested `Added` one above, which
/// isn't a guess at all.
///
/// No-ops (returns an empty `Vec`, `head_symbols` untouched) when
/// `old_changed_ranges` is empty and `head_symbols` is also empty — a pure
/// optimization (nothing from `classify_symbols` could possibly result),
/// sparing a base-content read/parse that would otherwise be pure waste
/// for e.g. a mode-change-only diff.
#[allow(clippy::too_many_arguments)]
fn classify_against_base(
    head_symbols: &mut [ExtractedSymbol],
    read_base_file: Option<ReadBaseFile>,
    lang: &dyn LanguageSupport,
    kind: ChangeKind,
    read_path: &str,
    report_path: &str,
    old_changed_ranges: &[crate::diff::LineRange],
) -> Vec<RemovedSymbol> {
    if kind == ChangeKind::Added {
        for symbol in head_symbols.iter_mut() {
            symbol.classification = Some(crate::extract::Classification::Added);
        }
        return Vec::new();
    }

    if head_symbols.is_empty() && old_changed_ranges.is_empty() {
        return Vec::new();
    }
    let Some(read_base_file) = read_base_file else {
        return Vec::new();
    };
    let Ok(base_source) = read_base_file(read_path) else {
        return Vec::new();
    };
    let base_symbols = extract_all_symbols(&base_source, lang);
    classify_symbols(head_symbols, &base_symbols, old_changed_ranges, report_path)
}

/// Splits `files` into (non-test symbols, per-file test-symbol counts) for
/// ADR 0009's default test-symbol exclusion. A symbol is a test if its
/// file's [`LanguageSupport::is_test_path`] says the whole file is a test
/// file, or if [`ExtractedSymbol::is_test`] says so by AST context (Rust's
/// `#[cfg(test)]`/`#[test]`, set during extraction).
///
/// A file that had symbols before filtering but ends up with none after
/// (every symbol it changed was a test) is dropped from the returned
/// `files` entirely — it contributes only a [`TestFileSummary`], not an
/// empty `FileReport` (which would otherwise render under "Other changed
/// files" as if it were an uninteresting pure rename, which it is not). A
/// file that already had no symbols *before* filtering (a genuine pure
/// rename, see `analyze_diff`'s doc comment) is left alone and still kept,
/// since filtering removed nothing from it.
fn partition_test_symbols(files: Vec<FileReport>) -> (Vec<FileReport>, Vec<TestFileSummary>) {
    let mut kept = Vec::new();
    let mut tests = Vec::new();

    for file in files {
        let had_symbols = !file.symbols.is_empty();
        let is_test_path = language_for_path(&file.path)
            .is_some_and(|lang: &dyn LanguageSupport| lang.is_test_path(&file.path));

        let (non_test, test): (Vec<ExtractedSymbol>, Vec<ExtractedSymbol>) = if is_test_path {
            (Vec::new(), file.symbols)
        } else {
            file.symbols.into_iter().partition(|symbol| !symbol.is_test)
        };

        if !test.is_empty() {
            tests.push(TestFileSummary {
                path: file.path.clone(),
                symbol_count: test.len(),
            });
        }
        // Drop the file only if filtering actually emptied it — a file
        // that had no symbols to begin with (pure rename) must stay.
        if !had_symbols || !non_test.is_empty() {
            kept.push(FileReport {
                path: file.path,
                symbols: non_test,
            });
        }
    }

    (kept, tests)
}

/// Parses `diff_text` and collects every name referenced by any changed
/// symbol across every changed file, reading each file's new-side content
/// through `read_file` — the same walk `analyze_diff` performs, but
/// stopping at `extract_changed_symbols` instead of going on to resolve
/// dependencies or build a `Report`.
///
/// Exists so `main.rs` can compute the reference-name set a `TagsResolver`
/// needs for its prefilter (`TagsResolver::new`'s `reference_names`
/// parameter, see `deps.rs`'s performance doc comment) *before*
/// constructing that resolver, which `analyze_diff` itself cannot do since
/// it takes the resolver as an input rather than building one. This means
/// the diff is parsed and changed files are read/parsed twice per run
/// (once here, once inside `analyze_diff`) — the same known double-parse
/// tradeoff `analyze_diff`'s doc comment already accepts for
/// `TagsResolver::new`'s own indexing pass, extended to this walk too.
///
/// Deleted, binary, and unsupported-language files are skipped exactly as
/// in `analyze_diff` (no names to collect from them). Files with no
/// changed ranges (pure renames) are also skipped without reading, same
/// rationale as `analyze_diff`.
pub fn collect_referenced_names(
    diff_text: &str,
    read_file: impl Fn(&str) -> std::io::Result<String>,
) -> Result<std::collections::HashSet<String>, AnalyzeError> {
    let changed_files = parse_unified_diff(diff_text)?;
    let mut names = std::collections::HashSet::new();

    for changed_file in changed_files {
        if changed_file.kind == ChangeKind::Deleted || changed_file.is_binary {
            continue;
        }
        let Some(lang) = language_for_path(&changed_file.path) else {
            continue;
        };
        if changed_file.changed_ranges.is_empty() {
            continue;
        }

        let source = read_file(&changed_file.path).map_err(|source| AnalyzeError::ReadFile {
            path: changed_file.path.clone(),
            source,
        })?;
        for symbol in extract_changed_symbols(&source, lang, &changed_file.changed_ranges) {
            names.extend(symbol.referenced_names);
        }
    }

    Ok(names)
}

// ADR 0028: tests split into `pipeline_tests/` by responsibility so this
// production file stays under the file-size warn threshold. See
// `pipeline_tests/mod.rs` for the topic layout.
#[cfg(test)]
#[path = "pipeline_tests/mod.rs"]
mod tests;
