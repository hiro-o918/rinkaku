//! Tests for `crate::order`, split from the source file (ADR 0028) and
//! grouped by which pub / private helper each block pins:
//!
//! - `rank_directories` — direct coverage of `rank_directories`: empty
//!   graph, no-edge zero-rank, cross-dir caller/callee ordering, cycle
//!   membership, and root-level / graph-only / chain cases
//! - `cycle_partners` — `cycle_partners` map: empty, pair, three-way
//! - `cycle_edges` — `cycle_edges` list: cross-dir, exclude non-cycle,
//!   exclude intra-dir
//! - `order_tree` — `order_tree` behavior over a `Tree`: directories
//!   before files, alphabetical mode, topological mode, unranked fallback,
//!   recursion, and the branching-intermediate integration test
//! - `scc_helpers` — direct unit coverage of `tarjan_sccs` and
//!   `topological_scc_order` at their raw `usize`-adjacency contract

use super::*;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::{Edge, Node, SymbolGraph};
use rinkaku_core::render::FileReport;

mod cycle_edges;
mod cycle_partners;
mod order_tree;
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
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
}

pub(super) fn dir_node(path: &str, children: Vec<crate::tree::TreeNode>) -> crate::tree::TreeNode {
    crate::tree::TreeNode {
        kind: crate::tree::NodeKind::Dir,
        path: path.to_string(),
        badges: crate::tree::Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn file_node(path: &str) -> crate::tree::TreeNode {
    crate::tree::TreeNode {
        kind: crate::tree::NodeKind::File,
        path: path.to_string(),
        badges: crate::tree::Badges::default(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn file_node_with_children(
    path: &str,
    children: Vec<crate::tree::TreeNode>,
) -> crate::tree::TreeNode {
    crate::tree::TreeNode {
        kind: crate::tree::NodeKind::File,
        path: path.to_string(),
        badges: crate::tree::Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}
