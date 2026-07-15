//! `resolve_goto` and `dispatch_non_source_key` regression tests
//! (ADR 0022): the 0/1/many candidate resolution for `gd`/`gr`, plus
//! the `run_app`-equivalent dispatch sequence that pins the
//! `pending_prefix` clear + jumplist scroll-restore contracts.

use super::{candidate, report_with_symbols_and_edges};
use crate::app::{self, App, InputKey};
use crate::diff_shape;
use crate::{GotoOutcome, dispatch_non_source_key, resolve_goto};

// resolve_goto tests (ADR 0022): the 0/1/many candidate resolution that
// needs `report`, extracted so it is unit-testable without a live
// terminal (this function's own doc comment).

#[test]
fn should_return_no_symbol_selected_when_cursor_is_not_on_a_symbol_row() {
    let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
    let app = App::new(&report); // cursor on "lib.rs" (a File row)

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(GotoOutcome::NoSymbolSelected, actual);
}

#[test]
fn should_return_no_candidates_when_selected_symbol_has_no_callees() {
    let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(GotoOutcome::NoCandidates("callees"), actual);
}

#[test]
fn should_return_one_candidate_when_selected_symbol_has_exactly_one_callee() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(
        GotoOutcome::One(candidate("lib.rs::bar", "bar", "lib.rs")),
        actual
    );
}

#[test]
fn should_return_many_candidates_when_selected_symbol_has_multiple_callees() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar", "baz"])],
        vec![
            ("lib.rs::foo", "lib.rs::bar"),
            ("lib.rs::foo", "lib.rs::baz"),
        ],
    );
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(
        GotoOutcome::Many(vec![
            candidate("lib.rs::bar", "bar", "lib.rs"),
            candidate("lib.rs::baz", "baz", "lib.rs"),
        ]),
        actual
    );
}

#[test]
fn should_resolve_callers_direction_when_goto_references_is_requested() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    // Cursor on "bar" (row 2): "foo" is its one caller.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    let actual = resolve_goto(&app, &report, InputKey::GotoReferences);

    assert_eq!(
        GotoOutcome::One(candidate("lib.rs::foo", "foo", "lib.rs")),
        actual
    );
}

// dispatch_non_source_key regression tests: the `run_app`-equivalent
// dispatch sequence (as opposed to calling `App::handle_key` directly,
// which every test above this point does and which was exactly why the
// `pending_prefix` bug survived until manual/review testing caught it —
// `App::handle_key`'s own unconditional prefix-clear ran fine in every
// one of those tests, but `run_app`'s old inline `GotoDefinition`/
// `GotoReferences` branch skipped calling it in the first place).

#[test]
fn should_clear_pending_prefix_so_next_d_toggles_diff_after_a_one_candidate_gd_jump() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    // Cursor on "foo" (row 1): "bar" is its one callee, so `gd` jumps
    // immediately rather than opening the popup.
    let app = App::new(&report).handle_key(InputKey::Down);
    let diff_content = diff_shape::DiffPaneContent::Empty;

    // Simulates the real `g` then `d` key sequence: `translate_key`
    // emits `PendingGoto` for `g`, then (because `pending_prefix` is
    // now set) `GotoDefinition` for the following `d` — both routed
    // through `dispatch_non_source_key`, the same function `run_app`
    // itself calls, rather than `App::handle_key` directly.
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    assert_eq!(Some(app::PendingPrefix::G), app.pending_prefix());
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoDefinition,
        app::DiffViewMode::Split,
    );
    assert_eq!(None, app.pending_prefix(), "gd must clear pending_prefix");

    // The regression itself: a *plain* `d` right after the jump must
    // toggle the right pane (`ToggleDiff`'s own ordinary meaning), not
    // silently re-resolve as another `gd` because `pending_prefix` was
    // still `Some(G)` — `crate::lib::translate_key` only produces
    // `GotoDefinition` for a `d` when `pending_prefix() == Some(G)`, so
    // this assertion on `right_pane()` is an indirect but faithful proxy
    // for "the next `d` meant ToggleDiff, not gd".
    let right_pane_before = app.right_pane();
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::ToggleDiff,
        app::DiffViewMode::Split,
    );
    assert_ne!(
        right_pane_before,
        app.right_pane(),
        "d after gd must toggle the right pane like an ordinary ToggleDiff press"
    );
}

#[test]
fn should_clear_pending_prefix_so_next_d_toggles_diff_after_a_multi_candidate_gr_popup_is_cancelled()
 {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar", "baz"])],
        vec![
            ("lib.rs::foo", "lib.rs::bar"),
            ("lib.rs::baz", "lib.rs::bar"),
        ],
    );
    // Cursor on "bar" (row 2): both "foo" and "baz" call it, so `gr`
    // opens the popup rather than jumping immediately.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    let diff_content = diff_shape::DiffPaneContent::Empty;

    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoReferences,
        app::DiffViewMode::Split,
    );
    assert!(
        app.jump_popup().is_some(),
        "gr with 2 candidates must open the popup"
    );
    assert_eq!(
        None,
        app.pending_prefix(),
        "gr must clear pending_prefix even though it opened a popup instead of jumping"
    );

    // Cancel the popup (Esc) — `App::handle_key`'s own popup-open early
    // return is the second path the #61-review finding flagged: it used
    // to return before the (then-later-positioned) `pending_prefix`
    // clear, so a stale prefix could survive an entire popup session.
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PopupCancel,
        app::DiffViewMode::Split,
    );
    assert_eq!(None, app.jump_popup());
    assert_eq!(None, app.pending_prefix());

    // Same regression check as the single-candidate test above: a plain
    // `d` after the cancelled popup must toggle the right pane, not
    // silently re-resolve as another `gr`.
    let right_pane_before = app.right_pane();
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::ToggleDiff,
        app::DiffViewMode::Split,
    );
    assert_ne!(
        right_pane_before,
        app.right_pane(),
        "d after a cancelled gr popup must toggle the right pane like an ordinary ToggleDiff press"
    );
}

#[test]
fn should_restore_the_scroll_offset_the_reviewer_was_at_when_jumping_back_after_gd() {
    // Independent-review finding: `dispatch_non_source_key` always calls
    // `app.handle_key(GotoDefinition)` first (for the `pending_prefix`
    // clear — see the test group above), and before this fix that call
    // hit `App::handle_key`'s own blanket scroll reset before
    // `App::jump_to_symbol` ever read `right_pane_scroll` to save it into
    // the jumplist entry — so every jumplist entry's saved scroll was
    // always 0 and `Ctrl-o` could never restore a real reading position.
    //
    // This test drives the *real* two-key `g` then `d` sequence
    // (`InputKey::PendingGoto` then `InputKey::GotoDefinition`), each
    // through `dispatch_non_source_key` — not `App::handle_key` directly,
    // and not `GotoDefinition` alone. An earlier version of this test did
    // call `GotoDefinition` alone and passed while the underlying bug was
    // still only half-fixed: `PendingGoto` (the leading `g`) is also
    // dispatched through `handle_key` on its own, one keypress *before*
    // `GotoDefinition`, and its own blanket scroll reset zeroed
    // `right_pane_scroll` before `d` was even pressed — a gap only a
    // real terminal run surfaced (see `InputKey::PendingGoto`'s own doc
    // comment). Scrolls to a nonzero offset, jumps via the real `gd` key
    // sequence, then jumps back via `Ctrl-o` (`InputKey::JumpBack`) and
    // asserts the original scroll offset is restored rather than 0.
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    let diff_content = diff_shape::DiffPaneContent::Empty;

    // Cursor on "foo" (row 1), scrolled 5 lines into its Diff pane.
    let mut app = App::new(&report).handle_key(InputKey::Down);
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::Open,
        app::DiffViewMode::Split,
    ); // focus -> Right, RightPane::Diff
    for _ in 0..5 {
        app = dispatch_non_source_key(
            app,
            &report,
            &diff_content,
            InputKey::Down,
            app::DiffViewMode::Split,
        );
    }
    assert_eq!(5, app.right_pane_scroll());

    // The real `gd` key sequence: `g` (PendingGoto) then `d`
    // (GotoDefinition) — "bar" is "foo"'s one callee, so this jumps
    // immediately (`GotoOutcome::One`) rather than opening the popup.
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    assert_eq!(
        5,
        app.right_pane_scroll(),
        "the leading g of gd must not disturb scroll either"
    );
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoDefinition,
        app::DiffViewMode::Split,
    );
    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
    assert_eq!(
        0,
        app.right_pane_scroll(),
        "the new target's own scroll must start at 0 (App::jump_to_symbol's own reset)"
    );

    // Ctrl-o: jump back to "foo" — the regression this test guards.
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::JumpBack,
        app::DiffViewMode::Split,
    );

    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
    assert_eq!(
        5,
        app.right_pane_scroll(),
        "jumping back must restore the scroll offset recorded when gd was pressed, not 0"
    );
}
