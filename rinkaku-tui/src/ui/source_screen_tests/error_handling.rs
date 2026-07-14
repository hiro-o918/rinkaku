use super::*;

#[test]
fn should_draw_source_screen_title_and_error_message_when_file_is_missing() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");
    let source_content = missing_file_source_content(&report, "lib.rs::foo");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // "lib.rs" does not exist under the placeholder repo root above, so
    // this exercises `draw_source_screen`'s error-message fallback path
    // rather than needing a real file on disk.
    let text = buffer_text(&terminal);
    assert!(text.contains("Source: lib.rs::foo"));
    assert!(text.contains("failed to read"));
    assert!(text.contains("back"));
}

#[test]
fn should_wrap_source_error_message_instead_of_truncating_it_in_a_narrow_pane() {
    // Regression test: `source::load_symbol_source`'s error message
    // (full path + io error + the "not present in the working tree"
    // hint) routinely exceeds one line, and `Paragraph` without
    // `.wrap(...)` silently truncates rather than overflowing — cutting
    // the hint off exactly where it explains the failure. A narrow
    // (40-column) pane makes the message wrap across multiple rows
    // whether or not `.wrap(...)` is set, but only *with* it does the
    // hint's text actually appear anywhere in the buffer; without it,
    // the trailing text is simply dropped.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(40, 20)).expect("terminal");
    let source_content = missing_file_source_content(&report, "lib.rs::foo");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // `buffer_text` joins rows with `\n`, so a phrase that happens to
    // wrap exactly at a row boundary (as "in the" / "present" do at
    // this width) would not appear as one contiguous substring even
    // though every word is visible — asserting on words rather than a
    // multi-word phrase keeps this test robust to exactly where the
    // wrap point falls, while still failing if `.wrap(...)` were
    // removed (the words after "working tree" would be dropped
    // entirely, not just split across rows).
    let text = buffer_text(&terminal);
    assert!(text.contains("Source: lib.rs::foo"));
    assert!(text.contains("present"));
    assert!(text.contains("working tree"));
    assert!(text.contains("historical commit not checked out"));
    assert!(text.contains("locally)"));
}

#[test]
fn should_report_none_clamped_scroll_but_source_viewport_height_when_source_screen_is_open() {
    // ADR 0026: the source screen scrolls via its own
    // `Screen::Source::scroll_top`, not `App::right_pane_scroll`, so
    // `DrawOutcome::clamped_right_pane_scroll` must stay `None` on this
    // screen (otherwise `crate::run_app`'s
    // `clamp_right_pane_scroll_after_draw` would fold a source-screen
    // offset back into the wrong field). At the same time the source
    // pane's inner height must be surfaced via
    // `scroll_viewport_height` — otherwise `Ctrl-d`/`Ctrl-u`/`G` from
    // the source screen would have no viewport to size their step
    // against, defaulting to `DEFAULT_SOURCE_VIEWPORT_HEIGHT`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    let mut actual = DrawOutcome {
        clamped_right_pane_scroll: Some(999),
        scroll_viewport_height: None,
        clamped_help_scroll: None,
        help_scroll_viewport_height: None,
    };
    terminal
        .draw(|frame| {
            actual = draw(
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

    assert_eq!(None, actual.clamped_right_pane_scroll);
    // A 20-row terminal with a 1-row status line leaves 19 rows for
    // the body; the source pane's bordered box takes 2 of them,
    // leaving 17 rows inside. Pinned exactly (rather than a range)
    // so a future layout refactor that silently changes the split
    // is caught, matching the specificity `ADR 0020`'s own
    // `right_pane_viewport_height` shares with `draw_entry_screen`.
    assert_eq!(Some(17), actual.scroll_viewport_height);
}
