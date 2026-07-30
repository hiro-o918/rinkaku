use super::report_with_one_symbol;
use crate::app::{App, Focus, InputKey, Screen};
use pretty_assertions::assert_eq;
use rstest::rstest;

#[test]
fn should_open_source_screen_when_source_key_is_pressed_on_a_symbol_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file, row 1 is the "foo" symbol.
    let app = App::new(&report).handle_key(InputKey::Down);

    let app = app.handle_key(InputKey::Source);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_stay_on_entry_screen_when_source_key_is_pressed_on_a_file_row() {
    let report = report_with_one_symbol();
    // Row 0 is the "lib.rs" file row itself, not a symbol.
    let app = App::new(&report);

    let app = app.handle_key(InputKey::Source);

    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_return_to_entry_screen_when_back_is_pressed_on_source_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );

    let app = app.handle_key(InputKey::Back);

    assert_eq!(Screen::Entry, *app.screen());
}

#[test]
fn should_scroll_source_screen_and_not_move_tree_cursor_when_down_is_pressed() {
    // ADR 0026: `j`/`k` on the source screen scroll
    // `Screen::Source::scroll_top` rather than moving the tree cursor
    // (which the reviewer can't see move behind the source screen
    // anyway — the point of ADR 0026's Context, and the whole reason
    // Source used to be Back-only). Pins both halves of the new
    // contract: the tree cursor stays put *and* the scroll offset
    // moves.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down);

    assert_eq!(cursor_before, app.nav().cursor());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 1,
        },
        *app.screen()
    );
}

// ADR 0026: scroll bindings — `App::handle_key`'s Source-screen `Up`/
// `Down` arms plus `App::handle_scroll_key`'s four scroll variants,
// exercised against both `Screen::Source` and `Screen::Entry` +
// `Focus::Right`.

#[test]
fn should_scroll_source_up_from_a_nonzero_position_when_up_is_pressed() {
    // Complements `should_scroll_source_screen_and_not_move_tree_cursor_when_down_is_pressed`
    // above (which covers the Down direction from 0). Starts at
    // scroll_top = 3 (via `with_source_scroll_top`, mirroring how
    // `crate::run_app` back-fills the centered initial position) so
    // the `Up` press has somewhere to move from, and pins the
    // `saturating_sub(1)` step size.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(3);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 2,
        },
        *app.screen()
    );
}

#[test]
fn should_saturate_source_scroll_up_at_zero_when_already_at_the_top() {
    // A `k` at scroll 0 must stay at 0 rather than underflowing to
    // `usize::MAX` — the same `saturating_sub` discipline the entry
    // view's own Up arm already uses for `right_pane_scroll`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_key(InputKey::Up);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_add_half_viewport_to_source_scroll_top_when_scroll_half_page_down_is_dispatched() {
    // `handle_scroll_key` needs the viewport height, so this is
    // exercised via `handle_scroll_key` directly (not `handle_key`)
    // — the same two-step dispatch `crate::run_app` uses. Viewport
    // height 20 → step size 10.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 10,
        },
        *app.screen()
    );
}

#[test]
fn should_saturate_source_scroll_half_page_up_at_zero() {
    // Half-page up from a low scroll must not underflow — mirrors
    // the Up arm's `saturating_sub` (`saturating_sub` inside
    // `handle_scroll_key` for this one).
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(3);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageUp, 20);

    // 3 - 10 saturates to 0.
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_reset_source_scroll_top_to_zero_when_scroll_to_top_is_dispatched() {
    // `gg` on the source screen. Viewport height is irrelevant for
    // this variant (it does not read it) but still required by the
    // shared `handle_scroll_key` signature.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_source_scroll_top(50);

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
}

#[test]
fn should_set_source_scroll_top_to_usize_max_sentinel_when_scroll_to_bottom_is_dispatched() {
    // `G` on the source screen. `App` has no notion of the file's
    // line count — the actual "bottom" is resolved by `ui`'s
    // `clamped_window` at draw time — so `handle_scroll_key` records
    // `usize::MAX` here and lets the draw-time clamp fold it down.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 20);

    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: usize::MAX,
        },
        *app.screen()
    );
}

#[test]
fn should_add_half_viewport_to_right_pane_scroll_when_focus_right_scroll_half_page_down() {
    // Entry-view side of ADR 0026: the same four scroll variants
    // act on `right_pane_scroll` while `Focus::Right`. Reach
    // `Focus::Right` via `Open` on the file row (ADR 0020) so the
    // scroll actually applies to a real pane.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 30);

    assert_eq!(15, app.right_pane_scroll());
}

#[test]
fn should_leave_right_pane_scroll_untouched_when_focus_tree_and_scroll_half_page_down() {
    // Tree focus is the entry view's default; `handle_scroll_key` moves
    // the tree cursor there (ADR 0026 amendment, see the
    // `should_move_tree_cursor_*` tests below), not `right_pane_scroll`
    // — that field must stay untouched since no pane is being scrolled.
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 30);

    assert_eq!(0, app.right_pane_scroll());
}

// ADR 0026 amendment: `Focus::Tree` on the entry view has no pane of its
// own to scroll (`crate::ui::entry::draw_tree_pane` windows around the
// cursor at draw time instead), so these four variants move the tree
// cursor — `Nav::Action::CursorPageDown`/`CursorPageUp`/`CursorTop`/
// `CursorBottom` — rather than any scroll offset. Uses
// `report_with_two_directories` (6 rows: a(0), a/one.rs(1), foo(2), b(3),
// b/two.rs(4), bar(5)) so half-page motion has more than one row of
// headroom to exercise.

#[test]
fn should_move_tree_cursor_down_by_half_viewport_when_focus_tree_scroll_half_page_down() {
    let report = super::report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());
    assert_eq!(0, app.nav().cursor());

    // Viewport height 4 -> step size 2.
    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 4);

    assert_eq!(2, app.nav().cursor());
}

#[test]
fn should_clamp_tree_cursor_to_last_row_when_focus_tree_scroll_half_page_down_overshoots() {
    let report = super::report_with_two_directories();
    let app = App::new(&report);

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, 100);

    assert_eq!(5, app.nav().cursor());
}

#[rstest]
#[case::zero_row_viewport(0)]
#[case::one_row_viewport(1)]
fn should_move_tree_cursor_by_one_row_when_viewport_is_too_short_for_a_half_page(
    #[case] viewport_height: usize,
) {
    let report = super::report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(0, app.nav().cursor());

    let app = app.handle_scroll_key(InputKey::ScrollHalfPageDown, viewport_height);

    assert_eq!(1, app.nav().cursor());
}

#[test]
fn should_move_tree_cursor_up_by_half_viewport_when_focus_tree_scroll_half_page_up() {
    let report = super::report_with_two_directories();
    let app = App::new(&report).handle_scroll_key(InputKey::ScrollToBottom, 4);
    assert_eq!(5, app.nav().cursor());

    // Viewport height 4 -> step size 2.
    let app = app.handle_scroll_key(InputKey::ScrollHalfPageUp, 4);

    assert_eq!(3, app.nav().cursor());
}

#[test]
fn should_move_tree_cursor_to_first_row_when_focus_tree_scroll_to_top() {
    let report = super::report_with_two_directories();
    let app = App::new(&report).handle_scroll_key(InputKey::ScrollToBottom, 4);
    assert_eq!(5, app.nav().cursor());

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 4);

    assert_eq!(0, app.nav().cursor());
}

#[test]
fn should_move_tree_cursor_to_last_row_when_focus_tree_scroll_to_bottom() {
    let report = super::report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(0, app.nav().cursor());

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 4);

    assert_eq!(5, app.nav().cursor());
}

#[test]
fn should_reset_right_pane_scroll_to_zero_on_focus_right_scroll_to_top() {
    // `gg` on the entry view + `Focus::Right`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .with_right_pane_scroll(42);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollToTop, 30);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_set_right_pane_scroll_to_usize_max_sentinel_on_focus_right_scroll_to_bottom() {
    // `G` on the entry view + `Focus::Right`. Same
    // `usize::MAX`-sentinel-plus-draw-time-clamp discipline the
    // source screen uses.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_scroll_key(InputKey::ScrollToBottom, 30);

    assert_eq!(usize::MAX, app.right_pane_scroll());
}
