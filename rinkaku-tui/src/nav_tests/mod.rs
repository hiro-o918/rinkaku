//! Tests for `crate::nav`, split from the source file (ADR 0028) and
//! grouped by which `Nav` concern each block pins:
//!
//! - `move_cursor` — `move_cursor_to_path`/`move_cursor_to_symbol`:
//!   directory/file/symbol targeting, collapsed-ancestor handling
//! - `expand_collapse` — `ToggleExpand`/`ExpandAll`/`CollapseAll`, cursor
//!   clamping, collapse-state stability across a tree rebuild
//! - `retarget_cursor` — the CRITICAL regressions: cursor re-targeting
//!   when a collapse hides the cursor's own row, elsewhere in the tree,
//!   or via `CollapseAll`
//! - `section_crossing` — ADR 0035 Phase B: `Nav` treating a
//!   `NodeKind::Section` like a `Dir` for expand/collapse and cursor
//!   traversal across the production/Tests boundary
//! - `default_collapse` — visual-encoding prototype:
//!   `Nav::new_collapsing_test_groups`'s initial collapse seeding

use super::*;
use crate::tree::Badges;

mod default_collapse;
mod expand_collapse;
mod move_cursor;
mod retarget_cursor;
mod section_crossing;

pub(super) fn symbol_node(path: &str, name: &str) -> TreeNode {
    TreeNode {
        kind: NodeKind::Symbol(crate::tree::SymbolRef {
            id: format!("{path}::{name}"),
            name: name.to_string(),
            kind: rinkaku_core::extract::SymbolKind::Function,
            classification: None,
            removed: false,
            is_test: false,
        }),
        path: path.to_string(),
        badges: Badges::default(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn file_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges: Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn dir_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::Dir,
        path: path.to_string(),
        badges: Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

/// A `NodeKind::Section(SectionKind::Tests)` node (ADR 0035 Phase B),
/// keyed by the same synthetic path `crate::tree::build_tree` uses.
pub(super) fn section_node(children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::Section(crate::tree::SectionKind::Tests),
        path: crate::tree::TESTS_SECTION_PATH.to_string(),
        badges: Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

/// A `NodeKind::TestGroup` node (visual-encoding prototype), keyed by the
/// same `{file_path}::tests` synthetic path `crate::tree::build_tree`
/// uses for a mixed file's test-symbol group.
pub(super) fn test_group_node(file_path: &str, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::TestGroup {
            count: children.len(),
        },
        path: format!("{file_path}::tests"),
        badges: Badges::default(),
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn sample_tree() -> Tree {
    // src/
    //   lib.rs
    //     fn foo
    //     fn bar
    Tree {
        roots: vec![dir_node(
            "src",
            vec![file_node(
                "src/lib.rs",
                vec![
                    symbol_node("src/lib.rs", "foo"),
                    symbol_node("src/lib.rs", "bar"),
                ],
            )],
        )],
    }
}

/// A deeper tree than `sample_tree` — src/pkg/lib.rs with two symbol
/// leaves — so the cursor can sit on a row nested under two ancestor
/// directories, matching the depth the CRITICAL 2 regression needs
/// (collapsing an ancestor several levels above the cursor).
/// Expanded row order: src(0), src/pkg(1), src/pkg/lib.rs(2), foo(3),
/// bar(4).
pub(super) fn deep_tree() -> Tree {
    Tree {
        roots: vec![dir_node(
            "src",
            vec![dir_node(
                "src/pkg",
                vec![file_node(
                    "src/pkg/lib.rs",
                    vec![
                        symbol_node("src/pkg/lib.rs", "foo"),
                        symbol_node("src/pkg/lib.rs", "bar"),
                    ],
                )],
            )],
        )],
    }
}

/// A production `Dir` root plus a trailing `Section` root (ADR 0035
/// Phase B), each with one nested file — used to pin `Nav`'s
/// expand/collapse/cursor behavior *across* the production/Tests
/// boundary, which none of `sample_tree`'s all-`Dir` shape exercises.
/// Expanded row order: src(0), src/lib.rs(1), Tests(2),
/// Tests-section-child a_test.go(3).
pub(super) fn tree_with_section() -> Tree {
    Tree {
        roots: vec![
            dir_node("src", vec![file_node("src/lib.rs", vec![])]),
            section_node(vec![file_node("a_test.go", vec![])]),
        ],
    }
}

pub(super) fn row_paths<'a>(rows: &'a [Row<'a>]) -> Vec<&'a str> {
    rows.iter().map(|r| r.node.path.as_str()).collect()
}

/// The `name` of a [`NodeKind::Symbol`] row — panics on any other row
/// kind, since every call site above already knows (from its own
/// fixture) that the cursor should have landed on a symbol row.
pub(super) fn symbol_name<'a>(row: &Row<'a>) -> &'a str {
    match &row.node.kind {
        NodeKind::Symbol(symbol_ref) => symbol_ref.name.as_str(),
        other => panic!("expected NodeKind::Symbol, got {other:?}"),
    }
}
