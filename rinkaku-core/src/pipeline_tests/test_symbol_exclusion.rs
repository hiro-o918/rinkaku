//! ADR 0009: `partition_test_symbols`'s behavior via `analyze_diff` —
//! per-file and whole-file test-symbol exclusion, `include_tests` gating,
//! mixed-file (production + test) symbol partitioning, and the
//! pure-rename-not-a-test-file retention invariant.

use super::fake_reader;
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::file_size::{FileSizeBand, FileSizeEntry};
use crate::pipeline::analyze_diff;
use crate::render::{FileReport, Report, ReportOrigin, TestFileSummary};
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

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

    let report = analyze_diff(
        diff,
        read_file,
        None,
        None,
        false,
        &HashSet::new(),
        true,
        None,
    )
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
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![FileSizeEntry {
            path: "src/lib.rs".to_string(),
            line_count: 4,
            band: FileSizeBand::Normal,
        }],
        removed: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
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

    let report = analyze_diff(
        diff,
        read_file,
        None,
        None,
        false,
        &HashSet::new(),
        true,
        None,
    )
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

    let report = analyze_diff(
        diff,
        read_file,
        None,
        None,
        false,
        &HashSet::new(),
        true,
        None,
    )
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
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![FileSizeEntry {
            path: "src/lib.rs".to_string(),
            line_count: 11,
            band: FileSizeBand::Normal,
        }],
        removed: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        false,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}
