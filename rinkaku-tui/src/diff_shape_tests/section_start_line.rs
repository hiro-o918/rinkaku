use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_none_for_symbol_start_when_content_is_empty() {
    let actual =
        section_start_line_for_symbol(&DiffPaneContent::Empty, LineRange { start: 1, end: 2 });

    assert_eq!(None, actual);
}

#[test]
fn should_return_zero_when_the_only_hunk_intersects_the_symbol_range() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = section_start_line_for_symbol(&content, LineRange { start: 1, end: 2 });

    assert_eq!(Some(0), actual);
}

#[test]
fn should_return_second_hunk_start_when_only_the_second_hunk_intersects_the_symbol_range() {
    // Hunk 0: header(0), 1 body line(1) — 2 lines. Blank(2), hunk 1
    // header(3) — hunk 1 starts at line 3.
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(
            1,
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ),
    ]);

    let actual = section_start_line_for_symbol(&content, LineRange { start: 10, end: 11 });

    assert_eq!(Some(3), actual);
}

#[test]
fn should_return_the_first_intersecting_hunk_start_when_the_symbol_range_spans_two_hunks() {
    // The symbol's range intersects both hunks (an adjacent/overlapping
    // case) — the *first* hunk in original order wins, not the one with
    // more overlap or the last one.
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,3 @@", Some((1, 3)), vec!["a", "b", "c"]),
        ),
        attributed(1, hunk("@@ -10,1 +4,2 @@", Some((4, 5)), vec!["d", "e"])),
    ]);

    let actual = section_start_line_for_symbol(&content, LineRange { start: 3, end: 5 });

    assert_eq!(Some(0), actual);
}

#[test]
fn should_return_none_when_no_hunk_intersects_the_symbol_range() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = section_start_line_for_symbol(
        &content,
        LineRange {
            start: 100,
            end: 200,
        },
    );

    assert_eq!(None, actual);
}
