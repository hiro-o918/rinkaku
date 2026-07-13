use super::{empty_report, report_with_one_symbol};
use crate::app::{App, InputKey};
use pretty_assertions::assert_eq;

#[test]
fn should_start_with_help_overlay_closed() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(false, app.help_open());
}

#[test]
fn should_open_help_overlay_when_toggle_help_is_pressed() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(true, app.help_open());
}

#[test]
fn should_close_help_overlay_when_toggle_help_is_pressed_again() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert_eq!(true, app.help_open());

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(false, app.help_open());
}

#[test]
fn should_ignore_quit_while_help_overlay_is_open() {
    // ADR 0020: the overlay must be a safe, low-stakes action that
    // cannot be short-circuited by an accidental app exit — `Quit`
    // reaching `handle_key` while the overlay is open (e.g. via a
    // translate_key bug) must still be swallowed defensively, not just
    // rely on `translate_key` never producing it in the first place.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert_eq!(true, app.help_open());

    let app = app.handle_key(InputKey::Quit);

    assert_eq!(true, app.help_open());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_scroll_help_overlay_instead_of_moving_tree_cursor_when_down_is_pressed_while_open() {
    // `Down` used to be ignored outright while the overlay was open; it now
    // scrolls the overlay's own content instead (this feature) — the tree
    // cursor underneath must still stay untouched either way, since the
    // overlay composites on top of it rather than replacing it.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(true, app.help_open());
    assert_eq!(1, app.help_scroll());
}

#[test]
fn should_leave_other_state_untouched_when_help_overlay_opens() {
    // Opening the overlay must not disturb whatever was already showing
    // underneath it (`Self::help_open`'s own doc comment: "nothing else
    // about `App`'s state changes while the overlay is open").
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleDiff);
    let right_pane_before = app.right_pane();
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(right_pane_before, app.right_pane());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_start_with_zero_help_scroll() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_down_by_one_line_when_down_is_pressed_while_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let app = app.handle_key(InputKey::Down);

    assert_eq!(1, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_up_by_one_line_when_up_is_pressed_while_open() {
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleHelp)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.help_scroll());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(1, app.help_scroll());
}

#[test]
fn should_not_underflow_help_scroll_when_up_is_pressed_at_the_top() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert_eq!(0, app.help_scroll());

    let app = app.handle_key(InputKey::Up);

    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_reset_help_scroll_when_overlay_closes() {
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleHelp)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.help_scroll());

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_reset_help_scroll_when_overlay_reopens_after_a_previous_scroll() {
    // Re-opening after a previous scrolled session must start from the top
    // again (this struct's own `help_scroll` doc comment: "a reviewer has
    // no way to see coming back" otherwise), not resume the stale offset.
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleHelp)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleHelp);
    assert_eq!(0, app.help_scroll());

    let app = app.handle_key(InputKey::ToggleHelp);

    assert_eq!(true, app.help_open());
    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_half_page_down_via_handle_scroll_key_when_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 20);

    assert_eq!(10, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_half_page_up_via_handle_scroll_key_when_open() {
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleHelp)
        .handle_scroll_key(InputKey::ScrollHalfPageDown, 20);
    assert_eq!(10, app.help_scroll());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageUp, 20);

    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_to_top_via_handle_scroll_key_when_open() {
    let report = empty_report();
    let app = App::new(&report)
        .handle_key(InputKey::ToggleHelp)
        .handle_scroll_key(InputKey::ScrollHalfPageDown, 20);
    assert_eq!(10, app.help_scroll());

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 20);

    assert_eq!(0, app.help_scroll());
}

#[test]
fn should_scroll_help_overlay_to_bottom_sentinel_via_handle_scroll_key_when_open() {
    // `usize::MAX` is the same "scroll to bottom" sentinel ADR 0026 already
    // uses for `right_pane_scroll`/`Screen::Source::scroll_top` — folded
    // down to the real max at draw time by `render_scrollable_pane`'s own
    // clamp, so no per-pane bottom sentinel is needed here either.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 20);

    assert_eq!(usize::MAX, app.help_scroll());
}

#[test]
fn should_not_scroll_right_pane_via_handle_scroll_key_when_help_overlay_is_open() {
    // Regression guard for the latent bug this feature's own implementation
    // found: without the `help_open` check at the top of `handle_scroll_key`,
    // `crate::run_app`'s unconditional two-step dispatch (`handle_key` then
    // `handle_scroll_key`) would fall through to the ordinary
    // `Screen::Entry` + `Focus::Right` branch and scroll the right pane
    // *behind* the overlay while it looked closed to the reviewer.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::ToggleHelp);
    let right_pane_scroll_before = app.right_pane_scroll();

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 20);

    assert_eq!(right_pane_scroll_before, app.right_pane_scroll());
    assert_eq!(10, app.help_scroll());
}

#[test]
fn should_leave_help_scroll_untouched_when_with_help_scroll_is_called_on_a_closed_overlay() {
    // `App::with_help_scroll` is a plain setter with no `help_open` guard
    // (mirroring `with_right_pane_scroll`'s own unconditional write) — this
    // pins that `crate::run_app`'s post-draw fold-back is itself gated by
    // `DrawOutcome::clamped_help_scroll` being `None` while closed
    // (`ui::draw`'s own doc comment), not by this setter refusing the call.
    let report = empty_report();
    let app = App::new(&report);

    let app = app.with_help_scroll(5);

    assert_eq!(5, app.help_scroll());
}
