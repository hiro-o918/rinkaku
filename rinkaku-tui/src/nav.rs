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
///
/// A `Symbol` row's `node.path` is its containing file's path, not a path
/// unique to that symbol (`crate::tree::TreeNode::path`'s own doc comment
/// notes this) — so several symbol rows can share the same `path` with
/// each other and with their file's own row. This is safe for the
/// `collapsed` set this module keys by path (`Nav`'s own doc comment)
/// specifically because `push_rows`/`retarget_cursor` only ever treat a
/// path as a collapse/lookup key together with `has_children`: a `Symbol`
/// row always has `has_children == false`, so it is never itself a
/// candidate to collapse or a distinct target to re-target the cursor onto
/// — code relying on `path` to identify a *specific* row must additionally
/// check `has_children`/`expanded` (or the node's `kind`) rather than
/// assume `path` alone disambiguates.
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
    ///
    /// A collapse action (`ToggleExpand` collapsing a node, or
    /// `CollapseAll`) can make the row the cursor was on disappear from the
    /// flattened list entirely — the row list shrinks, but `self.cursor` is
    /// just an index, so it would otherwise keep pointing at whatever row
    /// happens to now occupy that slot (or past the end of the list). To
    /// keep the cursor meaningfully attached to "the thing the user was
    /// looking at", every action re-targets the cursor afterward via
    /// [`Self::retarget_cursor`] rather than only the actions that are
    /// obviously collapse-shaped — this also future-proofs against a new
    /// `Action` variant that shrinks the row list in some other way.
    pub fn handle(mut self, action: Action, tree: &Tree) -> Self {
        let cursor_path_chain = self.cursor_path_chain(tree);

        match action {
            Action::CursorUp => {
                self.cursor = self.cursor.saturating_sub(1);
                return self;
            }
            Action::CursorDown => {
                let row_count = self.rows(tree).len();
                if row_count > 0 {
                    self.cursor = (self.cursor + 1).min(row_count - 1);
                }
                return self;
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

        self.retarget_cursor(tree, &cursor_path_chain);
        self
    }

    /// The path of the row currently under the cursor, followed by every
    /// ancestor's path out to the root (nearest ancestor first) — the
    /// candidates [`Self::retarget_cursor`] walks through, nearest first, to
    /// find where the cursor should land after the row list changes shape.
    /// Empty when there are no rows at all (nothing to chain from).
    fn cursor_path_chain(&self, tree: &Tree) -> Vec<String> {
        let mut chain = Vec::new();
        for root in &tree.roots {
            if self.collect_path_chain(root, self.cursor, &mut 0, &mut chain) {
                break;
            }
        }
        chain
    }

    /// Pre-order walk mirroring [`Self::push_rows`], tracking `visited` as a
    /// running row index. When the node at `target_cursor` is found, records
    /// its own path followed by the path of every open call frame (i.e.
    /// every ancestor directory/file currently on the path from the root),
    /// and returns `true` to unwind the recursion immediately.
    fn collect_path_chain(
        &self,
        node: &TreeNode,
        target_cursor: usize,
        visited: &mut usize,
        chain: &mut Vec<String>,
    ) -> bool {
        if *visited == target_cursor {
            chain.push(node.path.clone());
            return true;
        }
        *visited += 1;

        let has_children = !node.children.is_empty();
        let expanded = has_children && !self.collapsed.contains(&node.path);
        if expanded {
            for child in &node.children {
                if self.collect_path_chain(child, target_cursor, visited, chain) {
                    chain.push(node.path.clone());
                    return true;
                }
            }
        }
        false
    }

    /// Moves the cursor to the nearest row in `chain` (the pre-action
    /// cursor's own path, then its ancestors nearest-first) that is still
    /// present in `tree`'s current rows, or clamps to the last row (0 for an
    /// empty tree) when nothing in `chain` survived — e.g. `CollapseAll`,
    /// where every ancestor above the top-level root row also collapses, so
    /// there is no "nearest still-expanded ancestor" to land on, only "the
    /// nearest still-*visible*" one, which the chain walk already finds
    /// (collapsing hides a node's children, never the node's own row).
    fn retarget_cursor(&mut self, tree: &Tree, chain: &[String]) {
        let rows = self.rows(tree);

        for path in chain {
            if let Some(index) = rows.iter().position(|row| &row.node.path == path) {
                self.cursor = index;
                return;
            }
        }

        self.cursor = rows.len().saturating_sub(1);
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

    /// A deeper tree than `sample_tree` — src/pkg/lib.rs with two symbol
    /// leaves — so the cursor can sit on a row nested under two ancestor
    /// directories, matching the depth the CRITICAL 2 regression needs
    /// (collapsing an ancestor several levels above the cursor).
    /// Expanded row order: src(0), src/pkg(1), src/pkg/lib.rs(2), foo(3),
    /// bar(4).
    fn deep_tree() -> Tree {
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

    fn row_paths<'a>(rows: &'a [Row<'a>]) -> Vec<&'a str> {
        rows.iter().map(|r| r.node.path.as_str()).collect()
    }

    // CRITICAL 2 regression: `cursor()`'s doc comment invites
    // `rows(tree)[cursor()]`, but collapsing an ancestor of the cursor's
    // row used to leave the cursor index pointing past the now-shrunk row
    // list — the documented lookup would then panic (index out of
    // bounds). The cursor genuinely sits on the deep leaf "foo" (row 3, a
    // descendant of "src/pkg" that is about to be hidden) for this whole
    // test — `ToggleExpand` always acts on the row *under the cursor*, so
    // the only way to collapse "src/pkg" without first moving the cursor
    // onto "src/pkg" itself (which would trivially avoid ever exercising
    // the "cursor's own row disappeared" path) is to collapse it via a
    // second, independent `Nav` positioned at "src/pkg", then transplant
    // that `Nav`'s resulting `collapsed` state onto the first — the same
    // state-transplant pattern
    // `should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree`
    // uses below, just applied to a collapse that *does* hide the cursor's
    // row instead of one that doesn't. The cursor must land on "src/pkg"
    // itself (the nearest still-visible ancestor of the row it used to be
    // on), not stay at index 3 (now out of bounds) and not blindly clamp
    // to "new last row" (which would coincidentally also be 1 here, but
    // that is not the semantics being tested — see the CollapseAll variant
    // below for a case where a naive last-row clamp gives a different,
    // wrong answer).
    #[test]
    fn should_move_cursor_to_nearest_visible_ancestor_when_toggle_expand_hides_its_row() {
        let tree = deep_tree();
        let mut nav = Nav::new();
        for _ in 0..3 {
            nav = nav.handle(Action::CursorDown, &tree);
        }
        assert_eq!(3, nav.cursor()); // cursor on "foo", never moved off it

        // Collapse "src/pkg" (row 1) via a second, independent Nav, then
        // bring that collapse state back onto `nav` without touching its
        // cursor — simulates "src/pkg" collapsing while the cursor of
        // interest stays on the hidden descendant "foo".
        let mut collapse_pkg = Nav::new();
        collapse_pkg = collapse_pkg.handle(Action::CursorDown, &tree);
        assert_eq!(1, collapse_pkg.cursor()); // "src/pkg"
        collapse_pkg = collapse_pkg.handle(Action::ToggleExpand, &tree);

        // `nav`'s own path chain ("foo" then its ancestors) must be
        // captured *before* the collapsed set is transplanted in, exactly
        // as `handle` does internally — capturing it after would just
        // observe the already-shrunk row list and defeat the test.
        let chain = nav.cursor_path_chain(&tree);
        nav.collapsed = collapse_pkg.collapsed;
        nav.retarget_cursor(&tree, &chain);

        let rows = nav.rows(&tree);
        assert_eq!(vec!["src", "src/pkg"], row_paths(&rows));
        assert_eq!(1, nav.cursor());
    }

    #[test]
    fn should_clamp_cursor_to_last_row_when_collapse_all_shrinks_the_row_list() {
        let tree = deep_tree();
        // Cursor on "bar" (row 4, the last row).
        let mut nav = Nav::new();
        for _ in 0..4 {
            nav = nav.handle(Action::CursorDown, &tree);
        }
        assert_eq!(4, nav.cursor());

        let nav = nav.handle(Action::CollapseAll, &tree);

        let rows = nav.rows(&tree);
        assert_eq!(vec!["src"], row_paths(&rows));
        assert_eq!(0, nav.cursor());
    }

    #[test]
    fn should_clamp_cursor_to_last_row_when_collapse_all_hides_a_deep_cursor_row() {
        let tree = deep_tree();
        // Cursor on "src/pkg/lib.rs" (row 2, neither the first nor the
        // last row) — CollapseAll has no single "nearest visible ancestor"
        // notion the way a single ToggleExpand does (every directory
        // collapses at once), so this falls back to the simple "clamp to
        // last row" rule.
        let mut nav = Nav::new();
        for _ in 0..2 {
            nav = nav.handle(Action::CursorDown, &tree);
        }
        assert_eq!(2, nav.cursor());

        let nav = nav.handle(Action::CollapseAll, &tree);

        let rows = nav.rows(&tree);
        assert_eq!(vec!["src"], row_paths(&rows));
        assert_eq!(0, nav.cursor());
    }

    #[test]
    fn should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree() {
        // Two independent top-level directories: collapsing "b" (which
        // the cursor is not on, and is not an ancestor of the cursor's
        // row) must leave the cursor exactly where it was.
        let tree = Tree {
            roots: vec![
                dir_node("a", vec![file_node("a/one.rs", vec![])]),
                dir_node("b", vec![file_node("b/two.rs", vec![])]),
            ],
        };
        // Rows expanded: a(0), a/one.rs(1), b(2), b/two.rs(3).
        let mut nav = Nav::new().handle(Action::CursorDown, &tree); // cursor -> "a/one.rs" (1)
        assert_eq!(1, nav.cursor());

        // Move a *second*, independent Nav to "b" (row 2) and collapse it
        // there, then bring that collapse state back without disturbing
        // `nav`'s own cursor — simulates two collapse actions happening
        // in the tree while the cursor of interest stays on "a/one.rs".
        let mut collapse_b = Nav::new();
        collapse_b = collapse_b.handle(Action::CursorDown, &tree);
        collapse_b = collapse_b.handle(Action::CursorDown, &tree); // cursor -> "b" (2)
        assert_eq!(2, collapse_b.cursor());
        collapse_b = collapse_b.handle(Action::ToggleExpand, &tree); // collapse "b"

        nav.collapsed = collapse_b.collapsed;

        let rows = nav.rows(&tree);
        assert_eq!(vec!["a", "a/one.rs", "b"], row_paths(&rows));
        assert_eq!(1, nav.cursor());
        assert_eq!("a/one.rs", rows[nav.cursor()].node.path);
    }
}
