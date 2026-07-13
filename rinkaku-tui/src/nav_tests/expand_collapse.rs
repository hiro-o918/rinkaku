use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_show_every_row_expanded_when_nav_is_new() {
    let tree = sample_tree();
    let nav = Nav::new();

    let rows = nav.rows(&tree);

    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src", "src/lib.rs", "src/lib.rs", "src/lib.rs"], paths);
}

#[test]
fn should_hide_children_when_toggle_expand_collapses_the_dir_under_cursor() {
    let tree = sample_tree();
    let nav = Nav::new(); // cursor at 0 ("src")

    let nav = nav.handle(Action::ToggleExpand, &tree);
    let rows = nav.rows(&tree);

    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src"], paths);
    assert_eq!(false, rows[0].expanded);
}

#[test]
fn should_show_children_again_when_toggle_expand_is_applied_twice() {
    let tree = sample_tree();
    let nav = Nav::new()
        .handle(Action::ToggleExpand, &tree)
        .handle(Action::ToggleExpand, &tree);

    let rows = nav.rows(&tree);

    assert_eq!(4, rows.len());
    assert_eq!(true, rows[0].expanded);
}

#[test]
fn should_not_toggle_when_cursor_is_on_a_symbol_leaf() {
    let tree = sample_tree();
    // Move cursor down twice: src (0) -> src/lib.rs (1) -> foo (2).
    let nav = Nav::new()
        .handle(Action::CursorDown, &tree)
        .handle(Action::CursorDown, &tree);
    assert_eq!(2, nav.cursor());

    let nav = nav.handle(Action::ToggleExpand, &tree);
    let rows = nav.rows(&tree);

    // Nothing collapsed: still every row visible.
    assert_eq!(4, rows.len());
}

#[test]
fn should_clamp_cursor_at_zero_when_cursor_up_past_the_top() {
    let tree = sample_tree();
    let nav = Nav::new().handle(Action::CursorUp, &tree);

    assert_eq!(0, nav.cursor());
}

#[test]
fn should_clamp_cursor_at_last_row_when_cursor_down_past_the_bottom() {
    let tree = sample_tree();
    let mut nav = Nav::new();
    for _ in 0..10 {
        nav = nav.handle(Action::CursorDown, &tree);
    }

    assert_eq!(3, nav.cursor());
}

#[test]
fn should_move_cursor_down_one_row_at_a_time() {
    let tree = sample_tree();
    let nav = Nav::new().handle(Action::CursorDown, &tree);

    assert_eq!(1, nav.cursor());
}

#[test]
fn should_collapse_every_dir_and_file_when_collapse_all_is_applied() {
    let tree = sample_tree();
    let nav = Nav::new().handle(Action::CollapseAll, &tree);

    let rows = nav.rows(&tree);

    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src"], paths);
}

#[test]
fn should_expand_every_node_when_expand_all_is_applied_after_collapse_all() {
    let tree = sample_tree();
    let nav = Nav::new()
        .handle(Action::CollapseAll, &tree)
        .handle(Action::ExpandAll, &tree);

    let rows = nav.rows(&tree);

    assert_eq!(4, rows.len());
}

#[test]
fn should_report_row_as_not_expanded_when_node_has_no_children() {
    // A childless file (e.g. a pure rename with no symbols) can never
    // be "expanded" — nothing to show — regardless of collapse state.
    let tree = Tree {
        roots: vec![file_node("renamed.rs", vec![])],
    };
    let nav = Nav::new();

    let rows = nav.rows(&tree);

    assert_eq!(1, rows.len());
    assert_eq!(false, rows[0].expanded);
}

#[test]
fn should_keep_collapse_state_stable_across_a_tree_rebuild_with_same_paths() {
    // Simulates a Report re-run producing a structurally identical
    // tree (same paths) — collapse state, keyed by path, must survive.
    let tree_v1 = sample_tree();
    let nav = Nav::new().handle(Action::ToggleExpand, &tree_v1);

    let tree_v2 = sample_tree(); // fresh tree, same paths/shape
    let rows = nav.rows(&tree_v2);

    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    assert_eq!(vec!["src"], paths);
}
