use super::*;
use crate::detail::FileSymbolSummary;
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::FileReport;

fn symbol(id: &str, name: &str) -> ExtractedSymbol {
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

fn empty_report() -> Report {
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
        removed: vec![],
    }
}

fn report_with_one_symbol() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
        }],
        ..empty_report()
    }
}

#[test]
fn should_start_on_entry_screen_with_topological_order_and_no_status() {
    let report = report_with_one_symbol();

    let app = App::new(&report);

    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(OrderMode::Topological, app.order_mode());
    assert_eq!(None, app.status());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_set_should_quit_when_quit_is_pressed_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Quit);

    assert_eq!(true, app.should_quit());
}

#[test]
fn should_move_cursor_down_when_down_is_pressed() {
    // lib.rs has one file row and one symbol row; Down should move off
    // the initial cursor position (0) onto the symbol row (1).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.nav().cursor());
}

#[test]
fn should_toggle_order_mode_between_topological_and_alpha_numeric() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(OrderMode::Topological, app.order_mode());

    let app = app.handle_key(InputKey::ToggleOrder);
    assert_eq!(OrderMode::AlphaNumeric, app.order_mode());

    let app = app.handle_key(InputKey::ToggleOrder);
    assert_eq!(OrderMode::Topological, app.order_mode());
}

#[test]
fn should_open_source_screen_when_source_key_is_pressed_on_a_symbol_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file, row 1 is the "foo" symbol.
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Source);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_stay_on_entry_screen_when_source_key_is_pressed_on_a_file_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file row itself, not a symbol.
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Source);

    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_return_to_entry_screen_when_back_is_pressed_on_source_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );

    let app = app.handle_key(InputKey::Back);

    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_scroll_source_screen_and_not_move_tree_cursor_when_down_is_pressed() {
    // ADR 0026: `j`/`k` on the source screen scroll
    // `Screen::Source::scroll_top` rather than moving the tree cursor
    // (which the reviewer can't see move behind the source screen
    // anyway — the point of ADR 0026's Context, and the whole reason
    // Source used to be Back-only). Pins both halves of the new
    // contract: the tree cursor stays put *and* the scroll offset
    // moves.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 1,
        },
        *app.screen()
    );
}

#[test]
fn should_clear_status_message_on_the_next_handled_key() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.set_status("a source read failed");
    assert_eq!(Some("a source read failed"), app.status());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(None, app.status());
}

#[test]
fn should_return_file_detail_when_cursor_is_on_a_file_row() {
    // Row 0 is the "lib.rs" file itself, not a symbol (TUI iteration
    // 2: a file row now gets its own detail instead of `None`).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let actual = app.selected_detail(&report);

    let expected = SelectedDetail::File(FileDetail {
        path: "lib.rs".to_string(),
        symbols: vec![FileSymbolSummary {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: false,
            fan_in: 0,
        }],
        skip_reason: None,
        test_symbol_count: None,
    });
    assert_eq!(Some(expected), actual);
}

#[test]
fn should_return_detail_view_when_cursor_is_on_a_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_detail(&report);

    match actual.expect("detail for selected symbol") {
        SelectedDetail::Symbol(detail) => assert_eq!("foo", detail.name),
        other => panic!("expected SelectedDetail::Symbol, got {other:?}"),
    }
}

#[test]
fn should_return_dir_detail_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    let app = App::new(&report);

    let actual = app.selected_detail(&report);

    let expected = SelectedDetail::Dir(DirDetail {
        path: "src".to_string(),
        badges: crate::tree::Badges {
            changed_symbols: 1,
            contract_changes: 0,
            fan_in: 0,
        },
        top_fan_in: vec![],
        cycle_partners: vec![],
        cycle_edges: vec![],
    });
    assert_eq!(Some(expected), actual);
}

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

/// Same shape as `report_with_two_directories`, but with a populated
/// `graph` (that fixture leaves `graph` empty since none of its own
/// nav-focused tests need one) — required for `selected_blast_radius_view` to
/// return `BlastRadiusSelection::View` rather than `Empty` for either
/// directory.
fn report_with_two_directories_and_graph() -> Report {
    let report = report_with_two_directories();
    let graph = rinkaku_core::graph::build_graph(&report.files);
    Report { graph, ..report }
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

#[test]
fn should_start_with_zero_right_pane_scroll() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_start_with_tree_focus() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_move_focus_to_right_when_open_is_pressed_on_a_file_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file row itself (cursor starts there).
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_move_focus_to_right_and_switch_to_diff_pane_when_open_is_pressed_on_a_symbol_row() {
    // Dogfooding fix: a symbol row's Enter used to open `Screen::Source`
    // directly (reading the file from the working tree, which could
    // fail), asymmetric with a file row's Enter, which only switched
    // panes. Both row kinds now behave identically — see `InputKey::Open`'s
    // own doc comment.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_move_focus_to_right_and_switch_to_diff_pane_when_open_is_pressed_on_a_removed_symbol_row()
{
    // A removed symbol has no source to open, but Enter no longer opens
    // source at all (`InputKey::Open`'s own doc comment) — it is a pure
    // pane switch regardless of row kind, so a removed symbol's Enter no
    // longer needs a special no-op case the way `InputKey::Source`'s own
    // `!symbol_ref.removed` guard still does.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        removed: vec![rinkaku_core::extract::RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_leave_scroll_unchanged_when_open_is_pressed_while_right_focused_on_diff() {
    // Map-assisted-review finding: Enter pressed a second time while
    // already reading the Diff pane (Focus::Right, RightPane::Diff) must
    // be a complete no-op — before this fix, the `(Screen::Entry, _,
    // InputKey::Open)` arm matched regardless of focus and the blanket
    // "reset scroll to 0" rule at the end of `handle_key` then threw
    // away the reviewer's reading position for no visible reason.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::Down) // scroll -> 1
        .handle_key(InputKey::Down); // scroll -> 2
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_switch_to_diff_pane_when_open_is_pressed_while_right_focused_on_detail() {
    // The other half of the map-assisted-review finding just above:
    // while Focus::Right but the pane showing is Detail (not Diff yet),
    // Enter is a *real* pane switch, so the ordinary scroll-reset-on-
    // other-keys rule is correct here, not a bug to preserve around.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::ToggleDiff); // RightPane::Detail, focus stays Right
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Detail, app.right_pane());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_leave_scroll_unchanged_when_pending_goto_is_pressed() {
    // Independent-review finding (`InputKey::PendingGoto`'s own doc
    // comment): `g`, the leading key of the two-key `gd`/`gr` sequence,
    // is dispatched through `handle_key` on its own, one keypress before
    // `GotoDefinition`/`GotoReferences` — its own blanket scroll reset
    // must not fire, or the jumplist entry recorded once `d`/`r`
    // eventually runs would already have lost the reviewer's scroll
    // position before the jump even started.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::Down) // scroll -> 1
        .handle_key(InputKey::Down); // scroll -> 2
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::PendingGoto);

    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_expand_collapse_and_keep_tree_focus_when_open_is_pressed_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    // Row 0 is the "src" directory itself.
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Tree, app.focus());
    let rows = app.nav().rows(app.tree());
    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src"], paths, "directory should have collapsed");
}

#[test]
fn should_not_move_focus_when_select_is_pressed_on_a_file_row() {
    // Space (`InputKey::Select`) must never move focus, even on a
    // file/symbol row — only Enter (`InputKey::Open`) does (ADR 0020).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Select);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_not_toggle_expand_when_select_is_pressed_while_right_focused() {
    // Finding-5 regression: Space used to fire regardless of focus, so
    // pressing it while Focus::Right silently toggled the expand state
    // of whichever file/symbol row the tree cursor was parked on (the
    // one currently being previewed in the right pane) — a change with
    // no visible effect until the user returned to Focus::Tree. Gated
    // to match InputKey::Open's own Tree-only reach for the same
    // "act on the row under the tree cursor" family of keys.
    let report = report_with_one_symbol();
    let app = App::new(&report); // cursor on the "lib.rs" file row
    let rows_before: Vec<String> = app
        .nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.clone())
        .collect();
    let app = app.handle_key(InputKey::Open); // focus -> Right
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_key(InputKey::Select);

    assert_eq!(Focus::Right, app.focus());
    let rows_after: Vec<String> = app
        .nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.clone())
        .collect();
    assert_eq!(
        rows_before, rows_after,
        "Select while Right-focused must not change which rows are visible"
    );
}

#[test]
fn should_move_cursor_while_tree_focused_when_down_is_pressed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.nav().cursor());
}

#[test]
fn should_scroll_right_pane_instead_of_moving_cursor_when_down_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open); // focus -> Right
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down).handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_decrement_right_pane_scroll_when_up_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open) // focus -> Right
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(1, app.right_pane_scroll());
}

#[test]
fn should_not_scroll_up_past_zero() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_return_focus_to_tree_when_focus_left_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_do_nothing_when_focus_left_is_pressed_while_already_tree_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_start_with_help_overlay_closed() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(false, app.help_open());
}

#[test]
fn should_open_help_overlay_when_toggle_help_is_pressed() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(true, app.help_open());
}

#[test]
fn should_close_help_overlay_when_toggle_help_is_pressed_again() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert_eq!(true, app.help_open());

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(false, app.help_open());
}

#[test]
fn should_ignore_quit_while_help_overlay_is_open() {
    // ADR 0020: the overlay must be a safe, low-stakes action that
    // cannot be short-circuited by an accidental app exit — `Quit`
    // reaching `handle_key` while the overlay is open (e.g. via a
    // translate_key bug) must still be swallowed defensively, not just
    // rely on `translate_key` never producing it in the first place.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert_eq!(true, app.help_open());

    let app = app.handle_key(InputKey::Quit);

    assert_eq!(true, app.help_open());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_ignore_navigation_keys_while_help_overlay_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(true, app.help_open());
}

#[test]
fn should_leave_other_state_untouched_when_help_overlay_opens() {
    // Opening the overlay must not disturb whatever was already showing
    // underneath it (`Self::help_open`'s own doc comment: "nothing else
    // about `App`'s state changes while the overlay is open").
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleDiff);
    let right_pane_before = app.right_pane();
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(right_pane_before, app.right_pane());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_reset_right_pane_scroll_when_focus_returns_to_tree() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_toggling_diff_pane() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ToggleDiff);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_preserve_right_pane_scroll_when_open_pressed_from_tree_focus_on_diff_pane() {
    // ADR 0027 dogfooding finding: pressing Enter from Tree focus onto
    // a file/symbol row that is already showing on the Diff pane must
    // not wipe `right_pane_scroll` — the pane's content is not
    // changing, only the focus is, so the reviewer's reading position
    // (whether set by the previous auto-scroll to the section start,
    // or by any manual `j`/`k` they did after that) must survive the
    // focus swap. Without this, `run_app`'s "only auto-scroll on focus
    // change" rule leaves nothing to re-derive the scroll from and the
    // pane snaps back to line 0.
    let report = report_with_one_symbol();
    // Start on Diff pane (the default per ADR 0020), cursor on the
    // symbol row, focus Right. Manually seed a nonzero scroll to
    // represent an auto-scroll offset or a manual `j` press.
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> the symbol row
        .with_right_pane_scroll(4);
    assert_eq!(Focus::Tree, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(4, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(4, app.right_pane_scroll());
}

#[test]
fn should_keep_right_pane_scroll_at_zero_when_returning_from_source_screen() {
    // Opening the source screen itself always resets scroll to 0
    // (`InputKey::Source`'s own reset, per the blanket rule) and every
    // key but `Back` is then a no-op while `Screen::Source` is active
    // (`App::handle_key`'s `Screen::Source` arm) — so scroll can never
    // become nonzero while the source screen is open in the first
    // place, unlike the pre-ADR-0020 world where `ScrollDown`/`ScrollUp`
    // were separate keys `Screen::Source`'s catch-all arm also
    // swallowed but which could still be pending from before entering.
    // This test pins that invariant end to end: `Back` finds scroll
    // already at 0 and leaves it there.
    //
    // Post-dogfooding-fix note: the source screen is now reached only
    // via the dedicated `s` key (`InputKey::Source`), not `Enter`
    // (`InputKey::Open`'s own doc comment) — `Open` is exercised
    // separately by the `Open`-specific tests above.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Source); // opens Screen::Source
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Back);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_scroll_source_screen_without_touching_right_pane_scroll_when_down_is_pressed() {
    // ADR 0026: `j`/`k` on the source screen updates
    // `Screen::Source::scroll_top`, not the entry view's own
    // `right_pane_scroll` — the two are independent pieces of scroll
    // state (see the field's own doc comment on why they were not
    // unified). Pins both halves: `right_pane_scroll` stays at 0
    // (the entry view has never scrolled) and the source screen's
    // `scroll_top` moves.
    //
    // Post-dogfooding-fix note: the source screen is reached only via
    // the dedicated `s` key (`InputKey::Source`), not `Enter` — see
    // `should_keep_right_pane_scroll_at_zero_when_returning_from_source_screen`'s
    // own note.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source); // opens source

    let app = app.handle_key(InputKey::Down);

    assert_eq!(0, app.right_pane_scroll());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 1,
        },
        *app.screen()
    );
}

/// Two independent top-level directories, each with one file holding
/// one symbol — deep/wide enough that `Nav::retarget_cursor` can land
/// the cursor on a genuinely different node after a collapse, matching
/// `nav.rs`'s own `should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree`
/// fixture shape. Expanded row order: a(0), a/one.rs(1), foo(2), b(3),
/// b/two.rs(4), bar(5).
fn report_with_two_directories() -> Report {
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

/// Moves the cursor down onto "a/one.rs" (a File row, row 1 of
/// [`report_with_two_directories`]'s expanded order), presses `Open` to
/// reach [`Focus::Right`] (ADR 0020: scrolling only applies there — a
/// Dir row's own `Open` never changes focus, per `App::handle_key`'s
/// `Open` arm, so this must land on a File/Symbol row specifically),
/// then scrolls down by one line. Shared setup for every "does *this*
/// action reset the scroll offset" test below, since
/// `CollapseAll`/`ExpandAll`/`ToggleOrder` all remain tree-affecting
/// regardless of which pane currently has focus (their `handle_key`
/// match arms are focus-independent).
fn focused_right_and_scrolled_one_line(app: App) -> App {
    app.handle_key(InputKey::Down)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down)
}

#[test]
fn should_reset_right_pane_scroll_when_select_collapses_the_row_under_the_cursor() {
    // Row 0 is "a" itself; collapsing it via `Select` hides its
    // children but the cursor's own row survives unmoved — still a
    // case the blanket reset rule must cover, since a directory row's
    // own detail content (fan-in/badges) does not depend on which of
    // its children are currently shown, but this pins the simplest
    // Select case regardless. `Open` on "a" (a directory row) does not
    // itself change focus (`App::handle_key`'s `Open` arm), so `Down`
    // right after it is still what actually reaches `Focus::Right` —
    // reusing the shared four-directory fixture below would change
    // which row is under the cursor, so this test builds its own
    // two-directory report and drives the two steps by hand instead of
    // via `focused_right_and_scrolled_one_line`.
    let report = report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::Select);

    // `Select` never moves focus (ADR 0020), and scrolling never
    // applied here in the first place (Focus::Tree the whole time), so
    // this collapses to: collapsing "a" leaves the scroll offset at its
    // already-zero default. Kept as its own test (rather than folded
    // into a broader one) since it pins that `Select` specifically
    // never becomes a scroll-affecting action just because it can
    // reshuffle the row list, matching `CollapseAll`'s own case below.
    assert_eq!(0, app.right_pane_scroll());
    let rows = app.nav().rows(app.tree());
    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    // "bar" (a Symbol row) carries its containing file's path
    // ("b/two.rs"), not a path of its own (`TreeNode::path`'s own doc
    // comment) — so the flattened path list repeats "b/two.rs" for both
    // the File row and its one Symbol child.
    assert_eq!(vec!["a", "b", "b/two.rs", "b/two.rs"], paths);
}

#[test]
fn should_reset_right_pane_scroll_when_collapse_all_retargets_cursor_onto_a_different_node() {
    // Cursor starts on "b/two.rs" (row 4, the File row directly under
    // "b"); CollapseAll hides every file/symbol row, and
    // `Nav::retarget_cursor` lands the cursor on "b" (the nearest
    // still-visible ancestor) — a genuinely different node's detail
    // than the one the pre-collapse scroll offset was scrolled into.
    // "b/two.rs" (a File row, not "bar"/a Symbol row) is the deliberate
    // choice: `Open` on a Symbol row also switches to `Screen::Source`
    // (`App::handle_key`'s `Open` arm), which would make the
    // `CollapseAll` this test presses next a no-op (every key but
    // `Back` is swallowed on `Screen::Source`) — a File row reaches
    // `Focus::Right` without leaving `Screen::Entry`.
    let report = report_with_two_directories();
    let mut app = App::new(&report);
    for _ in 0..4 {
        app = app.handle_key(InputKey::Down);
    }
    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
    let app = app
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::CollapseAll);

    let rows = app.nav().rows(app.tree());
    assert_eq!("b", rows[app.nav().cursor()].node.path);
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_expand_all_is_pressed() {
    // `CollapseAll` first (before establishing focus/scroll) would land
    // the cursor on a Dir row ("a"), which `Open` cannot move focus from
    // (`App::handle_key`'s `Open` arm) — so this test instead reaches
    // Focus::Right + a nonzero scroll on "a/one.rs" while still
    // expanded, then presses `CollapseAll` followed by `ExpandAll` in
    // one breath and asserts the scroll is (still) 0 after both, which
    // is what actually matters: `ExpandAll` itself must never leave a
    // stale nonzero scroll behind, regardless of what `CollapseAll`
    // already reset it to just before.
    let report = report_with_two_directories();
    let app = focused_right_and_scrolled_one_line(App::new(&report));
    assert_eq!(1, app.right_pane_scroll());
    let app = app.handle_key(InputKey::CollapseAll);
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ExpandAll);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_toggle_order_is_pressed() {
    // ToggleOrder can change which row now sits at the same cursor
    // index (reordering siblings), so it must reset the scroll offset
    // even though it never calls into `Nav` at all.
    let report = report_with_two_directories();
    let app = App::new(&report);
    let app = focused_right_and_scrolled_one_line(app);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ToggleOrder);

    assert_eq!(0, app.right_pane_scroll());
}

// Jump navigation tests (ADR 0022): `selected_symbol_id`,
// `jump_to_symbol`, `open_jump_popup`, the popup's own key handling,
// `pending_prefix` bookkeeping, and the jumplist (`JumpBack`/
// `JumpForward`).

/// Two symbols in two files, "a::foo" calling "b::bar" — enough for a
/// jump to have a real target to land on and expand a collapsed
/// ancestor. Expanded row order: a(0), a/one.rs(1), foo(2), b(3),
/// b/two.rs(4), bar(5) — same shape as `report_with_two_directories`,
/// with a populated `graph` so `symbol_mentions`-driven callers can
/// exercise it directly (this module's own tests call `Nav`/`App`
/// methods directly rather than through `crate::lib::resolve_goto`,
/// which lives in a different module and is tested there instead).
fn report_with_caller_and_callee() -> Report {
    let mut report = report_with_two_directories();
    report.files[0].symbols[0].id = "a/one.rs::foo".to_string();
    report.files[1].symbols[0].id = "b/two.rs::bar".to_string();
    report.graph = rinkaku_core::graph::SymbolGraph {
        nodes: vec![
            rinkaku_core::graph::Node {
                id: "a/one.rs::foo".to_string(),
                path: "a/one.rs".to_string(),
                name: "foo".to_string(),
            },
            rinkaku_core::graph::Node {
                id: "b/two.rs::bar".to_string(),
                path: "b/two.rs".to_string(),
                name: "bar".to_string(),
            },
        ],
        edges: vec![rinkaku_core::graph::Edge {
            from: "a/one.rs::foo".to_string(),
            to: "b/two.rs::bar".to_string(),
            is_cycle: false,
        }],
        roots: vec!["a/one.rs::foo".to_string()],
    };
    report
}

#[test]
fn should_return_selected_symbol_id_when_cursor_is_on_a_present_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
}

#[test]
fn should_return_none_selected_symbol_id_when_cursor_is_on_a_file_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    assert_eq!(None, app.selected_symbol_id());
}

#[test]
fn should_return_none_selected_symbol_id_when_cursor_is_on_a_removed_symbol() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        removed: vec![rinkaku_core::extract::RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };
    let app = App::new(&report).handle_key(InputKey::Down);

    assert_eq!(None, app.selected_symbol_id());
}

#[test]
fn should_move_cursor_to_target_symbol_when_jump_to_symbol_is_called() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report);

    let app = app.jump_to_symbol("b/two.rs::bar");

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_reset_scroll_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    let app = focused_right_and_scrolled_one_line(App::new(&report)); // cursor on "a/one.rs", scroll -> 1
    assert_eq!(1, app.right_pane_scroll());

    let app = app.jump_to_symbol("b/two.rs::bar");

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_keep_focus_unchanged_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open); // focus -> Right, cursor on "foo"
    assert_eq!(Focus::Right, app.focus());

    let app = app.jump_to_symbol("b/two.rs::bar");

    assert_eq!(Focus::Right, app.focus());
}

#[test]
fn should_push_current_symbol_onto_jumplist_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    // Cursor on "foo" (row 2).
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(
        "a/one.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.jump_to_symbol("b/two.rs::bar");
    let app = app.handle_key(InputKey::JumpBack);

    // Ctrl-o after the jump must land back on "foo" — proof the
    // pre-jump location was actually pushed.
    let rows = app.nav().rows(app.tree());
    assert_eq!("a/one.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_expand_collapsed_ancestor_when_jump_to_symbol_targets_a_hidden_row() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).handle_key(InputKey::CollapseAll);
    assert_eq!(vec!["a", "b"], row_paths_of(&app));

    let app = app.jump_to_symbol("b/two.rs::bar");

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_set_status_and_leave_cursor_unmoved_when_jump_to_symbol_target_does_not_exist() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).handle_key(InputKey::Down);
    let cursor_before = app.nav().cursor();

    let app = app.jump_to_symbol("no/such::id");

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(
        Some("note: symbol no/such::id is no longer present"),
        app.status()
    );
}

fn row_paths_of(app: &App) -> Vec<&str> {
    app.nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.as_str())
        .collect()
}

#[test]
fn should_open_jump_popup_with_given_candidates() {
    let report = empty_report();
    let app = App::new(&report);
    let candidates = vec![
        JumpCandidate {
            id: "a".to_string(),
            name: "a".to_string(),
            path: "a.rs".to_string(),
        },
        JumpCandidate {
            id: "b".to_string(),
            name: "b".to_string(),
            path: "b.rs".to_string(),
        },
    ];

    let app = app.open_jump_popup(candidates.clone());

    assert_eq!(
        Some(&JumpPopup {
            candidates,
            cursor: 0
        }),
        app.jump_popup()
    );
}

fn two_candidates() -> Vec<JumpCandidate> {
    vec![
        JumpCandidate {
            id: "a/one.rs::foo".to_string(),
            name: "foo".to_string(),
            path: "a/one.rs".to_string(),
        },
        JumpCandidate {
            id: "b/two.rs::bar".to_string(),
            name: "bar".to_string(),
            path: "b/two.rs".to_string(),
        },
    ]
}

#[test]
fn should_move_popup_cursor_down_when_down_is_pressed_while_popup_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_clamp_popup_cursor_at_last_candidate_when_down_is_pressed_past_the_end() {
    let report = empty_report();
    let app = App::new(&report)
        .open_jump_popup(two_candidates())
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    assert_eq!(1, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_clamp_popup_cursor_at_zero_when_up_is_pressed_past_the_top() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(0, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_close_popup_without_jumping_when_popup_cancel_is_pressed() {
    let report = report_with_caller_and_callee();
    let cursor_before = App::new(&report).nav().cursor();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::PopupCancel);

    assert_eq!(None, app.jump_popup());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_jump_to_highlighted_candidate_and_close_popup_when_popup_confirm_is_pressed() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .open_jump_popup(two_candidates())
        .handle_key(InputKey::Down); // highlight "bar"

    let app = app.handle_key(InputKey::PopupConfirm);

    assert_eq!(None, app.jump_popup());
    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_ignore_navigation_keys_while_jump_popup_is_open() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).open_jump_popup(two_candidates());
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::ExpandAll);

    assert_eq!(cursor_before, app.nav().cursor());
    assert!(app.jump_popup().is_some());
}

#[test]
fn should_set_pending_prefix_when_pending_goto_is_pressed() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(None, app.pending_prefix());

    let app = app.handle_key(InputKey::PendingGoto);

    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());
}

#[test]
fn should_clear_pending_prefix_when_any_other_key_follows_pending_goto() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);
    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(None, app.pending_prefix());
}

#[test]
fn should_set_status_when_jump_back_is_pressed_with_an_empty_back_stack() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::JumpBack);

    assert_eq!(Some("note: jumplist has no earlier location"), app.status());
}

#[test]
fn should_set_status_when_jump_forward_is_pressed_with_an_empty_forward_stack() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::JumpForward);

    assert_eq!(Some("note: jumplist has no later location"), app.status());
}

#[test]
fn should_return_to_pre_jump_symbol_when_jump_back_is_pressed_after_a_jump() {
    let report = report_with_caller_and_callee();
    // Cursor on "foo" (row 2) before jumping.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar");
    assert_eq!(
        "b/two.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.handle_key(InputKey::JumpBack);

    let rows = app.nav().rows(app.tree());
    assert_eq!("a/one.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_return_to_post_jump_symbol_when_jump_forward_is_pressed_after_jump_back() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar")
        .handle_key(InputKey::JumpBack);
    assert_eq!(
        "a/one.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.handle_key(InputKey::JumpForward);

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_clear_forward_stack_when_a_new_jump_is_made_from_the_middle_of_history() {
    // vim's own jumplist semantics: jumping to a new location abandons
    // whatever forward history existed, rather than preserving it.
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar")
        .handle_key(InputKey::JumpBack); // back on "foo", forward-stack has "bar"

    let app = app.jump_to_symbol("b/two.rs::bar"); // a fresh jump from "foo"

    let app = app.handle_key(InputKey::JumpForward);
    assert_eq!(Some("note: jumplist has no later location"), app.status());
}

// ADR 0026: scroll bindings — `App::handle_key`'s Source-screen `Up`/
// `Down` arms plus `App::handle_scroll_key`'s four scroll variants,
// exercised against both `Screen::Source` and `Screen::Entry` +
// `Focus::Right`.

#[test]
fn should_scroll_source_up_from_a_nonzero_position_when_up_is_pressed() {
    // Complements `should_scroll_source_screen_and_not_move_tree_cursor_when_down_is_pressed`
    // above (which covers the Down direction from 0). Starts at
    // scroll_top = 3 (via `with_source_scroll_top`, mirroring how
    // `crate::run_app` back-fills the centered initial position) so
    // the `Up` press has somewhere to move from, and pins the
    // `saturating_sub(1)` step size.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(3);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 2,
        },
        *app.screen()
    );
}

#[test]
fn should_saturate_source_scroll_up_at_zero_when_already_at_the_top() {
    // A `k` at scroll 0 must stay at 0 rather than underflowing to
    // `usize::MAX` — the same `saturating_sub` discipline the entry
    // view's own Up arm already uses for `right_pane_scroll`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_add_half_viewport_to_source_scroll_top_when_scroll_half_page_down_is_dispatched() {
    // `handle_scroll_key` needs the viewport height, so this is
    // exercised via `handle_scroll_key` directly (not `handle_key`)
    // — the same two-step dispatch `crate::run_app` uses. Viewport
    // height 20 → step size 10.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 10,
        },
        *app.screen()
    );
}

#[test]
fn should_saturate_source_scroll_half_page_up_at_zero() {
    // Half-page up from a low scroll must not underflow — mirrors
    // the Up arm's `saturating_sub` (`saturating_sub` inside
    // `handle_scroll_key` for this one).
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(3);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageUp, 20);

    // 3 - 10 saturates to 0.
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_reset_source_scroll_top_to_zero_when_scroll_to_top_is_dispatched() {
    // `gg` on the source screen. Viewport height is irrelevant for
    // this variant (it does not read it) but still required by the
    // shared `handle_scroll_key` signature.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(50);

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_set_source_scroll_top_to_usize_max_sentinel_when_scroll_to_bottom_is_dispatched() {
    // `G` on the source screen. `App` has no notion of the file's
    // line count — the actual "bottom" is resolved by `ui`'s
    // `clamped_window` at draw time — so `handle_scroll_key` records
    // `usize::MAX` here and lets the draw-time clamp fold it down.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: usize::MAX,
        },
        *app.screen()
    );
}

#[test]
fn should_add_half_viewport_to_right_pane_scroll_when_focus_right_scroll_half_page_down() {
    // Entry-view side of ADR 0026: the same four scroll variants
    // act on `right_pane_scroll` while `Focus::Right`. Reach
    // `Focus::Right` via `Open` on the file row (ADR 0020) so the
    // scroll actually applies to a real pane.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 30);

    assert_eq!(15, app.right_pane_scroll());
}

#[test]
fn should_leave_right_pane_scroll_untouched_when_focus_tree_and_scroll_half_page_down() {
    // Tree focus is the entry view's default; `handle_scroll_key` must
    // no-op there (ADR 0026's decision 3), leaving `right_pane_scroll`
    // at 0 rather than scrolling a pane the reviewer is not looking
    // at.
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 30);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_to_zero_on_focus_right_scroll_to_top() {
    // `gg` on the entry view + `Focus::Right`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .with_right_pane_scroll(42);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 30);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_set_right_pane_scroll_to_usize_max_sentinel_on_focus_right_scroll_to_bottom() {
    // `G` on the entry view + `Focus::Right`. Same
    // `usize::MAX`-sentinel-plus-draw-time-clamp discipline the
    // source screen uses.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 30);

    assert_eq!(usize::MAX, app.right_pane_scroll());
}
