//! Tests for `crate::review_flow` (ADR 0048's review-annotations integration
//! glue), split from the source file to keep it under the ADR 0028
//! file-size threshold. Grouped by which function each submodule pins:
//!
//! - `annotation_snapshot` — `first_anchor_run`, `derive_selection_snapshot`, and
//!   `dispatch_annotation_compose_key`
//! - `perform_export` — the clipboard sink's OSC 52 status passthrough
//!   (ADR 0048 sink B)
//! - `github_review_export` — sink A's summary + "Additional notes" body
//!   composition (ADR 0067)
//! - `open_pr_in_browser` — the no-`PrContext`/spawn-failure status-line
//!   messages and the URL built from a `PrContext` (ADR 0050)

mod annotation_snapshot;
mod github_review_export;
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
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
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
                referenced_method_names: vec![],
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

/// A report whose only file has one *removed* symbol and no present
/// symbols (ADR 0067's `RemovedSymbol` annotation target) — `files` still
/// carries an (empty-symbols) entry for `lib.rs` so the tree gets a `File`
/// row to nest the removed symbol under, mirroring how `build_tree`
/// merges `report.files`/`report.removed` on a shared path.
pub(super) fn report_with_one_removed_symbol() -> Report {
    use rinkaku_core::extract::{RemovedSymbol, SymbolKind};
    use rinkaku_core::render::FileReport;

    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![],
        }],
        removed: vec![RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    }
}

/// A report whose only file is a *whole* test file — every symbol flagged
/// `is_test` (Rust's `#[cfg(test)]` convention, ADR 0035 Phase B) and no
/// production symbols left over in the same file. `build_tree` lifts it out
/// of the production tree entirely into a synthetic `Section::Tests` root
/// (`crate::tree::tests_section::is_whole_test_file`'s own doc comment), so
/// with no other content in the report, the cursor at position 0 lands on
/// that `Section` row (ADR 0067's Decision 4: `Section`/`TestGroup` stay out
/// of annotation scope).
pub(super) fn report_with_one_test_file() -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::render::FileReport;

    Report {
        files: vec![FileReport {
            path: "tests.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                id: "tests.rs::test_foo".to_string(),
                name: "test_foo".to_string(),
                kind: SymbolKind::Function,
                signature: "fn test_foo()".to_string(),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: vec![],
                referenced_method_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
                is_test: true,
                classification: None,
                previous_signature: None,
            }],
        }],
        ..empty_report()
    }
}
