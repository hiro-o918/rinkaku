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
use crate::file_size::compute_file_size_warnings;
use crate::graph::{build_graph, compute_hotspots, stamp_ids};
use crate::language::{LanguageSupport, language_for_path};
use crate::render::{FileReport, Report, ReportOrigin, SkipReason, SkippedFile, TestFileSummary};
use rayon::prelude::*;
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
pub fn analyze_diff(
    diff_text: &str,
    read_file: impl Fn(&str) -> std::io::Result<String>,
    read_base_file: Option<ReadBaseFile>,
    resolver: Option<&dyn Resolver>,
    include_tests: bool,
    generated_paths: &std::collections::HashSet<String>,
    include_generated: bool,
) -> Result<Report, AnalyzeError> {
    let changed_files = parse_unified_diff(diff_text)?;

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
        if changed_file.kind == ChangeKind::Deleted {
            skipped.push(SkippedFile {
                path: changed_file.path,
                reason: SkipReason::Deleted,
            });
            continue;
        }
        if generated_paths.contains(&changed_file.path) {
            skipped.push(SkippedFile {
                path: changed_file.path,
                reason: SkipReason::Generated,
            });
            continue;
        }
        if changed_file.is_binary {
            skipped.push(SkippedFile {
                path: changed_file.path,
                reason: SkipReason::Binary,
            });
            continue;
        }
        let Some(lang) = language_for_path(&changed_file.path) else {
            skipped.push(SkippedFile {
                path: changed_file.path,
                reason: SkipReason::UnsupportedLanguage,
            });
            continue;
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
            continue;
        }

        let source = read_file(&changed_file.path).map_err(|source| AnalyzeError::ReadFile {
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
            continue;
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
    // hotspot's `used_by` names always match the stamped ids/nodes
    // (ADR 0013).
    let hotspots = compute_hotspots(&graph);
    // ADR 0028: file-size warnings from the `(path, line_count)` pairs
    // collected inline above during the per-file read loop.
    let file_size_warnings = compute_file_size_warnings(&sized_files);

    Ok(Report {
        origin: ReportOrigin::Diff,
        files,
        skipped,
        graph,
        tests,
        hotspots,
        file_size_warnings,
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
/// `files`/`graph`/`hotspots` are built the same way `analyze_diff` builds
/// them (`build_graph`, `stamp_ids`, `compute_hotspots`), so every
/// downstream renderer (Markdown, JSON, TUI) sees the same `Report` shape
/// regardless of which pipeline entry point produced it.
pub fn analyze_repo(
    paths: &[String],
    read_file: impl Fn(&str) -> std::io::Result<String> + Sync + Send,
    include_tests: bool,
    generated_paths: &std::collections::HashSet<String>,
    include_generated: bool,
) -> Report {
    // ADR 0029: the per-file body below is embarrassingly parallel —
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
    let per_file: Vec<Option<PerFileOutcome>> = paths
        .par_iter()
        .map(|path| {
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
    let hotspots = compute_hotspots(&graph);
    let file_size_warnings = compute_file_size_warnings(&sized_files);

    Report {
        origin: ReportOrigin::RepoOutline,
        files,
        skipped: Vec::new(),
        graph,
        tests: Vec::new(),
        hotspots,
        file_size_warnings,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::{ExtractedSymbol, SymbolKind};
    use pretty_assertions::assert_eq;
    use std::collections::{HashMap, HashSet};

    /// Builds a `read_file` port backed by an in-memory map, so tests never
    /// touch the real filesystem.
    fn fake_reader(
        files: HashMap<&'static str, &'static str>,
    ) -> impl Fn(&str) -> std::io::Result<String> {
        move |path: &str| {
            files
                .get(path)
                .map(|s| s.to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path.to_string()))
        }
    }

    /// An empty `SymbolGraph`, for tests where no changed symbols exist.
    fn empty_graph() -> crate::graph::SymbolGraph {
        crate::graph::SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        }
    }

    #[test]
    fn should_return_empty_report_when_diff_is_empty() {
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: empty_graph(),
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff("", read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_symbols_when_diff_touches_a_rust_file() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
        let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
        let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    id: "src/lib.rs::foo".to_string(),
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo(a: i32) -> i32".to_string(),
                    range: LineRange { start: 1, end: 3 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                    is_test: false,
                    classification: None,
                    previous_signature: None,
                }],
            }],
            skipped: vec![],
            graph: crate::graph::SymbolGraph {
                nodes: vec![crate::graph::Node {
                    id: "src/lib.rs::foo".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "foo".to_string(),
                }],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_deleted_file_without_reading_it() {
        let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 4b825dc..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn a() {}
-fn b() {}
";
        // No entry in the map: if the pipeline tried to read a deleted
        // file, this would return an Err and fail the test.
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![SkippedFile {
                path: "src/old.rs".to_string(),
                reason: SkipReason::Deleted,
            }],
            graph: empty_graph(),
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_binary_file_without_reading_it() {
        let diff = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: empty_graph(),
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_file_with_unsupported_language_without_reading_it() {
        // `.rb` has no registered `LanguageSupport` (only rs/go/py/ts/tsx
        // are registered — see `language.rs`), so this exercises the
        // unsupported-extension path without relying on an extension that
        // might gain support later.
        let diff = "\
diff --git a/src/main.rb b/src/main.rb
index e69de29..4b825dc 100644
--- a/src/main.rb
+++ b/src/main.rb
@@ -1,1 +1,2 @@
 def foo
+  1
";
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![SkippedFile {
                path: "src/main.rb".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
            graph: empty_graph(),
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    // Regression test: a pure rename (or a mode-change-only diff) has no
    // hunks, so `changed_ranges` is empty and there is no content change to
    // extract symbols from. The pipeline must not call `read_file` for such
    // an entry — doing so is wasted IO for content that, by construction,
    // yields no symbols (`extract_changed_symbols` already returns `[]` for
    // an empty `changed_ranges`). Reported as a `FileReport` with empty
    // `symbols` rather than a `SkippedFile`: the file *is* supported and
    // was looked at, it just has nothing to report, which is a different
    // situation from `SkipReason`'s "could not be analyzed" cases.
    #[test]
    fn should_skip_reading_pure_rename_with_no_changed_ranges() {
        let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 100%
rename from src/old_name.rs
rename to src/new_name.rs
";
        // No entry in the map: if the pipeline tried to read the renamed
        // file, this would return an Err and fail the test.
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            }],
            skipped: vec![],
            graph: empty_graph(),
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_err_when_diff_is_malformed() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 1,4 @@
 fn a() {}
";
        let read_file = fake_reader(HashMap::new());

        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true);

        assert!(matches!(actual, Err(AnalyzeError::Diff(_))));
    }

    #[test]
    fn should_return_err_when_read_file_fails() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,1 @@
-fn a() {}
+fn a() -> i32 { 0 }
";
        // Map has no entry for src/lib.rs, so the fake reader returns Err.
        let read_file = fake_reader(HashMap::new());

        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true);

        assert!(matches!(
            actual,
            Err(AnalyzeError::ReadFile { path, .. }) if path == "src/lib.rs"
        ));
    }

    #[test]
    fn should_process_multiple_files_with_mixed_outcomes_in_one_diff() {
        // `.rb` has no registered `LanguageSupport` (see the note on
        // `should_skip_file_with_unsupported_language_without_reading_it`).
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,1 @@
-fn a() {}
+fn a() -> i32 { 0 }
diff --git a/src/main.rb b/src/main.rb
index e69de29..4b825dc 100644
--- a/src/main.rb
+++ b/src/main.rb
@@ -1,1 +1,2 @@
 def foo
+  1
";
        let source = "fn a() -> i32 { 0 }\n";
        let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

        let expected = Report {
            origin: ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    id: "src/lib.rs::a".to_string(),
                    name: "a".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn a() -> i32".to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                    is_test: false,
                    classification: None,
                    previous_signature: None,
                }],
            }],
            skipped: vec![SkippedFile {
                path: "src/main.rb".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
            graph: crate::graph::SymbolGraph {
                nodes: vec![crate::graph::Node {
                    id: "src/lib.rs::a".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "a".to_string(),
                }],
                edges: vec![],
                roots: vec!["src/lib.rs::a".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    /// End-to-end regression test for ADR 0012 decision 2: a Go interface
    /// and a same-named receiver method that both change in one diff must
    /// render as a single tree (the method nested under the interface) in
    /// the "Change graph" section, not as two duplicate top-level roots —
    /// see the ADR's "listed twice" problem statement. Runs through the
    /// whole pipeline (`analyze_diff` then `render::render`) rather than
    /// building a `Report`/`SymbolGraph` by hand, since the point is to
    /// prove the real `Repo` interface's `referenced_names` (populated by
    /// `GoSupport::reference_query`) actually produces the edge, not to
    /// exercise `render.rs`'s formatting in isolation.
    #[test]
    fn should_nest_go_receiver_method_under_its_interface_when_both_change_in_one_diff() {
        let diff = "\
diff --git a/repo.go b/repo.go
index e69de29..4b825dc 100644
--- a/repo.go
+++ b/repo.go
@@ -1,10 +1,10 @@
 package main

 type Repo interface {
-	Save(id string) error
+	Save(id string) (err error)
 }

 type repoImpl struct{}

 func (r *repoImpl) Save(id string) error {
-	return errors.New(\"not implemented\")
+	return nil
 }
";
        let source = "\
package main

type Repo interface {
	Save(id string) (err error)
}

type repoImpl struct{}

func (r *repoImpl) Save(id string) error {
	return nil
}
";
        let read_file = fake_reader(HashMap::from([("repo.go", source)]));

        let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");
        let markdown = crate::render::render(&report, crate::render::OutputFormat::Markdown)
            .expect("markdown render should succeed");

        let expected = "\
## Change graph

2 changed symbols in 1 file

- interface Repo (repo.go)
  - fn Save (repo.go)

## Definitions

### interface Repo (repo.go)

```
Repo interface { Save(id string) (err error) }
```

### fn Save (repo.go)

```
// repoImpl
func (r *repoImpl) Save(id string) error
```

"
        .to_string();

        assert_eq!(expected, markdown);
    }

    /// A [`Resolver`] test double that records every name it was asked to
    /// resolve, so `--deps 0`'s "resolver is never called" contract can be
    /// verified directly rather than inferred from empty `dependencies`
    /// (which could also mean "called but found nothing").
    struct CountingResolver {
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl CountingResolver {
        fn new() -> Self {
            Self {
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl crate::deps::Resolver for CountingResolver {
        fn resolve(&self, name: &str) -> Vec<crate::deps::ResolvedSymbol> {
            self.calls.borrow_mut().push(name.to_string());
            Vec::new()
        }
    }

    #[test]
    fn should_not_call_resolver_when_resolver_is_none() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(p: Point) -> i32 {
-    0
+    helper(p)
 }
";
        let source = "\
fn foo(p: Point) -> i32 {
    helper(p)
}
";
        let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

        let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        // No resolver was passed, so every symbol's dependencies must stay
        // empty — this is `--deps 0`'s contract (main.rs), not merely "the
        // resolver found nothing".
        let expected: Vec<crate::deps::ResolvedSymbol> = Vec::new();
        let actual = report.files[0].symbols[0].dependencies.clone();

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_call_resolver_for_each_referenced_name_when_resolver_is_some() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(p: Point) -> i32 {
-    0
+    helper(p)
 }
";
        let source = "\
fn foo(p: Point) -> i32 {
    helper(p)
}
";
        let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
        let resolver = CountingResolver::new();

        analyze_diff(
            diff,
            read_file,
            None,
            Some(&resolver),
            true,
            &HashSet::new(),
            true,
        )
        .expect("analyze should succeed");

        let mut expected = vec!["Point".to_string(), "helper".to_string()];
        let mut actual = resolver.calls.borrow().clone();
        expected.sort();
        actual.sort();

        assert_eq!(expected, actual);
    }

    mod is_generated_content_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        // Real-world SQLBoiler header (Go ORM code generator).
        #[case::should_detect_sqlboiler_go_header(
            "// Code generated by SQLBoiler 4.19.5 (https://github.com/aarondl/sqlboiler). DO NOT EDIT.\n\npackage models\n",
            true
        )]
        // protobuf-generated Go, no tool URL.
        #[case::should_detect_protobuf_style_header(
            "// Code generated by protoc-gen-go. DO NOT EDIT.\n// versions:\n// \tprotoc-gen-go v1.28.0\n\npackage pb\n",
            true
        )]
        // Shell/Python-style `#` comment instead of Go's `//`.
        #[case::should_detect_hash_comment_header(
            "#!/usr/bin/env python3\n# Code generated by codegen. DO NOT EDIT.\n\nimport sys\n",
            true
        )]
        // Facebook-style bare marker, no "Code generated" wording at all.
        #[case::should_detect_at_generated_marker("// @generated\n\npackage models\n", true)]
        #[case::should_return_false_when_marker_is_on_line_six_or_later(
            "line1\nline2\nline3\nline4\nline5\n// Code generated by tool. DO NOT EDIT.\n",
            false
        )]
        #[case::should_return_false_when_code_generated_present_without_do_not_edit(
            "// Code generated by tool.\n\npackage models\n",
            false
        )]
        #[case::should_return_false_when_content_has_no_marker_at_all(
            "fn foo() -> i32 {\n    1\n}\n",
            false
        )]
        fn is_generated_content_cases(#[case] content: &str, #[case] expected: bool) {
            let actual = is_generated_content(content);

            assert_eq!(expected, actual);
        }

        // Regression case pinning down the exact case sensitivity ADR 0011
        // specifies (matches linguist's own casing): a differently-cased
        // marker must not match.
        #[test]
        fn should_return_false_when_do_not_edit_casing_does_not_match() {
            let content = "// Code generated by tool. do not edit.\n";

            let actual = is_generated_content(content);

            assert!(!actual);
        }

        #[test]
        fn should_return_true_when_marker_is_exactly_on_the_fifth_line() {
            let content = "line1\nline2\nline3\nline4\n// @generated\n";

            let actual = is_generated_content(content);

            assert!(actual);
        }
    }

    mod test_symbol_exclusion_tests {
        use super::*;
        use crate::render::TestFileSummary;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_exclude_rust_symbol_from_files_and_summarize_it_when_include_tests_is_false() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
 #[test]
 fn should_add_two_numbers() {
-    assert_eq!(1, 1 + 0);
+    assert_eq!(2, 1 + 1);
 }
";
            let source = "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let report = analyze_diff(diff, read_file, None, None, false, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected_files: Vec<FileReport> = Vec::new();
            let expected_tests = vec![TestFileSummary {
                path: "src/lib.rs".to_string(),
                symbol_count: 1,
            }];
            assert_eq!(expected_files, report.files);
            assert_eq!(expected_tests, report.tests);
        }

        #[test]
        fn should_keep_test_symbol_in_files_and_leave_tests_empty_when_include_tests_is_true() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
 #[test]
 fn should_add_two_numbers() {
-    assert_eq!(1, 1 + 0);
+    assert_eq!(2, 1 + 1);
 }
";
            let source = "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let expected = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![ExtractedSymbol {
                        id: "src/lib.rs::should_add_two_numbers".to_string(),
                        name: "should_add_two_numbers".to_string(),
                        kind: SymbolKind::Function,
                        signature: "fn should_add_two_numbers()".to_string(),
                        range: LineRange { start: 2, end: 4 },
                        container: None,
                        referenced_names: vec![],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: true,
                        classification: None,
                        previous_signature: None,
                    }],
                }],
                skipped: vec![],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![crate::graph::Node {
                        id: "src/lib.rs::should_add_two_numbers".to_string(),
                        path: "src/lib.rs".to_string(),
                        name: "should_add_two_numbers".to_string(),
                    }],
                    edges: vec![],
                    roots: vec!["src/lib.rs::should_add_two_numbers".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_drop_whole_file_from_files_when_go_test_file_has_only_test_symbols() {
            let diff = "\
diff --git a/repo_test.go b/repo_test.go
index e69de29..4b825dc 100644
--- a/repo_test.go
+++ b/repo_test.go
@@ -1,5 +1,5 @@
 package main

 func TestFoo(t *testing.T) {
-	old()
+	new_()
 }
";
            let source = "\
package main

func TestFoo(t *testing.T) {
	new_()
}
";
            let read_file = fake_reader(HashMap::from([("repo_test.go", source)]));

            let report = analyze_diff(diff, read_file, None, None, false, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected_files: Vec<FileReport> = Vec::new();
            let expected_tests = vec![TestFileSummary {
                path: "repo_test.go".to_string(),
                symbol_count: 1,
            }];
            assert_eq!(expected_files, report.files);
            assert_eq!(expected_tests, report.tests);
        }

        // Regression test: a genuine pure rename produces a `FileReport`
        // with an empty `symbols` list *before* test filtering ever runs
        // (see `analyze_diff`'s doc comment) — that emptiness has nothing
        // to do with tests, so `partition_test_symbols` must not drop it
        // the same way it drops a file that became empty *because of*
        // filtering (the Go all-test-file case above). Dropping it here
        // would wrongly hide it from "Other changed files".
        #[test]
        fn should_keep_file_with_no_symbols_when_it_was_a_pure_rename_not_a_test_file() {
            let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 100%
rename from src/old_name.rs
rename to src/new_name.rs
";
            let read_file = fake_reader(HashMap::new());

            let report = analyze_diff(diff, read_file, None, None, false, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected_files = vec![FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            }];
            let expected_tests: Vec<TestFileSummary> = Vec::new();
            assert_eq!(expected_files, report.files);
            assert_eq!(expected_tests, report.tests);
        }

        #[test]
        fn should_keep_non_test_symbols_and_summarize_test_symbols_when_file_mixes_both() {
            // A Rust file with one production function and one
            // `#[cfg(test)] mod tests` function both changed in the same
            // diff — the production symbol stays in `files`, the test
            // symbol is summarized in `tests`, and the file is not dropped
            // entirely (unlike the all-test-file case above).
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,9 +1,9 @@
 fn add(a: i32, b: i32) -> i32 {
-    a - b
+    a + b
 }

 #[cfg(test)]
 mod tests {
     #[test]
     fn should_add_two_numbers() {
-        assert_eq!(2, add(1, 1));
+        assert_eq!(3, add(1, 2));
     }
 }
";
            let source = "\
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_add_two_numbers() {
        assert_eq!(3, add(1, 2));
    }
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let expected = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![ExtractedSymbol {
                        id: "src/lib.rs::add".to_string(),
                        name: "add".to_string(),
                        kind: SymbolKind::Function,
                        signature: "fn add(a: i32, b: i32) -> i32".to_string(),
                        range: LineRange { start: 1, end: 3 },
                        container: None,
                        referenced_names: vec![],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: false,
                        classification: None,
                        previous_signature: None,
                    }],
                }],
                skipped: vec![],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![crate::graph::Node {
                        id: "src/lib.rs::add".to_string(),
                        path: "src/lib.rs".to_string(),
                        name: "add".to_string(),
                    }],
                    edges: vec![],
                    roots: vec!["src/lib.rs::add".to_string()],
                },
                tests: vec![TestFileSummary {
                    path: "src/lib.rs".to_string(),
                    symbol_count: 1,
                }],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_diff(diff, read_file, None, None, false, &HashSet::new(), true)
                .expect("analyze should succeed");

            assert_eq!(expected, actual);
        }
    }

    mod generated_path_exclusion_tests {
        use super::*;
        use crate::render::{SkipReason, SkippedFile};
        use pretty_assertions::assert_eq;

        #[test]
        fn should_skip_path_as_generated_when_in_generated_paths_set() {
            let diff = "\
diff --git a/Cargo.lock b/Cargo.lock
index e69de29..4b825dc 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,1 +1,1 @@
-version = 1
+version = 2
";
            // No entry in the map: if the pipeline tried to read a
            // generated file, this would return an Err and fail the test.
            let read_file = fake_reader(HashMap::new());
            let generated_paths: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();

            let report = analyze_diff(diff, read_file, None, None, true, &generated_paths, true)
                .expect("analyze should succeed");

            let expected = vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }];
            assert_eq!(expected, report.skipped);
        }

        // Regression test: a file that is both deleted and marked
        // generated (e.g. a lockfile removed from a repo that also
        // declares it `-diff`) must be reported as `Deleted`, not
        // `Generated` — the fact that the file was removed is more
        // important information for a reviewer than the (now moot)
        // attribute it used to carry, and `Deleted` already carries no
        // content to read either way.
        #[test]
        fn should_report_deleted_reason_when_a_deleted_path_is_also_marked_generated() {
            let diff = "\
diff --git a/Cargo.lock b/Cargo.lock
deleted file mode 100644
index 4b825dc..0000000
--- a/Cargo.lock
+++ /dev/null
@@ -1,1 +0,0 @@
-version = 1
";
            // No entry in the map: if the pipeline tried to read a deleted
            // file, this would return an Err and fail the test.
            let read_file = fake_reader(HashMap::new());
            let generated_paths: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();

            let report = analyze_diff(diff, read_file, None, None, true, &generated_paths, true)
                .expect("analyze should succeed");

            let expected = vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Deleted,
            }];
            assert_eq!(expected, report.skipped);
        }

        #[test]
        fn should_not_skip_path_when_generated_paths_set_is_empty() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected: Vec<SkippedFile> = Vec::new();
            assert_eq!(expected, report.skipped);
        }
    }

    mod generated_content_exclusion_tests {
        use super::*;
        use crate::render::{SkipReason, SkippedFile};
        use pretty_assertions::assert_eq;

        #[test]
        fn should_skip_file_as_generated_when_content_has_code_generated_do_not_edit_marker() {
            let diff = "\
diff --git a/models/user.go b/models/user.go
index e69de29..4b825dc 100644
--- a/models/user.go
+++ b/models/user.go
@@ -1,1 +1,1 @@
-package models
+package models // updated
";
            let source = "\
// Code generated by SQLBoiler 4.19.5 (https://github.com/aarondl/sqlboiler). DO NOT EDIT.

package models
";
            let read_file = fake_reader(HashMap::from([("models/user.go", source)]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), false)
                .expect("analyze should succeed");

            let expected_files: Vec<FileReport> = Vec::new();
            let expected_skipped = vec![SkippedFile {
                path: "models/user.go".to_string(),
                reason: SkipReason::Generated,
            }];
            assert_eq!(expected_files, report.files);
            assert_eq!(expected_skipped, report.skipped);
        }

        #[test]
        fn should_not_skip_file_as_generated_when_include_generated_is_true() {
            let diff = "\
diff --git a/models/user.go b/models/user.go
index e69de29..4b825dc 100644
--- a/models/user.go
+++ b/models/user.go
@@ -1,3 +1,3 @@
 // Code generated by SQLBoiler 4.19.5. DO NOT EDIT.

-func Foo() int { return 1 }
+func Foo() int { return 2 }
";
            let source = "\
// Code generated by SQLBoiler 4.19.5. DO NOT EDIT.

func Foo() int { return 2 }
";
            let read_file = fake_reader(HashMap::from([("models/user.go", source)]));

            let expected = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "models/user.go".to_string(),
                    symbols: vec![ExtractedSymbol {
                        id: "models/user.go::Foo".to_string(),
                        name: "Foo".to_string(),
                        kind: SymbolKind::Function,
                        signature: "func Foo() int".to_string(),
                        range: LineRange { start: 3, end: 3 },
                        container: None,
                        referenced_names: vec!["int".to_string()],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: false,
                        classification: None,
                        previous_signature: None,
                    }],
                }],
                skipped: vec![],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![crate::graph::Node {
                        id: "models/user.go::Foo".to_string(),
                        path: "models/user.go".to_string(),
                        name: "Foo".to_string(),
                    }],
                    edges: vec![],
                    roots: vec!["models/user.go::Foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_not_skip_ordinary_file_with_no_generated_marker() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let expected = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![ExtractedSymbol {
                        id: "src/lib.rs::foo".to_string(),
                        name: "foo".to_string(),
                        kind: SymbolKind::Function,
                        signature: "fn foo(a: i32) -> i32".to_string(),
                        range: LineRange { start: 1, end: 3 },
                        container: None,
                        referenced_names: vec![],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: false,
                        classification: None,
                        previous_signature: None,
                    }],
                }],
                skipped: vec![],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![crate::graph::Node {
                        id: "src/lib.rs::foo".to_string(),
                        path: "src/lib.rs".to_string(),
                        name: "foo".to_string(),
                    }],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), false)
                .expect("analyze should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_only_the_generated_file_when_diff_touches_both_kinds() {
            let diff = "\
diff --git a/models/user.go b/models/user.go
index e69de29..4b825dc 100644
--- a/models/user.go
+++ b/models/user.go
@@ -1,1 +1,1 @@
-// stale
+// Code generated by tool. DO NOT EDIT.
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let generated_source = "// Code generated by tool. DO NOT EDIT.\n\npackage models\n";
            let normal_source = "fn foo(a: i32) -> i32 {\n    a + 1\n}\n";
            let read_file = fake_reader(HashMap::from([
                ("models/user.go", generated_source),
                ("src/lib.rs", normal_source),
            ]));

            let expected = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![ExtractedSymbol {
                        id: "src/lib.rs::foo".to_string(),
                        name: "foo".to_string(),
                        kind: SymbolKind::Function,
                        signature: "fn foo(a: i32) -> i32".to_string(),
                        range: LineRange { start: 1, end: 3 },
                        container: None,
                        referenced_names: vec![],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: false,
                        classification: None,
                        previous_signature: None,
                    }],
                }],
                skipped: vec![SkippedFile {
                    path: "models/user.go".to_string(),
                    reason: SkipReason::Generated,
                }],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![crate::graph::Node {
                        id: "src/lib.rs::foo".to_string(),
                        path: "src/lib.rs".to_string(),
                        name: "foo".to_string(),
                    }],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), false)
                .expect("analyze should succeed");

            assert_eq!(expected, actual);
        }

        // Regression test: an attribute-based generated_paths match must
        // take priority and skip the file before its content is ever read
        // — content-marker detection is purely additive coverage on top of
        // ADR 0010's attribute-based skipping, not a second independent
        // check that could disagree with it.
        #[test]
        fn should_not_read_file_content_when_already_skipped_by_generated_paths() {
            let diff = "\
diff --git a/Cargo.lock b/Cargo.lock
index e69de29..4b825dc 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,1 +1,1 @@
-version = 1
+version = 2
";
            // No entry in the map: if the pipeline tried to read this file
            // (to run the content-marker check), this would return an Err
            // and fail the test.
            let read_file = fake_reader(HashMap::new());
            let generated_paths: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();

            let report = analyze_diff(diff, read_file, None, None, true, &generated_paths, false)
                .expect("analyze should succeed");

            let expected = vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }];
            assert_eq!(expected, report.skipped);
        }
    }

    mod hotspots_tests {
        use super::*;
        use crate::graph::Hotspot;
        use pretty_assertions::assert_eq;

        // ADR 0013 end-to-end: two changed functions ("caller_one",
        // "caller_two") both call "shared_helper" in the same file — fan-in
        // 2 qualifies "shared_helper" as a hotspot, and `analyze_diff` must
        // populate `Report::hotspots` from the graph it builds, not leave
        // it empty.
        //
        // NOTE: asserts only `report.hotspots` instead of the whole
        // `Report` — files/graph/tests wiring is already covered by the
        // surrounding analyze_diff tests, and this module's concern is
        // solely that the hotspot aggregation is hooked up.
        #[test]
        fn should_populate_hotspots_when_diff_has_a_symbol_with_fan_in_of_two() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,11 +1,11 @@
 fn shared_helper() -> i32 {
-    0
+    1
 }

 fn caller_one() -> i32 {
-    0
+    shared_helper()
 }

 fn caller_two() -> i32 {
-    0
+    shared_helper()
 }
";
            let source = "\
fn shared_helper() -> i32 {
    1
}

fn caller_one() -> i32 {
    shared_helper()
}

fn caller_two() -> i32 {
    shared_helper()
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected = vec![Hotspot {
                id: "src/lib.rs::shared_helper".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared_helper".to_string(),
                used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
            }];
            assert_eq!(expected, report.hotspots);
        }

        // NOTE: partial assert on `report.hotspots` only, same rationale
        // as the test above.
        #[test]
        fn should_return_empty_hotspots_when_no_node_has_fan_in_of_two() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected: Vec<Hotspot> = Vec::new();
            assert_eq!(expected, report.hotspots);
        }
    }

    mod classification_wiring_tests {
        use super::*;
        use crate::extract::{Classification, RemovedSymbol};
        use pretty_assertions::assert_eq;

        // ADR 0014 end-to-end: a signature-changing edit on a Rust function,
        // with base content supplied via `read_base_file`, must set the
        // reported symbol's `classification`/`previous_signature` — proves
        // `analyze_diff` actually wires `classify_symbols` into the
        // pipeline, not just that the pure function itself works (already
        // covered by `extract::tests::classification_tests`).
        #[test]
        fn should_classify_symbol_as_signature_changed_when_base_file_reader_is_some() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-fn foo(a: i32) -> i32 {
+fn foo(a: i32, b: i32) -> i32 {
     a
 }
";
            let base_source = "\
fn foo(a: i32) -> i32 {
    a
}
";
            let head_source = "\
fn foo(a: i32, b: i32) -> i32 {
    a
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", head_source)]));
            let read_base_file = fake_reader(HashMap::from([("src/lib.rs", base_source)]));

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(
                Some(Classification::SignatureChanged),
                symbol.classification
            );
            assert_eq!(
                Some("fn foo(a: i32) -> i32".to_string()),
                symbol.previous_signature
            );
        }

        // Without a base reader (stdin-pipe mode's contract), classification
        // must stay `None` — "not attempted" — rather than defaulting to
        // some guessed value.
        #[test]
        fn should_leave_classification_none_when_read_base_file_is_none() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(None, symbol.classification);
            assert_eq!(None, symbol.previous_signature);
        }

        // A base symbol removed entirely (no head-side match, and its
        // base-side range overlaps the diff's old-side hunk range) must
        // surface in `report.removed`.
        #[test]
        fn should_populate_removed_when_a_base_symbol_has_no_head_side_match() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-fn old_name() -> i32 {
+fn new_name() -> i32 {
     1
 }
";
            let base_source = "\
fn old_name() -> i32 {
    1
}
";
            let head_source = "\
fn new_name() -> i32 {
    1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", head_source)]));
            let read_base_file = fake_reader(HashMap::from([("src/lib.rs", base_source)]));

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let expected = vec![RemovedSymbol {
                name: "old_name".to_string(),
                kind: crate::extract::SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn old_name() -> i32".to_string(),
            }];
            assert_eq!(expected, report.removed);
        }

        // Regression test: a hunk that only *removes* lines (no `+` lines
        // at all — e.g. an entire function deleted from a file that also
        // has other, untouched content) produces an empty new-side
        // `changed_ranges`. Before this fix, `analyze_diff` treated that
        // the same as a pure rename (no content change at all) and skipped
        // straight past classification, so a whole-function deletion could
        // never be reported as `removed` — exactly the case ADR 0014's
        // `removed` classification exists for.
        #[test]
        fn should_populate_removed_when_a_hunk_only_removes_lines_with_no_additions() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,7 +1,3 @@
 fn kept() -> i32 {
     1
 }
-
-fn old_helper() -> i32 {
-    2
-}
";
            let base_source = "\
fn kept() -> i32 {
    1
}

fn old_helper() -> i32 {
    2
}
";
            let head_source = "\
fn kept() -> i32 {
    1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", head_source)]));
            let read_base_file = fake_reader(HashMap::from([("src/lib.rs", base_source)]));

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let expected = vec![RemovedSymbol {
                name: "old_helper".to_string(),
                kind: crate::extract::SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn old_helper() -> i32".to_string(),
            }];
            assert_eq!(expected, report.removed);
            // The file itself still reports as having no (head-side)
            // symbols, same as any other empty-changed_ranges file.
            let expected_files = vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![],
            }];
            assert_eq!(expected_files, report.files);
        }

        // A brand-new file (`ChangeKind::Added`) must classify every symbol
        // `Added` using the diff's own knowledge (a `new file mode`/
        // `+++ b/...` header already says there is no base side), not by
        // attempting a base read and treating the resulting failure as
        // "unknown" — `read_base_file` here has no entry for the path at
        // all, so if it were ever called this test would still pass
        // classification as `Added` only by accident of the fallback
        // behavior; the dedicated regression test below
        // (`should_never_call_read_base_file_for_an_added_file`) pins that
        // `read_base_file` is not called at all for this kind.
        #[test]
        fn should_classify_as_added_when_file_is_brand_new() {
            let diff = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..4b825dc 100644
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,3 @@
+fn foo() -> i32 {
+    1
+}
";
            let source = "\
fn foo() -> i32 {
    1
}
";
            let read_file = fake_reader(HashMap::from([("src/new.rs", source)]));
            // No entry for "src/new.rs": proves classification does not
            // depend on this port succeeding for an Added file.
            let read_base_file = fake_reader(HashMap::new());

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(Some(Classification::Added), symbol.classification);
        }

        // Regression test: `classify_against_base` must special-case
        // `ChangeKind::Added` by classifying directly from the diff's own
        // knowledge, never by calling `read_base_file` and interpreting an
        // IO failure — a `read_base_file` that panics if called proves it
        // genuinely never runs for this file, rather than merely happening
        // to return `Err` the way `fake_reader` over an empty map would.
        #[test]
        fn should_never_call_read_base_file_for_an_added_file() {
            let diff = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..4b825dc 100644
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,3 @@
+fn foo() -> i32 {
+    1
+}
";
            let source = "\
fn foo() -> i32 {
    1
}
";
            let read_file = fake_reader(HashMap::from([("src/new.rs", source)]));
            let read_base_file = |_: &str| -> std::io::Result<String> {
                panic!("must not be called for an Added file")
            };

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(Some(Classification::Added), symbol.classification);
        }

        // Sibling case: a `Modified` file (unlike `Added`) has no
        // diff-attested "no base side" fact to fall back on, so a
        // `read_base_file` failure here (a transient git failure, in
        // practice) must still leave classification unattempted rather than
        // guessing — ADR 0014's "never guess" contract, preserved for every
        // kind except the diff-attested `Added` case above.
        #[test]
        fn should_leave_classification_none_when_modified_files_base_read_errs() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
            let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
            // No entry for "src/lib.rs": the base reader errs for this
            // path, same as a real `git show <base>:src/lib.rs` failing.
            let read_base_file = fake_reader(HashMap::new());

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(None, symbol.classification);
        }

        // ADR 0014: a renamed file's base content lives at `old_path`, not
        // at the new-side `path` (which never existed on the base side
        // under a rename) — `read_base_file` must be called with
        // `old_path`, not `path`, so a signature change survives the rename
        // and still classifies as `signature_changed`.
        #[test]
        fn should_classify_as_signature_changed_when_renamed_file_has_a_signature_change() {
            let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 90%
rename from src/old_name.rs
rename to src/new_name.rs
index e69de29..4b825dc 100644
--- a/src/old_name.rs
+++ b/src/new_name.rs
@@ -1,3 +1,3 @@
-fn foo(a: i32) -> i32 {
+fn foo(a: i32, b: i32) -> i32 {
     a
 }
";
            let base_source = "\
fn foo(a: i32) -> i32 {
    a
}
";
            let head_source = "\
fn foo(a: i32, b: i32) -> i32 {
    a
}
";
            let read_file = fake_reader(HashMap::from([("src/new_name.rs", head_source)]));
            // Keyed by the *old* path: proves `read_base_file` is called
            // with `old_path`, not the new-side `path` (which would miss
            // here, since there is no "src/new_name.rs" entry at all).
            let read_base_file = fake_reader(HashMap::from([("src/old_name.rs", base_source)]));

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let symbol = &report.files[0].symbols[0];
            assert_eq!(
                Some(Classification::SignatureChanged),
                symbol.classification
            );
            assert_eq!(
                Some("fn foo(a: i32) -> i32".to_string()),
                symbol.previous_signature
            );
        }

        // Sibling case: a symbol present at the old path but no longer
        // present after the rename (e.g. the rename hunk also deletes a
        // second function outright) must be reported as `removed`, under
        // the file's new-side path — the path a reviewer looking at this
        // diff actually has open, not the pre-rename path the comparison
        // content happened to be read from.
        #[test]
        fn should_report_removed_when_symbol_at_old_path_is_gone_after_rename() {
            let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 60%
rename from src/old_name.rs
rename to src/new_name.rs
index e69de29..4b825dc 100644
--- a/src/old_name.rs
+++ b/src/new_name.rs
@@ -1,7 +1,3 @@
 fn kept() -> i32 {
     1
 }
-
-fn old_helper() -> i32 {
-    2
-}
";
            let base_source = "\
fn kept() -> i32 {
    1
}

fn old_helper() -> i32 {
    2
}
";
            let head_source = "\
fn kept() -> i32 {
    1
}
";
            let read_file = fake_reader(HashMap::from([("src/new_name.rs", head_source)]));
            let read_base_file = fake_reader(HashMap::from([("src/old_name.rs", base_source)]));

            let report = analyze_diff(
                diff,
                read_file,
                Some(&read_base_file),
                None,
                true,
                &HashSet::new(),
                true,
            )
            .expect("analyze should succeed");

            let expected = vec![RemovedSymbol {
                name: "old_helper".to_string(),
                kind: crate::extract::SymbolKind::Function,
                path: "src/new_name.rs".to_string(),
                signature: "fn old_helper() -> i32".to_string(),
            }];
            assert_eq!(expected, report.removed);
        }
    }

    mod collect_referenced_names_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_collect_names_referenced_by_changed_symbols() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(p: Point) -> i32 {
-    0
+    helper(p)
 }
";
            let source = "\
fn foo(p: Point) -> i32 {
    helper(p)
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

            let expected: std::collections::HashSet<String> =
                ["Point".to_string(), "helper".to_string()]
                    .into_iter()
                    .collect();
            let actual =
                collect_referenced_names(diff, read_file).expect("collection should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_empty_set_when_diff_is_empty() {
            let read_file = fake_reader(HashMap::new());

            let expected: std::collections::HashSet<String> = std::collections::HashSet::new();
            let actual =
                collect_referenced_names("", read_file).expect("collection should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_deleted_file_without_reading_it() {
            let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 4b825dc..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn a() {}
-fn b() {}
";
            // No entry in the map: if this tried to read a deleted file,
            // it would return Err and fail the test.
            let read_file = fake_reader(HashMap::new());

            let expected: std::collections::HashSet<String> = std::collections::HashSet::new();
            let actual =
                collect_referenced_names(diff, read_file).expect("collection should succeed");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_err_when_diff_is_malformed() {
            let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 1,4 @@
 fn a() {}
";
            let read_file = fake_reader(HashMap::new());

            let actual = collect_referenced_names(diff, read_file);

            assert!(matches!(actual, Err(AnalyzeError::Diff(_))));
        }
    }

    mod analyze_repo_tests {
        use super::*;
        use crate::extract::SymbolKind;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_return_empty_report_when_paths_is_empty() {
            let read_file = fake_reader(HashMap::new());

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&[], read_file, true, &HashSet::new(), true);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_every_symbol_when_file_has_no_changes_to_speak_of() {
            // Unlike `analyze_diff`, there is no diff here at all — every
            // symbol in the file is reported, not just ones touching a
            // changed line, since there is no changed-line concept in this
            // mode (ADR 0017).
            let source = "\
fn helper(x: i32) -> i32 {
    x
}

struct Point {
    x: i32,
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
            let paths = vec!["src/lib.rs".to_string()];

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![
                        ExtractedSymbol {
                            id: "src/lib.rs::helper".to_string(),
                            name: "helper".to_string(),
                            kind: SymbolKind::Function,
                            signature: "fn helper(x: i32) -> i32".to_string(),
                            range: LineRange { start: 1, end: 3 },
                            container: None,
                            referenced_names: vec![],
                            dependencies: vec![],
                            omitted_dependency_matches: 0,
                            is_test: false,
                            classification: None,
                            previous_signature: None,
                        },
                        ExtractedSymbol {
                            id: "src/lib.rs::Point".to_string(),
                            name: "Point".to_string(),
                            kind: SymbolKind::Struct,
                            signature: "struct Point { x: i32, }".to_string(),
                            range: LineRange { start: 5, end: 7 },
                            container: None,
                            referenced_names: vec!["Point".to_string()],
                            dependencies: vec![],
                            omitted_dependency_matches: 0,
                            is_test: false,
                            classification: None,
                            previous_signature: None,
                        },
                    ],
                }],
                skipped: vec![],
                graph: crate::graph::SymbolGraph {
                    nodes: vec![
                        crate::graph::Node {
                            id: "src/lib.rs::helper".to_string(),
                            path: "src/lib.rs".to_string(),
                            name: "helper".to_string(),
                        },
                        crate::graph::Node {
                            id: "src/lib.rs::Point".to_string(),
                            path: "src/lib.rs".to_string(),
                            name: "Point".to_string(),
                        },
                    ],
                    edges: vec![],
                    roots: vec![
                        "src/lib.rs::helper".to_string(),
                        "src/lib.rs::Point".to_string(),
                    ],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            assert_eq!(expected, actual);
        }

        // No `classification: Some(Added)`: ADR 0017's whole point is that
        // whole-repo mode must not mistake "nothing changed" for "every
        // symbol was just added" the way a synthetic empty-tree diff would.
        #[test]
        fn should_leave_classification_none_for_every_symbol() {
            let source = "fn foo() -> i32 { 1 }\n";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
            let paths = vec!["src/lib.rs".to_string()];

            let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            let expected: Option<crate::extract::Classification> = None;
            let actual = report.files[0].symbols[0].classification;
            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_path_without_registered_language_support() {
            // `.rb` has no registered `LanguageSupport` (see the note on
            // `should_skip_file_with_unsupported_language_without_reading_it`
            // above) — silently excluded from the outline, no `SkippedFile`
            // entry (unlike `analyze_diff`, there is no diff-touched file to
            // report a skip reason for; see `analyze_repo`'s doc comment).
            let read_file = fake_reader(HashMap::new());
            let paths = vec!["src/main.rb".to_string()];

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_path_when_read_file_fails() {
            // No entry in the map for this path: `read_file` returns `Err`,
            // which `analyze_repo` treats as best-effort "skip this file"
            // rather than failing the whole run (see its own doc comment).
            let read_file = fake_reader(HashMap::new());
            let paths = vec!["src/lib.rs".to_string()];

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_path_in_generated_paths_set_without_reading_it() {
            // No entry in the map: if `analyze_repo` tried to read a
            // generated file, this would return `Err` and (being treated as
            // best-effort) silently produce the same empty result either
            // way — so this test also pins that the file is excluded
            // *before* any read is attempted, matching
            // `TagsResolver::new`'s check ordering (deps.rs).
            let read_file = fake_reader(HashMap::new());
            let paths = vec!["Cargo.lock".to_string()];
            let generated_paths: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, true, &generated_paths, true);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_skip_file_with_generated_content_marker_when_include_generated_is_false() {
            let source =
                "// Code generated by SQLBoiler 4.19.5. DO NOT EDIT.\n\nfn foo() -> i32 { 1 }\n";
            let read_file = fake_reader(HashMap::from([("models/user.rs", source)]));
            let paths = vec!["models/user.rs".to_string()];

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), false);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_not_skip_file_with_generated_content_marker_when_include_generated_is_true() {
            let source =
                "// Code generated by SQLBoiler 4.19.5. DO NOT EDIT.\n\nfn foo() -> i32 { 1 }\n";
            let read_file = fake_reader(HashMap::from([("models/user.rs", source)]));
            let paths = vec!["models/user.rs".to_string()];

            let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            assert_eq!(1, report.files.len());
        }

        #[test]
        fn should_drop_whole_file_from_files_when_test_path_has_only_test_symbols() {
            let source = "\
package main

func TestFoo(t *testing.T) {
	1
}
";
            let read_file = fake_reader(HashMap::from([("repo_test.go", source)]));
            let paths = vec!["repo_test.go".to_string()];

            let expected = Report {
                origin: ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };
            let actual = analyze_repo(&paths, read_file, false, &HashSet::new(), true);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_keep_test_symbol_when_include_tests_is_true() {
            let source = "\
package main

func TestFoo(t *testing.T) {
	1
}
";
            let read_file = fake_reader(HashMap::from([("repo_test.go", source)]));
            let paths = vec!["repo_test.go".to_string()];

            let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            assert_eq!(1, report.files.len());
        }

        #[test]
        fn should_keep_non_test_symbol_and_drop_test_symbol_when_file_mixes_both() {
            // A Rust file with one production function and one
            // `#[cfg(test)] mod tests` function — the production symbol is
            // kept, the test symbol is dropped, and the file itself is kept
            // (not emptied entirely) since it still has a non-test symbol
            // left after filtering.
            let source = "\
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_add_two_numbers() {
        assert_eq!(3, add(1, 2));
    }
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
            let paths = vec!["src/lib.rs".to_string()];

            let report = analyze_repo(&paths, read_file, false, &HashSet::new(), true);

            let expected_names = vec!["add".to_string()];
            let actual_names: Vec<String> = report.files[0]
                .symbols
                .iter()
                .map(|s| s.name.clone())
                .collect();
            assert_eq!(expected_names, actual_names);
        }

        #[test]
        fn should_populate_hotspots_when_repo_has_a_symbol_with_fan_in_of_two() {
            // ADR 0017's Consequences: fan-in hotspots are computed over the
            // whole repository in this mode, same aggregation as diff mode.
            let source = "\
fn shared_helper() -> i32 {
    1
}

fn caller_one() -> i32 {
    shared_helper()
}

fn caller_two() -> i32 {
    shared_helper()
}
";
            let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
            let paths = vec!["src/lib.rs".to_string()];

            let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            let expected = vec![crate::graph::Hotspot {
                id: "src/lib.rs::shared_helper".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared_helper".to_string(),
                used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
            }];
            assert_eq!(expected, report.hotspots);
        }
    }

    /// ADR 0028 integration tests: end-to-end wiring of `compute_file_size_warnings`
    /// through both pipeline entry points. Unit-level ordering/threshold
    /// behavior is already covered by `crate::file_size::tests`; these
    /// tests only prove that the pipeline collects `(path, line_count)`
    /// pairs correctly and threads them through to `Report::file_size_warnings`.
    mod file_size_warnings_tests {
        use super::*;
        use crate::file_size::{FileSizeSeverity, FileSizeWarning, WARN_LINE_THRESHOLD};
        use pretty_assertions::assert_eq;

        /// Builds a Rust source string of roughly `line_count` lines whose
        /// first line is a real function definition (so `analyze_diff` has
        /// something to extract) padded to the requested line count by
        /// trivial let-bindings on subsequent lines. The head function's
        /// body-length itself is what pushes the file's total line count
        /// over the threshold.
        fn rust_source_with_line_count(line_count: usize) -> String {
            let mut buf = String::from("fn touched() -> i32 {\n");
            // Two lines already used (`fn touched() ... {` and the trailing
            // `}`); the rest are body lines.
            let filler_lines = line_count.saturating_sub(2);
            for i in 0..filler_lines {
                buf.push_str(&format!("    let _v{i} = {i};\n"));
            }
            buf.push_str("}\n");
            buf
        }

        #[test]
        fn should_include_warn_when_analyze_diff_reads_a_file_over_warn_threshold() {
            let big_source = rust_source_with_line_count(WARN_LINE_THRESHOLD + 100);
            let actual_line_count = big_source.lines().count();
            // The diff itself only needs to touch one line for the file to
            // enter the pipeline's per-file read loop — line-count
            // measurement is on the read source, not on the diff hunks.
            let diff = "\
diff --git a/src/big.rs b/src/big.rs
index e69de29..4b825dc 100644
--- a/src/big.rs
+++ b/src/big.rs
@@ -1,1 +1,1 @@
-fn touched() -> i32 {
+fn touched() -> i32 {
";
            let read_file = fake_reader(HashMap::from([(
                "src/big.rs",
                Box::leak(big_source.into_boxed_str()) as &'static str,
            )]));

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected = vec![FileSizeWarning {
                path: "src/big.rs".to_string(),
                line_count: actual_line_count,
                severity: FileSizeSeverity::Warn,
            }];
            assert_eq!(expected, report.file_size_warnings);
        }

        #[test]
        fn should_exclude_skipped_files_from_file_size_warnings_when_analyze_diff_runs() {
            // A binary file is skipped before any read happens, so it can
            // never appear in `file_size_warnings` regardless of size — the
            // (path, line_count) collection only records files whose
            // content was actually read.
            let diff = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
            let read_file = fake_reader(HashMap::new());

            let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected: Vec<FileSizeWarning> = vec![];
            assert_eq!(expected, report.file_size_warnings);
        }

        #[test]
        fn should_include_warn_when_analyze_repo_reads_a_file_over_warn_threshold() {
            let big_source = rust_source_with_line_count(WARN_LINE_THRESHOLD + 200);
            let actual_line_count = big_source.lines().count();
            let read_file = fake_reader(HashMap::from([(
                "src/big.rs",
                Box::leak(big_source.into_boxed_str()) as &'static str,
            )]));
            let paths = vec!["src/big.rs".to_string()];

            let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

            let expected = vec![FileSizeWarning {
                path: "src/big.rs".to_string(),
                line_count: actual_line_count,
                severity: FileSizeSeverity::Warn,
            }];
            assert_eq!(expected, report.file_size_warnings);
        }
    }

    /// ADR 0029 regression: `analyze_repo`'s per-file loop is now driven
    /// by rayon's `par_iter`, whose ordered `collect` contract is what
    /// keeps the output for a given input deterministic (byte-identical
    /// across runs and, within a single run, in the same order as the
    /// input `paths`). Locks that invariant down at the top-level `Report`
    /// so any future accidental switch to an unordered combinator (e.g.
    /// `par_bridge`, unordered `flat_map`, `fold`+`reduce` without a
    /// merge) fails loudly here rather than only misbehaving on the
    /// three-crate-workspace test set that happens to have short enough
    /// inputs to hide it.
    mod parallel_determinism_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_produce_deterministic_output_on_repeated_calls() {
            // Ten distinct files with distinct symbol shapes across three
            // languages: enough distinct paths that a shuffled order would
            // show up in the `Vec<FileReport>`/graph node lists rather
            // than being masked by a same-content file being reordered
            // with itself.
            let files: Vec<(&'static str, &'static str)> = vec![
                ("src/a.rs", "fn a1() {}\nfn a2() {}\n"),
                ("src/b.rs", "fn b1() {}\nstruct B { x: i32 }\n"),
                ("src/c.rs", "fn c1() {}\ntrait C { fn m(&self); }\n"),
                ("src/d.rs", "fn d1() {}\nenum D { X, Y }\n"),
                ("src/e.rs", "fn e1(x: i32) -> i32 { x }\n"),
                ("pkg/f.go", "package pkg\n\nfunc F1() {}\nfunc F2() {}\n"),
                ("pkg/g.go", "package pkg\n\ntype G struct{}\n"),
                ("py/h.py", "def h1():\n    pass\n\ndef h2():\n    pass\n"),
                ("py/i.py", "class I:\n    def m(self):\n        pass\n"),
                (
                    "py/j.py",
                    "def j1(x):\n    return x\n\ndef j2(y):\n    return y\n",
                ),
            ];
            let read_file = fake_reader(HashMap::from_iter(files.iter().copied()));
            let paths: Vec<String> = files.iter().map(|(p, _)| p.to_string()).collect();

            let first = analyze_repo(&paths, &read_file, true, &HashSet::new(), true);
            // Repeated calls must produce byte-identical `Report`s: the
            // per-file body is pure (no interior mutability, no
            // wall-clock), rayon's `par_iter().collect()` preserves source
            // order, and downstream graph building is already
            // deterministic — so any inequality here means one of those
            // invariants regressed.
            for _ in 0..4 {
                let again = analyze_repo(&paths, &read_file, true, &HashSet::new(), true);
                assert_eq!(first, again);
            }

            // Source-order invariant: the `Vec<FileReport>` must be in
            // the same order as the input `paths` (rayon's ordered
            // `collect` contract). Every path here maps to one
            // `FileReport`, so equality of the two path lists is the
            // strongest possible check.
            let expected_paths: Vec<String> = paths.clone();
            let actual_paths: Vec<String> = first.files.iter().map(|f| f.path.clone()).collect();
            assert_eq!(expected_paths, actual_paths);
        }
    }
}
