//! `scroll_target_line_for_symbol` tests (ADR 0027, ADR 0072, ADR 0074):
//! the logical-line offset a symbol selection auto-scrolls the diff pane to
//! — the first rendered row whose own new-side coordinate falls inside that
//! symbol's range.

use super::*;
use crate::diff_view::{DiffLine, DiffLineKind};
use pretty_assertions::assert_eq;

/// A hunk whose body lines carry explicit kinds, unlike [`super::hunk`]'s
/// all-`Context` bodies — needed by the cases below that pin where a
/// `Removed` row (which has no new-side line of its own) resolves to.
fn mixed_hunk(
    header: &str,
    new_range: Option<(usize, usize)>,
    lines: Vec<(DiffLineKind, &str)>,
) -> Hunk {
    Hunk {
        header: header.to_string(),
        new_range,
        lines: lines
            .into_iter()
            .map(|(kind, content)| DiffLine {
                kind,
                content: content.to_string(),
            })
            .collect(),
    }
}

#[test]
fn should_return_none_for_symbol_start_when_content_is_empty() {
    let actual =
        scroll_target_line_for_symbol(&DiffPaneContent::Empty, LineRange { start: 1, end: 2 });

    assert_eq!(None, actual);
}

#[test]
fn should_return_zero_when_the_symbol_starts_where_the_only_hunk_starts() {
    // The `@@` header row shares its hunk's first new-side line, so a
    // symbol starting exactly there still lands on the header rather than
    // one row past it.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = scroll_target_line_for_symbol(&content, LineRange { start: 1, end: 2 });

    assert_eq!(Some(0), actual);
}

#[test]
fn should_return_second_hunk_start_when_only_the_second_hunk_covers_the_symbol_range() {
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

    let actual = scroll_target_line_for_symbol(&content, LineRange { start: 10, end: 11 });

    assert_eq!(Some(3), actual);
}

#[test]
fn should_return_the_symbols_own_row_when_it_starts_inside_a_hunk() {
    // ADR 0074's regression pin, from the dogfooding report this fix came
    // from: a whole new file arrives as one hunk covering every symbol in
    // it, so resolving to the *hunk's* start gave all of them the same
    // target and moving the tree cursor between them scrolled the pane
    // nowhere. Rows: header(0), then new-side lines 1..=4 at rows 1..=4.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -0,0 +1,4 @@",
            Some((1, 4)),
            vec!["fn foo() {}", "", "fn bar() {}", ""],
        ),
    )]);

    let foo = scroll_target_line_for_symbol(&content, LineRange { start: 1, end: 2 });
    let bar = scroll_target_line_for_symbol(&content, LineRange { start: 3, end: 4 });

    assert_eq!((Some(0), Some(3)), (foo, bar));
}

#[test]
fn should_return_the_first_covering_row_when_the_symbol_range_spans_two_hunks() {
    // The symbol's range is covered by rows in both hunks — the *first*
    // such row in render order wins, which is inside hunk 0 (its new-side
    // line 3 at row 3), not hunk 0's header.
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,3 @@", Some((1, 3)), vec!["a", "b", "c"]),
        ),
        attributed(1, hunk("@@ -10,1 +4,2 @@", Some((4, 5)), vec!["d", "e"])),
    ]);

    let actual = scroll_target_line_for_symbol(&content, LineRange { start: 3, end: 5 });

    assert_eq!(Some(3), actual);
}

#[test]
fn should_return_the_removed_row_of_a_replaced_signature_rather_than_its_added_row() {
    // A `Removed` row carries the new-side line it immediately precedes
    // (`diff_view::new_side_positions`), so the `-` half of a replaced
    // signature shares the `+` half's coordinate and the target lands on
    // the pair's first row — the old signature stays on screen.
    // Rows: header(0) at line 1, context(1) at line 1, removed(2) at line
    // 2, added(3) at line 2.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        mixed_hunk(
            "@@ -1,2 +1,2 @@",
            Some((1, 2)),
            vec![
                (DiffLineKind::Context, "mod a;"),
                (DiffLineKind::Removed, "fn foo(a: u32) {}"),
                (DiffLineKind::Added, "fn foo(a: u64) {}"),
            ],
        ),
    )]);

    let actual = scroll_target_line_for_symbol(&content, LineRange { start: 2, end: 2 });

    assert_eq!(Some(2), actual);
}

#[test]
fn should_return_none_when_no_row_covers_the_symbol_range() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = scroll_target_line_for_symbol(
        &content,
        LineRange {
            start: 100,
            end: 200,
        },
    );

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_the_hunk_header_has_no_readable_new_side_start() {
    // `Hunk::new_range` is `None` for an unreadable header or a new-side
    // start of 0 — no row of that hunk carries a coordinate, so nothing
    // can resolve to it.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ garbled @@", None, vec!["fn foo() {}"]),
    )]);

    let actual = scroll_target_line_for_symbol(&content, LineRange { start: 1, end: 2 });

    assert_eq!(None, actual);
}
