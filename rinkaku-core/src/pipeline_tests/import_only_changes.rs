//! Regression: a diff touching only import/`use` lines must produce no
//! changed symbols and no false `removed` entries, across every
//! import-bearing built-in language. Import statements sit outside every
//! language's `definition_query`, so `extract_changed_symbols` already
//! returns empty for them (see each language's
//! `should_return_empty_vec_when_changed_line_is_outside_any_definition`
//! extract-level test) — this module pins the same guarantee end-to-end
//! through [`analyze_diff`], including the base-comparison path
//! (`read_base_file: Some`), where an import-line rewrite still produces
//! non-empty `old_changed_ranges` but must not make `classify_symbols`
//! misreport anything as removed, since no definition's range overlaps an
//! import line.

use super::fake_reader;
use crate::extract::RemovedSymbol;
use crate::non_symbol_changes::NonSymbolChange;
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_report_no_symbols_when_python_diff_only_adds_an_import() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..1e5c9a1 100644
--- a/foo.py
+++ b/foo.py
@@ -1,2 +1,3 @@
+import os
 def helper():
     return 1
";
    let base_source = "\
def helper():
    return 1
";
    let head_source = "\
import os
def helper():
    return 1
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

    assert_eq!(
        Vec::<crate::extract::ExtractedSymbol>::new(),
        report.files[0].symbols
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// ADR 0070: a symbol-less file's `Report.non_symbol_changes` carries the
// changed-line count `render_markdown`'s "Other changed files" annotation
// is built from — the same import-only diff as
// `should_report_no_symbols_when_python_diff_only_adds_an_import`, this
// time asserting the sibling field that guarantee's regression coverage
// didn't previously touch.
#[test]
fn should_report_non_symbol_change_line_count_when_python_diff_only_adds_an_import() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..1e5c9a1 100644
--- a/foo.py
+++ b/foo.py
@@ -1,2 +1,3 @@
+import os
 def helper():
     return 1
";
    let head_source = "\
import os
def helper():
    return 1
";
    let read_file = fake_reader(HashMap::from([("foo.py", head_source)]));

    let report = analyze_diff(
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

    assert_eq!(
        vec![NonSymbolChange {
            path: "foo.py".to_string(),
            changed_line_count: 1,
        }],
        report.non_symbol_changes
    );
}

#[test]
fn should_report_no_symbols_when_python_diff_rewrites_an_import_line() {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..1e5c9a1 100644
--- a/foo.py
+++ b/foo.py
@@ -1,3 +1,3 @@
-import os
+import sys
 def helper():
     return 1
";
    let base_source = "\
import os
def helper():
    return 1
";
    let head_source = "\
import sys
def helper():
    return 1
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

    assert_eq!(
        Vec::<crate::extract::ExtractedSymbol>::new(),
        report.files[0].symbols
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

#[test]
fn should_report_no_symbols_when_rust_diff_only_changes_a_use_line() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 57b03c6..1e5c9a1 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
-use std::collections::HashMap;
+use std::collections::HashSet;
 fn helper() -> i32 {
     1
 }
";
    let base_source = "\
use std::collections::HashMap;
fn helper() -> i32 {
    1
}
";
    let head_source = "\
use std::collections::HashSet;
fn helper() -> i32 {
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
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(
        Vec::<crate::extract::ExtractedSymbol>::new(),
        report.files[0].symbols
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

#[test]
fn should_report_no_symbols_when_go_diff_removes_an_import() {
    let diff = "\
diff --git a/foo.go b/foo.go
index 57b03c6..1e5c9a1 100644
--- a/foo.go
+++ b/foo.go
@@ -1,7 +1,6 @@
 package main

 import (
-\t\"fmt\"
 \t\"os\"
 )

";
    let base_source = "\
package main

import (
\t\"fmt\"
\t\"os\"
)

func helper() int {
\treturn 1
}
";
    let head_source = "\
package main

import (
\t\"os\"
)

func helper() int {
\treturn 1
}
";
    let read_file = fake_reader(HashMap::from([("foo.go", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("foo.go", base_source)]));

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

    assert_eq!(
        Vec::<crate::extract::ExtractedSymbol>::new(),
        report.files[0].symbols
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

#[test]
fn should_report_no_symbols_when_typescript_diff_renames_an_import_alias() {
    let diff = "\
diff --git a/foo.ts b/foo.ts
index 57b03c6..1e5c9a1 100644
--- a/foo.ts
+++ b/foo.ts
@@ -1,4 +1,4 @@
-import { helper as h } from \"./helper\";
+import { helper as helperFn } from \"./helper\";
 function useHelper(): number {
     return 1;
 }
";
    let base_source = "\
import { helper as h } from \"./helper\";
function useHelper(): number {
    return 1;
}
";
    let head_source = "\
import { helper as helperFn } from \"./helper\";
function useHelper(): number {
    return 1;
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

    assert_eq!(
        Vec::<crate::extract::ExtractedSymbol>::new(),
        report.files[0].symbols
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

// Compound case (item 2 of the disappearance-regression matrix): an
// import rewrite in the same diff as a real usage-site change must still
// surface the function whose body actually changed, not get swallowed by
// the no-op import edit sitting in the same hunk range.
#[test]
fn should_report_the_changed_function_when_python_diff_combines_an_import_rewrite_with_a_usage_change()
 {
    let diff = "\
diff --git a/foo.py b/foo.py
index 57b03c6..1e5c9a1 100644
--- a/foo.py
+++ b/foo.py
@@ -1,3 +1,3 @@
-import json
+import orjson as json
 def load(raw):
-    return json.loads(raw)
+    return json.loads(raw, strict=False)
";
    let base_source = "\
import json
def load(raw):
    return json.loads(raw)
";
    let head_source = "\
import orjson as json
def load(raw):
    return json.loads(raw, strict=False)
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

    assert_eq!(1, report.files[0].symbols.len());
    let symbol = &report.files[0].symbols[0];
    assert_eq!("load", symbol.name);
    assert_eq!(
        Some(crate::extract::Classification::BodyOnly),
        symbol.classification
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}

#[test]
fn should_report_the_changed_function_when_typescript_diff_combines_an_import_rewrite_with_a_usage_change()
 {
    let diff = "\
diff --git a/foo.ts b/foo.ts
index 57b03c6..1e5c9a1 100644
--- a/foo.ts
+++ b/foo.ts
@@ -1,4 +1,4 @@
-import { parse } from \"./parser\";
+import { parse as parseInput } from \"./parser\";
 function load(raw: string): unknown {
-    return parse(raw);
+    return parseInput(raw, { strict: false });
 }
";
    let base_source = "\
import { parse } from \"./parser\";
function load(raw: string): unknown {
    return parse(raw);
}
";
    let head_source = "\
import { parse as parseInput } from \"./parser\";
function load(raw: string): unknown {
    return parseInput(raw, { strict: false });
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

    assert_eq!(1, report.files[0].symbols.len());
    let symbol = &report.files[0].symbols[0];
    assert_eq!("load", symbol.name);
    assert_eq!(
        Some(crate::extract::Classification::BodyOnly),
        symbol.classification
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}
