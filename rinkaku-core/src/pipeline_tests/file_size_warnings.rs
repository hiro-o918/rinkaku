//! ADR 0028 integration: end-to-end wiring of
//! [`crate::file_size::compute_file_size_warnings`] through both
//! pipeline entry points. Unit-level ordering/threshold behavior is
//! already covered by `crate::file_size::tests`; these tests only prove
//! that the pipeline collects `(path, line_count)` pairs correctly and
//! threads them through to `Report::file_size_warnings`.

use super::fake_reader;
use crate::file_size::{FileSizeSeverity, FileSizeWarning, WARN_LINE_THRESHOLD};
use crate::pipeline::{analyze_diff, analyze_repo};
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

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
