//! Tests for `crate::ui::detail_pane`, split from the source file
//! (ADR 0028) and grouped by which pane concern each block pins:
//!
//! - `content_by_row_kind` — the "which content shows for which row"
//!   coverage: placeholder, dir, skipped file, whole-test-file,
//!   mixed test+prod file, symbol, and the ADR 0017 `RepoOutline`
//!   wording switch
//! - `scroll_behavior` — overflow indicator, scroll-down, page clamp,
//!   scroll reset on selection change, and the clamped-offset return
//!   contract exercised via `DrawOutcome`
//! - `wrap_reachability` — narrow-pane long-line regression for
//!   reaching the wrapped tail and reporting the wrapped-row indicator
//!   total
//! - `size_warning` — ADR 0028 `file_detail_lines` size-warning line
//!   rendering (Warn vs Split label / trailing hint)

use super::file_detail_lines;
use crate::app::{App, BlastRadiusSelection};
use crate::detail::FileDetail;
use crate::ui::{DrawOutcome, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

mod content_by_row_kind;
mod scroll_behavior;
mod size_warning;
mod wrap_reachability;

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
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
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

/// A [`Report`] whose only entry is a skipped file (no `files`, no
/// `tests`) — pairs with `report_with_one_symbol` for the detail-pane
/// tests below.
pub(super) fn report_with_one_skipped_file() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        skipped: vec![rinkaku_core::render::SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
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

/// A [`Report`] whose only entry is a whole-test-file summary (no
/// `files`, no `skipped`).
pub(super) fn report_with_one_test_file() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![rinkaku_core::render::TestFileSummary {
            path: "src/lib_test.go".to_string(),
            symbol_count: 3,
        }],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    }
}

/// A report whose single file has `count` symbols, each referencing
/// `report_with_one_symbol`'s pattern but repeated enough times that
/// `file_detail_lines` produces more lines than a typical test
/// viewport's height — used to exercise `draw_detail_pane`'s scrolling
/// and overflow-indicator paths, which need content that does not fit
/// in one screen.
pub(super) fn report_with_many_symbols(count: usize) -> Report {
    let symbols: Vec<ExtractedSymbol> = (0..count)
        .map(|i| symbol(&format!("lib.rs::sym{i}"), &format!("sym{i}")))
        .collect();
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols,
        }],
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
