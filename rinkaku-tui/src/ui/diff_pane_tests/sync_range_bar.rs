//! The sync range bar (`┃`): `range_bar_lines`/`mark_range_bar_lines`'s pure
//! logic, then an end-to-end `TestBackend` check that `draw_diff_pane`
//! actually paints the glyph on every row the tree cursor's current symbol
//! selection spans — the reason this exists at all is that auto-scroll
//! alone gives no visual feedback once the whole diff already fits on
//! screen (this feature's own motivation), and a single marked line does
//! not communicate the selected symbol's actual extent when it spans
//! several rows.

use super::*;
use crate::app::{DiffViewMode, InputKey};
use pretty_assertions::assert_eq;

/// [`super::symbol`] pins its range at line 1 for every other test in this
/// module — the cases below need a symbol at an arbitrary line to land on a
/// specific rendered row, so this variant takes `range` explicitly instead.
fn symbol_with_range(
    id: &str,
    name: &str,
    range: LineRange,
) -> rinkaku_core::extract::ExtractedSymbol {
    rinkaku_core::extract::ExtractedSymbol {
        range,
        ..super::symbol(id, name)
    }
}

// --- mark_range_bar_lines ---

#[test]
fn should_overwrite_the_gutter_span_on_every_marked_row() {
    let mut lines = vec![
        prefix_annotation_marker(Line::raw("+fn foo() {"), false),
        prefix_annotation_marker(Line::raw("+    1"), false),
        prefix_annotation_marker(Line::raw("+}"), false),
    ];

    mark_range_bar_lines(&mut lines, &[0, 1, 2]);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(
        vec![
            "┃+fn foo() {".to_string(),
            "┃+    1".to_string(),
            "┃+}".to_string(),
        ],
        actual
    );
}

#[test]
fn should_leave_unmarked_rows_on_their_plain_gutter() {
    let mut lines = vec![
        prefix_annotation_marker(Line::raw("+fn foo() {}"), false),
        prefix_annotation_marker(Line::raw(" fn bar() {}"), false),
    ];

    mark_range_bar_lines(&mut lines, &[0]);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(
        vec!["┃+fn foo() {}".to_string(), "  fn bar() {}".to_string()],
        actual
    );
}

#[test]
fn should_lose_to_an_existing_annotation_marker_on_the_same_row() {
    let mut lines = vec![prefix_annotation_marker(Line::raw("+fn foo() {}"), true)];

    mark_range_bar_lines(&mut lines, &[0]);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec!["*+fn foo() {}".to_string()], actual);
}

#[test]
fn should_do_nothing_for_a_marked_row_past_lines_end() {
    let mut lines = vec![prefix_annotation_marker(Line::raw("+fn foo() {}"), false)];

    mark_range_bar_lines(&mut lines, &[5]);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec![" +fn foo() {}".to_string()], actual);
}

// --- range_bar_lines ---

#[test]
fn should_return_empty_when_the_cursor_is_on_a_file_row() {
    let report = report_with_one_symbol();
    // Row 0 is the file row (App::new's default cursor position).
    let app = App::new(&report);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_content = diff_content_for(&report, &diff_files, &app);

    let actual = range_bar_lines(&app, &report, &diff_content, DiffViewMode::Unified);

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_resolve_the_focused_symbols_own_row_when_the_cursor_is_on_a_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_content = diff_content_for(&report, &diff_files, &app);

    let actual = range_bar_lines(&app, &report, &diff_content, DiffViewMode::Unified);

    // `report_with_one_symbol`'s "foo" spans line 1, which is also `+fn
    // foo() {}`'s own new-side line — row 1 (row 0 is the `@@` header,
    // never marked).
    assert_eq!(vec![1], actual);
}

// --- draw_diff_pane end-to-end ---

fn report_with_symbol_range(range: LineRange) -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![rinkaku_core::render::FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol_with_range("lib.rs::foo", "foo", range)],
        }],
        skipped: vec![],
        graph: rinkaku_core::graph::SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    }
}

#[test]
fn should_render_the_bar_on_every_row_of_a_multi_line_symbol_in_unified_view() {
    let report = report_with_symbol_range(LineRange { start: 1, end: 3 });
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        // Split is the default `DiffViewMode` (ADR 0044 amendment); force
        // unified so this test pins the unified-view gutter column.
        .handle_key(InputKey::ToggleSplitView);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -0,0 +1,3 @@
+fn foo() {
+    1
+}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
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
    for needle in ["+fn foo() {", "+    1", "+}"] {
        let row = text
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("expected a row containing {needle:?}, got:\n{text}"));
        assert!(row.contains(&format!("┃{needle}")), "row was: {row}");
    }
    // The `@@` header row carries no gutter and must never be marked.
    let header_row = text
        .lines()
        .find(|line| line.contains("@@ -0,0 +1,3 @@"))
        .expect("hunk header present");
    assert!(!header_row.contains('┃'));
}

#[test]
fn should_render_no_bar_when_the_cursor_is_on_a_file_row() {
    let report = report_with_symbol_range(LineRange { start: 1, end: 3 });
    // Row 0 is the file row (App::new's default cursor position) — no
    // `DiffFocus`, so nothing should be marked.
    let app = App::new(&report).handle_key(InputKey::ToggleSplitView);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -0,0 +1,3 @@
+fn foo() {
+    1
+}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
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
    assert!(!text.contains('┃'));
}

#[test]
fn should_render_the_bar_on_the_right_column_only_in_split_view() {
    let report = report_with_symbol_range(LineRange { start: 1, end: 2 });
    // Split is already the default `DiffViewMode` (ADR 0044 amendment).
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,2 @@
-fn old_foo() {}
-fn old_bar() {}
+fn foo() {}
+fn bar() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    // Wide enough to clear `MIN_SPLIT_VIEW_WIDTH` through the pane's
    // 60%-of-width share (`ENTRY_RIGHT_WIDTH_PERCENT`).
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
    let paired_row = text
        .lines()
        .find(|line| line.contains("old_foo") && line.contains("fn foo()"))
        .unwrap_or_else(|| panic!("expected a row with both sides, got:\n{text}"));
    // The marker sits in the 1-column gutter immediately before the row's
    // content, so it belongs to whichever half's content follows it —
    // splitting right at `+fn foo() {}`'s own start would misattribute the
    // gutter column to the left half instead. Indexed by chars, not bytes:
    // both the marker and the pane borders are multi-byte UTF-8.
    let chars: Vec<char> = paired_row.chars().collect();
    let right_content_char = chars
        .windows("+fn foo() {}".chars().count())
        .position(|window| window.iter().collect::<String>() == "+fn foo() {}")
        .expect("right column");
    let right_half_start = right_content_char - 1;
    let left_half: String = chars[..right_half_start].iter().collect();
    let right_half: String = chars[right_half_start..].iter().collect();
    assert!(!left_half.contains('┃'));
    assert!(right_half.starts_with('┃'));
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
