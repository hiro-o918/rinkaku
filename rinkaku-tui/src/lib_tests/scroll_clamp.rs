//! Post-draw scroll fold-back tests: `clamp_right_pane_scroll_after_draw`
//! and `clamp_help_scroll_after_draw` keep `App`'s scroll state in sync
//! with the frame that was actually drawn. Also covers
//! `is_scroll_input_key`, the classifier that routes the ADR 0026
//! half-page/top/bottom variants through the two-step scroll dispatch.

use super::empty_report;
use crate::app::{App, InputKey};
use crate::{
    clamp_help_scroll_after_draw, clamp_right_pane_scroll_after_draw, is_scroll_input_key,
};

// --- clamp_right_pane_scroll_after_draw ---
//
// Dogfooding fix: `render_scrollable_pane`'s clamp only ever affected
// what was drawn, never `App`'s own `right_pane_scroll` — so an
// overshot scroll request stayed recorded in `App` even once the pane
// visibly stopped moving, and winding it back down took as many `k`
// presses as it took to overshoot in the first place. These tests pin
// the fold-back that keeps `App`'s state in sync with the frame that
// was actually drawn.

#[test]
fn should_overwrite_right_pane_scroll_with_the_clamped_value_when_some() {
    let report = empty_report();
    let app = App::new(&report).with_right_pane_scroll(999);

    let app = clamp_right_pane_scroll_after_draw(app, Some(7));

    assert_eq!(7, app.right_pane_scroll());
}

#[test]
fn should_leave_right_pane_scroll_untouched_when_none() {
    // `None` means the drawn pane had nothing scrollable this frame
    // (`ui::draw`'s own doc comment: the source screen, or a
    // placeholder) — `App`'s own requested scroll must survive
    // unchanged rather than being zeroed or otherwise disturbed by a
    // frame that never consulted it.
    let report = empty_report();
    let app = App::new(&report).with_right_pane_scroll(3);

    let app = clamp_right_pane_scroll_after_draw(app, None);

    assert_eq!(3, app.right_pane_scroll());
}

// --- clamp_help_scroll_after_draw ---
//
// Same fold-back discipline as `clamp_right_pane_scroll_after_draw`
// above, applied to the `?` help overlay's own independent scroll
// state (this feature).

#[test]
fn should_overwrite_help_scroll_with_the_clamped_value_when_some() {
    let report = empty_report();
    let app = App::new(&report).with_help_scroll(999);

    let app = clamp_help_scroll_after_draw(app, Some(4));

    assert_eq!(4, app.help_scroll());
}

#[test]
fn should_leave_help_scroll_untouched_when_none() {
    let report = empty_report();
    let app = App::new(&report).with_help_scroll(2);

    let app = clamp_help_scroll_after_draw(app, None);

    assert_eq!(2, app.help_scroll());
}

// --- is_scroll_input_key ---

#[test]
fn should_treat_the_four_adr_0026_scroll_variants_as_scroll_input_keys() {
    for key in [
        InputKey::ScrollHalfPageDown,
        InputKey::ScrollHalfPageUp,
        InputKey::ScrollToTop,
        InputKey::ScrollToBottom,
    ] {
        assert!(is_scroll_input_key(key), "{key:?} should be a scroll key");
    }
}

#[test]
fn should_not_treat_up_or_down_as_scroll_input_keys() {
    // `Up`/`Down` scroll the help overlay too, but through the ordinary
    // `dispatch_non_source_key` path (`App::handle_key`'s own
    // `help_open` branch), not the two-step `handle_scroll_key`
    // dispatch reserved for the four ADR 0026 variants — this pins
    // that boundary stays where `run_app`'s own dispatch expects it.
    assert!(!is_scroll_input_key(InputKey::Up));
    assert!(!is_scroll_input_key(InputKey::Down));
}
