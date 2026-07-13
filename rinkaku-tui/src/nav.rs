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

    /// Moves the cursor to the first visible [`Dir`](NodeKind::Dir)/
    /// [`File`](NodeKind::File) row whose path exactly equals `path`, or
    /// leaves the cursor untouched (returning `false`) when no such row is
    /// currently visible — either because no row's path matches at all, or
    /// because a matching row exists but sits under a collapsed ancestor
    /// (`Nav::new`'s everything-expanded default means this only happens if
    /// the caller collapsed something first). Deliberately excludes
    /// [`NodeKind::Symbol`] rows even though a symbol row's own `node.path`
    /// equals its containing file's path (`TreeNode::path`'s own doc
    /// comment) — this method exists for `--entry`-style directory/file
    /// pivoting (`crate::app::App`'s own entry-path wiring), which has no
    /// single-symbol-scoped meaning (ADR 0019, mirroring
    /// `App::selected_pivot_view`'s symbol-row `NotApplicable` case), so
    /// landing on a same-path symbol row instead of the file/dir row itself
    /// would silently change what the pivot pane shows.
    pub fn move_cursor_to_path(&mut self, tree: &Tree, path: &str) -> bool {
        let rows = self.rows(tree);
        let Some(index) = rows
            .iter()
            .position(|row| row.node.path == path && !matches!(row.node.kind, NodeKind::Symbol(_)))
        else {
            return false;
        };
        self.cursor = index;
        true
    }

    /// Moves the cursor to the [`NodeKind::Symbol`] row whose
    /// [`crate::tree::SymbolRef::id`] equals `symbol_id`, expanding every
    /// collapsed ancestor directory/file on the path to it, or leaves the
    /// cursor untouched (returning `false`) when no row's symbol id matches
    /// at all. Exists for jump navigation (ADR 0022's `gd`/`gr`), a sibling
    /// to [`Self::move_cursor_to_path`] rather than a generalization of it —
    /// that method deliberately excludes `Symbol` rows (its own doc comment)
    /// because pivoting has no single-symbol-scoped meaning, and this method
    /// is symbol-only for the mirror-image reason: a jump target is always a
    /// specific symbol, never a directory/file.
    ///
    /// Unlike `move_cursor_to_path`'s "no-op if hidden under a collapsed
    /// ancestor" contract (appropriate for `--entry`'s startup-time pivot,
    /// where a fresh `App` is always fully expanded and a miss is genuinely
    /// a wrong path), a mid-session jump target is very likely to be folded
    /// away by the time the reviewer presses `gd`/`gr` — silently failing to
    /// jump because of unrelated fold state would defeat the feature's own
    /// purpose, so this method expands whatever stands in the way instead of
    /// giving up.
    pub fn move_cursor_to_symbol(&mut self, tree: &Tree, symbol_id: &str) -> bool {
        let Some(path_chain) = find_symbol_ancestor_chain(tree, symbol_id) else {
            return false;
        };
        // Expand every ancestor directory/file on the path to the target
        // (all but the last entry, which is the symbol's own file — files
        // have no `collapsed` entry of their own to expand, only their
        // directory ancestors do, but removing a path that was never
        // collapsed is a harmless no-op).
        for ancestor_path in &path_chain {
            self.collapsed.remove(ancestor_path);
        }

        let rows = self.rows(tree);
        let Some(index) = rows.iter().position(|row| {
            matches!(&row.node.kind, NodeKind::Symbol(symbol_ref) if symbol_ref.id == symbol_id)
        }) else {
            // Defensive: `find_symbol_ancestor_chain` already found this
            // node in `tree`, and every ancestor was just expanded above, so
            // the row must now be visible — this branch only guards against
            // a future bug in the expansion logic rather than a reachable
            // runtime condition.
            return false;
        };
        self.cursor = index;
        true
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
    /// looking at", every action that can shrink or reshuffle the row list
    /// re-targets the cursor afterward via [`Self::retarget_cursor`] — that
    /// covers `ToggleExpand`, `ExpandAll`, and `CollapseAll` below, falling
    /// through to the shared `retarget_cursor` call after the `match`
    /// rather than only the branches that are obviously collapse-shaped, so
    /// this also future-proofs against a new `Action` variant added there
    /// later that shrinks the row list in some other way.
    ///
    /// `CursorUp`/`CursorDown` are the two exceptions: they `return self`
    /// directly from inside the `match`, bypassing `retarget_cursor`
    /// entirely. This is safe rather than an oversight — moving the cursor
    /// is the action that *sets* the cursor's new target, so there is
    /// nothing to re-target it against; both arms already do their own
    /// bounds clamping against the current `rows(tree).len()` inline.
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

/// Every ancestor directory/file path (nearest first, i.e. the symbol's own
/// containing file first, then that file's directory, out to the root) on
/// the way to the [`NodeKind::Symbol`] row whose id equals `symbol_id`, or
/// `None` when no such symbol exists anywhere in `tree` — a plain tree walk
/// independent of any [`Nav`]'s collapse state (unlike
/// [`Nav::push_rows`]/[`Nav::rows`], which skip collapsed subtrees), since
/// [`Nav::move_cursor_to_symbol`] needs to find the target and its ancestors
/// *regardless* of what is currently folded, precisely so it can expand
/// them.
fn find_symbol_ancestor_chain(tree: &Tree, symbol_id: &str) -> Option<Vec<String>> {
    for root in &tree.roots {
        if let Some(chain) = collect_symbol_ancestor_chain(root, symbol_id) {
            return Some(chain);
        }
    }
    None
}

fn collect_symbol_ancestor_chain(node: &TreeNode, symbol_id: &str) -> Option<Vec<String>> {
    if let NodeKind::Symbol(symbol_ref) = &node.kind
        && symbol_ref.id == symbol_id
    {
        return Some(Vec::new());
    }
    for child in &node.children {
        if let Some(mut chain) = collect_symbol_ancestor_chain(child, symbol_id) {
            chain.push(node.path.clone());
            return Some(chain);
        }
    }
    None
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
            skip_reason: None,
            test_symbol_count: None,
        }
    }

    fn file_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges: Badges::default(),
            children,
            skip_reason: None,
            test_symbol_count: None,
        }
    }

    fn dir_node(path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            kind: NodeKind::Dir,
            path: path.to_string(),
            badges: Badges::default(),
            children,
            skip_reason: None,
            test_symbol_count: None,
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
    fn should_move_cursor_to_matching_dir_row_when_path_matches_a_directory() {
        let tree = sample_tree();
        let mut nav = Nav::new();

        let actual = nav.move_cursor_to_path(&tree, "src");

        assert!(actual);
        assert_eq!(0, nav.cursor());
    }

    #[test]
    fn should_move_cursor_to_matching_file_row_when_path_matches_a_file() {
        let tree = sample_tree();
        let mut nav = Nav::new();

        let actual = nav.move_cursor_to_path(&tree, "src/lib.rs");

        assert!(actual);
        // Row 1 is the File row itself, not either of its two Symbol rows
        // (2, 3) which share the same `node.path` — `move_cursor_to_path`
        // must land on the File row specifically.
        assert_eq!(1, nav.cursor());
    }

    #[test]
    fn should_not_move_cursor_when_no_row_matches_the_path() {
        let tree = sample_tree();
        let mut nav = Nav::new().handle(Action::CursorDown, &tree);
        assert_eq!(1, nav.cursor());

        let actual = nav.move_cursor_to_path(&tree, "no/such/path");

        assert!(!actual);
        assert_eq!(1, nav.cursor());
    }

    #[test]
    fn should_not_move_cursor_to_a_matching_row_hidden_under_a_collapsed_ancestor() {
        let tree = sample_tree();
        let mut nav = Nav::new().handle(Action::ToggleExpand, &tree); // collapse "src"
        assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

        let actual = nav.move_cursor_to_path(&tree, "src/lib.rs");

        assert!(!actual);
        assert_eq!(0, nav.cursor());
    }

    #[test]
    fn should_move_cursor_to_matching_symbol_row_when_symbol_id_matches() {
        let tree = sample_tree();
        let mut nav = Nav::new();

        let actual = nav.move_cursor_to_symbol(&tree, "src/lib.rs::bar");

        assert!(actual);
        // Row 3 is "bar" (src(0), src/lib.rs(1), foo(2), bar(3)).
        assert_eq!(3, nav.cursor());
    }

    #[test]
    fn should_not_move_cursor_when_no_symbol_id_matches() {
        let tree = sample_tree();
        let mut nav = Nav::new().handle(Action::CursorDown, &tree);
        assert_eq!(1, nav.cursor());

        let actual = nav.move_cursor_to_symbol(&tree, "no/such/id");

        assert!(!actual);
        assert_eq!(1, nav.cursor());
    }

    #[test]
    fn should_expand_collapsed_ancestor_when_jumping_to_a_hidden_symbol() {
        let tree = sample_tree();
        let mut nav = Nav::new().handle(Action::ToggleExpand, &tree); // collapse "src"
        assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

        let actual = nav.move_cursor_to_symbol(&tree, "src/lib.rs::foo");

        assert!(actual);
        let rows = nav.rows(&tree);
        assert_eq!(
            vec!["src", "src/lib.rs", "src/lib.rs", "src/lib.rs"],
            row_paths(&rows)
        );
        assert_eq!("foo", symbol_name(&rows[nav.cursor()]));
    }

    #[test]
    fn should_expand_multiple_collapsed_ancestors_when_jumping_to_a_deeply_hidden_symbol() {
        let tree = deep_tree();
        let mut nav = Nav::new();
        // Collapse both "src" and "src/pkg" so the target symbol is hidden
        // two levels deep.
        nav = nav.handle(Action::ToggleExpand, &tree); // collapse "src" (cursor was on it)
        assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

        let actual = nav.move_cursor_to_symbol(&tree, "src/pkg/lib.rs::bar");

        assert!(actual);
        let rows = nav.rows(&tree);
        assert_eq!(
            vec![
                "src",
                "src/pkg",
                "src/pkg/lib.rs",
                "src/pkg/lib.rs",
                "src/pkg/lib.rs"
            ],
            row_paths(&rows)
        );
        assert_eq!("bar", symbol_name(&rows[nav.cursor()]));
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

    /// The `name` of a [`NodeKind::Symbol`] row — panics on any other row
    /// kind, since every call site above already knows (from its own
    /// fixture) that the cursor should have landed on a symbol row.
    fn symbol_name<'a>(row: &Row<'a>) -> &'a str {
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) => symbol_ref.name.as_str(),
            other => panic!("expected NodeKind::Symbol, got {other:?}"),
        }
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
