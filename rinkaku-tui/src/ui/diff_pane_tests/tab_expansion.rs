use super::*;
use crate::ui::style::TAB_WIDTH;
use pretty_assertions::assert_eq;
use rinkaku_core::render::FileReport;

fn go_report() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "main.go".to_string(),
            symbols: vec![symbol("main.go::run", "run")],
        }],
        skipped: vec![],
        graph: SymbolGraph {
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

fn row_containing(terminal: &Terminal<TestBackend>, needle: &str) -> String {
    buffer_text(terminal)
        .lines()
        .find(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("expected a rendered row containing {needle:?}"))
        .to_string()
}

/// The rendered column at which `token` starts, counted from the `+`/`-`/` `
/// diff marker glyph that opens the line's content.
fn column_after_marker(row: &str, marker_and_rest: &str, token: &str) -> usize {
    let line_start = row
        .find(marker_and_rest)
        .unwrap_or_else(|| panic!("expected {marker_and_rest:?} within {row:?}"));
    let token_offset = row[line_start..]
        .find(token)
        .unwrap_or_else(|| panic!("expected {token:?} after {marker_and_rest:?}"));
    row[line_start..line_start + token_offset].chars().count()
}

fn draw_go_diff(diff_text: &str) -> Terminal<TestBackend> {
    let report = go_report();
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

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
    terminal
}

#[test]
fn should_render_tab_indented_go_lines_at_tab_stop_columns_in_diff_pane() {
    let diff_text = "\
diff --git a/main.go b/main.go
index e69de29..4b825dc 100644
--- a/main.go
+++ b/main.go
@@ -1,1 +1,4 @@
 package main
+func run() {
+\tif ok {
+\t\tdeep()
+\t}
";

    let terminal = draw_go_diff(diff_text);
    let buffer = buffer_text(&terminal);

    let one_level = row_containing(&terminal, "if ok {");
    let two_levels = row_containing(&terminal, "deep()");

    assert_eq!(
        (1 + TAB_WIDTH, 1 + TAB_WIDTH * 2),
        (
            column_after_marker(&one_level, "+", "if"),
            column_after_marker(&two_levels, "+", "deep"),
        ),
        "rendered buffer:\n{buffer}"
    );
}

#[test]
fn should_not_leave_tab_characters_in_the_rendered_buffer_when_diff_is_tab_indented() {
    let diff_text = "\
diff --git a/main.go b/main.go
index e69de29..4b825dc 100644
--- a/main.go
+++ b/main.go
@@ -1,1 +1,3 @@
 package main
+func run() {
+\t\tdeep()
";

    let terminal = draw_go_diff(diff_text);

    let actual: Vec<char> = buffer_text(&terminal)
        .chars()
        .filter(|c| *c == '\t')
        .collect();

    assert_eq!(Vec::<char>::new(), actual);
}

#[test]
fn should_expand_tabs_on_the_unhighlighted_path_when_extension_is_unknown() {
    // A `Makefile` has no bundled grammar, so `highlight_diff_files` yields
    // no spans and the line renders through `plain_diff_line` — the path
    // that never reaches `styled_content_spans`.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "Makefile".to_string(),
            symbols: vec![symbol("Makefile::build", "build")],
        }],
        skipped: vec![],
        graph: SymbolGraph {
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
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/Makefile b/Makefile
index e69de29..4b825dc 100644
--- a/Makefile
+++ b/Makefile
@@ -1,1 +1,2 @@
 build:
+\tcargo build
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

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

    let row = row_containing(&terminal, "cargo build");

    assert_eq!(1 + TAB_WIDTH, column_after_marker(&row, "+", "cargo"));
}
