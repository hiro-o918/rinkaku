//! ADR 0014: `classify_against_base` end-to-end via `analyze_diff` —
//! `SignatureChanged` / `Added` / `removed` population, the
//! `read_base_file: None` "not attempted" contract, base-path routing
//! for renamed files, and the "never call `read_base_file` for an
//! `Added` file" contract.

use super::fake_reader;
use crate::extract::{Classification, RemovedSymbol};
use crate::pipeline::analyze_diff;
use crate::render::FileReport;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

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
    let read_base_file =
        |_: &str| -> std::io::Result<String> { panic!("must not be called for an Added file") };

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
