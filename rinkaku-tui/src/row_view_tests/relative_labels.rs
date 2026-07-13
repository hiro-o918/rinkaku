use super::*;

#[test]
fn should_strip_ancestor_prefix_from_nested_dir_label() {
    // "src" (depth 0) then "src/foo" (depth 1): the second row's label
    // must be just "foo", not the full "src/foo" its `path` carries.
    let root = dir_node("src", Badges::default(), vec![]);
    let child = dir_node("src/foo", Badges::default(), vec![]);
    let rows = vec![
        Row {
            node: &root,
            depth: 0,
            expanded: true,
        },
        Row {
            node: &child,
            depth: 1,
            expanded: false,
        },
    ];

    let labels = relative_labels(&rows);

    assert_eq!(vec!["src".to_string(), "foo".to_string()], labels);
}

#[test]
fn should_keep_full_collapsed_label_when_no_ancestor_row_precedes_it() {
    // A root-level collapsed chain "src/foo/bar" has no ancestor row
    // above it at all, so its label stays the full collapsed path.
    let root = dir_node("src/foo/bar", Badges::default(), vec![]);
    let rows = vec![Row {
        node: &root,
        depth: 0,
        expanded: true,
    }];

    let labels = relative_labels(&rows);

    assert_eq!(vec!["src/foo/bar".to_string()], labels);
}

#[test]
fn should_not_strip_partial_string_overlap_between_sibling_directory_names() {
    // "src" and "src2" are two independent top-level roots (both
    // depth 0, i.e. siblings, not ancestor/descendant) that happen to
    // share "src" as a string prefix. `relative_labels` only ever
    // compares a row against its own ancestor chain (via
    // `ancestor_path_at[row.depth - 1]`), never against a sibling at
    // the same depth, so "src2" must keep its full label rather than
    // having "src" spuriously stripped off it as if "src" were its
    // parent.
    let src = dir_node("src", Badges::default(), vec![]);
    let src2 = dir_node("src2", Badges::default(), vec![]);
    let rows = vec![
        Row {
            node: &src,
            depth: 0,
            expanded: false,
        },
        Row {
            node: &src2,
            depth: 0,
            expanded: false,
        },
    ];

    let labels = relative_labels(&rows);

    assert_eq!(vec!["src".to_string(), "src2".to_string()], labels);
}

#[test]
fn should_return_empty_label_for_symbol_rows() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let rows = vec![Row {
        node: &node,
        depth: 1,
        expanded: false,
    }];

    let labels = relative_labels(&rows);

    assert_eq!(vec![String::new()], labels);
}

#[test]
fn should_recompute_label_correctly_after_returning_to_a_shallower_sibling() {
    // "a" (depth 0) -> "a/one.rs" (depth 1) -> "b" (depth 0, a sibling
    // of "a", not a descendant) -> "b/two.rs" (depth 1). The second
    // "depth 1" row must strip "b"'s prefix, not stale ancestor state
    // left over from "a"'s subtree.
    let a = dir_node("a", Badges::default(), vec![]);
    let a_file = file_node("a/one.rs", Badges::default());
    let b = dir_node("b", Badges::default(), vec![]);
    let b_file = file_node("b/two.rs", Badges::default());
    let rows = vec![
        Row {
            node: &a,
            depth: 0,
            expanded: true,
        },
        Row {
            node: &a_file,
            depth: 1,
            expanded: false,
        },
        Row {
            node: &b,
            depth: 0,
            expanded: true,
        },
        Row {
            node: &b_file,
            depth: 1,
            expanded: false,
        },
    ];

    let labels = relative_labels(&rows);

    assert_eq!(
        vec![
            "a".to_string(),
            "one.rs".to_string(),
            "b".to_string(),
            "two.rs".to_string(),
        ],
        labels
    );
}
