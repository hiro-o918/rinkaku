//! Tests for `crate::tree`, split from the source file (ADR 0028) and
//! grouped by which build_tree concern each block pins:
//!
//! - `build_tree_structure` — tree shape: empty, flat, single-child
//!   collapse, non-collapse, own-file-plus-subdir, bottom-up badge
//!   aggregation, empty-file leaves, source-order preservation
//! - `symbol_badges` — per-symbol badge derivation: contract-change
//!   counting, removed-symbol marking, and fan-in propagation from
//!   `report.hotspots`
//! - `file_size_warnings` — ADR 0028 file-size badge propagation:
//!   own line count, per-severity aggregation, and dir-node severity
//!   invariant
//! - `skipped_files` — skipped file rows: reason carry, generated
//!   filtering, mixed-severity preservation, dir merge
//! - `test_files_and_overlap` — whole-test-file rows, dir merge, the
//!   ordinary-file guard, `debug_assert!` panic paths on invalid
//!   overlap, and the valid mixed-file (files + tests) regression

use super::*;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, RemovedSymbol};
use rinkaku_core::graph::{Hotspot, SymbolGraph};
use rinkaku_core::render::{FileReport, SkippedFile, TestFileSummary};

mod build_tree_structure;
mod file_size_warnings;
mod skipped_files;
mod symbol_badges;
mod test_files_and_overlap;

pub(super) fn symbol(id: &str, name: &str, kind: SymbolKind) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind,
        signature: format!("fn {name}()"),
        range: LineRange { start: 1, end: 1 },
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
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
}
