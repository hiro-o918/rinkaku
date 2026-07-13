use super::{empty_report, report_with_one_symbol, symbol};
use crate::app::{App, Focus, InputKey, PendingPrefix, RightPane, Screen};
use pretty_assertions::assert_eq;
use rinkaku_core::extract::SymbolKind;
use rinkaku_core::render::{FileReport, Report};

#[test]
fn should_start_with_zero_right_pane_scroll() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_start_with_tree_focus() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_move_focus_to_right_when_open_is_pressed_on_a_file_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file row itself (cursor starts there).
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_move_focus_to_right_and_switch_to_diff_pane_when_open_is_pressed_on_a_symbol_row() {
    // Dogfooding fix: a symbol row's Enter used to open `Screen::Source`
    // directly (reading the file from the working tree, which could
    // fail), asymmetric with a file row's Enter, which only switched
    // panes. Both row kinds now behave identically — see `InputKey::Open`'s
    // own doc comment.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_move_focus_to_right_and_switch_to_diff_pane_when_open_is_pressed_on_a_removed_symbol_row()
{
    // A removed symbol has no source to open, but Enter no longer opens
    // source at all (`InputKey::Open`'s own doc comment) — it is a pure
    // pane switch regardless of row kind, so a removed symbol's Enter no
    // longer needs a special no-op case the way `InputKey::Source`'s own
    // `!symbol_ref.removed` guard still does.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        removed: vec![rinkaku_core::extract::RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(Screen::Entry, *app.screen());
    assert_eq!(RightPane::Diff, app.right_pane());
}

#[test]
fn should_leave_scroll_unchanged_when_open_is_pressed_while_right_focused_on_diff() {
    // Map-assisted-review finding: Enter pressed a second time while
    // already reading the Diff pane (Focus::Right, RightPane::Diff) must
    // be a complete no-op — before this fix, the `(Screen::Entry, _,
    // InputKey::Open)` arm matched regardless of focus and the blanket
    // "reset scroll to 0" rule at the end of `handle_key` then threw
    // away the reviewer's reading position for no visible reason.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::Down) // scroll -> 1
        .handle_key(InputKey::Down); // scroll -> 2
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_switch_to_diff_pane_when_open_is_pressed_while_right_focused_on_detail() {
    // The other half of the map-assisted-review finding just above:
    // while Focus::Right but the pane showing is Detail (not Diff yet),
    // Enter is a *real* pane switch, so the ordinary scroll-reset-on-
    // other-keys rule is correct here, not a bug to preserve around.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::ToggleDiff); // RightPane::Detail, focus stays Right
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Detail, app.right_pane());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_leave_scroll_unchanged_when_pending_goto_is_pressed() {
    // Independent-review finding (`InputKey::PendingGoto`'s own doc
    // comment): `g`, the leading key of the two-key `gd`/`gr` sequence,
    // is dispatched through `handle_key` on its own, one keypress before
    // `GotoDefinition`/`GotoReferences` — its own blanket scroll reset
    // must not fire, or the jumplist entry recorded once `d`/`r`
    // eventually runs would already have lost the reviewer's scroll
    // position before the jump even started.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Open) // focus -> Right, RightPane::Diff
        .handle_key(InputKey::Down) // scroll -> 1
        .handle_key(InputKey::Down); // scroll -> 2
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::PendingGoto);

    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_expand_collapse_and_keep_tree_focus_when_open_is_pressed_on_a_directory_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo")],
        }],
        ..empty_report()
    };
    // Row 0 is the "src" directory itself.
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Tree, app.focus());
    let rows = app.nav().rows(app.tree());
    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src"], paths, "directory should have collapsed");
}

#[test]
fn should_not_move_focus_when_select_is_pressed_on_a_file_row() {
    // Space (`InputKey::Select`) must never move focus, even on a
    // file/symbol row — only Enter (`InputKey::Open`) does (ADR 0020).
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Select);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_not_toggle_expand_when_select_is_pressed_while_right_focused() {
    // Finding-5 regression: Space used to fire regardless of focus, so
    // pressing it while Focus::Right silently toggled the expand state
    // of whichever file/symbol row the tree cursor was parked on (the
    // one currently being previewed in the right pane) — a change with
    // no visible effect until the user returned to Focus::Tree. Gated
    // to match InputKey::Open's own Tree-only reach for the same
    // "act on the row under the tree cursor" family of keys.
    let report = report_with_one_symbol();
    let app = App::new(&report); // cursor on the "lib.rs" file row
    let rows_before: Vec<String> = app
        .nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.clone())
        .collect();
    let app = app.handle_key(InputKey::Open); // focus -> Right
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_key(InputKey::Select);

    assert_eq!(Focus::Right, app.focus());
    let rows_after: Vec<String> = app
        .nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.clone())
        .collect();
    assert_eq!(
        rows_before, rows_after,
        "Select while Right-focused must not change which rows are visible"
    );
}

#[test]
fn should_move_cursor_while_tree_focused_when_down_is_pressed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.nav().cursor());
}

#[test]
fn should_scroll_right_pane_instead_of_moving_cursor_when_down_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open); // focus -> Right
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down).handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_decrement_right_pane_scroll_when_up_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open) // focus -> Right
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(1, app.right_pane_scroll());
}

#[test]
fn should_not_scroll_up_past_zero() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_return_focus_to_tree_when_focus_left_is_pressed_while_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(Focus::Tree, app.focus());
}

#[test]
fn should_do_nothing_when_focus_left_is_pressed_while_already_tree_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(Focus::Tree, app.focus());
}
