use super::*;

#[test]
fn should_draw_placeholder_message_when_there_are_no_rows_at_all() {
    let report = Report {
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
    };
    // ADR 0020 defaults the right pane to Diff, whose own placeholder
    // text differs ("select a symbol or file row..."); `ToggleDiff`
    // switches to Detail, whose placeholder is what this test checks.
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("select a row"));
}

#[test]
fn should_draw_dir_detail_content_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
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
    };
    // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
    // Detail, which is what this test actually exercises. (A directory
    // row has no diff-specific content of its own, so leaving it on the
    // default Diff pane would just show that pane's placeholder rather
    // than the dir-detail content this test checks for.)
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Dir src"));
    assert!(text.contains("changed symbols:"));
    assert!(text.contains("Top fan-in"));
}

// ADR 0017: a whole-repo outline's directory detail must not say
// "changed symbols" — nothing changed in that mode — so this pins
// `dir_detail_lines`'s label switching on `report.origin`, using the
// same report shape as
// `should_draw_dir_detail_content_when_cursor_is_on_a_directory_row`
// above (differing only in `origin`) so the two tests read as a pair.
#[test]
fn should_draw_symbols_label_without_changed_wording_when_origin_is_repo_outline() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::RepoOutline,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
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
    };
    // See the sibling test above for why `ToggleDiff` is needed to
    // reach the Detail pane this test actually exercises.
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Dir src"));
    assert!(text.contains("symbols:"));
    assert!(!text.contains("changed symbols:"));
}

#[test]
fn should_draw_skip_reason_in_detail_pane_when_cursor_is_on_a_skipped_file_row() {
    let report = report_with_one_skipped_file();
    // Row 0 is the collapsing "assets" dir (single child, see
    // `crate::tree::build_tree`'s collapsing rule); row 1 is the
    // skipped "logo.png" file itself. ADR 0020 defaults the right pane
    // to Diff, so `ToggleDiff` is needed here to reach Detail (unlike
    // the pre-v2 default this test originally relied on).
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("File assets/logo.png"));
    assert!(text.contains("Skipped: binary"));
    assert!(!text.contains("Symbols ("));
}

#[test]
fn should_draw_test_symbol_count_in_detail_pane_when_cursor_is_on_a_whole_test_file_row() {
    let report = report_with_one_test_file();
    // Row 0 is the collapsing "src" dir; row 1 is the whole test file.
    // ADR 0020 defaults the right pane to Diff, so `ToggleDiff` is
    // needed here to reach Detail.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("File src/lib_test.go"));
    // "Test file: 3 changed test symbols" wraps across two rendered
    // lines at this terminal's pane width, so assert on a substring
    // that survives the wrap rather than the whole phrase.
    assert!(text.contains("Test file: 3 changed test"));
    assert!(!text.contains("Symbols ("));
}

// Regression test (post-rebase integration check): a mixed file — real
// symbols in `report.files` *and* a test-symbol count in `report.tests`
// for the same path (`pipeline::partition_test_symbols`'s legitimate
// output for a file with both production and test code changed) — must
// show both the test-file note and the real "Symbols (N)" listing in
// the detail pane, not just one. This is the exact shape that caused a
// live panic (`rinkaku-tui/src/app.rs` in this repo's own diff) before
// `TreeBuilder::insert_test_file` stopped rejecting a file that already
// carries real symbols.
#[test]
fn should_draw_both_test_note_and_real_symbols_in_detail_pane_when_file_is_mixed() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "app.rs".to_string(),
            symbols: vec![symbol("app.rs::handle_key", "handle_key")],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![rinkaku_core::render::TestFileSummary {
            path: "app.rs".to_string(),
            symbol_count: 5,
        }],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    // Row 0 is the "app.rs" file row itself.
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("File app.rs"));
    assert!(text.contains("Test file: 5 changed test"));
    assert!(text.contains("Symbols (1)"));
    assert!(text.contains("handle_key"));
}

#[test]
fn should_draw_detail_pane_content_when_cursor_is_on_a_symbol_row() {
    let report = report_with_one_symbol();
    // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
    // Detail, which is what this test actually exercises.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("foo"));
    assert!(text.contains("Used by"));
}
