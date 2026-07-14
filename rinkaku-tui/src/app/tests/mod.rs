//! Tests for `crate::app`, split from `app/mod.rs` (and previously from
//! a single `tests.rs`) to keep each file under the ADR 0028 file-size
//! threshold. Grouped by which App method or behavior contract each
//! submodule pins:
//!
//! - `basics` — startup defaults, `Quit`, `ToggleOrder`, status
//!   clearing, cursor `Down` on the entry screen, and `selected_detail`
//!   for file/symbol/directory rows
//! - `source_screen` — `Source`/`Back` transitions and all
//!   `Screen::Source` scroll behaviors (ADR 0026: `j`/`k`,
//!   half-page/`gg`/`G`), plus the entry-view `handle_scroll_key`
//!   variants that act on `right_pane_scroll` while `Focus::Right`
//! - `right_pane` — right-pane toggles (`ToggleDiff`,
//!   `ToggleBlastRadius`), `blast_radius_return_pane`,
//!   `selected_blast_radius_view`, `with_entry_pivot`, and
//!   `selected_diff_target`/`selected_diff_focus`
//! - `focus` — `Focus::Tree`/`Focus::Right` defaults, `Open` on file/
//!   symbol/directory/removed rows, `Select` focus gating, `Down`/`Up`
//!   while right-focused, `FocusLeft`, and `PendingGoto` scroll
//!   preservation
//! - `scroll_reset` — the blanket `right_pane_scroll` reset on
//!   `FocusLeft`/`ToggleDiff`/`CollapseAll`/`ExpandAll`/`ToggleOrder`/
//!   `Select`-collapse, plus the `Open`-from-tree preservation and the
//!   Source-return-keeps-zero invariant
//! - `help_overlay` — `ToggleHelp` opening/closing, help scroll
//!   (`j`/`k` and `handle_scroll_key` half-page/`gg`/`G`), the "ignore
//!   quit while open" swallow, and `with_help_scroll`
//! - `jump` — `selected_symbol_id`, `jump_to_symbol`, the jump popup's
//!   own key handling, `PendingPrefix::G` bookkeeping, and the
//!   `JumpBack`/`JumpForward` jumplist (ADR 0022)
//! - `review` — `handle_review_key`'s notes-list overlay dispatch (ADR
//!   0048)

use crate::app::App;
use crate::app::InputKey;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

mod basics;
mod focus;
mod help_overlay;
mod jump;
mod review;
mod right_pane;
mod scroll_reset;
mod source_screen;

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
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
        }],
        ..empty_report()
    }
}

/// Two independent top-level directories, each with one file holding
/// one symbol — deep/wide enough that `Nav::retarget_cursor` can land
/// the cursor on a genuinely different node after a collapse, matching
/// `nav.rs`'s own `should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree`
/// fixture shape. Expanded row order: a(0), a/one.rs(1), foo(2), b(3),
/// b/two.rs(4), bar(5).
pub(super) fn report_with_two_directories() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "a/one.rs".to_string(),
                symbols: vec![symbol("a/one.rs::foo", "foo")],
            },
            FileReport {
                path: "b/two.rs".to_string(),
                symbols: vec![symbol("b/two.rs::bar", "bar")],
            },
        ],
        ..empty_report()
    }
}

/// Same shape as [`report_with_two_directories`], but with a populated
/// `graph` (that fixture leaves `graph` empty since none of its own
/// nav-focused tests need one) — required for `selected_blast_radius_view`
/// to return `BlastRadiusSelection::View` rather than `Empty` for either
/// directory.
pub(super) fn report_with_two_directories_and_graph() -> Report {
    let report = report_with_two_directories();
    let graph = rinkaku_core::graph::build_graph(&report.files);
    Report { graph, ..report }
}

/// One production symbol plus one *whole* test file (every symbol in
/// `lib_test.go` is test code, and its `_test.go` path also matches
/// Go's `LanguageSupport::is_test_path` convention), so
/// `crate::tree::build_tree` produces a trailing `NodeKind::Section`
/// root alongside the production `Dir` root. Expanded row order:
/// `lib.rs`(0), foo(1), Tests(2), `lib_test.go`(3).
pub(super) fn report_with_a_whole_test_file() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo")],
            },
            FileReport {
                path: "lib_test.go".to_string(),
                symbols: vec![ExtractedSymbol {
                    is_test: true,
                    ..symbol("lib_test.go::TestFoo", "TestFoo")
                }],
            },
        ],
        ..empty_report()
    }
}

/// Moves the cursor down onto "a/one.rs" (a File row, row 1 of
/// [`report_with_two_directories`]'s expanded order), presses `Open` to
/// reach [`crate::app::Focus::Right`] (ADR 0020: scrolling only applies
/// there — a Dir row's own `Open` never changes focus, per
/// `App::handle_key`'s `Open` arm, so this must land on a File/Symbol
/// row specifically), then scrolls down by one line. Shared setup for
/// every "does *this* action reset the scroll offset" test below, since
/// `CollapseAll`/`ExpandAll`/`ToggleOrder` all remain tree-affecting
/// regardless of which pane currently has focus (their `handle_key`
/// match arms are focus-independent).
pub(super) fn focused_right_and_scrolled_one_line(app: App) -> App {
    app.handle_key(InputKey::Down)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down)
}
