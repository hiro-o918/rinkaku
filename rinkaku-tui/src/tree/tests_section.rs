//! The trailing "Tests" section (ADR 0035 Phase B): identifies *whole*
//! test files so [`super::build_tree`] can route them into a separate
//! [`super::TreeBuilder`] from production files, then wraps that
//! builder's already-built roots into one [`super::NodeKind::Section`]
//! node — kept sorted A-Z unconditionally, since a section has no
//! production dependency story left to rank once test symbols/edges are
//! excluded from `crate::order::rank`.

use super::{Badges, NodeKind, SectionKind, TESTS_SECTION_PATH, TreeNode};

/// Whether `path`'s `FileReport` counts as a *whole* test file rather
/// than ordinary production code or a *mixed* file: either
/// `LanguageSupport::is_test_path` says so by convention (Go's
/// `*_test.go`, etc.), or every symbol in `symbols` has `is_test ==
/// true` — mirroring `pipeline::partition_test_symbols`'s own
/// test-exclusion rule exactly, so this module's notion of "whole test
/// file" agrees with `rinkaku-core`'s. A file with an empty `symbols`
/// list (a pure rename) is never whole-test, matching
/// `partition_test_symbols`'s own `had_symbols` guard.
pub(super) fn is_whole_test_file(
    path: &str,
    symbols: &[rinkaku_core::extract::ExtractedSymbol],
) -> bool {
    let is_test_path =
        rinkaku_core::language::language_for_path(path).is_some_and(|lang| lang.is_test_path(path));
    is_test_path || (!symbols.is_empty() && symbols.iter().all(|symbol| symbol.is_test))
}

/// Wraps `section_roots` (the already-built roots of a `TreeBuilder`
/// that only ever received whole-test files) into one `Section` node, or
/// `None` when there were no whole-test files at all — a report with no
/// tests must not grow a trailing empty "Tests" row.
pub(super) fn wrap_section(mut section_roots: Vec<TreeNode>) -> Option<TreeNode> {
    if section_roots.is_empty() {
        return None;
    }

    sort_alphabetically(&mut section_roots);
    let mut badges = Badges::default();
    for child in &section_roots {
        badges.merge(child.badges);
    }
    Some(TreeNode {
        kind: NodeKind::Section(SectionKind::Tests),
        path: TESTS_SECTION_PATH.to_string(),
        badges,
        children: section_roots,
        skip_reason: None,
        test_symbol_count: None,
    })
}

/// Recursively re-sorts `nodes` (and every directory's children,
/// depth-first) A-Z by path, directories before files — independent of
/// `crate::order::sort::order_siblings` since a section's ordering is
/// unconditional (never subject to the topological/alphabetical toggle).
/// `NodeKind::Symbol` children are left in their extraction order,
/// matching `order_siblings`'s own "symbols are never reordered" rule.
fn sort_alphabetically(nodes: &mut [TreeNode]) {
    for node in nodes.iter_mut() {
        if matches!(node.kind, NodeKind::Dir) {
            sort_alphabetically(&mut node.children);
        }
    }
    nodes.sort_by(|a, b| {
        let a_is_dir = matches!(a.kind, NodeKind::Dir);
        let b_is_dir = matches!(b.kind, NodeKind::Dir);
        b_is_dir.cmp(&a_is_dir).then_with(|| a.path.cmp(&b.path))
    });
}

#[cfg(test)]
#[path = "tests_section_tests/mod.rs"]
mod tests;
