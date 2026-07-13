//! Tests for `crate::row_view`, split from the source file (ADR 0028) and
//! grouped by which pub function / row-render concern each block pins:
//!
//! - `entry_row_line` — general `entry_row_line` behavior: indent,
//!   badges, skip-reason, test-file badge, cycle marker, classification
//!   markers, selection modifier
//! - `relative_labels` — `relative_labels`' ancestor-prefix stripping
//! - `file_size_badges` — ADR 0028 file-size warning badges on file and
//!   dir rows (`lines:N`, `warn:N split:N`)
//! - `label_badges` — ADR 0013 amendments (2026-07-13,
//!   feat/label-contract-changes-badge) split-span coloring for
//!   `chg:N` / `api:N` / `ref:N` badges

use super::*;
use crate::tree::{NodeKind, TreeNode};

mod entry_row_line;
mod file_size_badges;
mod label_badges;
mod relative_labels;

pub(super) fn dir_node(path: &str, badges: Badges, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::Dir,
        path: path.to_string(),
        badges,
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn file_node(path: &str, badges: Badges) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges,
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn skipped_file_node(path: &str, reason: rinkaku_core::render::SkipReason) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges: Badges::default(),
        children: vec![],
        skip_reason: Some(reason),
        test_symbol_count: None,
    }
}

pub(super) fn test_file_node(path: &str, symbol_count: usize) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges: Badges::default(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: Some(symbol_count),
    }
}

pub(super) fn symbol_node(path: &str, symbol_ref: SymbolRef, badges: Badges) -> TreeNode {
    TreeNode {
        kind: NodeKind::Symbol(symbol_ref),
        path: path.to_string(),
        badges,
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

pub(super) fn plain_symbol(name: &str) -> SymbolRef {
    SymbolRef {
        id: format!("lib.rs::{name}"),
        name: name.to_string(),
        kind: SymbolKind::Function,
        classification: None,
        removed: false,
    }
}

pub(super) fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Locates the styled span whose visible text is `content` in `line`,
/// returning its foreground color (or `None` when the span exists but
/// has no explicit fg, matching ratatui's default). Panics when no
/// such span exists — a matching-span assertion is what the test
/// wanted, so a missing span is a test failure, not a `None`.
pub(super) fn fg_of_span_with_content(line: &Line<'_>, content: &str) -> Option<Color> {
    line.spans
        .iter()
        .find(|span| span.content.as_ref() == content)
        .unwrap_or_else(|| {
            panic!(
                "no span with content {content:?} found in line: {:?}",
                line_text(line)
            )
        })
        .style
        .fg
}
