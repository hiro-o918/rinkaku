//! The empty-graph short-circuit and the `classDef` trailer every render
//! path shares (ADR 0040: the in-diagram `Legend` subgraph ADR 0039 added
//! is gone — the Markdown legend now lives in
//! `compose_and_post_comment.sh`, generated from these same `classDef`
//! lines).

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_render_minimal_valid_document_with_class_defs_when_graph_is_empty() {
    let report = empty_report(
        SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        vec![],
    );

    let expected = format!("flowchart LR\n%% no symbols\n{}", CLASS_DEFS);
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_class_defs_with_no_legend_subgraph_when_graph_has_symbols() {
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

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"foo\"]\n",
        "  end\n",
        "  class n0 referenced\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}
