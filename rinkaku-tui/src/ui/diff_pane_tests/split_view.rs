use super::*;
use crate::app::InputKey;
use crate::diff_shape::{ContractHeader, DiffSection};
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
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("-fn old_foo() {}"));
    assert!(text.contains("+fn foo() {}"));
    assert!(text.contains("split view needs a wider pane"));
}

#[test]
fn should_pair_old_and_new_signature_on_one_row_when_section_has_a_contract_header() {
    // Regression coverage for the diagonal-placement bug a static review
    // caught: the contract header's old/new signatures must land on the
    // *same* row (left = old, right = new), not on two separate rows with
    // one side blank each — the whole point of a split view is comparing
    // them without scanning past an interleaved row in between.
    let section = DiffSection {
        title: "fn foo(a: i32, b: i32)".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: Some(ContractHeader {
            previous_signature: "fn foo(a: i32)".to_string(),
            signature: "fn foo(a: i32, b: i32)".to_string(),
        }),
        hunks: vec![],
    };

    let (left, right) = diff_pane_split_rows(&[&section], true, None);

    assert_eq!(
        vec![
            Line::styled(
                "fn foo(a: i32, b: i32)".to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                "- fn foo(a: i32)".to_string(),
                Style::default().fg(Color::Red),
            ),
            Line::raw(""),
        ],
        left
    );
    assert_eq!(
        vec![
            Line::styled(
                "fn foo(a: i32, b: i32)".to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                "+ fn foo(a: i32, b: i32)".to_string(),
                Style::default().fg(Color::Green),
            ),
            Line::raw(""),
        ],
        right
    );
    // ADR 0044 decision 4's shared line-counting invariant: both sides
    // stay the same length regardless of the contract-header pairing.
    assert_eq!(left.len(), right.len());
}

#[test]
fn should_draw_old_and_new_signature_side_by_side_when_symbol_signature_changed() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::SignatureChanged),
                previous_signature: Some("fn foo(a: i32)".to_string()),
                signature: "fn foo(a: i32, b: i32)".to_string(),
                ..symbol("lib.rs::foo", "foo")
            }],
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
    // Row 0 is the "lib.rs" file row, row 1 is the "foo" symbol. ADR 0044
    // amendment: split is now the default `DiffViewMode`, so no
    // `ToggleSplitView` press is needed to reach it here.
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn foo(a: i32) {}
+fn foo(a: i32, b: i32) {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
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
    let contract_row = text
        .lines()
        .find(|line| line.contains("- fn foo(a: i32)") && line.contains("+ fn foo(a: i32, b: i32)"))
        .unwrap_or_else(|| panic!("expected the old/new signature on one row, got:\n{text}"));
    assert!(contract_row.contains("- fn foo(a: i32)"));
    assert!(contract_row.contains("+ fn foo(a: i32, b: i32)"));
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
