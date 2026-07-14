//! Tests for `crate::order::sort`: directories before files before a
//! trailing section, alphabetical mode, topological mode, unranked
//! fallback, recursion, the branching-intermediate integration test, and
//! (ADR 0035) `Section` sorting/children handling.

use super::*;
use crate::order::rank_directories;
use rinkaku_core::graph::{Edge, Node, SymbolGraph};
use rinkaku_core::render::Report;

mod order_tree;

pub(super) fn node(id: &str, path: &str, name: &str) -> Node {
    Node {
        id: id.to_string(),
        path: path.to_string(),
        name: name.to_string(),
        is_test: false,
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
        file_size_bands: vec![],
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

/// A `NodeKind::Section(SectionKind::Tests)` root (ADR 0035 Phase B),
/// keyed by [`crate::tree::TESTS_SECTION_PATH`] to match `build_tree`'s
/// own construction.
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
