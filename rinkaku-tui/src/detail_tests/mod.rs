//! Tests for `crate::detail`, split from the source file (ADR 0028) and
//! grouped by which pub function each block pins:
//!
//! - `build_detail` — signature (Current / Changed / fall-back), fan-in
//!   used-by, high-fan-in used-by, callers/callees, and the defensive
//!   dedup / self-edge exclusion (SHOULD-FIX 5)
//! - `symbol_mentions` — the `symbol_mentions` extraction reused by
//!   jump navigation (ADR 0022)
//! - `build_dir_detail` — directory-row detail: badges, top fan-in,
//!   truncation, and cycle explanation
//! - `build_file_detail` — file-row detail: symbol summaries, removed
//!   symbols, skip / test-file / mixed / size-warning carries

use super::*;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::ExtractedSymbol;
use rinkaku_core::graph::{Edge, FanIn, Node, SymbolGraph};
use rinkaku_core::render::FileReport;

mod build_detail;
mod build_dir_detail;
mod build_file_detail;
mod symbol_mentions;

pub(super) fn symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
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

pub(super) fn node(id: &str, path: &str, name: &str) -> Node {
    Node {
        id: id.to_string(),
        path: path.to_string(),
        name: name.to_string(),
        is_test: false,
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
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    }
}
