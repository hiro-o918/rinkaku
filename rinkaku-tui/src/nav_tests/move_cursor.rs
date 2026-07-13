use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_move_cursor_to_matching_dir_row_when_path_matches_a_directory() {
    let tree = sample_tree();
    let mut nav = Nav::new();

    let actual = nav.move_cursor_to_path(&tree, "src");

    assert!(actual);
    assert_eq!(0, nav.cursor());
}

#[test]
fn should_move_cursor_to_matching_file_row_when_path_matches_a_file() {
    let tree = sample_tree();
    let mut nav = Nav::new();

    let actual = nav.move_cursor_to_path(&tree, "src/lib.rs");

    assert!(actual);
    // Row 1 is the File row itself, not either of its two Symbol rows
    // (2, 3) which share the same `node.path` — `move_cursor_to_path`
    // must land on the File row specifically.
    assert_eq!(1, nav.cursor());
}

#[test]
fn should_not_move_cursor_when_no_row_matches_the_path() {
    let tree = sample_tree();
    let mut nav = Nav::new().handle(Action::CursorDown, &tree);
    assert_eq!(1, nav.cursor());

    let actual = nav.move_cursor_to_path(&tree, "no/such/path");

    assert!(!actual);
    assert_eq!(1, nav.cursor());
}

#[test]
fn should_not_move_cursor_to_a_matching_row_hidden_under_a_collapsed_ancestor() {
    let tree = sample_tree();
    let mut nav = Nav::new().handle(Action::ToggleExpand, &tree); // collapse "src"
    assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

    let actual = nav.move_cursor_to_path(&tree, "src/lib.rs");

    assert!(!actual);
    assert_eq!(0, nav.cursor());
}

#[test]
fn should_move_cursor_to_matching_symbol_row_when_symbol_id_matches() {
    let tree = sample_tree();
    let mut nav = Nav::new();

    let actual = nav.move_cursor_to_symbol(&tree, "src/lib.rs::bar");

    assert!(actual);
    // Row 3 is "bar" (src(0), src/lib.rs(1), foo(2), bar(3)).
    assert_eq!(3, nav.cursor());
}

#[test]
fn should_not_move_cursor_when_no_symbol_id_matches() {
    let tree = sample_tree();
    let mut nav = Nav::new().handle(Action::CursorDown, &tree);
    assert_eq!(1, nav.cursor());

    let actual = nav.move_cursor_to_symbol(&tree, "no/such/id");

    assert!(!actual);
    assert_eq!(1, nav.cursor());
}

#[test]
fn should_expand_collapsed_ancestor_when_jumping_to_a_hidden_symbol() {
    let tree = sample_tree();
    let mut nav = Nav::new().handle(Action::ToggleExpand, &tree); // collapse "src"
    assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

    let actual = nav.move_cursor_to_symbol(&tree, "src/lib.rs::foo");

    assert!(actual);
    let rows = nav.rows(&tree);
    assert_eq!(
        vec!["src", "src/lib.rs", "src/lib.rs", "src/lib.rs"],
        row_paths(&rows)
    );
    assert_eq!("foo", symbol_name(&rows[nav.cursor()]));
}

#[test]
fn should_expand_multiple_collapsed_ancestors_when_jumping_to_a_deeply_hidden_symbol() {
    let tree = deep_tree();
    let mut nav = Nav::new();
    // Collapse both "src" and "src/pkg" so the target symbol is hidden
    // two levels deep.
    nav = nav.handle(Action::ToggleExpand, &tree); // collapse "src" (cursor was on it)
    assert_eq!(vec!["src"], row_paths(&nav.rows(&tree)));

    let actual = nav.move_cursor_to_symbol(&tree, "src/pkg/lib.rs::bar");

    assert!(actual);
    let rows = nav.rows(&tree);
    assert_eq!(
        vec![
            "src",
            "src/pkg",
            "src/pkg/lib.rs",
            "src/pkg/lib.rs",
            "src/pkg/lib.rs"
        ],
        row_paths(&rows)
    );
    assert_eq!("bar", symbol_name(&rows[nav.cursor()]));
}
