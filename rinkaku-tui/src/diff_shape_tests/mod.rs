//! Tests for `crate::diff_shape`, split from the source file (ADR 0028) and
//! grouped by which pub function each block pins:
//!
//! - `build_diff_pane_content` — every scenario `build_diff_pane_content`
//!   supports (per-symbol grouping, deletion / overlap / new-file
//!   attribution, contract-header inclusion, and the empty-input degrade
//!   paths)
//! - `hunk_start_lines` — the `]c`/`[c` jump-stop layout math
//! - `section_start_line` — `section_start_line_for_symbol`'s title-line
//!   pointer including its contract-header offset behavior
//! - `symbol_id_for_scroll_line` — the reverse lookup added by ADR 0030
//!   (scroll offset → symbol id under it), powering the diff → tree
//!   auto-sync
//! - `changed_line_ranges` — the Diff pane header's `range:` line data
//!   (distinct new-side line spans, sorted and deduped across sections
//!   so ADR 0029's cloned hunks produce one entry per span)
//!
//! `pair_hunk_lines`'s own tests moved to `crate::split_pairing_tests`
//! alongside the rest of that module's split-out implementation.

use super::*;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, ReportOrigin};

mod build_diff_pane_content;
mod changed_line_ranges;
mod hunk_start_lines;
mod section_start_line;
mod symbol_id_for_scroll_line;

pub(super) fn symbol(id: &str, name: &str, range: LineRange) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range,
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

pub(super) fn empty_report() -> Report {
    Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    }
}

pub(super) fn hunk(header: &str, new_range: Option<(usize, usize)>, lines: Vec<&str>) -> Hunk {
    Hunk {
        header: header.to_string(),
        new_range,
        lines: lines
            .into_iter()
            .map(|content| crate::diff_view::DiffLine {
                kind: crate::diff_view::DiffLineKind::Context,
                content: content.to_string(),
            })
            .collect(),
    }
}

/// Wraps `hunk` with the `source_index` it occupies in the fixture's
/// `FileHunks::hunks` — every test below builds its `diff_files`
/// fixture with hunks in a fixed order, so this index is just "which
/// position in that `vec![...]` this hunk was written at". `origin_offset`
/// is `0` (ADR 0053: this hunk was never split — the fixture's own
/// `hunk(...)` always builds the whole hunk).
pub(super) fn attributed(source_index: usize, hunk: Hunk) -> AttributedHunk {
    AttributedHunk {
        source_index,
        hunk,
        origin_offset: 0,
    }
}
