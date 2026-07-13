//! Defensive `SymbolLookup::get` returning `None` branches:
//! `render_markdown` and `render_tree_node` both hand-build a
//! [`Report`] whose `graph` refers to node ids that have no
//! corresponding `ExtractedSymbol` in `files`. `pipeline::analyze_diff`
//! never actually produces this inconsistency (the graph is always
//! built from, and ids stamped onto, the very same `files` list), but
//! these tests exercise the lookup-miss fallbacks so they stay covered
//! rather than being unreachable-in-practice dead code.

use super::*;
use crate::extract::SymbolKind;
use crate::graph::Edge;
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_skip_definitions_entry_when_visit_order_id_has_no_matching_symbol() {
    // `dfs_pre_order`'s `visit_order` is derived from `graph.nodes`, not
    // `files`, so a node with no matching `ExtractedSymbol` reaches the
    // `let Some(..) = lookup.get(id) else { continue }` branch in the
    // "Definitions" loop; the malformed root must simply be skipped
    // rather than panicking or emitting a broken heading.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
            edges: vec![],
            roots: vec!["src/lib.rs::ghost".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_nothing_for_root_when_root_id_has_no_matching_symbol() {
    // Same lookup miss as above, but hit inside `render_tree_node`'s
    // own `let Some(..) = lookup.get(id) else { return Ok(()) }` guard
    // (the "Change graph" tree-line branch) rather than the
    // "Definitions" loop.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
            edges: vec![],
            roots: vec!["src/lib.rs::ghost".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    // Both malformed-root branches (the "Change graph" line and the
    // "Definitions" entry) are exercised by the same minimal report;
    // asserted together since there is no simpler input that isolates
    // only one of the two lookups.
    assert_eq!(expected, actual);
}

#[test]
fn should_omit_cycle_warning_line_when_cycle_target_id_has_no_matching_symbol() {
    // A cycle edge whose `to` id has no matching symbol hits
    // `render_tree_node`'s inner `let Some(..) = lookup.get(child_id)
    // else { continue }` guard (the cycle-warning branch specifically,
    // as opposed to the two tests above which exercise the
    // non-cycle-edge lookups) — the warning line is simply omitted
    // rather than rendering a broken label.
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
            edges: vec![Edge {
                from: "src/lib.rs::foo".to_string(),
                to: "src/lib.rs::ghost".to_string(),
                is_cycle: true,
            }],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
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
