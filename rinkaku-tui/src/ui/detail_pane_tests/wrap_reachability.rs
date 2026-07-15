// --- long-line scroll reachability regression (TestBackend) ---

use super::*;

#[test]
fn should_reach_the_last_wrapped_line_of_content_via_scrolling_when_a_logical_line_is_long_enough_to_wrap()
 {
    // A narrow pane (30 inner columns after the 2-column border) with a
    // single logical line far longer than that — mirrors a real fan-in
    // entry's full path being too long for the pane. Before wrapping was
    // applied before the scroll offset, the scroll unit (logical lines)
    // and the render unit (wrapped rows) disagreed, so a marker placed
    // near the end of this one long logical line was unreachable at any
    // scroll offset. Regression coverage for that desync.
    let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: long_line.clone(),
            symbols: vec![],
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
    // ADR 0020 defaults the right pane to Diff, whose own placeholder
    // text also happens to embed the file path (`"(no diff hunks found
    // for <path>)"`) — but not through this test's actual target,
    // `render_scrollable_pane`'s wrap-before-scroll behavior, so
    // `ToggleDiff` switches to Detail to keep exercising that.
    let mut app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleDiff);
    // Scroll far enough down to reach the wrapped tail of the long path
    // line, however many wrapped rows that turns out to be.
    for _ in 0..200 {
        app = app.handle_key(crate::app::InputKey::Down);
    }
    let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("TAIL_MARKER"));
}

#[test]
fn should_report_indicator_total_as_wrapped_row_count_not_logical_line_count_when_a_line_wraps() {
    // Same narrow pane/long-path setup as the reachability regression
    // above: the file row's detail is exactly 2 logical lines ("File
    // <path>" plus a blank line, since this report has no symbols), but
    // the long path line wraps into several rows — the indicator's
    // "/total" must count wrapped rows, not the 2 logical lines, or the
    // indicator would (wrongly) claim everything fits and hide it
    // entirely.
    let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: long_line,
            symbols: vec![],
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
    // Detail, which is what this test actually exercises.
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // Inner width is 34 - 2 = 32 columns; the long line alone wraps into
    // ceil(211 / 32) = 7 rows, well over the "/2" a logical-line count
    // would have produced.
    assert!(text.contains("Detail (1-"));
    assert!(!text.contains("/2)"));
}
