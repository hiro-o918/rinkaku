//! Tests for `crate::review_flow` (ADR 0048's review-annotations integration
//! glue), split from the source file to keep it under the ADR 0028
//! file-size threshold. Grouped by which function each submodule pins:
//!
//! - `annotation_snapshot` — `first_anchor_run`, `derive_selection_snapshot`, and
//!   `dispatch_annotation_compose_key`
//! - `perform_export` — the clipboard sink's OSC 52 status passthrough
//!   (ADR 0048 sink B)
//! - `open_pr_in_browser` — the no-`PrContext`/spawn-failure status-line
//!   messages and the URL built from a `PrContext` (ADR 0050)

mod annotation_snapshot;
mod open_pr_in_browser;
mod perform_export;

use crate::review::ports::BrowserOpener;
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::Report;

/// A [`BrowserOpener`] fake shared by [`perform_export`]/[`open_pr_in_browser`]'s
/// tests — `ReviewPorts::browser` is always present (ADR 0050), so every
/// `ReviewPorts` fixture needs one even when the test itself is not
/// exercising `w`. `opened_url` records the last URL passed to
/// [`BrowserOpener::open_url`] so a test can assert the exact URL built from
/// a [`crate::review::PrContext`], not just the resulting status message.
pub(super) struct FakeBrowserOpener {
    pub(super) result: Result<(), String>,
    pub(super) opened_url: std::cell::RefCell<Option<String>>,
}

impl FakeBrowserOpener {
    pub(super) fn new(result: Result<(), String>) -> Self {
        Self {
            result,
            opened_url: std::cell::RefCell::new(None),
        }
    }
}

impl BrowserOpener for FakeBrowserOpener {
    fn open_url(&self, url: &str) -> Result<(), String> {
        *self.opened_url.borrow_mut() = Some(url.to_string());
        self.result.clone()
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

pub(super) fn report_with_one_symbol() -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::render::FileReport;

    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                id: "lib.rs::foo".to_string(),
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "fn foo()".to_string(),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
                is_test: false,
                classification: None,
                previous_signature: None,
            }],
        }],
        ..empty_report()
    }
}
