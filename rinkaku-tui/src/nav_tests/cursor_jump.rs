use super::*;
use pretty_assertions::assert_eq;

// `deep_tree()`'s expanded row order: src(0), src/pkg(1), src/pkg/lib.rs(2),
// foo(3), bar(4) — 5 rows total, used throughout so `CursorPageDown`/
// `CursorPageUp`'s clamping has more than one row of headroom to exercise.

#[test]
fn should_move_cursor_to_last_row_when_cursor_bottom() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorBottom, &tree);

    assert_eq!(4, nav.cursor());
}

#[test]
fn should_move_cursor_to_first_row_when_cursor_top() {
    let tree = deep_tree();
    let nav = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorTop, &tree);

    assert_eq!(0, nav.cursor());
}

#[test]
fn should_keep_cursor_at_first_row_when_cursor_top_and_already_at_top() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorTop, &tree);

    assert_eq!(0, nav.cursor());
}

#[test]
fn should_move_cursor_down_by_step_when_cursor_page_down() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorPageDown(2), &tree);

    assert_eq!(2, nav.cursor());
}

#[test]
fn should_clamp_cursor_to_last_row_when_cursor_page_down_overshoots() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorPageDown(100), &tree);

    assert_eq!(4, nav.cursor());
}

#[test]
fn should_move_cursor_up_by_step_when_cursor_page_up() {
    let tree = deep_tree();
    let nav = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorPageUp(2), &tree);

    assert_eq!(2, nav.cursor());
}

#[test]
fn should_clamp_cursor_to_first_row_when_cursor_page_up_undershoots() {
    let tree = deep_tree();
    let nav = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorPageUp(100), &tree);

    assert_eq!(0, nav.cursor());
}

#[test]
fn should_move_cursor_to_the_given_index_when_cursor_to() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorTo(3), &tree);

    assert_eq!(3, nav.cursor());
}

#[test]
fn should_clamp_cursor_to_last_row_when_cursor_to_index_is_out_of_bounds() {
    let tree = deep_tree();
    let nav = Nav::new().handle(Action::CursorTo(100), &tree);

    assert_eq!(4, nav.cursor());
}
