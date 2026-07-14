use super::{empty_report, report_with_one_symbol, report_with_two_directories_and_graph, symbol};
use crate::app::{
    App, BlastRadiusSelection, DiffFocus, DiffTarget, DiffViewMode, InputKey, RightPane, Screen,
};
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::ExtractedSymbol;
use rinkaku_core::render::{FileReport, Report};

#[test]
fn should_default_right_pane_to_diff() {
    // ADR 0020: "what changed" is what a reviewer wants first, ahead of
    // the aggregated used-by/callers view Detail shows.
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_toggle_right_pane_between_diff_and_detail() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(RightPane::Diff, app.right_pane());

    let app = app.handle_key(InputKey::ToggleDiff);
    assert_eq!(RightPane::Detail, app.right_pane());

    let app = app.handle_key(InputKey::ToggleDiff);
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_toggle_right_pane_between_diff_and_blast_radius() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(RightPane::Diff, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(RightPane::BlastRadius, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_switch_from_blast_radius_to_diff_when_toggle_diff_is_pressed() {
    // ADR 0019/0023: "R" re-press or "d" both leave blast-radius mode —
    // "d" always lands on Diff regardless of `blast_radius_return_pane`
    // (a deliberate, unconditional gesture — see `handle_key`'s
    // `ToggleDiff` arm). Uses Detail (not the default Diff) as the pane
    // the blast-radius pane was opened from, so this test still shows
    // something once "d" is pressed even though the destination is
    // unconditional either way.
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleDiff) // Diff -> Detail
        .handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(RightPane::BlastRadius, app.right_pane());

    let app = app.handle_key(InputKey::ToggleDiff);

    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_switch_from_detail_to_blast_radius_when_toggle_blast_radius_is_pressed() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleDiff); // Diff -> Detail
    assert_eq!(RightPane::Detail, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);

    assert_eq!(RightPane::BlastRadius, app.right_pane());
}

#[test]
fn should_return_to_diff_when_blast_radius_is_toggled_off_after_entering_from_the_default_diff_pane()
 {
    // Opening the blast-radius pane straight from `App::new`'s own
    // default (Diff, ADR 0020) must restore Diff specifically on `R`'s
    // re-press, pinning that `blast_radius_return_pane` is actually
    // captured on entry rather than this behavior being a coincidence
    // of `RightPane::default()`.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(RightPane::BlastRadius, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);

    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_return_to_detail_when_blast_radius_is_toggled_off_after_entering_from_detail() {
    // Companion to the Diff-return-pane test above: opening the
    // blast-radius pane from Detail (reached via `d`, not the default)
    // must still restore Detail specifically, not "whatever the default
    // happens to be".
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleDiff) // Diff -> Detail
        .handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(RightPane::BlastRadius, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);

    assert_eq!(RightPane::Detail, app.right_pane());
}

#[test]
fn should_default_diff_view_mode_to_split() {
    // ADR 0044 amendment: dogfooding found split the more useful opening
    // state for the pane's typical case (a signature or small block edit).
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(DiffViewMode::Split, app.diff_view_mode());
}

#[test]
fn should_toggle_diff_view_mode_between_split_and_unified() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(DiffViewMode::Split, app.diff_view_mode());

    let app = app.handle_key(InputKey::ToggleSplitView);
    assert_eq!(DiffViewMode::Unified, app.diff_view_mode());

    let app = app.handle_key(InputKey::ToggleSplitView);
    assert_eq!(DiffViewMode::Split, app.diff_view_mode());
}

#[test]
fn should_preserve_diff_view_mode_when_cursor_moves_to_a_different_row() {
    // A per-`App` mode, not a per-row one (ADR 0044 decision 2) — mirrors
    // `RightPane`'s own persistence across cursor moves.
    let report = report_with_two_directories_and_graph();
    let app = App::new(&report).handle_key(InputKey::ToggleSplitView);
    assert_eq!(DiffViewMode::Unified, app.diff_view_mode());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(DiffViewMode::Unified, app.diff_view_mode());
}

#[test]
fn should_toggle_diff_view_mode_while_source_screen_is_open() {
    // ADR 0049: `v`/`V` is a genuinely global toggle, not scoped to
    // `Screen::Entry`'s diff pane — it flips the same shared
    // `diff_view_mode` field while `Screen::Source` is open, and the
    // screen itself is left untouched (unlike the fully no-op keys the
    // catch-all arm below this one swallows).
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert_eq!(DiffViewMode::Split, app.diff_view_mode());

    let app = app.handle_key(InputKey::ToggleSplitView);

    assert_eq!(DiffViewMode::Unified, app.diff_view_mode());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_ignore_toggle_blast_radius_while_source_screen_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert_eq!(RightPane::Diff, app.right_pane());

    let app = app.handle_key(InputKey::ToggleBlastRadius);

    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_reset_right_pane_scroll_when_toggling_blast_radius_pane() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ToggleBlastRadius);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_return_not_applicable_blast_radius_selection_when_cursor_is_on_a_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_blast_radius_view(&report);

    assert_eq!(BlastRadiusSelection::NotApplicable, actual);
}

#[test]
fn should_return_blast_radius_view_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        graph: rinkaku_core::graph::SymbolGraph {
            nodes: vec![rinkaku_core::graph::Node {
                id: "src/lib.rs::foo".to_string(),
                path: "src/lib.rs".to_string(),
                name: "foo".to_string(),
                is_test: false,
            }],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        ..empty_report()
    };
    // Row 0 is the "src" directory itself (a single-child directory
    // collapsed with "src/lib.rs" would still leave "src" as the
    // top-level row — see `crate::tree::build_tree`'s collapsing rule;
    // this fixture's one file under one directory does not collapse
    // further since "src/lib.rs" is a file, not a subdirectory).
    let app = App::new(&report);

    let actual = app.selected_blast_radius_view(&report);

    match actual {
        BlastRadiusSelection::View(view) => assert_eq!("src".to_string(), view.path),
        other => panic!("expected BlastRadiusSelection::View, got {other:?}"),
    }
}

#[test]
fn should_return_not_applicable_blast_radius_selection_when_there_are_no_rows_at_all() {
    // The cursor has no row to sit on when the tree itself is empty —
    // distinct from `should_return_empty_blast_radius_selection_when_file_row_path_matches_no_graph_node`
    // below, which pins the actual `BlastRadiusSelection::Empty` trigger
    // (a real File row whose path matches no graph node).
    let report = empty_report();
    let app = App::new(&report);

    let actual = app.selected_blast_radius_view(&report);

    assert_eq!(BlastRadiusSelection::NotApplicable, actual);
}

#[test]
fn should_return_empty_blast_radius_selection_when_file_row_path_matches_no_graph_node() {
    // The real-world trigger for `BlastRadiusSelection::Empty` (not the
    // previous version of this test, which used an empty report and so
    // only ever exercised `NotApplicable` — the cursor had no row at
    // all): a `FileReport` with an empty `symbols` list (e.g. a file
    // whose only changes are comments, or a pure rename) still produces
    // a `File` tree row (`crate::tree::build_tree`'s own doc comment:
    // "a pure rename, still shown as a `File` node with zero badges"),
    // but contributes no node to `report.graph` at all — `graph` here
    // is deliberately left at `empty_report`'s empty default, mirroring
    // that mismatch. `App::new` starts fully expanded with the cursor
    // on the tree's first (and only) row, this file itself.
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![],
        }],
        ..empty_report()
    };
    let app = App::new(&report);

    let actual = app.selected_blast_radius_view(&report);

    assert_eq!(
        BlastRadiusSelection::Empty {
            path: "lib.rs".to_string()
        },
        actual
    );
}

#[test]
fn should_return_not_applicable_blast_radius_selection_when_cursor_is_on_the_tests_section_row() {
    // ADR 0035 Phase B: a section's synthetic path is not a real
    // file-tree prefix, so it has no blast radius to pivot from — same
    // treatment as a symbol row (`NotApplicable`), not `Empty` (which
    // would misleadingly read as "valid selection, nothing under it").
    let report = super::report_with_a_whole_test_file();
    // Row order: lib.rs(0), foo(1), Tests(2), lib_test.go(3).
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    let actual = app.selected_blast_radius_view(&report);

    assert_eq!(BlastRadiusSelection::NotApplicable, actual);
}

#[test]
fn should_follow_cursor_when_moving_between_directory_rows_while_blast_radius_pane_is_active() {
    let report = report_with_two_directories_and_graph();
    let app = App::new(&report).handle_key(InputKey::ToggleBlastRadius);

    let first = match app.selected_blast_radius_view(&report) {
        BlastRadiusSelection::View(view) => view.path,
        other => panic!("expected BlastRadiusSelection::View, got {other:?}"),
    };
    assert_eq!("a".to_string(), first);

    // Row 0 is "a", row 3 is "b" (per report_with_two_directories's own
    // doc comment on expanded row order) — three Down presses land the
    // cursor on "b".
    let app = app
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    let second = match app.selected_blast_radius_view(&report) {
        BlastRadiusSelection::View(view) => view.path,
        other => panic!("expected BlastRadiusSelection::View, got {other:?}"),
    };
    assert_eq!("b".to_string(), second);
}

#[test]
fn should_move_cursor_and_open_blast_radius_pane_when_entry_pivot_path_matches_a_row() {
    let report = report_with_two_directories_and_graph();
    let app = App::new(&report);
    // ADR 0020 made Diff the default right pane; this pins that
    // `with_entry_pivot` still unconditionally overrides it to
    // BlastRadius regardless, since it sets `right_pane` directly after
    // `App::new` rather than consulting `RightPane::default()`.
    assert_eq!(RightPane::Diff, app.right_pane());

    let app = app.with_entry_pivot("b");

    // Row 3 is "b" (per `report_with_two_directories`'s own doc comment
    // on expanded row order).
    assert_eq!(3, app.nav().cursor());
    assert_eq!(RightPane::BlastRadius, app.right_pane());
    assert_eq!(None, app.status());
    let selected = match app.selected_blast_radius_view(&report) {
        BlastRadiusSelection::View(view) => view.path,
        other => panic!("expected BlastRadiusSelection::View, got {other:?}"),
    };
    assert_eq!("b".to_string(), selected);
}

#[test]
fn should_set_status_note_and_leave_defaults_when_entry_pivot_path_matches_no_row() {
    let report = report_with_two_directories_and_graph();
    let app = App::new(&report);

    let app = app.with_entry_pivot("no/such/path");

    assert_eq!(0, app.nav().cursor());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(Some("note: no tree row matches no/such/path"), app.status());
}

#[test]
fn should_ignore_toggle_diff_while_source_screen_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert_eq!(RightPane::Diff, app.right_pane());

    let app = app.handle_key(InputKey::ToggleDiff);

    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_return_none_diff_target_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    let app = App::new(&report);

    let actual = app.selected_diff_target(&report);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_diff_target_when_cursor_is_on_the_tests_section_row() {
    // ADR 0035 Phase B: a section spans multiple files, same reasoning
    // as a directory row above — no single diff to show.
    let report = super::report_with_a_whole_test_file();
    // Row order: lib.rs(0), foo(1), Tests(2), lib_test.go(3).
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    let actual = app.selected_diff_target(&report);

    assert_eq!(None, actual);
}

#[test]
fn should_return_file_diff_target_when_cursor_is_on_a_file_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let actual = app.selected_diff_target(&report);

    assert_eq!(
        Some(DiffTarget::File {
            path: "lib.rs".to_string()
        }),
        actual
    );
}

#[test]
fn should_return_file_diff_target_when_cursor_is_on_a_skipped_file_row() {
    // A skipped file has no symbols, but `selected_diff_target` scopes
    // a file row's diff to the whole file regardless of `skip_reason`
    // (only the entry-tree label/detail pane change for a skipped
    // file) — the raw diff hunks for it still exist and should still
    // be reachable via the diff pane.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![rinkaku_core::render::SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
        ..empty_report()
    };
    // Row 0 is the collapsing "assets" dir (single child, see
    // `crate::tree::build_tree`'s collapsing rule); row 1 is the
    // skipped "logo.png" file itself.
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_diff_target(&report);

    assert_eq!(
        Some(DiffTarget::File {
            path: "assets/logo.png".to_string()
        }),
        actual
    );
}

#[test]
fn should_return_file_diff_target_when_cursor_is_on_a_symbol_row() {
    // ADR 0027 decision 1: even on a symbol row the diff pane resolves
    // to a file-scoped target — "which symbol is focused" is carried by
    // `selected_diff_focus` on a separate accessor instead.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange { start: 3, end: 7 },
                ..symbol("lib.rs::foo", "foo")
            }],
        }],
        ..empty_report()
    };
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_diff_target(&report);

    assert_eq!(
        Some(DiffTarget::File {
            path: "lib.rs".to_string(),
        }),
        actual
    );
}

#[test]
fn should_return_symbol_name_for_diff_header_when_cursor_is_on_a_present_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_diff_header_name();

    assert_eq!(Some("foo"), actual);
}

#[test]
fn should_return_file_path_for_diff_header_when_cursor_is_on_a_file_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let actual = app.selected_diff_header_name();

    assert_eq!(Some("lib.rs"), actual);
}

#[test]
fn should_return_none_for_diff_header_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    let app = App::new(&report);

    let actual = app.selected_diff_header_name();

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_for_diff_header_when_cursor_is_on_a_removed_symbol_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        removed: vec![rinkaku_core::extract::RemovedSymbol {
            name: "old_foo".to_string(),
            kind: rinkaku_core::extract::SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn old_foo()".to_string(),
        }],
        ..empty_report()
    };
    // Row 0 is the "lib.rs" file row, row 1 is the removed "old_foo" symbol.
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_diff_header_name();

    assert_eq!(None, actual);
}

#[test]
fn should_return_diff_focus_when_cursor_is_on_a_present_symbol_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange { start: 3, end: 7 },
                ..symbol("lib.rs::foo", "foo")
            }],
        }],
        ..empty_report()
    };
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_diff_focus(&report);

    assert_eq!(
        Some(DiffFocus {
            path: "lib.rs".to_string(),
            symbol_id: "lib.rs::foo".to_string(),
        }),
        actual
    );
}

#[test]
fn should_return_no_diff_focus_when_cursor_is_on_a_file_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    // App::new lands the cursor on the first row, which is the file row
    // (the tree only has one path so no directory row collapses in).
    let app = App::new(&report);

    let actual = app.selected_diff_focus(&report);

    assert_eq!(None, actual);
}
