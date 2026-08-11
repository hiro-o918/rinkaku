use super::*;
use crate::app::InputKey;
use pretty_assertions::assert_eq;

#[test]
fn should_draw_old_and_new_lines_side_by_side_by_default() {
    // ADR 0044 amendment: split is now the default `DiffViewMode`, so no
    // `ToggleSplitView` press is needed to reach it here.
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
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
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
    // ADR 0044 amendment: split is now the default `DiffViewMode`, so no
    // `ToggleSplitView` press is needed to have `diff_view_mode` be `Split`
    // here.
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
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("-fn old_foo() {}"));
    assert!(text.contains("+fn foo() {}"));
    assert!(text.contains("split view needs a wider pane"));
}

#[test]
fn should_render_unified_when_split_view_is_toggled_off() {
    // ADR 0044 amendment: split is now the default `DiffViewMode`, so
    // reaching unified rendering here needs an explicit `ToggleSplitView`
    // press (the opposite of this test's pre-amendment setup).
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
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
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

#[test]
fn should_look_up_highlight_by_source_index_in_split_view() {
    // ADR 0072: every `AttributedHunk` maps 1:1 to its original hunk (no
    // more sub-hunk splitting), so `split_side_line`'s highlight lookup is
    // keyed by plain `source_index` + line position — this pins that the
    // second hunk's own `source_index` (1) is used, not the first hunk's.
    let hunks = vec![
        crate::diff_shape::AttributedHunk {
            source_index: 0,
            hunk: crate::diff_view::Hunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                new_range: Some((1, 1)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Added,
                    content: "foo_body".to_string(),
                }],
            },
        },
        crate::diff_shape::AttributedHunk {
            source_index: 1,
            hunk: crate::diff_view::Hunk {
                header: "@@ -2,1 +2,1 @@".to_string(),
                new_range: Some((2, 2)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Added,
                    content: "bar_body".to_string(),
                }],
            },
        },
    ];
    let highlighted_file = HighlightedFile {
        path: "lib.rs".to_string(),
        hunks: vec![
            vec![Some(vec![TokenSpan {
                start: 0,
                end: 8,
                palette_index: 0,
            }])],
            vec![Some(vec![TokenSpan {
                start: 0,
                end: 8,
                palette_index: 1,
            }])],
        ],
    };

    let (_, right) = diff_pane_split_rows(
        &hunks,
        Some(&highlighted_file),
        &crate::annotation_markers::AnnotationMarkers::default(),
        "lib.rs",
    );

    let body_line = right
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content == "bar_body"))
        .expect("body line present");
    let token_style = body_line
        .spans
        .iter()
        .find(|span| span.content == "bar_body")
        .expect("bar_body span present")
        .style;
    assert_eq!(crate::ui::style::palette_style(1).fg, token_style.fg);
}
