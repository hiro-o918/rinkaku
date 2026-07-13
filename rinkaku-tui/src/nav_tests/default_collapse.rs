use super::*;

/// A mixed file with one production symbol and a `TestGroup` grouping one
/// test symbol — the shape `crate::tree::build_tree` produces for a file
/// like `mermaid.rs` mixing real code with `#[cfg(test)]` functions.
/// Row order if the `TestGroup` were expanded: src(0), src/lib.rs(1),
/// foo(2), tests-group(3), test_it(4).
fn tree_with_test_group() -> Tree {
    Tree {
        roots: vec![dir_node(
            "src",
            vec![file_node(
                "src/lib.rs",
                vec![
                    symbol_node("src/lib.rs", "foo"),
                    test_group_node("src/lib.rs", vec![symbol_node("src/lib.rs", "test_it")]),
                ],
            )],
        )],
    }
}

#[test]
fn should_collapse_test_group_by_default_when_building_a_fresh_nav() {
    let tree = tree_with_test_group();

    let nav = Nav::new_collapsing_test_groups(&tree);
    let rows = nav.rows(&tree);

    let expected = vec!["src", "src/lib.rs", "src/lib.rs", "src/lib.rs::tests"];
    let actual = row_paths(&rows);

    assert_eq!(expected, actual);
}

#[test]
fn should_reveal_test_group_children_after_toggling_expand() {
    let tree = tree_with_test_group();
    let nav = Nav::new_collapsing_test_groups(&tree);

    // Move the cursor onto the (collapsed) test-group row, then expand it.
    let mut nav = nav;
    nav = nav.handle(Action::CursorDown, &tree);
    nav = nav.handle(Action::CursorDown, &tree);
    nav = nav.handle(Action::CursorDown, &tree);
    let expanded = nav.handle(Action::ToggleExpand, &tree);

    let rows = expanded.rows(&tree);
    let expected = vec![
        "src",
        "src/lib.rs",
        "src/lib.rs",
        "src/lib.rs::tests",
        "src/lib.rs",
    ];
    let actual = row_paths(&rows);

    assert_eq!(expected, actual);
}

#[test]
fn should_leave_non_test_group_nodes_expanded_by_default() {
    let tree = tree_with_test_group();

    let nav = Nav::new_collapsing_test_groups(&tree);

    // "src" and "src/lib.rs" both have children and are not `TestGroup`
    // rows, so both must still be expanded (only the `TestGroup` row
    // itself starts collapsed).
    let rows = nav.rows(&tree);
    assert!(
        rows.iter()
            .any(|row| row.node.path == "src" && row.expanded)
    );
    assert!(rows.iter().any(|row| row.node.path == "src/lib.rs"
        && matches!(row.node.kind, NodeKind::File)
        && row.expanded));
}
