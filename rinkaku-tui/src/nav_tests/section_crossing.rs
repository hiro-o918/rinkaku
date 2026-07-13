use super::*;
use pretty_assertions::assert_eq;

// ADR 0035 Phase B: `Nav` treats a `NodeKind::Section` exactly like a
// `Dir` for every concern it already generalizes over
// (expand/collapse, pre-order row flattening, cursor movement) —
// these tests exist to prove that generalization actually holds for
// the new variant, not to add new `Nav` behavior (none of `nav.rs`
// needed to change for Phase B — see this module's own doc comment
// history).

#[test]
fn should_show_section_row_expanded_by_default_alongside_production_rows() {
    let tree = tree_with_section();
    let nav = Nav::new();

    let rows = nav.rows(&tree);

    assert_eq!(
        vec![
            "src",
            "src/lib.rs",
            crate::tree::TESTS_SECTION_PATH,
            "a_test.go"
        ],
        row_paths(&rows)
    );
    // The Section row itself (index 2) must report `expanded: true`
    // like any other non-empty Dir/File row would.
    assert_eq!(true, rows[2].expanded);
}

#[test]
fn should_collapse_section_children_when_toggle_expand_is_applied_on_the_section_row() {
    let tree = tree_with_section();
    let mut nav = Nav::new();
    // Move the cursor onto the Section row (index 2) before toggling.
    nav = nav.handle(Action::CursorDown, &tree); // -> src/lib.rs (1)
    nav = nav.handle(Action::CursorDown, &tree); // -> Tests section (2)
    assert_eq!(2, nav.cursor());

    nav = nav.handle(Action::ToggleExpand, &tree);

    let rows = nav.rows(&tree);
    assert_eq!(
        vec!["src", "src/lib.rs", crate::tree::TESTS_SECTION_PATH],
        row_paths(&rows)
    );
    assert_eq!(false, rows[2].expanded);
}

#[test]
fn should_re_expand_section_children_when_toggle_expand_is_applied_twice() {
    let tree = tree_with_section();
    let mut nav = Nav::new();
    nav = nav.handle(Action::CursorDown, &tree);
    nav = nav.handle(Action::CursorDown, &tree); // cursor -> Tests section (2)
    nav = nav.handle(Action::ToggleExpand, &tree); // collapse
    nav = nav.handle(Action::ToggleExpand, &tree); // re-expand (cursor retargeted onto the section row by the first toggle, still there)

    let rows = nav.rows(&tree);
    assert_eq!(4, rows.len());
    assert_eq!(true, rows[2].expanded);
}

#[test]
fn should_move_cursor_down_from_last_production_row_into_the_section_row() {
    let tree = tree_with_section();
    let mut nav = Nav::new();
    nav = nav.handle(Action::CursorDown, &tree); // -> src/lib.rs (1)

    nav = nav.handle(Action::CursorDown, &tree); // -> Tests section (2)

    assert_eq!(2, nav.cursor());
    let rows = nav.rows(&tree);
    assert_eq!(
        crate::tree::TESTS_SECTION_PATH,
        rows[nav.cursor()].node.path
    );
}

#[test]
fn should_move_cursor_up_from_the_section_row_back_into_production_rows() {
    let tree = tree_with_section();
    let mut nav = Nav::new();
    nav = nav.handle(Action::CursorDown, &tree);
    nav = nav.handle(Action::CursorDown, &tree); // -> Tests section (2)
    assert_eq!(2, nav.cursor());

    nav = nav.handle(Action::CursorUp, &tree);

    assert_eq!(1, nav.cursor());
    let rows = nav.rows(&tree);
    assert_eq!("src/lib.rs", rows[nav.cursor()].node.path);
}

#[test]
fn should_move_cursor_down_from_the_section_row_into_its_own_children() {
    let tree = tree_with_section();
    let mut nav = Nav::new();
    nav = nav.handle(Action::CursorDown, &tree);
    nav = nav.handle(Action::CursorDown, &tree); // -> Tests section (2)

    nav = nav.handle(Action::CursorDown, &tree); // -> a_test.go (3)

    assert_eq!(3, nav.cursor());
    let rows = nav.rows(&tree);
    assert_eq!("a_test.go", rows[nav.cursor()].node.path);
}

#[test]
fn should_collapse_all_include_the_section_row() {
    let tree = tree_with_section();
    let nav = Nav::new().handle(Action::CollapseAll, &tree);

    let rows = nav.rows(&tree);

    assert_eq!(
        vec!["src", crate::tree::TESTS_SECTION_PATH],
        row_paths(&rows)
    );
    assert_eq!(false, rows[1].expanded);
}

#[test]
fn should_expand_all_restore_the_section_row_children() {
    let tree = tree_with_section();
    let nav = Nav::new()
        .handle(Action::CollapseAll, &tree)
        .handle(Action::ExpandAll, &tree);

    let rows = nav.rows(&tree);

    assert_eq!(
        vec![
            "src",
            "src/lib.rs",
            crate::tree::TESTS_SECTION_PATH,
            "a_test.go"
        ],
        row_paths(&rows)
    );
}

#[test]
fn should_move_cursor_to_the_section_row_via_move_cursor_to_path() {
    let tree = tree_with_section();
    let mut nav = Nav::new();

    let actual = nav.move_cursor_to_path(&tree, crate::tree::TESTS_SECTION_PATH);

    assert!(actual);
    assert_eq!(2, nav.cursor());
}
