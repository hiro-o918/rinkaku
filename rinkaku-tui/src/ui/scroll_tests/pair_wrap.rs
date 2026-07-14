use super::*;

#[test]
fn should_return_empty_columns_when_both_sides_are_empty() {
    let actual = pair_wrap(&[], &[], 10, 10);

    assert_eq!((Vec::new(), Vec::new()), actual);
}

#[test]
fn should_keep_one_row_per_side_when_neither_line_wraps() {
    let left = vec![Line::raw("old")];
    let right = vec![Line::raw("new")];

    let actual = pair_wrap(&left, &right, 10, 10);

    assert_eq!((vec![Line::raw("old")], vec![Line::raw("new")]), actual);
}

#[test]
fn should_pad_shorter_wrapped_side_when_left_line_wraps_and_right_does_not() {
    // "abcdefgh" wraps to 2 rows at width 4 ("abcd"/"efgh"); the right
    // side's one logical row must grow a blank filler row so both
    // columns stay the same length for the shared scroll offset (ADR
    // 0044 decision 6).
    let left = vec![Line::raw("abcdefgh")];
    let right = vec![Line::raw("new")];

    let actual = pair_wrap(&left, &right, 4, 10);

    assert_eq!(
        (
            vec![Line::raw("abcd"), Line::raw("efgh")],
            vec![Line::raw("new"), Line::raw("")],
        ),
        actual
    );
}

#[test]
fn should_pad_missing_side_when_one_column_has_fewer_logical_rows() {
    // The left column has two logical rows, the right only one — the
    // second logical row pairs a real left line against an absent right
    // line, which must still produce a filler row on the right.
    let left = vec![Line::raw("old 1"), Line::raw("old 2")];
    let right = vec![Line::raw("new 1")];

    let actual = pair_wrap(&left, &right, 10, 10);

    assert_eq!(
        (
            vec![Line::raw("old 1"), Line::raw("old 2")],
            vec![Line::raw("new 1"), Line::raw("")],
        ),
        actual
    );
}
