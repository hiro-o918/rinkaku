//! Wiring the diff parser, language registry, and symbol extractor into a
//! single pure pipeline.
//!
//! [`analyze_diff`] takes a diff's text and a `read_file` port for fetching
//! a changed file's new-side content, and produces a [`crate::render::Report`].
//! File reads are injected rather than performed here so this module stays
//! pure and testable: `main.rs` supplies a closure that reads the working
//! tree, tests supply a closure backed by an in-memory map.

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
pub fn analyze_diff(
    diff_text: &str,
    read_file: impl Fn(&str) -> std::io::Result<String>,
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

    Ok(Report { files, skipped })
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
        let actual = analyze_diff("", read_file).expect("analyze should succeed");

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
                }],
            }],
            skipped: vec![],
        };
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

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
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

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
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_file_with_unsupported_language_without_reading_it() {
        let diff = "\
diff --git a/src/main.py b/src/main.py
index e69de29..4b825dc 100644
--- a/src/main.py
+++ b/src/main.py
@@ -1,1 +1,2 @@
 def foo():
+    pass
";
        let read_file = fake_reader(HashMap::new());

        let expected = Report {
            files: vec![],
            skipped: vec![SkippedFile {
                path: "src/main.py".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
        };
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

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
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

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

        let actual = analyze_diff(diff, read_file);

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

        let actual = analyze_diff(diff, read_file);

        assert!(matches!(
            actual,
            Err(AnalyzeError::ReadFile { path, .. }) if path == "src/lib.rs"
        ));
    }

    #[test]
    fn should_process_multiple_files_with_mixed_outcomes_in_one_diff() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,1 @@
-fn a() {}
+fn a() -> i32 { 0 }
diff --git a/src/main.py b/src/main.py
index e69de29..4b825dc 100644
--- a/src/main.py
+++ b/src/main.py
@@ -1,1 +1,2 @@
 def foo():
+    pass
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
                }],
            }],
            skipped: vec![SkippedFile {
                path: "src/main.py".to_string(),
                reason: SkipReason::UnsupportedLanguage,
            }],
        };
        let actual = analyze_diff(diff, read_file).expect("analyze should succeed");

        assert_eq!(expected, actual);
    }
}
