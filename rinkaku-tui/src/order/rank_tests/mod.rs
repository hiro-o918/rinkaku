//! Tests for `crate::order::rank`, grouped by which function each block
//! pins:
//!
//! - `rank_directories` — empty graph, no-edge zero-rank, caller/callee
//!   ordering, cycle membership, root-level/graph-only/chain cases, and
//!   the ADR 0035 test-symbol/edge exclusion
//! - `cycle_partners` — empty, pair, three-way cycle membership
//! - `cycle_edges` — cross-dir, exclude non-cycle, exclude intra-dir
//! - `scc_helpers` — `tarjan_sccs`/`topological_scc_order` at their raw
//!   `usize`-adjacency contract

use super::*;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::{Edge, Node, SymbolGraph};
use rinkaku_core::render::FileReport;

mod cycle_edges;
mod cycle_partners;
mod rank_directories;
mod scc_helpers;

pub(super) fn symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range: rinkaku_core::diff::LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

pub(super) fn node(id: &str, path: &str, name: &str) -> Node {
    Node {
        id: id.to_string(),
        path: path.to_string(),
        name: name.to_string(),
    }
}

pub(super) fn report_with_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes,
            edges,
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
}

/// Same as [`report_with_graph`], plus `files`: needed by tests pinning
/// the ADR 0035 test-node/edge exclusion, which reads
/// `report.files[..].symbols[..].is_test` to decide which
/// `graph.nodes`/`graph.edges` to drop before ranking.
pub(super) fn report_with_graph_and_files(
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    files: Vec<FileReport>,
) -> Report {
    Report {
        files,
        ..report_with_graph(nodes, edges)
    }
}

/// Same as [`symbol`], but marked as test code (`is_test: true`).
pub(super) fn test_symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        is_test: true,
        ..symbol(id, name)
    }
}
