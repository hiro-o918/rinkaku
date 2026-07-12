//! Wiring the diff parser, language registry, and symbol extractor into a
//! single pure pipeline.
//!
//! [`analyze_diff`] takes a diff's text and a `read_file` port for fetching
//! a changed file's new-side content, and produces a [`crate::render::Report`].
//! File reads are injected rather than performed here so this module stays
//! pure and testable: `main.rs` supplies a closure that reads the working
//! tree, tests supply a closure backed by an in-memory map.

use crate::deps::{Resolver, resolve_dependencies};
use crate::diff::{ChangeKind, parse_unified_diff};
use crate::extract::{ExtractedSymbol, extract_changed_symbols};
use crate::graph::{build_graph, stamp_ids};
use crate::language::{LanguageSupport, language_for_path};
use crate::render::{FileReport, Report, SkipReason, SkippedFile, TestFileSummary};
use thiserror::Error;

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
/// `include_tests` controls ADR 0009's test-symbol exclusion: `false` (the
/// CLI's default) drops every symbol a file's
/// [`crate::language::LanguageSupport`] considers a test — by path
/// ([`LanguageSupport::is_test_path`], the whole file) or by AST context
/// ([`ExtractedSymbol::is_test`], set per-definition during extraction) —
/// from `files` before dependency resolution and graph-building run, and
/// summarizes the excluded counts per file in the returned `Report`'s
/// `tests`. `true` (`--include-tests`) keeps every symbol in `files` as
/// before and leaves `tests` empty. Filtering happens before
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
pub fn analyze_diff(
    diff_text: &str,
    read_file: impl Fn(&str) -> std::io::Result<String>,
    resolver: Option<&dyn Resolver>,
    include_tests: bool,
    generated_paths: &std::collections::HashSet<String>,
    include_generated: bool,
) -> Result<Report, AnalyzeError> {
    let changed_files = parse_unified_diff(diff_text)?;

    let mut files = Vec::new();
    let mut skipped = Vec::new();

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

        // No hunks means no content change (a pure rename or a
        // mode-change-only diff): extract_changed_symbols would return no
        // symbols for an empty changed_ranges anyway, so skip the read
        // entirely rather than pay IO for a result already known to be
        // empty.
        if changed_file.changed_ranges.is_empty() {
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
        let symbols = extract_changed_symbols(&source, lang, &changed_file.changed_ranges);
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

    Ok(Report {
        files,
        skipped,
        graph,
        tests,
    })
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

/// Number of leading lines checked by [`is_generated_content`] — mirrors
/// GitHub linguist's own "near the top of the file" scope for its
/// content-based generated-file heuristics (ADR 0011).
const GENERATED_MARKER_SCAN_LINES: usize = 5;

/// Whether `content`'s first [`GENERATED_MARKER_SCAN_LINES`] lines carry a
/// linguist-compatible generated-file marker (ADR 0011): a `@generated`
/// marker (Facebook-style, matched as a plain substring — deliberately not
/// narrowed further per the ADR's "don't overthink context around
/// `@generated`" decision), or a single line containing both `Code
/// generated` and `DO NOT EDIT` (Go tooling/protobuf's
/// `// Code generated by <tool>. DO NOT EDIT.` convention and its `#`-
/// commented equivalents — matched by substring rather than anchoring to a
/// specific comment syntax, since the comment marker itself varies by
/// language). Case-sensitive, matching linguist's own casing for these
/// exact markers.
///
/// A pure text check with no knowledge of `LanguageSupport`/comment syntax
/// by design (ADR 0011's rejected alternative: porting linguist's full
/// rule set) — deliberately a small, easily-audited subset rather than a
/// comprehensive port.
fn is_generated_content(content: &str) -> bool {
    content
        .lines()
        .take(GENERATED_MARKER_SCAN_LINES)
        .any(|line| line.contains("@generated") || is_code_generated_do_not_edit_line(line))
}

/// Whether `line` contains both `Code generated` and `DO NOT EDIT` —
/// linguist's `^// Code generated .* DO NOT EDIT\.$` pattern, relaxed to a
/// same-line substring match on both phrases (see
/// [`is_generated_content`]'s doc comment for why the comment prefix and
/// trailing-period anchor are not checked).
fn is_code_generated_do_not_edit_line(line: &str) -> bool {
    line.contains("Code generated") && line.contains("DO NOT EDIT")
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
            files: vec![],
            skipped: vec![],
            graph: empty_graph(),
            tests: vec![],
        };
        let actual = analyze_diff("", read_file, None, true, &HashSet::new(), true)
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
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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
            files: vec![],
            skipped: vec![SkippedFile {
                path: "src/old.rs".to_string(),
                reason: SkipReason::Deleted,
            }],
            graph: empty_graph(),
            tests: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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
            files: vec![],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: empty_graph(),
            tests: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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
            files: vec![],
            skipped: vec![SkippedFile {
                path: "src/main.rb".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
            graph: empty_graph(),
            tests: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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
            files: vec![FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            }],
            skipped: vec![],
            graph: empty_graph(),
            tests: vec![],
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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

        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true);

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

        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true);

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
        };
        let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
            .expect("analyze should succeed");

        assert_eq!(expected, actual);
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

        let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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

            let report = analyze_diff(diff, read_file, None, false, &HashSet::new(), true)
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
            };
            let actual = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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

            let report = analyze_diff(diff, read_file, None, false, &HashSet::new(), true)
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

            let report = analyze_diff(diff, read_file, None, false, &HashSet::new(), true)
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
            };
            let actual = analyze_diff(diff, read_file, None, false, &HashSet::new(), true)
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

            let report = analyze_diff(diff, read_file, None, true, &generated_paths, true)
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

            let report = analyze_diff(diff, read_file, None, true, &generated_paths, true)
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

            let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
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

            let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), false)
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

            let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), true)
                .expect("analyze should succeed");

            let expected_skipped: Vec<SkippedFile> = Vec::new();
            assert_eq!(1, report.files.len());
            assert_eq!(1, report.files[0].symbols.len());
            assert_eq!(expected_skipped, report.skipped);
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

            let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), false)
                .expect("analyze should succeed");

            let expected_skipped: Vec<SkippedFile> = Vec::new();
            assert_eq!(1, report.files.len());
            assert_eq!(expected_skipped, report.skipped);
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

            let report = analyze_diff(diff, read_file, None, true, &HashSet::new(), false)
                .expect("analyze should succeed");

            let expected_skipped = vec![SkippedFile {
                path: "models/user.go".to_string(),
                reason: SkipReason::Generated,
            }];
            assert_eq!(1, report.files.len());
            assert_eq!("src/lib.rs", report.files[0].path);
            assert_eq!(expected_skipped, report.skipped);
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

            let report = analyze_diff(diff, read_file, None, true, &generated_paths, false)
                .expect("analyze should succeed");

            let expected = vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }];
            assert_eq!(expected, report.skipped);
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
}
