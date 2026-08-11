//! ADR 0071 end-to-end: a reported container's signature narrows to its
//! touched member lines, and this narrowing must not change ADR 0014
//! classification — `classify_symbols` compares against `all_head_symbols`'
//! always-whole-class signature, not the narrowed `head_symbols` one (see
//! `extract::classify_symbols`'s doc comment).
//!
//! NOTE: asserts individual fields of the reported symbol rather than the
//! whole `ExtractedSymbol`, matching the established partial-assert style
//! of the sibling `classification_wiring` module this test's shape
//! mirrors — the fields not asserted here (`id`, `range`,
//! `referenced_names`, `dependencies`, ...) are irrelevant to the
//! narrowing/classification interaction under test.

use super::fake_reader;
use crate::extract::Classification;
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

// A field-only edit inside a Python class with an unrelated, untouched
// method: the reported `Widget` signature must drop `move`/`scale`
// entirely, and — despite the head-side signature being narrower than
// the base-side's whole-class signature — still classify `BodyOnly`,
// exactly as it did before ADR 0071 narrowed the displayed signature
// (the class's own touched line changed value only, not shape).
#[test]
fn should_drop_untouched_methods_from_reported_class_signature_when_only_a_field_changed() {
    let diff = "\
diff --git a/widget.py b/widget.py
index 57b03c6..75530be 100644
--- a/widget.py
+++ b/widget.py
@@ -1,7 +1,7 @@
 class Widget:
-    label = \"a\"
+    label = \"a\"

     def move(self, dx, dy):
         return dx + dy

     def scale(self, factor):
         return factor
";
    let base_source = "\
class Widget:
    label = \"a\"

    def move(self, dx, dy):
        return dx + dy

    def scale(self, factor):
        return factor
";
    let head_source = base_source;
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
    assert_eq!("Widget", symbol.name);
    assert_eq!("class Widget:\n    label = \"a\"", symbol.signature);
    assert_eq!(Some(Classification::BodyOnly), symbol.classification);
}

// Companion case: the touched field's *value* actually changes. The
// narrowed signature differs from the base's whole-class signature
// for a real reason (the field's own text), so classification must
// still correctly report `SignatureChanged` — narrowing the displayed
// signature must not turn a real change into a false `BodyOnly` either.
#[test]
fn should_classify_signature_changed_when_a_touched_field_value_actually_differs() {
    let diff = "\
diff --git a/widget.py b/widget.py
index 57b03c6..8b34e01 100644
--- a/widget.py
+++ b/widget.py
@@ -1,7 +1,7 @@
 class Widget:
-    label = \"a\"
+    label = \"b\"

     def move(self, dx, dy):
         return dx + dy

     def scale(self, factor):
         return factor
";
    let base_source = "\
class Widget:
    label = \"a\"

    def move(self, dx, dy):
        return dx + dy

    def scale(self, factor):
        return factor
";
    let head_source = "\
class Widget:
    label = \"b\"

    def move(self, dx, dy):
        return dx + dy

    def scale(self, factor):
        return factor
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
    assert_eq!("Widget", symbol.name);
    assert_eq!("class Widget:\n    label = \"b\"", symbol.signature);
    assert_eq!(
        Some(Classification::SignatureChanged),
        symbol.classification
    );
    assert_eq!(
        Some("class Widget:\n    label = \"a\"\n\n    def move(self, dx, dy):\n\n    def scale(self, factor):".to_string()),
        symbol.previous_signature
    );
}
