//! Sibling ordering for the entry view (ADR 0016 decision 4): applies
//! [`super::rank::DirRank`] to a built [`crate::tree::Tree`], sorting
//! directories before files before a trailing `Section` (ADR 0035 Phase
//! B), independent of `rinkaku-core`'s own graph.

use super::rank::DirRank;
use std::collections::HashMap;

/// How sibling directories/files are ordered in the entry view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderMode {
    /// Topological: least-depended-on directories first, foundations last
    /// (ADR 0016 decision 4, this module's default per that ADR).
    #[default]
    Topological,
    /// Plain alphabetical ordering, the toggle ADR 0016 keeps available.
    AlphaNumeric,
}

/// Reorders every level of `tree` in place according to `mode`, using
/// `ranks` (from [`super::rank::rank_directories`]) when `mode` is
/// [`OrderMode::Topological`].
///
/// A `Section` node (ADR 0035 Phase B) always sorts after every
/// directory/file regardless of `mode`, and its own children are left
/// untouched — `crate::tree::build_tree` already sorts them
/// alphabetically, and a section has no production dependency story
/// left to rank once its test symbols/edges are excluded (see
/// `super::rank`'s own test-exclusion).
pub fn order_tree(tree: &mut crate::tree::Tree, ranks: &HashMap<String, DirRank>, mode: OrderMode) {
    let effective = effective_ranks(tree, ranks);
    order_siblings(&mut tree.roots, &effective, mode);
}

/// For every directory node in `tree`, its effective rank: the minimum
/// [`DirRank::rank`] across the directory itself and every directory
/// nested under it, or `None` when no directory in that subtree has a
/// `ranks` entry at all.
///
/// `rank_directories` only promises ranks for *leaf* directories (the
/// direct parent of a graph node's path); this function is what makes a
/// branching intermediate directory (e.g. `"src"` when only its
/// subdirectories own nodes) rank correctly too, by propagating the
/// minimum rank up from its descendants — the minimum (not e.g. an
/// average) is what preserves "entry points first": an ancestor
/// containing a rank-0 descendant should show early too.
fn effective_ranks(
    tree: &crate::tree::Tree,
    ranks: &HashMap<String, DirRank>,
) -> HashMap<String, usize> {
    let mut effective = HashMap::new();
    for root in &tree.roots {
        compute_effective_rank(root, ranks, &mut effective);
    }
    effective
}

/// Post-order walk: computes every descendant directory's effective rank
/// first, then this node's own as `min(own direct rank, min of
/// children's effective ranks)`, returning it so the caller can fold it
/// into its own minimum without re-reading the map.
fn compute_effective_rank(
    node: &crate::tree::TreeNode,
    ranks: &HashMap<String, DirRank>,
    effective: &mut HashMap<String, usize>,
) -> Option<usize> {
    if !matches!(node.kind, crate::tree::NodeKind::Dir) {
        return None;
    }

    let own_rank = ranks.get(&node.path).map(|r| r.rank);
    let min_child_rank = node
        .children
        .iter()
        .filter_map(|child| compute_effective_rank(child, ranks, effective))
        .min();

    let resolved = match (own_rank, min_child_rank) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    if let Some(rank) = resolved {
        effective.insert(node.path.clone(), rank);
    }
    resolved
}

fn order_siblings(
    nodes: &mut [crate::tree::TreeNode],
    effective_ranks: &HashMap<String, usize>,
    mode: OrderMode,
) {
    for node in nodes.iter_mut() {
        if matches!(node.kind, crate::tree::NodeKind::Dir) {
            order_siblings(&mut node.children, effective_ranks, mode);
        }
    }

    nodes.sort_by(|a, b| {
        tier(a).cmp(&tier(b)).then_with(|| match mode {
            OrderMode::Topological
                if tier(a) == SiblingTier::Dir && tier(b) == SiblingTier::Dir =>
            {
                // `None` (unranked) must sort after every `Some` rank —
                // the opposite of `Option<usize>`'s derived `Ord` — so
                // rank is compared via an explicit "unranked last" key
                // (`usize::MAX`). Ties break A-Z on path.
                let rank_key =
                    |path: &str| effective_ranks.get(path).copied().unwrap_or(usize::MAX);
                rank_key(&a.path)
                    .cmp(&rank_key(&b.path))
                    .then_with(|| a.path.cmp(&b.path))
            }
            _ => a.path.cmp(&b.path),
        })
    });
}

/// Which of the three sibling tiers a node belongs to, for
/// [`order_siblings`]'s primary sort key. `#[derive(Ord)]`'s
/// declaration-order-is-sort-order behavior gives `Dir` (0) < `File` (1)
/// < `Section` (2) directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SiblingTier {
    Dir,
    File,
    Section,
}

fn tier(node: &crate::tree::TreeNode) -> SiblingTier {
    match node.kind {
        crate::tree::NodeKind::Dir => SiblingTier::Dir,
        crate::tree::NodeKind::Section(_) => SiblingTier::Section,
        crate::tree::NodeKind::File | crate::tree::NodeKind::Symbol(_) => SiblingTier::File,
    }
}

#[cfg(test)]
#[path = "sort_tests/mod.rs"]
mod tests;
