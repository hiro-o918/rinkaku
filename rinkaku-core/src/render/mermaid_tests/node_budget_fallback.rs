//! The [`super::MERMAID_NODE_BUDGET`] boundary and the file-level
//! aggregation fallback.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_stay_symbol_level_when_node_count_equals_budget_exactly() {
    // Exactly MERMAID_NODE_BUDGET (30) nodes: the fallback condition is
    // `> budget`, so this boundary case must still render one
    // subgraph/node per symbol, not the file-level aggregation — pins the
    // off-by-one the sibling over-budget test alone can't rule out (that
    // test only proves 31 falls back, not that 30 doesn't).
    let mut nodes = Vec::new();
    let mut symbols = Vec::new();
    for i in 0..30 {
        let id = format!("src/lib.rs::s{i}");
        nodes.push(node(&id, "src/lib.rs", &format!("s{i}")));
        symbols.push(symbol(&id, &format!("s{i}"), SymbolKind::Function, None));
    }
    assert_eq!(30, nodes.len());

    let report = empty_report(
        SymbolGraph {
            nodes,
            edges: vec![],
            roots: vec!["src/lib.rs::s0".to_string()],
        },
        vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols,
        }],
    );

    let mut expected = String::from("flowchart LR\n  subgraph sub0[\"src/lib.rs\"]\n");
    for i in 0..30 {
        expected.push_str(&format!("    n{i}[\"s{i}\"]\n"));
    }
    expected.push_str("  end\n");
    expected.push_str(LEGEND_AND_CLASS_DEFS);

    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_fall_back_to_file_level_graph_when_node_count_exceeds_budget() {
    // 31 nodes (one over MERMAID_NODE_BUDGET's 30) across two files: 16 in
    // src/a.rs (one classified Added, so a.rs is "changed"), 15 in
    // src/b.rs. Two edges cross from a.rs to b.rs (aggregated with count
    // 2); one edge stays within a.rs (dropped: an intra-file edge carries
    // no file-level signal).
    let mut nodes = Vec::new();
    let mut files_a_symbols = Vec::new();
    for i in 0..16 {
        let id = format!("src/a.rs::a{i}");
        nodes.push(node(&id, "src/a.rs", &format!("a{i}")));
        let classification = if i == 0 {
            Some(Classification::Added)
        } else {
            None
        };
        files_a_symbols.push(symbol(
            &id,
            &format!("a{i}"),
            SymbolKind::Function,
            classification,
        ));
    }
    let mut files_b_symbols = Vec::new();
    for i in 0..15 {
        let id = format!("src/b.rs::b{i}");
        nodes.push(node(&id, "src/b.rs", &format!("b{i}")));
        files_b_symbols.push(symbol(&id, &format!("b{i}"), SymbolKind::Function, None));
    }
    assert_eq!(31, nodes.len());

    let edges = vec![
        Edge {
            from: "src/a.rs::a0".to_string(),
            to: "src/b.rs::b0".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/a.rs::a1".to_string(),
            to: "src/b.rs::b1".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/a.rs::a0".to_string(),
            to: "src/a.rs::a1".to_string(),
            is_cycle: false,
        },
    ];
    let roots = vec!["src/a.rs::a0".to_string(), "src/b.rs::b0".to_string()];

    let report = empty_report(
        SymbolGraph {
            nodes,
            edges,
            roots,
        },
        vec![
            FileReport {
                path: "src/a.rs".to_string(),
                symbols: files_a_symbols,
            },
            FileReport {
                path: "src/b.rs".to_string(),
                symbols: files_b_symbols,
            },
        ],
    );

    let expected = concat!(
        "flowchart LR\n",
        "%% aggregated to file level (31 symbols > budget)\n",
        "  n0[\"src/a.rs\"]\n",
        "  n1[\"src/b.rs\"]\n",
        "  n0 -- 2 --> n1\n",
        "  class n0 changed\n",
    )
    .to_string()
        + LEGEND_AND_CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}
