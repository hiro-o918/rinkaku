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
use crate::extract::extract_changed_symbols;
use crate::language::language_for_path;
use crate::render::{FileReport, Report, SkipReason, SkippedFile};
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
        let symbols = extract_changed_symbols(&source, lang, &changed_file.changed_ranges);
        files.push(FileReport {
            path: changed_file.path,
            symbols,
        });
    }

    let files = match resolver {
        Some(resolver) => resolve_dependencies(files, resolver),
        None => files,
    };

    Ok(Report { files, skipped })
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
    use std::collections::HashMap;

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

    #[test]
    fn should_return_empty_report_when_diff_is_empty() {
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            files: vec![],
            skipped: vec![],
        };
        let actual = analyze_diff("", read_file, None).expect("analyze should succeed");

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
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo(a: i32) -> i32".to_string(),
                    range: LineRange { start: 1, end: 3 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                }],
            }],
            skipped: vec![],
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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

        let actual = analyze_diff(diff, read_file, None);

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

        let actual = analyze_diff(diff, read_file, None);

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
                    name: "a".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn a() -> i32".to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                }],
            }],
            skipped: vec![SkippedFile {
                path: "src/main.rb".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
        };
        let actual = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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

        let report = analyze_diff(diff, read_file, None).expect("analyze should succeed");

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

        analyze_diff(diff, read_file, Some(&resolver)).expect("analyze should succeed");

        let mut expected = vec!["Point".to_string(), "helper".to_string()];
        let mut actual = resolver.calls.borrow().clone();
        expected.sort();
        actual.sort();

        assert_eq!(expected, actual);
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
