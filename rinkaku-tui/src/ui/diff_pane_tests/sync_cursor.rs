//! The sync-target cursor marker (`▶`): `sync_target_line`/`is_body_row`/
//! `mark_sync_target_line`'s pure logic, then an end-to-end `TestBackend`
//! check that `draw_diff_pane` actually renders the glyph on the row the
//! tree cursor's current selection maps to — the reason this exists at all
//! is that auto-scroll alone gives no visual feedback once the whole diff
//! already fits on screen (this feature's own motivation).

use super::*;
use crate::app::{DiffViewMode, InputKey};
use crate::diff_shape::AttributedHunk;
use pretty_assertions::assert_eq;

fn hunk_line(kind: crate::diff_view::DiffLineKind, content: &str) -> crate::diff_view::DiffLine {
    crate::diff_view::DiffLine {
        kind,
        content: content.to_string(),
    }
}

fn attributed_hunk(header: &str, new_range: (usize, usize), lines: Vec<&str>) -> AttributedHunk {
    AttributedHunk {
        source_index: 0,
        hunk: crate::diff_view::Hunk {
            header: header.to_string(),
            new_range: Some(new_range),
            lines: lines
                .into_iter()
                .map(|content| hunk_line(crate::diff_view::DiffLineKind::Context, content))
                .collect(),
        },
    }
}

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

// --- is_body_row ---

#[test]
fn should_return_false_for_the_first_hunks_header_row() {
    let hunks = vec![attributed_hunk("@@ -1,2 +1,2 @@", (1, 2), vec!["a", "b"])];

    let actual = is_body_row(&hunks, 0);

    assert!(!actual);
}

#[test]
fn should_return_true_for_a_row_inside_the_first_hunks_body() {
    let hunks = vec![attributed_hunk("@@ -1,2 +1,2 @@", (1, 2), vec!["a", "b"])];

    let actual = is_body_row(&hunks, 1);

    assert!(actual);
    let actual_second_body_row = is_body_row(&hunks, 2);
    assert!(actual_second_body_row);
}

#[test]
fn should_return_false_for_the_blank_separator_between_two_hunks() {
    let hunks = vec![
        attributed_hunk("@@ -1,1 +1,1 @@", (1, 1), vec!["a"]),
        attributed_hunk("@@ -5,1 +5,1 @@", (5, 1), vec!["e"]),
    ];
    // Row layout: 0 = header 1, 1 = body 1, 2 = separator, 3 = header 2,
    // 4 = body 2.

    let actual = is_body_row(&hunks, 2);

    assert!(!actual);
}

#[test]
fn should_return_false_for_the_second_hunks_header_row() {
    let hunks = vec![
        attributed_hunk("@@ -1,1 +1,1 @@", (1, 1), vec!["a"]),
        attributed_hunk("@@ -5,1 +5,1 @@", (5, 1), vec!["e"]),
    ];

    let actual = is_body_row(&hunks, 3);

    assert!(!actual);
}

#[test]
fn should_return_true_for_a_row_inside_the_second_hunks_body() {
    let hunks = vec![
        attributed_hunk("@@ -1,1 +1,1 @@", (1, 1), vec!["a"]),
        attributed_hunk("@@ -5,1 +5,1 @@", (5, 1), vec!["e"]),
    ];

    let actual = is_body_row(&hunks, 4);

    assert!(actual);
}

#[test]
fn should_return_false_for_an_out_of_range_target_line() {
    let hunks = vec![attributed_hunk("@@ -1,1 +1,1 @@", (1, 1), vec!["a"])];

    let actual = is_body_row(&hunks, 99);

    assert!(!actual);
}

// --- mark_sync_target_line ---

#[test]
fn should_overwrite_the_gutter_span_when_the_target_row_has_one() {
    let mut lines = vec![prefix_annotation_marker(Line::raw("+fn foo() {}"), false)];

    mark_sync_target_line(&mut lines, 0, true);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec!["▶+fn foo() {}".to_string()], actual);
}

#[test]
fn should_win_over_an_existing_annotation_marker_on_the_same_row() {
    let mut lines = vec![prefix_annotation_marker(Line::raw("+fn foo() {}"), true)];

    mark_sync_target_line(&mut lines, 0, true);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec!["▶+fn foo() {}".to_string()], actual);
}

#[test]
fn should_prepend_the_glyph_when_the_target_row_has_no_gutter() {
    let mut lines = vec![Line::styled(
        "@@ -1,1 +1,1 @@",
        Style::default().fg(Color::DarkGray),
    )];

    mark_sync_target_line(&mut lines, 0, false);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec!["▶@@ -1,1 +1,1 @@".to_string()], actual);
}

#[test]
fn should_do_nothing_when_the_target_line_is_out_of_range() {
    let mut lines = vec![Line::raw("unchanged")];

    mark_sync_target_line(&mut lines, 5, true);

    let actual: Vec<String> = lines.iter().map(line_text).collect();
    assert_eq!(vec!["unchanged".to_string()], actual);
}

// --- sync_target_line ---

#[test]
fn should_return_none_when_the_cursor_is_on_a_file_row() {
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

    let actual = sync_target_line(&app, &report, &diff_content, DiffViewMode::Unified);

    assert_eq!(None, actual);
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

    let actual = sync_target_line(&app, &report, &diff_content, DiffViewMode::Unified);

    // `report_with_one_symbol`'s "foo" spans line 1, which is where the
    // hunk's `@@` header itself starts (`scroll_target_line_for_symbol`'s
    // own "header shares its first body line's coordinate" rule) — row 0.
    assert_eq!(Some(0), actual);
}

// --- draw_diff_pane end-to-end ---

#[test]
fn should_render_the_cursor_glyph_on_the_focused_symbols_row_in_unified_view() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![rinkaku_core::render::FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol_with_range(
                "lib.rs::foo",
                "foo",
                LineRange { start: 2, end: 2 },
            )],
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
    };
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
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
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
    let target_row = text
        .lines()
        .find(|line| line.contains("+fn foo() {}"))
        .unwrap_or_else(|| panic!("expected the added line's row, got:\n{text}"));
    assert!(target_row.contains("▶+fn foo() {}"));
    // The context line above stays on the plain space gutter — the marker
    // is on the focused symbol's own row only.
    let context_row = text
        .lines()
        .find(|line| line.contains("fn a() {}"))
        .expect("context line present");
    assert!(!context_row.contains('▶'));
}

#[test]
fn should_render_the_cursor_glyph_on_the_right_column_only_in_split_view() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![rinkaku_core::render::FileReport {
            path: "lib.rs".to_string(),
            // Line 2, not line 1: a range starting exactly where the hunk
            // starts would resolve to the `@@` header row instead (the same
            // case `should_resolve_the_focused_symbols_own_row_when_the_cursor_is_on_a_symbol_row`
            // pins) — this test needs the target to land on a paired body
            // row so it can assert the left/right split.
            symbols: vec![symbol_with_range(
                "lib.rs::foo",
                "foo",
                LineRange { start: 2, end: 2 },
            )],
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
    };
    // Split is already the default `DiffViewMode` (ADR 0044 amendment).
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,2 @@
 fn a() {}
-fn old_foo() {}
+fn foo() {}
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
    assert!(!left_half.contains('▶'));
    assert!(right_half.starts_with('▶'));
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
