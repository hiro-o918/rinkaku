//! ADR 0037 `removed`-classed nodes, including the removed-only-file case
//! and the node-budget interaction.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_render_removed_node_in_its_file_subgraph_when_report_has_removed_symbols() {
    // "foo" survives alongside the removed "old_helper" so the expected
    // output can pin ordering: removed after surviving.
    let mut report = empty_report(
        SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                Some(Classification::Added),
            )],
        }],
    );
    report.removed = vec![removed_symbol("old_helper", "src/lib.rs")];

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"foo\"]\n",
        "    n1[\"old_helper\"]\n",
        "  end\n",
        "  class n0 added\n",
        "  class n1 removed\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_removed_only_file_subgraph_when_file_has_no_surviving_symbols() {
    let mut report = empty_report(
        SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        vec![],
    );
    report.removed = vec![removed_symbol("old_only", "src/gone.rs")];

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/gone.rs\"]\n",
        "    n0[\"old_only\"]\n",
        "  end\n",
        "  class n0 removed\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_count_removed_symbols_toward_node_budget_when_deciding_fallback() {
    // 30 head-side nodes alone stay symbol-level (see the sibling boundary
    // test in `node_budget_fallback`); the 1 removed symbol added here
    // must be what tips this report over budget.
    let mut nodes = Vec::new();
    let mut symbols = Vec::new();
    for i in 0..30 {
        let id = format!("src/lib.rs::s{i}");
        nodes.push(node(&id, "src/lib.rs", &format!("s{i}")));
        symbols.push(symbol(&id, &format!("s{i}"), SymbolKind::Function, None));
    }
    let mut report = empty_report(
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
    report.removed = vec![removed_symbol("old_helper", "src/lib.rs")];

    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert!(
        actual.contains("%% aggregated to file level (31 symbols > budget)"),
        "expected file-level fallback comment in output, got:\n{actual}"
    );
}

#[test]
fn should_render_removed_only_file_as_removed_node_when_fallback_fires() {
    // src/a.rs (16 nodes) + src/b.rs (15) already exceed the budget;
    // src/gone.rs adds only a removed symbol, no head-side node.
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

    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/a.rs".to_string(),
                symbols: files_a_symbols,
            },
            FileReport {
                path: "src/b.rs".to_string(),
                symbols: files_b_symbols,
            },
        ],
        skipped: vec![],
        graph: SymbolGraph {
            nodes,
            edges: vec![],
            roots: vec!["src/a.rs::a0".to_string(), "src/b.rs::b0".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![removed_symbol("old_only", "src/gone.rs")],
    };

    let expected = concat!(
        "flowchart LR\n",
        "%% aggregated to file level (32 symbols > budget)\n",
        "  n0[\"src/a.rs\"]\n",
        "  n1[\"src/b.rs\"]\n",
        "  n2[\"src/gone.rs\"]\n",
        "  class n0 changed\n",
        "  class n2 removed\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}
