//! Regression: a still-alive container (Python `class` / TypeScript
//! `class`) was misreported as `removed` when only one of its nested
//! members changed. `extract_changed_symbols` suppresses a container in
//! favor of its narrowest touched member (the "narrowest enclosing
//! definition" rule, so the Change graph doesn't report both the member
//! and its enclosing class), so `classify_symbols` must judge removal
//! against the head file's *complete* symbol set, not against that
//! narrowed `head_symbols` list — see [`crate::extract::classify_symbols`]'s
//! `all_head_symbols` parameter.

use super::fake_reader;
use crate::extract::{Classification, RemovedSymbol};
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

// Regression: editing only a nested member's *body* (no signature change)
// inside an otherwise-untouched Python class must not report the class
// itself as removed.
#[test]
fn should_not_report_container_as_removed_when_only_a_nested_member_body_changed_python() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..75530be 100644
--- a/foo.py
+++ b/foo.py
@@ -1,6 +1,6 @@
 class Foo:
     def __init__(self):
-        self.x = 1
+        self.x = 2

     def bar(self):
         return self.x
";
    let base_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self):
        return self.x
";
    let head_source = "\
class Foo:
    def __init__(self):
        self.x = 2

    def bar(self):
        return self.x
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.py", base_source)]));

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

    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// Same regression, TypeScript: a class field/method body edit must not
// report the enclosing `class` as removed.
#[test]
fn should_not_report_container_as_removed_when_only_a_nested_member_body_changed_typescript() {
    let diff = "\
diff --git a/foo.ts b/foo.ts
index 1f05983..c7fbb49 100644
--- a/foo.ts
+++ b/foo.ts
@@ -2,6 +2,6 @@ class Foo {
     x: number = 1;

     bar(): number {
-        return this.x;
+        return this.x + 1;
     }
 }
";
    let base_source = "\
class Foo {
    x: number = 1;

    bar(): number {
        return this.x;
    }
}
";
    let head_source = "\
class Foo {
    x: number = 1;

    bar(): number {
        return this.x + 1;
    }
}
";
    let read_file = fake_reader(HashMap::from([("foo.ts", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.ts", base_source)]));

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

    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// Companion to the body-only case: a nested member's *signature* change
// must still classify that member `SignatureChanged` while leaving the
// still-alive container out of `removed` — the two facts are not in
// tension, but before the fix they contradicted each other in the same
// report (`bar` shown as changed while `class Foo` was also reported
// gone).
#[test]
fn should_not_report_container_as_removed_when_a_nested_member_signature_changed() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..5084600 100644
--- a/foo.py
+++ b/foo.py
@@ -2,5 +2,5 @@ class Foo:
     def __init__(self):
         self.x = 1

-    def bar(self):
-        return self.x
+    def bar(self, y):
+        return self.x + y
";
    let base_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self):
        return self.x
";
    let head_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self, y):
        return self.x + y
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.py", base_source)]));

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
    assert_eq!("bar", symbol.name);
    assert_eq!(
        Some(Classification::SignatureChanged),
        symbol.classification
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// Regression guard: adding a brand-new method to an existing class is
// already handled correctly today (the class was never suppressed as
// "removed" in this case, since the new method has no base-side
// counterpart at all) — pinned here so a future change to the removal
// check cannot silently break this adjacent case while fixing the one
// above.
#[test]
fn should_not_report_container_as_removed_when_a_new_method_is_added_to_an_existing_class() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..ea46dfd 100644
--- a/foo.py
+++ b/foo.py
@@ -4,3 +4,6 @@ class Foo:

     def bar(self):
         return self.x
+
+    def baz(self):
+        return self.x * 2
";
    let base_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self):
        return self.x
";
    let head_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self):
        return self.x

    def baz(self):
        return self.x * 2
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.py", base_source)]));

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
    assert_eq!("baz", symbol.name);
    assert_eq!(Some(Classification::Added), symbol.classification);
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// Sibling case: when the *whole* class is actually deleted (but the file
// itself survives with other content), it and its members must still be
// reported removed — the fix must not overcorrect into never reporting a
// container removed at all. This diff hunk has no `+` lines at all (a
// pure deletion), exercising `analyze_diff`'s empty-`changed_ranges`
// branch, which now also needs to read head-side content lazily to tell
// "class truly gone" apart from "class survived, only a member changed".
#[test]
fn should_report_class_and_members_as_removed_when_whole_class_deleted_from_a_still_alive_file() {
    let diff = "\
diff --git a/foo.py b/foo.py
index a483def..c961442 100644
--- a/foo.py
+++ b/foo.py
@@ -1,10 +1,2 @@
-class Foo:
-    def __init__(self):
-        self.x = 1
-
-    def bar(self):
-        return self.x
-
-
 def helper():
     return 42
";
    let base_source = "\
class Foo:
    def __init__(self):
        self.x = 1

    def bar(self):
        return self.x


def helper():
    return 42
";
    let head_source = "\
def helper():
    return 42
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.py", base_source)]));

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

    let mut removed = report.removed.clone();
    removed.sort_by(|a, b| a.name.cmp(&b.name));
    let mut expected = vec![
        RemovedSymbol {
            name: "Foo".to_string(),
            kind: crate::extract::SymbolKind::Class,
            path: "foo.py".to_string(),
            signature: "class Foo:\n    def __init__(self):\n\n    def bar(self):".to_string(),
        },
        RemovedSymbol {
            name: "__init__".to_string(),
            kind: crate::extract::SymbolKind::Function,
            path: "foo.py".to_string(),
            signature: "def __init__(self):".to_string(),
        },
        RemovedSymbol {
            name: "bar".to_string(),
            kind: crate::extract::SymbolKind::Function,
            path: "foo.py".to_string(),
            signature: "def bar(self):".to_string(),
        },
    ];
    expected.sort_by(|a, b| a.name.cmp(&b.name));
    // `expected` is exhaustive (fully qualified assert): if `helper` —
    // the still-alive free function — were ever wrongly swept in, this
    // comparison would fail on its own without a separate assertion.
    assert_eq!(expected, removed);
}

// Guards the container-identity boundary: a free function and a class
// method sharing the same bare name ("helper") must be told apart by
// `(name, container)` identity, not name alone — editing only the
// method's body must not make the unrelated free function of the same
// name look removed, and vice versa.
#[test]
fn should_not_report_free_function_as_removed_when_only_a_same_named_method_changed() {
    let diff = "\
diff --git a/foo.py b/foo.py
index deacb04..2bf7031 100644
--- a/foo.py
+++ b/foo.py
@@ -4,4 +4,4 @@ def helper():

 class Foo:
     def helper(self):
-        return 2
+        return 3
";
    let base_source = "\
def helper():
    return 1


class Foo:
    def helper(self):
        return 2
";
    let head_source = "\
def helper():
    return 1


class Foo:
    def helper(self):
        return 3
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.py", base_source)]));

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

    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}
