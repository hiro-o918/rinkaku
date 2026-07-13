//! Tests for `crate::lib` (the run/dispatch/translate glue), split from the
//! source file to keep it under the ADR 0028 file-size threshold. Grouped
//! by which crate-private function each set of tests pins:
//!
//! - `translate_key` — keyboard → `InputKey` translation, including the
//!   help-overlay and jump-popup "swallow" contracts
//! - `translate_mouse` — mouse-wheel/click translation
//! - `recompute_and_reload` — the per-frame recompute gates
//!   (`should_recompute_diff_pane_content`,
//!   `should_recompute_blast_radius_selection`) and the source-cache
//!   reload gate (`should_reload_source_content`)
//! - `hunk_jump` — `should_apply_hunk_jump` and `jump_scroll_target`
//! - `scroll_clamp` — post-draw fold-back
//!   (`clamp_right_pane_scroll_after_draw`,
//!   `clamp_help_scroll_after_draw`) and `is_scroll_input_key`
//! - `goto_dispatch` — `resolve_goto` and the `dispatch_non_source_key`
//!   regression coverage (gd/gr sequences, jumplist restore)

use crate::app::JumpCandidate;
use crate::source;
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::Report;

mod goto_dispatch;
mod hunk_jump;
mod recompute_and_reload;
mod scroll_clamp;
mod translate_key;
mod translate_mouse;

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

pub(super) fn report_with_symbols_and_edges(
    symbols_by_file: Vec<(&str, Vec<&str>)>,
    edges: Vec<(&str, &str)>,
) -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::{Edge, SymbolGraph};
    use rinkaku_core::render::FileReport;

    let files: Vec<FileReport> = symbols_by_file
        .iter()
        .map(|(path, names)| FileReport {
            path: path.to_string(),
            symbols: names
                .iter()
                .map(|name| ExtractedSymbol {
                    id: format!("{path}::{name}"),
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
                })
                .collect(),
        })
        .collect();

    let nodes: Vec<rinkaku_core::graph::Node> = symbols_by_file
        .iter()
        .flat_map(|(path, names)| {
            names.iter().map(move |name| rinkaku_core::graph::Node {
                id: format!("{path}::{name}"),
                path: path.to_string(),
                name: name.to_string(),
            })
        })
        .collect();

    let graph_edges: Vec<Edge> = edges
        .into_iter()
        .map(|(from, to)| Edge {
            from: from.to_string(),
            to: to.to_string(),
            is_cycle: false,
        })
        .collect();

    Report {
        files,
        graph: SymbolGraph {
            nodes,
            edges: graph_edges,
            roots: vec![],
        },
        ..empty_report()
    }
}

pub(super) fn dummy_view(path: &str) -> source::HighlightedSourceView {
    source::HighlightedSourceView {
        view: source::SourceView {
            path: path.to_string(),
            lines: vec![],
            highlight_start: 1,
            highlight_end: 1,
        },
        token_highlights: vec![],
    }
}

pub(super) fn candidate(id: &str, name: &str, path: &str) -> JumpCandidate {
    JumpCandidate {
        id: id.to_string(),
        name: name.to_string(),
        path: path.to_string(),
    }
}
