use super::*;
use crate::app::InputKey;

#[test]
fn should_draw_old_and_new_lines_side_by_side_when_split_view_is_toggled_on() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn old_foo() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    // Wide enough that the diff pane's own 60%-of-width share
    // (`ENTRY_RIGHT_WIDTH_PERCENT`) still clears `MIN_SPLIT_VIEW_WIDTH`.
    let mut terminal = Terminal::new(TestBackend::new(200, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // Both sides render on the same row since one removed line pairs
    // positionally against one added line (`pair_hunk_lines`), and the
    // pane is wide enough to stay in split mode.
    let paired_row = text
        .lines()
        .find(|line| line.contains("old_foo") && line.contains("fn foo()"))
        .unwrap_or_else(|| panic!("expected a row with both sides, got:\n{text}"));
    assert!(paired_row.contains("-fn old_foo() {}"));
    assert!(paired_row.contains("+fn foo() {}"));
}

#[test]
fn should_fall_back_to_unified_when_pane_is_narrower_than_the_split_view_minimum() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn old_foo() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    // Narrower than `MIN_SPLIT_VIEW_WIDTH` (100): the pane must render
    // unified (ADR 0044 decision 7) even though `diff_view_mode` is
    // `Split`, with a note explaining why the toggle had no visible
    // effect.
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("-fn old_foo() {}"));
    assert!(text.contains("+fn foo() {}"));
    assert!(text.contains("split view needs a wider pane"));
}

#[test]
fn should_render_unified_when_split_view_is_not_toggled_on() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn old_foo() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    // Wide enough that the diff pane's own 60%-of-width share
    // (`ENTRY_RIGHT_WIDTH_PERCENT`) still clears `MIN_SPLIT_VIEW_WIDTH`.
    let mut terminal = Terminal::new(TestBackend::new(200, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(!text.contains("split view needs a wider pane"));
    // Unified mode interleaves the two lines rather than pairing them
    // onto one row — the removed line's own row contains no added text.
    let removed_row = text
        .lines()
        .find(|line| line.contains("-fn old_foo() {}"))
        .unwrap_or_else(|| panic!("expected a row with the removed line, got:\n{text}"));
    assert!(!removed_row.contains("+fn foo() {}"));
}
