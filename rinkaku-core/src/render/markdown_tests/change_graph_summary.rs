//! `change_graph_summary` and the "## Change graph"/"## Repository
//! graph" heading: pin the singular/plural nouns, the `— most in ...`
//! hotspot suffix and its tie-breaking, and the origin-dependent
//! wording chosen for `ReportOrigin::RepoOutline` (ADR 0017).

use super::*;
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_render_change_graph_and_definitions_when_report_has_one_symbol() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo(a: i32) -> i32",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![],
        hotspots: vec![],
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
fn foo(a: i32) -> i32
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_summary_with_hotspot_when_report_has_multiple_symbols_and_files() {
    // 5 nodes across store/items.go (3, the hotspot) and store/db.go (2)
    // — pins the plural "changed symbols"/"files" wording together with
    // the "— most in ..." suffix and its count.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("store/items.go::A", "store/items.go", "A"),
                node("store/items.go::B", "store/items.go", "B"),
                node("store/items.go::C", "store/items.go", "C"),
                node("store/db.go::D", "store/db.go", "D"),
                node("store/db.go::E", "store/db.go", "E"),
            ],
            edges: vec![],
            roots: vec![
                "store/items.go::A".to_string(),
                "store/items.go::B".to_string(),
                "store/items.go::C".to_string(),
                "store/db.go::D".to_string(),
                "store/db.go::E".to_string(),
            ],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

5 changed symbols in 2 files — most in store/items.go (3)


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// ADR 0017: a whole-repo outline has no diff, so "Change graph"/
// "changed symbols" would misdescribe it — this pins the alternate
// heading and noun `ReportOrigin::RepoOutline` selects, using the same
// multi-file/hotspot shape as
// `should_render_summary_with_hotspot_when_report_has_multiple_symbols_and_files`
// so the two tests differ only in `origin` and its wording.
#[test]
fn should_render_repository_graph_heading_and_drop_changed_wording_when_origin_is_repo_outline() {
    let report = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("store/items.go::A", "store/items.go", "A"),
                node("store/items.go::B", "store/items.go", "B"),
                node("store/items.go::C", "store/items.go", "C"),
                node("store/db.go::D", "store/db.go", "D"),
                node("store/db.go::E", "store/db.go", "E"),
            ],
            edges: vec![],
            roots: vec![
                "store/items.go::A".to_string(),
                "store/items.go::B".to_string(),
                "store/items.go::C".to_string(),
                "store/db.go::D".to_string(),
                "store/db.go::E".to_string(),
            ],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Repository graph

5 symbols in 2 files — most in store/items.go (3)


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_hotspot_suffix_when_all_symbols_are_in_one_file() {
    // Every node lives in the same file, so naming "the file with the
    // most nodes" would be redundant — the suffix must be dropped
    // entirely, not degenerate into e.g. "(2)".
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::a", "src/lib.rs", "a"),
                node("src/lib.rs::b", "src/lib.rs", "b"),
            ],
            edges: vec![],
            roots: vec!["src/lib.rs::a".to_string(), "src/lib.rs::b".to_string()],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

2 changed symbols in 1 file


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_break_hotspot_tie_by_first_seen_path_order_when_counts_are_equal() {
    // b.rs and a.rs both have 2 nodes each; b.rs's node appears first in
    // `graph.nodes`, so it must win the tie over a.rs despite sorting
    // after it alphabetically — the tie-break is source order, not a
    // path-string comparison.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("b.rs::x", "b.rs", "x"),
                node("a.rs::y", "a.rs", "y"),
                node("b.rs::z", "b.rs", "z"),
                node("a.rs::w", "a.rs", "w"),
            ],
            edges: vec![],
            roots: vec![
                "b.rs::x".to_string(),
                "a.rs::y".to_string(),
                "b.rs::z".to_string(),
                "a.rs::w".to_string(),
            ],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

4 changed symbols in 2 files — most in b.rs (2)


## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_include_start_line_in_label_when_node_id_is_disambiguated_by_line() {
    // `graph::collect_nodes` appends `@{start_line}` to a node's id only
    // when its `(path, name)` pair is not unique in the report (e.g.
    // two overloaded free functions sharing a name). Without a visible
    // line number, "Change graph"/"Definitions" would show two
    // identical-looking `fn foo (src/lib.rs)` entries with no way to
    // tell them apart.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 1, end: 3 },
                    ..symbol(
                        "src/lib.rs::foo@1",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(a: i32)",
                    )
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 12 },
                    ..symbol(
                        "src/lib.rs::foo@10",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(a: i32, b: i32)",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo@1", "src/lib.rs", "foo"),
                node("src/lib.rs::foo@10", "src/lib.rs", "foo"),
            ],
            edges: vec![],
            roots: vec![
                "src/lib.rs::foo@1".to_string(),
                "src/lib.rs::foo@10".to_string(),
            ],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

2 changed symbols in 1 file

- fn foo (src/lib.rs:1)
- fn foo (src/lib.rs:10)

## Definitions

### fn foo (src/lib.rs:1)

```
fn foo(a: i32)
```

### fn foo (src/lib.rs:10)

```
fn foo(a: i32, b: i32)
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
