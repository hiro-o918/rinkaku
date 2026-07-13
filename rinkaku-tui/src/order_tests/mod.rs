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
//!   recursion, the branching-intermediate integration test, and (ADR
//!   0035) a trailing `Section` root sorting after every `Dir`/`File`
//!   with its own children left untouched
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
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
}

/// Same as [`report_with_graph`], plus `files`: needed by tests that pin
/// [`crate::order::DirCondensation::build`]'s test-node/edge exclusion
/// (ADR 0035), which reads `report.files[..].symbols[..].is_test` to
/// decide which `graph.nodes`/`graph.edges` to drop before ranking —
/// unlike every other `rank_directories`/`cycle_partners`/`cycle_edges`
/// concern, which is graph-only (see
/// `rank_directories.rs`'s `should_ignore_files_field_and_rank_from_graph_alone`).
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

/// Same as [`symbol`], but marked as test code (`is_test: true`) — for
/// pinning ADR 0035's rank-exclusion behavior, which keys off this flag.
pub(super) fn test_symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        is_test: true,
        ..symbol(id, name)
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

/// A `NodeKind::Section(SectionKind::Tests)` root (ADR 0035 Phase B) —
/// always keyed by [`crate::tree::TESTS_SECTION_PATH`], matching
/// `build_tree`'s own construction, so `order_tree`'s sort sees the same
/// path a real `Tree` would carry.
pub(super) fn section_node(children: Vec<crate::tree::TreeNode>) -> crate::tree::TreeNode {
    crate::tree::TreeNode {
        kind: crate::tree::NodeKind::Section(crate::tree::SectionKind::Tests),
        path: crate::tree::TESTS_SECTION_PATH.to_string(),
        badges: crate::tree::Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}
