//! The empty-graph short-circuit and the ADR 0038 `Legend` subgraph every
//! render path shares.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_render_minimal_valid_document_with_legend_when_graph_is_empty() {
    let report = empty_report(
        SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        vec![],
    );

    let expected = format!("flowchart LR\n%% no symbols\n{}", LEGEND_AND_CLASS_DEFS);
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

// NOTE: partial (`contains`) assertions here, not a full-string compare —
// every other test in this module already pins the exact full document
// (including this same trailer via `LEGEND_AND_CLASS_DEFS`); this test's
// purpose is narrower, to have one failure point squarely at the legend
// block itself if its content changes, independent of whichever
// class-assignment test happens to also cover it.
#[test]
fn should_render_legend_subgraph_with_one_styled_node_per_class_when_graph_has_symbols() {
    let report = empty_report(
        SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo", SymbolKind::Function, None)],
        }],
    );

    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert!(
        actual.contains(
            "  subgraph Legend\n\
             \x20   legend_added[\"added\"]\n\
             \x20   legend_changed[\"API changed\"]\n\
             \x20   legend_removed[\"removed\"]\n\
             \x20   legend_fan_in[\"fan-in (in:N)\"]\n\
             \x20 end\n"
        ),
        "expected Legend subgraph block in output, got:\n{actual}"
    );
    assert!(
        actual.contains("  class legend_added added\n"),
        "expected legend_added class assignment, got:\n{actual}"
    );
    assert!(
        actual.contains("  class legend_fan_in fan-in\n"),
        "expected legend_fan_in class assignment, got:\n{actual}"
    );
}
