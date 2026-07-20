//! Tests for `crate::ui::diff_pane`, split from the source file (ADR 0028)
//! and grouped by which pane concern each block pins:
//!
//! - `header_lines` — `diff_pane_header_lines`'s pure identification/stats
//!   formatting and truncation
//! - `row_kinds` — the "which content shows for which row" coverage:
//!   symbol row, skipped file (binary and textual), file selection with
//!   per-symbol section headers, and the contract-header disclosure order
//! - `split_view` — ADR 0044's side-by-side rendering: paired old/new
//!   lines, the narrow-pane fallback to unified, and unified staying the
//!   default when the toggle was never pressed
//! - `styling` — token/diff background tint, hunk-header color, and the
//!   unrecognized-extension style fallback
//! - `annotation_markers` — the ADR 0048 `*`-marker column's positive case
//!   (unified and split), since every other block in this module exercises
//!   only an empty `AnnotationMarkers`

use super::*;
use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::ui::draw;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{Classification, ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::FileReport;

mod annotation_markers;
mod header_lines;
mod row_kinds;
mod split_view;
mod styling;

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

pub(super) fn report_with_one_symbol() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    }
}

pub(super) fn diff_content_for(
    report: &Report,
    diff_files: &[crate::diff_view::FileHunks],
    app: &App,
) -> crate::diff_shape::DiffPaneContent {
    crate::diff_shape::build_diff_pane_content(
        report,
        diff_files,
        app.selected_diff_target(report).as_ref(),
    )
}

pub(super) fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn find_cell_style(
    terminal: &Terminal<TestBackend>,
    line_needle: &str,
    token: &str,
) -> Style {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    for y in 0..area.height {
        let row: String = (0..area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        let Some(needle_byte_offset) = row.find(line_needle) else {
            continue;
        };
        let Some(token_byte_offset) = row[needle_byte_offset..].find(token) else {
            continue;
        };
        let byte_offset = needle_byte_offset + token_byte_offset;
        let column = row[..byte_offset].chars().count() as u16;
        return buffer[(column, y)].style();
    }
    panic!("expected to find {token:?} within a row containing {line_needle:?}");
}
