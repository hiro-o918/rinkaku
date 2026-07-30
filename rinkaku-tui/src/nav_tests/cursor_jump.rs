use super::*;
use pretty_assertions::assert_eq;

// `deep_tree()`'s expanded row order: src(0), src/pkg(1), src/pkg/lib.rs(2),
// foo(3), bar(4) — 5 rows total, used throughout so `CursorPageDown`/
// `CursorPageUp`'s clamping has more than one row of headroom to exercise.
//
// Every assert compares the whole `Nav` (cursor *and* collapse state): a
// cursor-only assert would pass even if a cursor action corrupted the
// collapse set on the way.

fn nav_at_cursor(cursor: usize) -> Nav {
    Nav {
        cursor,
        ..Nav::new()
    }
}

#[test]
fn should_move_cursor_to_last_row_when_cursor_bottom() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorBottom, &tree);

    assert_eq!(nav_at_cursor(4), actual);
}

#[test]
fn should_move_cursor_to_first_row_when_cursor_top() {
    let tree = deep_tree();
    let actual = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorTop, &tree);

    assert_eq!(nav_at_cursor(0), actual);
}

#[test]
fn should_keep_cursor_at_first_row_when_cursor_top_and_already_at_top() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorTop, &tree);

    assert_eq!(nav_at_cursor(0), actual);
}

#[test]
fn should_leave_nav_untouched_when_cursor_top_and_tree_has_no_rows() {
    let tree = Tree { roots: vec![] };
    let actual = Nav::new().handle(Action::CursorTop, &tree);

    assert_eq!(Nav::new(), actual);
}

#[test]
fn should_move_cursor_down_by_step_when_cursor_page_down() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorPageDown(2), &tree);

    assert_eq!(nav_at_cursor(2), actual);
}

#[test]
fn should_clamp_cursor_to_last_row_when_cursor_page_down_overshoots() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorPageDown(100), &tree);

    assert_eq!(nav_at_cursor(4), actual);
}

#[test]
fn should_move_cursor_up_by_step_when_cursor_page_up() {
    let tree = deep_tree();
    let actual = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorPageUp(2), &tree);

    assert_eq!(nav_at_cursor(2), actual);
}

#[test]
fn should_clamp_cursor_to_first_row_when_cursor_page_up_undershoots() {
    let tree = deep_tree();
    let actual = Nav::new()
        .handle(Action::CursorBottom, &tree)
        .handle(Action::CursorPageUp(100), &tree);

    assert_eq!(nav_at_cursor(0), actual);
}

#[test]
fn should_move_cursor_to_the_given_index_when_cursor_to() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorTo(3), &tree);

    assert_eq!(nav_at_cursor(3), actual);
}

#[test]
fn should_clamp_cursor_to_last_row_when_cursor_to_index_is_out_of_bounds() {
    let tree = deep_tree();
    let actual = Nav::new().handle(Action::CursorTo(100), &tree);

    assert_eq!(nav_at_cursor(4), actual);
}
