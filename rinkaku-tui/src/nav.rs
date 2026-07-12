//! Navigation state machine over a flattened, visible view of a
//! [`crate::tree::Tree`] (ADR 0015/0016): cursor movement, expand/collapse,
//! and a stable mapping from the cursor back to the underlying node so a
//! future detail pane knows what is selected.
//!
//! Every transition here is pure — `Nav::handle` takes an [`Action`] and
//! returns the next `Nav`, no IO, no `ratatui`/`crossterm` types in any
//! signature (ADR 0016 decision 3).

use crate::tree::{NodeKind, Tree, TreeNode};
use std::collections::HashSet;

/// A user-driven navigation action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CursorUp,
    CursorDown,
    /// Toggles the node under the cursor between expanded and collapsed.
    /// A no-op on a [`NodeKind::Symbol`] row (symbols are leaves — see
    /// this module's doc comment) or when there are no visible rows at
    /// all.
    ToggleExpand,
    ExpandAll,
    CollapseAll,
}

/// One visible row in the flattened tree: a reference into the [`Tree`]
/// this [`Nav`] was built from (by path, not by owned data — the view-
/// model stays cheap to rebuild every frame, matching `ratatui`'s
/// immediate-mode redraw-from-state model per ADR 0016 decision 1) plus
/// its depth for indentation and whether it is currently expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<'a> {
    pub node: &'a TreeNode,
    pub depth: usize,
    /// `true` when this row has children *and* they are currently shown
    /// beneath it. Always `false` for a childless node (a file with no
    /// symbols, or a symbol leaf) — there is nothing to expand, regardless
    /// of collapse-state bookkeeping.
    pub expanded: bool,
}

/// Navigation state: which nodes are collapsed (keyed by [`TreeNode::path`]
/// — stable across tree rebuilds from a re-run `Report`, per
/// `crate::tree::TreeNode::path`'s own doc comment) and where the cursor
/// sits, as a position in the *current* flattened visible-row list rather
/// than a node reference — the row list is recomputed from `collapsed` on
/// every call to [`Nav::rows`], so the cursor position is the only stable
/// coordinate to keep between calls.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Nav {
    collapsed: HashSet<String>,
    cursor: usize,
}

impl Nav {
    /// Starts with every directory/file expanded (`collapsed` empty) and
    /// the cursor on the first visible row — the most useful default for
    /// a reviewer opening the TUI fresh: everything is already visible,
    /// nothing needs an initial expand pass.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current cursor position, as an index into [`Nav::rows`]'s
    /// result for the same `tree`. Exposed so a caller building a detail
    /// pane can look up `rows(tree)[cursor()]` without this module
    /// duplicating that indexing itself.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Flattens `tree` into visible [`Row`]s given this `Nav`'s collapse
    /// state: a pre-order walk that skips a node's children whenever that
    /// node's path is in `collapsed`. Recomputed from `tree` + `collapsed`
    /// on every call rather than cached, matching the immediate-mode
    /// philosophy the rest of the TUI stack follows (ADR 0016 decision 1)
    /// — the tree itself is already cheap to rebuild from a `Report`, and
    /// this flattening is cheaper still.
    pub fn rows<'a>(&self, tree: &'a Tree) -> Vec<Row<'a>> {
        let mut rows = Vec::new();
        for root in &tree.roots {
            self.push_rows(root, 0, &mut rows);
        }
        rows
    }

    fn push_rows<'a>(&self, node: &'a TreeNode, depth: usize, rows: &mut Vec<Row<'a>>) {
        let has_children = !node.children.is_empty();
        let expanded = has_children && !self.collapsed.contains(&node.path);
        rows.push(Row {
            node,
            depth,
            expanded,
        });
        if expanded {
            for child in &node.children {
                self.push_rows(child, depth + 1, rows);
            }
        }
    }

    /// Applies one [`Action`] against the current `tree`, returning the
    /// resulting `Nav`. `tree` must be the same tree (or a tree with the
    /// same shape/paths) the caller is rendering rows from — recomputing
    /// `rows(tree)` internally is how cursor bounds and "what is under the
    /// cursor" are determined, consistent with `rows` never being cached
    /// (see its own doc comment).
    pub fn handle(mut self, action: Action, tree: &Tree) -> Self {
        match action {
            Action::CursorUp => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            Action::CursorDown => {
                let row_count = self.rows(tree).len();
                if row_count > 0 {
                    self.cursor = (self.cursor + 1).min(row_count - 1);
                }
            }
            Action::ToggleExpand => {
                let rows = self.rows(tree);
                if let Some(row) = rows.get(self.cursor)
                    && !matches!(row.node.kind, NodeKind::Symbol(_))
                    && !row.node.children.is_empty()
                {
                    let path = row.node.path.clone();
                    if !self.collapsed.remove(&path) {
                        self.collapsed.insert(path);
                    }
                }
            }
            Action::ExpandAll => {
                self.collapsed.clear();
            }
            Action::CollapseAll => {
                self.collapsed = collapsible_paths(tree);
            }
        }
        self
    }
}

/// Every path in `tree` that has at least one child — i.e. every
/// directory/file node eligible to collapse (symbols never are, see this
/// module's doc comment) — used by [`Action::CollapseAll`] to mark every
/// such node collapsed in one step.
fn collapsible_paths(tree: &Tree) -> HashSet<String> {
    let mut paths = HashSet::new();
    for root in &tree.roots {
        collect_collapsible(root, &mut paths);
    }
    paths
}

fn collect_collapsible(node: &TreeNode, paths: &mut HashSet<String>) {
    if !node.children.is_empty() {
        paths.insert(node.path.clone());
        for child in &node.children {
            collect_collapsible(child, paths);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Badges;
    use pretty_assertions::assert_eq;

    fn symbol_node(path: &str, name: &str) -> TreeNode {
        TreeNode {
            kind: NodeKind::Symbol(crate::tree::SymbolRef {
                id: format!("{path}::{name}"),
                name: name.to_string(),
                kind: rinkaku_core::extract::SymbolKind::Function,
                classification: None,
                removed: false,
            }),
            path: path.to_string(),
            badges: Badges::default(),
            children: vec![],
        }
    }

    fn file_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges: Badges::default(),
            children,
        }
    }

    fn dir_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            kind: NodeKind::Dir,
            path: path.to_string(),
            badges: Badges::default(),
            children,
        }
    }

    fn sample_tree() -> Tree {
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

    #[test]
    fn should_show_every_row_expanded_when_nav_is_new() {
        let tree = sample_tree();
        let nav = Nav::new();

        let rows = nav.rows(&tree);

        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        assert_eq!(vec!["src", "src/lib.rs", "src/lib.rs", "src/lib.rs"], paths);
    }

    #[test]
    fn should_hide_children_when_toggle_expand_collapses_the_dir_under_cursor() {
        let tree = sample_tree();
        let nav = Nav::new(); // cursor at 0 ("src")

        let nav = nav.handle(Action::ToggleExpand, &tree);
        let rows = nav.rows(&tree);

        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        assert_eq!(vec!["src"], paths);
        assert_eq!(false, rows[0].expanded);
    }

    #[test]
    fn should_show_children_again_when_toggle_expand_is_applied_twice() {
        let tree = sample_tree();
        let nav = Nav::new()
            .handle(Action::ToggleExpand, &tree)
            .handle(Action::ToggleExpand, &tree);

        let rows = nav.rows(&tree);

        assert_eq!(4, rows.len());
        assert_eq!(true, rows[0].expanded);
    }

    #[test]
    fn should_not_toggle_when_cursor_is_on_a_symbol_leaf() {
        let tree = sample_tree();
        // Move cursor down twice: src (0) -> src/lib.rs (1) -> foo (2).
        let nav = Nav::new()
            .handle(Action::CursorDown, &tree)
            .handle(Action::CursorDown, &tree);
        assert_eq!(2, nav.cursor());

        let nav = nav.handle(Action::ToggleExpand, &tree);
        let rows = nav.rows(&tree);

        // Nothing collapsed: still every row visible.
        assert_eq!(4, rows.len());
    }

    #[test]
    fn should_clamp_cursor_at_zero_when_cursor_up_past_the_top() {
        let tree = sample_tree();
        let nav = Nav::new().handle(Action::CursorUp, &tree);

        assert_eq!(0, nav.cursor());
    }

    #[test]
    fn should_clamp_cursor_at_last_row_when_cursor_down_past_the_bottom() {
        let tree = sample_tree();
        let mut nav = Nav::new();
        for _ in 0..10 {
            nav = nav.handle(Action::CursorDown, &tree);
        }

        assert_eq!(3, nav.cursor());
    }

    #[test]
    fn should_move_cursor_down_one_row_at_a_time() {
        let tree = sample_tree();
        let nav = Nav::new().handle(Action::CursorDown, &tree);

        assert_eq!(1, nav.cursor());
    }

    #[test]
    fn should_collapse_every_dir_and_file_when_collapse_all_is_applied() {
        let tree = sample_tree();
        let nav = Nav::new().handle(Action::CollapseAll, &tree);

        let rows = nav.rows(&tree);

        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        assert_eq!(vec!["src"], paths);
    }

    #[test]
    fn should_expand_every_node_when_expand_all_is_applied_after_collapse_all() {
        let tree = sample_tree();
        let nav = Nav::new()
            .handle(Action::CollapseAll, &tree)
            .handle(Action::ExpandAll, &tree);

        let rows = nav.rows(&tree);

        assert_eq!(4, rows.len());
    }

    #[test]
    fn should_report_row_as_not_expanded_when_node_has_no_children() {
        // A childless file (e.g. a pure rename with no symbols) can never
        // be "expanded" — nothing to show — regardless of collapse state.
        let tree = Tree {
            roots: vec![file_node("renamed.rs", vec![])],
        };
        let nav = Nav::new();

        let rows = nav.rows(&tree);

        assert_eq!(1, rows.len());
        assert_eq!(false, rows[0].expanded);
    }

    #[test]
    fn should_keep_collapse_state_stable_across_a_tree_rebuild_with_same_paths() {
        // Simulates a Report re-run producing a structurally identical
        // tree (same paths) — collapse state, keyed by path, must survive.
        let tree_v1 = sample_tree();
        let nav = Nav::new().handle(Action::ToggleExpand, &tree_v1);

        let tree_v2 = sample_tree(); // fresh tree, same paths/shape
        let rows = nav.rows(&tree_v2);

        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        assert_eq!(vec!["src"], paths);
    }
}
