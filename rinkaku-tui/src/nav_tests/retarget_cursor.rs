use super::*;
use pretty_assertions::assert_eq;

// CRITICAL 2 regression: `cursor()`'s doc comment invites
// `rows(tree)[cursor()]`, but collapsing an ancestor of the cursor's
// row used to leave the cursor index pointing past the now-shrunk row
// list — the documented lookup would then panic (index out of
// bounds). The cursor genuinely sits on the deep leaf "foo" (row 3, a
// descendant of "src/pkg" that is about to be hidden) for this whole
// test — `ToggleExpand` always acts on the row *under the cursor*, so
// the only way to collapse "src/pkg" without first moving the cursor
// onto "src/pkg" itself (which would trivially avoid ever exercising
// the "cursor's own row disappeared" path) is to collapse it via a
// second, independent `Nav` positioned at "src/pkg", then transplant
// that `Nav`'s resulting `collapsed` state onto the first — the same
// state-transplant pattern
// `should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree`
// uses below, just applied to a collapse that *does* hide the cursor's
// row instead of one that doesn't. The cursor must land on "src/pkg"
// itself (the nearest still-visible ancestor of the row it used to be
// on), not stay at index 3 (now out of bounds) and not blindly clamp
// to "new last row" (which would coincidentally also be 1 here, but
// that is not the semantics being tested — see the CollapseAll variant
// below for a case where a naive last-row clamp gives a different,
// wrong answer).
#[test]
fn should_move_cursor_to_nearest_visible_ancestor_when_toggle_expand_hides_its_row() {
    let tree = deep_tree();
    let mut nav = Nav::new();
    for _ in 0..3 {
        nav = nav.handle(Action::CursorDown, &tree);
    }
    assert_eq!(3, nav.cursor()); // cursor on "foo", never moved off it

    // Collapse "src/pkg" (row 1) via a second, independent Nav, then
    // bring that collapse state back onto `nav` without touching its
    // cursor — simulates "src/pkg" collapsing while the cursor of
    // interest stays on the hidden descendant "foo".
    let mut collapse_pkg = Nav::new();
    collapse_pkg = collapse_pkg.handle(Action::CursorDown, &tree);
    assert_eq!(1, collapse_pkg.cursor()); // "src/pkg"
    collapse_pkg = collapse_pkg.handle(Action::ToggleExpand, &tree);

    // `nav`'s own path chain ("foo" then its ancestors) must be
    // captured *before* the collapsed set is transplanted in, exactly
    // as `handle` does internally — capturing it after would just
    // observe the already-shrunk row list and defeat the test.
    let chain = nav.cursor_path_chain(&tree);
    nav.collapsed = collapse_pkg.collapsed;
    nav.retarget_cursor(&tree, &chain);

    let rows = nav.rows(&tree);
    assert_eq!(vec!["src", "src/pkg"], row_paths(&rows));
    assert_eq!(1, nav.cursor());
}

#[test]
fn should_clamp_cursor_to_last_row_when_collapse_all_shrinks_the_row_list() {
    let tree = deep_tree();
    // Cursor on "bar" (row 4, the last row).
    let mut nav = Nav::new();
    for _ in 0..4 {
        nav = nav.handle(Action::CursorDown, &tree);
    }
    assert_eq!(4, nav.cursor());

    let nav = nav.handle(Action::CollapseAll, &tree);

    let rows = nav.rows(&tree);
    assert_eq!(vec!["src"], row_paths(&rows));
    assert_eq!(0, nav.cursor());
}

#[test]
fn should_clamp_cursor_to_last_row_when_collapse_all_hides_a_deep_cursor_row() {
    let tree = deep_tree();
    // Cursor on "src/pkg/lib.rs" (row 2, neither the first nor the
    // last row) — CollapseAll has no single "nearest visible ancestor"
    // notion the way a single ToggleExpand does (every directory
    // collapses at once), so this falls back to the simple "clamp to
    // last row" rule.
    let mut nav = Nav::new();
    for _ in 0..2 {
        nav = nav.handle(Action::CursorDown, &tree);
    }
    assert_eq!(2, nav.cursor());

    let nav = nav.handle(Action::CollapseAll, &tree);

    let rows = nav.rows(&tree);
    assert_eq!(vec!["src"], row_paths(&rows));
    assert_eq!(0, nav.cursor());
}

#[test]
fn should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree() {
    // Two independent top-level directories: collapsing "b" (which
    // the cursor is not on, and is not an ancestor of the cursor's
    // row) must leave the cursor exactly where it was.
    let tree = Tree {
        roots: vec![
            dir_node("a", vec![file_node("a/one.rs", vec![])]),
            dir_node("b", vec![file_node("b/two.rs", vec![])]),
        ],
    };
    // Rows expanded: a(0), a/one.rs(1), b(2), b/two.rs(3).
    let mut nav = Nav::new().handle(Action::CursorDown, &tree); // cursor -> "a/one.rs" (1)
    assert_eq!(1, nav.cursor());

    // Move a *second*, independent Nav to "b" (row 2) and collapse it
    // there, then bring that collapse state back without disturbing
    // `nav`'s own cursor — simulates two collapse actions happening
    // in the tree while the cursor of interest stays on "a/one.rs".
    let mut collapse_b = Nav::new();
    collapse_b = collapse_b.handle(Action::CursorDown, &tree);
    collapse_b = collapse_b.handle(Action::CursorDown, &tree); // cursor -> "b" (2)
    assert_eq!(2, collapse_b.cursor());
    collapse_b = collapse_b.handle(Action::ToggleExpand, &tree); // collapse "b"

    nav.collapsed = collapse_b.collapsed;

    let rows = nav.rows(&tree);
    assert_eq!(vec!["a", "a/one.rs", "b"], row_paths(&rows));
    assert_eq!(1, nav.cursor());
    assert_eq!("a/one.rs", rows[nav.cursor()].node.path);
}
