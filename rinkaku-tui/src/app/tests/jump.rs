use super::{
    empty_report, focused_right_and_scrolled_one_line, report_with_one_symbol,
    report_with_two_directories,
};
use crate::app::{App, Focus, InputKey, JumpCandidate, JumpPopup, PendingPrefix};
use pretty_assertions::assert_eq;
use rinkaku_core::extract::SymbolKind;
use rinkaku_core::render::Report;

// Jump navigation tests (ADR 0022): `selected_symbol_id`,
// `jump_to_symbol`, `open_jump_popup`, the popup's own key handling,
// `pending_prefix` bookkeeping, and the jumplist (`JumpBack`/
// `JumpForward`).

/// Two symbols in two files, "a::foo" calling "b::bar" — enough for a
/// jump to have a real target to land on and expand a collapsed
/// ancestor. Expanded row order: a(0), a/one.rs(1), foo(2), b(3),
/// b/two.rs(4), bar(5) — same shape as `report_with_two_directories`,
/// with a populated `graph` so `symbol_mentions`-driven callers can
/// exercise it directly (this module's own tests call `Nav`/`App`
/// methods directly rather than through `crate::lib::resolve_goto`,
/// which lives in a different module and is tested there instead).
fn report_with_caller_and_callee() -> Report {
    let mut report = report_with_two_directories();
    report.files[0].symbols[0].id = "a/one.rs::foo".to_string();
    report.files[1].symbols[0].id = "b/two.rs::bar".to_string();
    report.graph = rinkaku_core::graph::SymbolGraph {
        nodes: vec![
            rinkaku_core::graph::Node {
                id: "a/one.rs::foo".to_string(),
                path: "a/one.rs".to_string(),
                name: "foo".to_string(),
                is_test: false,
            },
            rinkaku_core::graph::Node {
                id: "b/two.rs::bar".to_string(),
                path: "b/two.rs".to_string(),
                name: "bar".to_string(),
                is_test: false,
            },
        ],
        edges: vec![rinkaku_core::graph::Edge {
            from: "a/one.rs::foo".to_string(),
            to: "b/two.rs::bar".to_string(),
            is_cycle: false,
        }],
        roots: vec!["a/one.rs::foo".to_string()],
    };
    report
}

#[test]
fn should_return_selected_symbol_id_when_cursor_is_on_a_present_symbol_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Down);

    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
}

#[test]
fn should_return_none_selected_symbol_id_when_cursor_is_on_a_file_row() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    assert_eq!(None, app.selected_symbol_id());
}

#[test]
fn should_return_none_selected_symbol_id_when_cursor_is_on_a_removed_symbol() {
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

    assert_eq!(None, app.selected_symbol_id());
}

#[test]
fn should_move_cursor_to_target_symbol_when_jump_to_symbol_is_called() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report);

    let app = app.jump_to_symbol("b/two.rs::bar");

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_reset_scroll_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    let app = focused_right_and_scrolled_one_line(App::new(&report)); // cursor on "a/one.rs", scroll -> 1
    assert_eq!(1, app.right_pane_scroll());

    let app = app.jump_to_symbol("b/two.rs::bar");

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_keep_focus_unchanged_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open); // focus -> Right, cursor on "foo"
    assert_eq!(Focus::Right, app.focus());

    let app = app.jump_to_symbol("b/two.rs::bar");

    assert_eq!(Focus::Right, app.focus());
}

#[test]
fn should_push_current_symbol_onto_jumplist_when_jump_to_symbol_succeeds() {
    let report = report_with_caller_and_callee();
    // Cursor on "foo" (row 2).
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(
        "a/one.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.jump_to_symbol("b/two.rs::bar");
    let app = app.handle_key(InputKey::JumpBack);

    // Ctrl-o after the jump must land back on "foo" — proof the
    // pre-jump location was actually pushed.
    let rows = app.nav().rows(app.tree());
    assert_eq!("a/one.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_expand_collapsed_ancestor_when_jump_to_symbol_targets_a_hidden_row() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).handle_key(InputKey::CollapseAll);
    assert_eq!(vec!["a", "b"], row_paths_of(&app));

    let app = app.jump_to_symbol("b/two.rs::bar");

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_set_status_and_leave_cursor_unmoved_when_jump_to_symbol_target_does_not_exist() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).handle_key(InputKey::Down);
    let cursor_before = app.nav().cursor();

    let app = app.jump_to_symbol("no/such::id");

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(
        Some("note: symbol no/such::id is no longer present"),
        app.status()
    );
}

fn row_paths_of(app: &App) -> Vec<&str> {
    app.nav()
        .rows(app.tree())
        .iter()
        .map(|r| r.node.path.as_str())
        .collect()
}

#[test]
fn should_open_jump_popup_with_given_candidates() {
    let report = empty_report();
    let app = App::new(&report);
    let candidates = vec![
        JumpCandidate {
            id: "a".to_string(),
            name: "a".to_string(),
            path: "a.rs".to_string(),
        },
        JumpCandidate {
            id: "b".to_string(),
            name: "b".to_string(),
            path: "b.rs".to_string(),
        },
    ];

    let app = app.open_jump_popup(candidates.clone());

    assert_eq!(
        Some(&JumpPopup {
            candidates,
            cursor: 0
        }),
        app.jump_popup()
    );
}

fn two_candidates() -> Vec<JumpCandidate> {
    vec![
        JumpCandidate {
            id: "a/one.rs::foo".to_string(),
            name: "foo".to_string(),
            path: "a/one.rs".to_string(),
        },
        JumpCandidate {
            id: "b/two.rs::bar".to_string(),
            name: "bar".to_string(),
            path: "b/two.rs".to_string(),
        },
    ]
}

#[test]
fn should_move_popup_cursor_down_when_down_is_pressed_while_popup_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_clamp_popup_cursor_at_last_candidate_when_down_is_pressed_past_the_end() {
    let report = empty_report();
    let app = App::new(&report)
        .open_jump_popup(two_candidates())
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    assert_eq!(1, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_clamp_popup_cursor_at_zero_when_up_is_pressed_past_the_top() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(0, app.jump_popup().expect("popup open").cursor);
}

#[test]
fn should_close_popup_without_jumping_when_popup_cancel_is_pressed() {
    let report = report_with_caller_and_callee();
    let cursor_before = App::new(&report).nav().cursor();
    let app = App::new(&report).open_jump_popup(two_candidates());

    let app = app.handle_key(InputKey::PopupCancel);

    assert_eq!(None, app.jump_popup());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_jump_to_highlighted_candidate_and_close_popup_when_popup_confirm_is_pressed() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .open_jump_popup(two_candidates())
        .handle_key(InputKey::Down); // highlight "bar"

    let app = app.handle_key(InputKey::PopupConfirm);

    assert_eq!(None, app.jump_popup());
    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_ignore_navigation_keys_while_jump_popup_is_open() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report).open_jump_popup(two_candidates());
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::ExpandAll);

    assert_eq!(cursor_before, app.nav().cursor());
    assert!(app.jump_popup().is_some());
}

#[test]
fn should_set_pending_prefix_when_pending_goto_is_pressed() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(None, app.pending_prefix());

    let app = app.handle_key(InputKey::PendingGoto);

    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());
}

#[test]
fn should_clear_pending_prefix_when_any_other_key_follows_pending_goto() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);
    assert_eq!(Some(PendingPrefix::G), app.pending_prefix());

    let app = app.handle_key(InputKey::Down);

    assert_eq!(None, app.pending_prefix());
}

#[test]
fn should_set_status_when_jump_back_is_pressed_with_an_empty_back_stack() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::JumpBack);

    assert_eq!(Some("note: jumplist has no earlier location"), app.status());
}

#[test]
fn should_set_status_when_jump_forward_is_pressed_with_an_empty_forward_stack() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::JumpForward);

    assert_eq!(Some("note: jumplist has no later location"), app.status());
}

#[test]
fn should_return_to_pre_jump_symbol_when_jump_back_is_pressed_after_a_jump() {
    let report = report_with_caller_and_callee();
    // Cursor on "foo" (row 2) before jumping.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar");
    assert_eq!(
        "b/two.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.handle_key(InputKey::JumpBack);

    let rows = app.nav().rows(app.tree());
    assert_eq!("a/one.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_return_to_post_jump_symbol_when_jump_forward_is_pressed_after_jump_back() {
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar")
        .handle_key(InputKey::JumpBack);
    assert_eq!(
        "a/one.rs",
        app.nav().rows(app.tree())[app.nav().cursor()].node.path
    );

    let app = app.handle_key(InputKey::JumpForward);

    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
}

#[test]
fn should_clear_forward_stack_when_a_new_jump_is_made_from_the_middle_of_history() {
    // vim's own jumplist semantics: jumping to a new location abandons
    // whatever forward history existed, rather than preserving it.
    let report = report_with_caller_and_callee();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .jump_to_symbol("b/two.rs::bar")
        .handle_key(InputKey::JumpBack); // back on "foo", forward-stack has "bar"

    let app = app.jump_to_symbol("b/two.rs::bar"); // a fresh jump from "foo"

    let app = app.handle_key(InputKey::JumpForward);
    assert_eq!(Some("note: jumplist has no later location"), app.status());
}
