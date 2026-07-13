//! Top-level [`analyze_diff`] behavior: empty input, per-file skip cases
//! (deleted / binary / unsupported-language / pure rename), diff-parse
//! and read-file error paths, multi-file mixed outcomes, Go
//! interface/receiver nesting end-to-end (ADR 0012 decision 2), resolver
//! invocation contract (`Some`/`None`), and hotspot wiring (ADR 0013
//! end-to-end).

use super::{empty_graph, fake_reader};
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::graph::Hotspot;
use crate::pipeline::{AnalyzeError, analyze_diff};
use crate::render::{FileReport, Report, ReportOrigin, SkipReason, SkippedFile};
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_return_empty_report_when_diff_is_empty() {
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff("", read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                id: "src/lib.rs::foo".to_string(),
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "fn foo(a: i32) -> i32".to_string(),
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
                id: "src/lib.rs::foo".to_string(),
                path: "src/lib.rs".to_string(),
                name: "foo".to_string(),
            }],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "src/old.rs".to_string(),
            reason: SkipReason::Deleted,
        }],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: SkipReason::Binary,
        }],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "src/main.rb".to_string(),
            reason: SkipReason::UnsupportedLanguage,
        }],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/new_name.rs".to_string(),
            symbols: vec![],
        }],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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

    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true);

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

    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true);

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
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                id: "src/lib.rs::a".to_string(),
                name: "a".to_string(),
                kind: SymbolKind::Function,
                signature: "fn a() -> i32".to_string(),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
                is_test: false,
                classification: None,
                previous_signature: None,
            }],
        }],
        skipped: vec![SkippedFile {
            path: "src/main.rb".to_string(),
            reason: SkipReason::UnsupportedLanguage,
        }],
        graph: crate::graph::SymbolGraph {
            nodes: vec![crate::graph::Node {
                id: "src/lib.rs::a".to_string(),
                path: "src/lib.rs".to_string(),
                name: "a".to_string(),
            }],
            edges: vec![],
            roots: vec!["src/lib.rs::a".to_string()],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

/// End-to-end regression test for ADR 0012 decision 2: a Go interface
/// and a same-named receiver method that both change in one diff must
/// render as a single tree (the method nested under the interface) in
/// the "Change graph" section, not as two duplicate top-level roots —
/// see the ADR's "listed twice" problem statement. Runs through the
/// whole pipeline (`analyze_diff` then `render::render`) rather than
/// building a `Report`/`SymbolGraph` by hand, since the point is to
/// prove the real `Repo` interface's `referenced_names` (populated by
/// `GoSupport::reference_query`) actually produces the edge, not to
/// exercise `render.rs`'s formatting in isolation.
#[test]
fn should_nest_go_receiver_method_under_its_interface_when_both_change_in_one_diff() {
    let diff = "\
diff --git a/repo.go b/repo.go
index e69de29..4b825dc 100644
--- a/repo.go
+++ b/repo.go
@@ -1,10 +1,10 @@
 package main

 type Repo interface {
-	Save(id string) error
+	Save(id string) (err error)
 }

 type repoImpl struct{}

 func (r *repoImpl) Save(id string) error {
-	return errors.New(\"not implemented\")
+	return nil
 }
";
    let source = "\
package main

type Repo interface {
	Save(id string) (err error)
}

type repoImpl struct{}

func (r *repoImpl) Save(id string) error {
	return nil
}
";
    let read_file = fake_reader(HashMap::from([("repo.go", source)]));

    let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");
    let markdown = crate::render::render(&report, crate::render::OutputFormat::Markdown)
        .expect("markdown render should succeed");

    let expected = "\
## Change graph

2 changed symbols in 1 file

- interface Repo (repo.go)
  - fn Save (repo.go)

## Definitions

### interface Repo (repo.go)

```
Repo interface { Save(id string) (err error) }
```

### fn Save (repo.go)

```
// repoImpl
func (r *repoImpl) Save(id string) error
```

"
    .to_string();

    assert_eq!(expected, markdown);
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

    let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

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

    analyze_diff(
        diff,
        read_file,
        None,
        Some(&resolver),
        true,
        &HashSet::new(),
        true,
    )
    .expect("analyze should succeed");

    let mut expected = vec!["Point".to_string(), "helper".to_string()];
    let mut actual = resolver.calls.borrow().clone();
    expected.sort();
    actual.sort();

    assert_eq!(expected, actual);
}

// ADR 0013 end-to-end: two changed functions ("caller_one",
// "caller_two") both call "shared_helper" in the same file — fan-in
// 2 qualifies "shared_helper" as a hotspot, and `analyze_diff` must
// populate `Report::hotspots` from the graph it builds, not leave
// it empty.
//
// NOTE: asserts only `report.hotspots` instead of the whole
// `Report` — files/graph/tests wiring is already covered by the
// surrounding analyze_diff tests, and this module's concern is
// solely that the hotspot aggregation is hooked up.
#[test]
fn should_populate_hotspots_when_diff_has_a_symbol_with_fan_in_of_two() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,11 +1,11 @@
 fn shared_helper() -> i32 {
-    0
+    1
 }

 fn caller_one() -> i32 {
-    0
+    shared_helper()
 }

 fn caller_two() -> i32 {
-    0
+    shared_helper()
 }
";
    let source = "\
fn shared_helper() -> i32 {
    1
}

fn caller_one() -> i32 {
    shared_helper()
}

fn caller_two() -> i32 {
    shared_helper()
}
";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

    let report = analyze_diff(diff, read_file, None, None, true, &HashSet::new(), true)
        .expect("analyze should succeed");

    let expected = vec![Hotspot {
        id: "src/lib.rs::shared_helper".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared_helper".to_string(),
        used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
    }];
    assert_eq!(expected, report.hotspots);
}

// NOTE: partial assert on `report.hotspots` only, same rationale
// as the test above.
#[test]
fn should_return_empty_hotspots_when_no_node_has_fan_in_of_two() {
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

    let expected: Vec<Hotspot> = Vec::new();
    assert_eq!(expected, report.hotspots);
}
