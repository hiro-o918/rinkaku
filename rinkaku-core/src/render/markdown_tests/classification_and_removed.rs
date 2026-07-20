//! ADR 0014 contract-impact markers on tree/definition/fan-in lines,
//! the ` ```diff ` block for a signature-changed symbol (with and
//! without a container comment), and the "## Removed symbols" section
//! including its deduplication rule for label-equivalent removed
//! symbols. Pins `classification_marker` /
//! `labeled_with_marker` / `render_definition`'s
//! `Classification::SignatureChanged` branch / `removed_symbol_label`.

use super::*;
use crate::extract::{Classification, RemovedSymbol, SymbolKind};
use crate::graph::FanIn;
use crate::render::report::{FileReport, ReportOrigin, TestFileSummary};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;
use rstest::rstest;

#[test]
fn should_append_new_marker_to_tree_and_definition_when_symbol_is_added() {
    let mut foo = symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()");
    foo.classification = Some(Classification::Added);
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![foo],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — new

## Definitions

### fn foo (src/lib.rs) — new

```
fn foo()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_diff_block_and_marker_when_symbol_is_signature_changed() {
    let mut foo = symbol(
        "src/lib.rs::foo",
        "foo",
        SymbolKind::Function,
        "fn foo(a: i32, b: i32) -> i32",
    );
    foo.classification = Some(Classification::SignatureChanged);
    foo.previous_signature = Some("fn foo(a: i32) -> i32".to_string());
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![foo],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — signature changed

## Definitions

### fn foo (src/lib.rs) — signature changed

```diff
-fn foo(a: i32) -> i32
+fn foo(a: i32, b: i32) -> i32
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// A signature-changed symbol with a container gets the container
// comment line rendered unchanged above the diff lines — it is not
// itself part of the base/head comparison (see `render_definition`'s
// doc comment).
#[test]
fn should_render_container_comment_above_diff_lines_when_signature_changed_symbol_has_container() {
    let mut bar = symbol(
        "src/lib.rs::bar",
        "bar",
        SymbolKind::Function,
        "fn bar(&self, extra: i32) -> i32",
    );
    bar.container = Some("impl Foo".to_string());
    bar.classification = Some(Classification::SignatureChanged);
    bar.previous_signature = Some("fn bar(&self) -> i32".to_string());
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![bar],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
            edges: vec![],
            roots: vec!["src/lib.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn bar (src/lib.rs) — signature changed

## Definitions

### fn bar (src/lib.rs) — signature changed

```diff
// impl Foo
-fn bar(&self) -> i32
+fn bar(&self, extra: i32) -> i32
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// `body_only` and unattempted (`None`) classification both render
// completely unmarked — no `— <marker>` suffix anywhere, and a
// plain (non-diff) fenced signature block.
#[rstest]
#[case::should_render_unmarked_when_classification_is_body_only(Some(Classification::BodyOnly))]
#[case::should_render_unmarked_when_classification_is_none(None)]
fn should_render_unmarked_tree_and_definition(#[case] classification: Option<Classification>) {
    let mut foo = symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()");
    foo.classification = classification;
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![foo],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_append_marker_to_fan_in_line_before_used_by() {
    let mut shared = symbol(
        "src/lib.rs::shared",
        "shared",
        SymbolKind::Function,
        "fn shared()",
    );
    shared.classification = Some(Classification::SignatureChanged);
    shared.previous_signature = Some("fn shared(a: i32)".to_string());
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![shared],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::shared", "src/lib.rs", "shared")],
            edges: vec![],
            roots: vec!["src/lib.rs::shared".to_string()],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
        }],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        test_coverage: vec![],
    };

    let markdown = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");
    // NOTE: partial assert (searching for one line) rather than a
    // fully qualified comparison of the whole render — this test's
    // concern is solely the "High fan-in symbols" line's marker
    // placement, and the "Change graph"/"Definitions" sections above it
    // are already covered by other tests in this module (e.g.
    // `should_render_diff_block_and_marker_when_symbol_is_signature_changed`).
    let fan_in_line = markdown
        .lines()
        .find(|line| line.contains("used by"))
        .expect("high fan-in symbols section must contain the shared symbol's line");

    assert_eq!(
        "- fn shared (src/lib.rs) — signature changed — used by 2: caller_one, caller_two",
        fan_in_line
    );
}

#[test]
fn should_render_removed_symbols_section_between_definitions_and_tests() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo()",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![TestFileSummary {
            path: "src/lib.rs".to_string(),
            symbol_count: 1,
        }],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![RemovedSymbol {
            name: "old_helper".to_string(),
            kind: SymbolKind::Function,
            path: "src/lib.rs".to_string(),
            signature: "fn old_helper()".to_string(),
        }],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Removed symbols

- fn old_helper (src/lib.rs)

## Tests

- src/lib.rs: 1 changed test symbol

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// A diff whose only changed-symbol-level content is a removal (no
// graph nodes at all, e.g. a whole function deleted with nothing
// added back — see `pipeline::tests::classification_wiring_tests`'s
// "hunk only removes lines" case) must still render "## Removed
// symbols" on its own — the empty-output guard at the top of
// `render_markdown` must not treat an empty `graph.nodes` as "there
// is nothing to say" when `removed` is non-empty.
#[test]
fn should_render_removed_symbols_section_alone_when_graph_is_empty() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![RemovedSymbol {
            name: "old_helper".to_string(),
            kind: SymbolKind::Function,
            path: "src/lib.rs".to_string(),
            signature: "fn old_helper()".to_string(),
        }],
    };

    let expected = "\
## Removed symbols

- fn old_helper (src/lib.rs)

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// Regression test: `removed_symbol_label` deliberately omits
// `container` (see its own doc comment), so two distinct removed
// symbols sharing name+kind but differing only by container (e.g.
// the same method name removed from two different impls in one
// file) render identical lines. Without deduplication this would
// print the same line twice; the section must show it once, in
// first-occurrence order.
#[test]
fn should_deduplicate_identical_removed_symbol_lines() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![
            RemovedSymbol {
                name: "save".to_string(),
                kind: SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn save(&self)".to_string(),
            },
            RemovedSymbol {
                name: "other".to_string(),
                kind: SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn other(&self)".to_string(),
            },
            // Same name+kind+path as the first entry, but a
            // different container/signature — the label is
            // identical to the first line even though this is a
            // genuinely distinct removed symbol (different impl).
            RemovedSymbol {
                name: "save".to_string(),
                kind: SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn save(&self, id: &str)".to_string(),
            },
        ],
    };

    let expected = "\
## Removed symbols

- fn save (src/lib.rs)
- fn other (src/lib.rs)

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_removed_symbols_section_when_removed_is_empty() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo()",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let markdown = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert!(!markdown.contains("## Removed symbols"));
}

// A diff isn't valid Markdown to embed unfenced — ensure the fence
// still widens against a backtick run appearing in either the base
// or head signature text, not just the head signature the way
// `fence_for` alone would check.
#[test]
fn should_widen_fence_when_previous_signature_contains_a_backtick_run() {
    let mut foo = symbol(
        "src/lib.rs::foo",
        "foo",
        SymbolKind::Function,
        "fn foo() -> i32",
    );
    foo.classification = Some(Classification::SignatureChanged);
    // Three consecutive backticks in the *base* signature only —
    // proves the fence widens against `previous_signature`, not
    // just the head `signature` the way plain `fence_for` would.
    foo.previous_signature =
        Some("fn foo() { let s = \"```rust\\nfn f() {}\\n```\"; }".to_string());
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![foo],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — signature changed

## Definitions

### fn foo (src/lib.rs) — signature changed

````diff
-fn foo() { let s = \"```rust\\nfn f() {}\\n```\"; }
+fn foo() -> i32
````

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
