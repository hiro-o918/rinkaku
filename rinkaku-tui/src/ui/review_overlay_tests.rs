use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::review::{ReviewState, SelectionSnapshot};
use crate::ui::draw;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

fn symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range: LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

fn report_with_one_symbol() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
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
    }
}

fn snapshot() -> SelectionSnapshot {
    SelectionSnapshot {
        path: "lib.rs".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        symbol_name: Some("foo".to_string()),
        range: Some((1, 5)),
        anchor: Some((1, 5)),
        signature: Some("fn foo()".to_string()),
    }
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw_app(app: &App, report: &Report) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
    terminal
        .draw(|frame| {
            draw(
                frame,
                app,
                report,
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
    buffer_text(&terminal)
}

#[test]
fn should_not_draw_review_overlay_when_review_is_idle() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let text = draw_app(&app, &report);

    assert!(!text.contains("New annotation"));
    assert!(!text.contains("Review annotations"));
}

#[test]
fn should_draw_compose_overlay_with_location_and_buffer_when_composing() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('h')
        .push_char('i');
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("New annotation"));
    assert!(text.contains("lib.rs:1-5 foo"));
    assert!(text.contains("hi"));
    assert!(text.contains("Enter: save"));
}

#[test]
fn should_draw_annotations_list_overlay_with_annotation_summary() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('f')
        .push_char('i')
        .push_char('x')
        .confirm_compose()
        .open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Review annotations"));
    assert!(text.contains("lib.rs:1-5 foo: fix"));
    assert!(text.contains("Enter: export"));
    assert!(text.contains("d: delete"));
}

#[test]
fn should_draw_empty_annotations_list_placeholder_when_there_are_no_annotations() {
    let report = report_with_one_symbol();
    let review = ReviewState::default().open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("no annotations yet"));
}

#[test]
fn should_draw_both_export_menu_entries_when_sink_a_is_available() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report)
        .with_review_sink_a_available(true)
        .with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Export to"));
    assert!(text.contains("GitHub PR review"));
    assert!(text.contains("Clipboard"));
}

#[test]
fn should_draw_only_clipboard_entry_when_sink_a_is_unavailable() {
    // Regression test: the export menu's *rendering* must match
    // `ReviewState::confirm_export`'s own `sink_a_available`-gated entry
    // list (`export_menu_entries`) — drawing "GitHub PR review"
    // unconditionally, regardless of whether a `PrContext` was ever wired
    // up, misleads the reviewer into thinking cursor position 0 posts a
    // GitHub review when it actually confirms whatever
    // `export_menu_entries(false)` put there instead (`Clipboard`).
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report).with_review(review);
    assert!(!app.review_sink_a_available());

    let text = draw_app(&app, &report);

    assert!(text.contains("Export to"));
    assert!(!text.contains("GitHub PR review"));
    assert!(text.contains("Clipboard"));
}

#[test]
fn should_export_to_clipboard_when_confirming_cursor_zero_with_sink_a_unavailable() {
    // The other half of the regression above: confirming the menu's
    // cursor-0 entry while sink A is unavailable must produce the same
    // `ExportRequest` the rendered (sink-A-omitted) menu actually shows at
    // that position — `Clipboard`, not a silently-closed menu.
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report).with_review(review);

    let app = app.handle_key(crate::app::InputKey::PopupConfirm);

    let mut review = app.review().clone();
    assert_eq!(
        Some(crate::review::ExportRequest::Clipboard),
        review.take_pending_export()
    );
}

#[test]
fn should_draw_verdict_menu_overlay_entries() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu()
        .confirm_export(true);
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Submit review as"));
    assert!(text.contains("Approve"));
    assert!(text.contains("Request changes"));
    assert!(text.contains("Comment"));
}

#[test]
fn should_show_last_status_message_in_annotations_list_overlay() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .set_status("posted 1 review comment(s) to PR #7")
        .open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("posted 1 review comment(s) to PR #7"));
}
