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
    /// re-rooting (`crate::app::App`'s own entry-path wiring), which has no
    /// single-symbol-scoped meaning (ADR 0019, mirroring
    /// `App::selected_blast_radius_view`'s symbol-row `NotApplicable` case),
    /// so landing on a same-path symbol row instead of the file/dir row
    /// itself would silently change what the blast-radius pane shows.
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
#[path = "nav_tests/mod.rs"]
mod tests;
