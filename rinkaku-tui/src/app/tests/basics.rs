use super::{empty_report, report_with_one_symbol, symbol};
use crate::app::{App, InputKey, Screen, SelectedDetail};
use crate::detail::{DirDetail, FileDetail, FileSymbolSummary};
use crate::order::OrderMode;
use pretty_assertions::assert_eq;
use rinkaku_core::extract::SymbolKind;
use rinkaku_core::render::{FileReport, Report};

#[test]
fn should_start_on_entry_screen_with_topological_order_and_no_status() {
    let report = report_with_one_symbol();

    let app = App::new(&report);

    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(OrderMode::Topological, app.order_mode());
    assert_eq!(None, app.status());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_set_should_quit_when_quit_is_pressed_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Quit);

    assert_eq!(true, app.should_quit());
}

#[test]
fn should_move_cursor_down_when_down_is_pressed() {
    // lib.rs has one file row and one symbol row; Down should move off
    // the initial cursor position (0) onto the symbol row (1).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.nav().cursor());
}

#[test]
fn should_toggle_order_mode_between_topological_and_alpha_numeric() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(OrderMode::Topological, app.order_mode());

    let app = app.handle_key(InputKey::ToggleOrder);
    assert_eq!(OrderMode::AlphaNumeric, app.order_mode());

    let app = app.handle_key(InputKey::ToggleOrder);
    assert_eq!(OrderMode::Topological, app.order_mode());
}

#[test]
fn should_clear_status_message_on_the_next_handled_key() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.set_status("a source read failed");
    assert_eq!(Some("a source read failed"), app.status());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(None, app.status());
}

#[test]
fn should_return_file_detail_when_cursor_is_on_a_file_row() {
    // Row 0 is the "lib.rs" file itself, not a symbol (TUI iteration
    // 2: a file row now gets its own detail instead of `None`).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let actual = app.selected_detail(&report);

    let expected = SelectedDetail::File(FileDetail {
        path: "lib.rs".to_string(),
        symbols: vec![FileSymbolSummary {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: false,
            fan_in: 0,
        }],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: None,
    });
    assert_eq!(Some(expected), actual);
}

#[test]
fn should_return_detail_view_when_cursor_is_on_a_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let actual = app.selected_detail(&report);

    match actual.expect("detail for selected symbol") {
        SelectedDetail::Symbol(detail) => assert_eq!("foo", detail.name),
        other => panic!("expected SelectedDetail::Symbol, got {other:?}"),
    }
}

#[test]
fn should_return_dir_detail_when_cursor_is_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    let app = App::new(&report);

    let actual = app.selected_detail(&report);

    let expected = SelectedDetail::Dir(DirDetail {
        path: "src".to_string(),
        badges: crate::tree::Badges {
            changed_symbols: 1,
            contract_changes: 0,
            fan_in: 0,
            ..crate::tree::Badges::default()
        },
        top_fan_in: vec![],
        cycle_partners: vec![],
        cycle_edges: vec![],
    });
    assert_eq!(Some(expected), actual);
}
