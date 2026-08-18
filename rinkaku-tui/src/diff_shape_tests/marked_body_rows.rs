//! `marked_body_rows` tests: the set of body-row logical-line offsets a
//! symbol's range bar should mark — same new-side row attribution
//! `scroll_target_line_for_symbol` uses, but every covering *body* row
//! rather than only the first one, and never a hunk's own `@@` header row.

use super::*;
use crate::app::DiffViewMode;
use crate::diff_view::{DiffLine, DiffLineKind};
use pretty_assertions::assert_eq;

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
fn should_return_empty_when_content_is_empty() {
    let actual = marked_body_rows(
        &DiffPaneContent::Empty,
        LineRange { start: 1, end: 2 },
        DiffViewMode::Unified,
    );

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_return_every_body_row_spanning_a_multi_line_symbol_range() {
    // Rows: header(0), body 1..=4 at rows 1..=4 (new-side lines 1..=4).
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -0,0 +1,4 @@",
            Some((1, 4)),
            vec!["fn foo() {", "    1", "    2", "}"],
        ),
    )]);

    let actual = marked_body_rows(
        &content,
        LineRange { start: 1, end: 4 },
        DiffViewMode::Unified,
    );

    assert_eq!(vec![1, 2, 3, 4], actual);
}

#[test]
fn should_exclude_the_hunk_header_row_even_when_it_shares_the_symbol_starts_coordinate() {
    // The header shares line 1's coordinate with the first body row
    // (`hunk_header_position`), but only the body row (index 1) is a `Body`
    // row — the header itself (index 0) must never be marked, since it
    // carries no gutter column to paint the bar onto.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = marked_body_rows(
        &content,
        LineRange { start: 1, end: 2 },
        DiffViewMode::Unified,
    );

    assert_eq!(vec![1], actual);
}

#[test]
fn should_exclude_the_blank_separator_row_between_two_hunks() {
    // Rows: header0(0), body0(1), separator(2), header1(3), body1(4). A
    // symbol range wide enough to span both hunks' coordinates must still
    // skip the separator at row 2.
    let content = DiffPaneContent::File(vec![
        attributed(0, hunk("@@ -1,1 +1,1 @@", Some((1, 1)), vec!["a"])),
        attributed(1, hunk("@@ -5,1 +5,1 @@", Some((5, 5)), vec!["e"])),
    ]);

    let actual = marked_body_rows(
        &content,
        LineRange { start: 1, end: 5 },
        DiffViewMode::Unified,
    );

    assert_eq!(vec![1, 4], actual);
}

#[test]
fn should_include_a_removed_row_that_precedes_a_line_inside_the_range() {
    // Rows: header(0) line 1, context(1) line 1, removed(2) line 2 (carries
    // the line it precedes, `new_side_positions`), added(3) line 2.
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

    let actual = marked_body_rows(
        &content,
        LineRange { start: 2, end: 2 },
        DiffViewMode::Unified,
    );

    assert_eq!(vec![2, 3], actual);
}

#[test]
fn should_target_the_rows_a_symbol_actually_renders_on_in_split_view() {
    // Mirrors `scroll_target_line`'s own split-view pin: `pair_hunk_lines`
    // reorders a replace run's rows, so the split-mode marked set differs
    // from the unified one for the same symbol range.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        mixed_hunk(
            "@@ -10,3 +10,3 @@",
            Some((10, 12)),
            vec![
                (DiffLineKind::Removed, "fn alpha(x: u32) {}"),
                (DiffLineKind::Removed, "fn beta(x: u32) {}"),
                (DiffLineKind::Removed, "fn gamma(x: u32) {}"),
                (DiffLineKind::Added, "fn alpha(x: u64) {}"),
                (DiffLineKind::Added, "fn beta(x: u64) {}"),
                (DiffLineKind::Added, "fn gamma(x: u64) {}"),
            ],
        ),
    )]);

    let unified = marked_body_rows(
        &content,
        LineRange { start: 12, end: 12 },
        DiffViewMode::Unified,
    );
    let split = marked_body_rows(
        &content,
        LineRange { start: 12, end: 12 },
        DiffViewMode::Split,
    );

    assert_eq!((vec![6], vec![3]), (unified, split));
}

#[test]
fn should_return_empty_when_no_row_covers_the_symbol_range() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);

    let actual = marked_body_rows(
        &content,
        LineRange {
            start: 100,
            end: 200,
        },
        DiffViewMode::Unified,
    );

    assert_eq!(Vec::<usize>::new(), actual);
}
