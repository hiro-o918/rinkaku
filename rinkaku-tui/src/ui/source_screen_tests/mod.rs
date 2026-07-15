//! Tests for `crate::ui::source_screen`, split from the source file (ADR
//! 0028) and grouped by which pane concern each block pins:
//!
//! - `error_handling` — the missing-file/wrapped-error fallback paths and
//!   the scroll-outcome contract (`DrawOutcome`)
//! - `highlighting` — token foreground + symbol-range background
//!   composition, and the unrecognized-extension style fallback
//! - `overlay` — ADR 0046's unified added/removed diff overlay, including
//!   the working-tree-drift fallback
//! - `split_view` — ADR 0049's side-by-side rendering of that overlay

use super::*;
use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::ui::{DrawOutcome, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

mod error_handling;
mod highlighting;
mod overlay;
mod split_view;

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
        file_size_warnings: vec![],
        file_size_bands: vec![],
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

/// A real `Err` from `crate::source::load_symbol_source`/
/// `load_highlighted_symbol_source`, produced by actually attempting a
/// read under a placeholder repo root nothing on disk matches — used to
/// build `source_content` for `draw_source_screen` tests below rather
/// than fabricating the error string by hand, so these tests stay
/// pinned to the real message format `crate::source` produces.
pub(super) fn missing_file_source_content(
    report: &Report,
    symbol_id: &str,
) -> Option<Result<crate::source::HighlightedSourceView, String>> {
    Some(crate::source::load_highlighted_symbol_source(
        report,
        symbol_id,
        std::path::Path::new("/repo"),
        &crate::source::WorkingTreeSourceReader,
    ))
}

pub(super) fn draw_source_screen_for_test(
    report: &Report,
    repo_root: &std::path::Path,
    diff_hunks: &[crate::diff_view::FileHunks],
) -> Terminal<TestBackend> {
    let app = App::new(report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        report,
        "lib.rs::foo",
        repo_root,
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                diff_hunks,
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    terminal
}
