//! ADR 0073 end-to-end: a diff touching only a decorator (Python) or
//! attribute (Rust) line is detected via `analyze_diff` and classified
//! against the base side, exercising the full pipeline path
//! (`extract_changed_symbols` -> `classify_symbols`) rather than just
//! `extract_changed_symbols` in isolation like `extract_tests/`.
//!
//! NOTE: asserts individual fields of the reported symbol rather than the
//! whole `ExtractedSymbol`, matching the established partial-assert style
//! of the sibling `container_slice_regression`/`classification_wiring`
//! modules this test's shape mirrors — the fields not asserted here
//! (`id`, `referenced_names`, `dependencies`, ...) are irrelevant to the
//! span-widening/classification interaction under test.

use super::fake_reader;
use crate::extract::Classification;
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_classify_signature_changed_when_only_a_python_decorator_line_changed() {
    let diff = "\
diff --git a/widget.py b/widget.py
index 57b03c6..75530be 100644
--- a/widget.py
+++ b/widget.py
@@ -1,3 +1,3 @@
-@decorator_v1
+@decorator_v2
 def handler(a):
     return a
";
    let base_source = "\
@decorator_v1
def handler(a):
    return a
";
    let head_source = "\
@decorator_v2
def handler(a):
    return a
";
    let read_file = fake_reader(HashMap::from([("widget.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("widget.py", base_source)]));

    let report = analyze_diff(
        diff,
        read_file,
        Some(&read_base_file),
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    let symbol = &report.files[0].symbols[0];
    assert_eq!("handler", symbol.name);
    assert_eq!("@decorator_v2\ndef handler(a):", symbol.signature);
    assert_eq!(
        Some(Classification::SignatureChanged),
        symbol.classification
    );
    assert_eq!(
        Some("@decorator_v1\ndef handler(a):".to_string()),
        symbol.previous_signature
    );
}

#[test]
fn should_classify_signature_changed_when_only_a_rust_derive_attribute_line_changed() {
    let diff = "\
diff --git a/point.rs b/point.rs
index 57b03c6..75530be 100644
--- a/point.rs
+++ b/point.rs
@@ -1,4 +1,4 @@
-#[derive(Debug)]
+#[derive(Debug, Clone)]
 struct Point {
     x: i32,
 }
";
    let base_source = "\
#[derive(Debug)]
struct Point {
    x: i32,
}
";
    let head_source = "\
#[derive(Debug, Clone)]
struct Point {
    x: i32,
}
";
    let read_file = fake_reader(HashMap::from([("point.rs", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("point.rs", base_source)]));

    let report = analyze_diff(
        diff,
        read_file,
        Some(&read_base_file),
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    let symbol = &report.files[0].symbols[0];
    assert_eq!("Point", symbol.name);
    assert_eq!(
        "#[derive(Debug, Clone)]\nstruct Point {\n    x: i32,\n}",
        symbol.signature
    );
    assert_eq!(
        Some(Classification::SignatureChanged),
        symbol.classification
    );
    assert_eq!(
        Some("#[derive(Debug)]\nstruct Point {\n    x: i32,\n}".to_string()),
        symbol.previous_signature
    );
}
