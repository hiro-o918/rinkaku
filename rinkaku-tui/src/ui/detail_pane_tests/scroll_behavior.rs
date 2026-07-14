// --- rendered scroll behavior (TestBackend) ---

use super::*;

#[test]
fn should_show_overflow_indicator_in_detail_pane_title_when_content_exceeds_viewport() {
    // Row 0 is the "lib.rs" file row itself: `file_detail_lines` lists
    // a "File lib.rs" header, a blank line, a "Symbols (40)" header,
    // then all 40 symbols (43 lines total) — comfortably more than a
    // 20-row terminal's inner pane height can show at once.
    let report = report_with_many_symbols(40);
    // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
    // Detail, which is what this test actually exercises.
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
    // Exact bounds depend on the pane's inner height (20 - 2 for the
    // status line/border layout), so this only pins the shape/start
    // rather than the literal end number, keeping the test robust to
    // an unrelated layout tweak elsewhere in this module.
    assert!(text.contains("Detail (1-"));
    assert!(text.contains("/43)"));
}

#[test]
fn should_not_show_overflow_indicator_when_content_fits_the_viewport() {
    let report = report_with_one_symbol();
    // See the test above for why `ToggleDiff` is needed to reach the
    // Detail pane this test actually exercises.
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
    assert!(text.contains(" Detail "));
    assert!(!text.contains("Detail ("));
}

#[test]
fn should_scroll_detail_pane_content_down_when_scroll_down_is_pressed() {
    let report = report_with_many_symbols(40);
    // `Open` on the file row (cursor starts there) reaches Focus::Right
    // (ADR 0020) without changing the selected row itself, so `Down`
    // afterward scrolls instead of moving the cursor. `ToggleDiff`
    // switches from the default Diff pane to Detail, which is what this
    // test actually exercises.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleDiff)
        .handle_key(crate::app::InputKey::Down);
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
    // One line scrolled down: the first visible content line is now 2
    // instead of 1, and the "File lib.rs" header line (the very first
    // content line, before the two blank/"Symbols (40)" header lines
    // that precede the actual symbol list) has scrolled out of view.
    assert!(text.contains("Detail (2-"));
    assert!(!text.contains("File lib.rs"));
}

#[test]
fn should_clamp_detail_pane_scroll_at_the_last_page() {
    // Request an enormous scroll far past the end of a 40-symbol
    // report; the pane must clamp to its last full page rather than
    // showing a mostly-blank pane past the end of the content.
    let report = report_with_many_symbols(40);
    // `ToggleDiff` switches from the default Diff pane to Detail, which
    // is what this test actually exercises.
    let mut app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleDiff);
    for _ in 0..1000 {
        app = app.handle_key(crate::app::InputKey::Down);
    }
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
    // The last symbol must be visible once clamped to the final page.
    assert!(text.contains("sym39"));
}

#[test]
fn should_return_the_clamped_scroll_from_draw_when_requested_scroll_overshoots() {
    // Dogfooding fix: `draw` must hand back the *clamped* offset it
    // actually rendered (not the caller's unclamped `right_pane_scroll`
    // request), since `crate::run_app` folds this return value back into
    // `App` so an overshot scroll request cannot silently outlive the
    // frame that visibly clamped it.
    let report = report_with_many_symbols(40);
    let mut app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleDiff);
    for _ in 0..1000 {
        app = app.handle_key(crate::app::InputKey::Down);
    }
    assert!(
        app.right_pane_scroll() > 100,
        "the unclamped request must actually have overshot for this test to be meaningful"
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    let mut actual = DrawOutcome::default();
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

    let clamped = actual
        .clamped_right_pane_scroll
        .expect("right pane rendered scrollable content, so a clamped offset must be reported");
    assert!(
        clamped < app.right_pane_scroll(),
        "clamped scroll must be strictly less than the overshot request"
    );
}

#[test]
fn should_reset_scroll_indicator_when_cursor_moves_to_a_different_row() {
    // Scroll down on the file row's detail, then move the cursor onto
    // a symbol row: `App::handle_key`'s reset-on-cursor-move rule means
    // the newly selected row's own (short) detail must render from the
    // top, not carry over the file row's scroll offset.
    let report = report_with_many_symbols(40);
    // `ToggleDiff` switches from the default Diff pane to Detail, which
    // is what this test actually exercises.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleDiff)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::FocusLeft)
        .handle_key(crate::app::InputKey::Down);
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
    // A single symbol's own detail (used-by/callees, both empty here)
    // fits well within the viewport, so no overflow indicator should
    // appear even though the file row's detail definitely overflowed.
    assert!(text.contains(" Detail "));
    assert!(!text.contains("Detail ("));
}
