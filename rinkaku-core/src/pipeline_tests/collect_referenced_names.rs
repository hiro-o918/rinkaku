//! [`collect_referenced_names`] helper: reference-name gathering,
//! empty-input handling, deleted-file skip, and the malformed-diff error
//! path.

use super::fake_reader;
use crate::pipeline::{AnalyzeError, collect_referenced_names};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

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

    let expected: std::collections::HashSet<String> = ["Point".to_string(), "helper".to_string()]
        .into_iter()
        .collect();
    let actual = collect_referenced_names(diff, read_file).expect("collection should succeed");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_empty_set_when_diff_is_empty() {
    let read_file = fake_reader(HashMap::new());

    let expected: std::collections::HashSet<String> = std::collections::HashSet::new();
    let actual = collect_referenced_names("", read_file).expect("collection should succeed");

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
    let actual = collect_referenced_names(diff, read_file).expect("collection should succeed");

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
