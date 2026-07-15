//! Tests for `crate::input_translate` (raw `crossterm` events -> this
//! crate's terminal-agnostic `InputKey`), split from the source file to
//! keep it under the ADR 0028 file-size threshold. Grouped by which
//! function each submodule pins:
//!
//! - `translate_key` — keyboard -> `InputKey` translation, covering the
//!   plain keymap and the help-overlay / jump-popup "swallow" contracts
//!   (ADR 0020, ADR 0022, ADR 0026)
//! - `translate_mouse` — mouse-wheel/click translation

mod translate_key;
mod translate_mouse;

use crate::app::JumpCandidate;
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::Report;

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

pub(super) fn candidate(id: &str, name: &str, path: &str) -> JumpCandidate {
    JumpCandidate {
        id: id.to_string(),
        name: name.to_string(),
        path: path.to_string(),
    }
}
