use super::*;

// --- wrap_lines_with_origins / pair_wrap_with_origins: per-row logical-line
// index ---

#[test]
fn should_map_every_display_row_to_its_source_logical_line_when_one_line_wraps_into_three() {
    let lines = vec![Line::raw("abcdefghij"), Line::raw("k")];

    let (wrapped, origins) = wrap_lines_with_origins(&lines, 4);

    assert_eq!(
        vec![
            Line::raw("abcd"),
            Line::raw("efgh"),
            Line::raw("ij"),
            Line::raw("k")
        ],
        wrapped
    );
    assert_eq!(vec![0, 0, 0, 1], origins);
}

#[test]
fn should_map_every_display_row_to_its_shared_logical_index_when_split_rows_wrap_unevenly() {
    let left = vec![Line::raw("abcdefgh"), Line::raw("z")];
    let right = vec![Line::raw("new")];

    let (left_rows, right_rows, origins) = pair_wrap_with_origins(&left, &right, 4, 10);

    assert_eq!(
        vec![Line::raw("abcd"), Line::raw("efgh"), Line::raw("z")],
        left_rows
    );
    assert_eq!(
        vec![Line::raw("new"), Line::raw(""), Line::raw("")],
        right_rows
    );
    assert_eq!(vec![0, 0, 1], origins);
}

// --- logical_line_to_display_row ---

#[test]
fn should_return_zero_when_content_is_empty() {
    let actual = logical_line_to_display_row(&[], 0);

    assert_eq!(0, actual);
}

#[test]
fn should_return_the_first_display_row_of_a_logical_line_that_did_not_wrap() {
    let origins = vec![0, 1, 2];

    let actual = logical_line_to_display_row(&origins, 1);

    assert_eq!(1, actual);
}

#[test]
fn should_return_the_first_wrapped_row_when_the_target_logical_line_wraps_into_several_rows() {
    // Logical line 0 wraps into display rows 0-2, logical line 1 is
    // display row 3 — landing on logical line 0 must resolve to its
    // *first* row (0), not any of its later wrapped rows.
    let origins = vec![0, 0, 0, 1];

    let actual = logical_line_to_display_row(&origins, 0);

    assert_eq!(0, actual);
}

#[test]
fn should_return_one_past_the_last_display_row_when_logical_line_overscrolls() {
    let origins = vec![0, 0, 1];

    let actual = logical_line_to_display_row(&origins, 5);

    assert_eq!(3, actual);
}

// --- display_row_to_logical_line ---

#[test]
fn should_return_zero_when_origins_is_empty() {
    let actual = display_row_to_logical_line(&[], 0);

    assert_eq!(0, actual);
}

#[test]
fn should_resolve_every_wrapped_row_of_a_logical_line_to_the_same_logical_index() {
    // Logical line 0 wraps into display rows 0-2 — all three must
    // resolve back to logical line 0, matching the reverse-lookup
    // behavior `crate::diff_shape::symbol_id_for_scroll_line` needs once
    // `App::right_pane_scroll` is folded back through this conversion
    // (symptom 2 of the scroll-unit bug: without it, scrolling past a
    // wrapped section always resolved to the last symbol instead of the
    // one actually on screen).
    let origins = vec![0, 0, 0, 1];

    assert_eq!(0, display_row_to_logical_line(&origins, 0));
    assert_eq!(0, display_row_to_logical_line(&origins, 1));
    assert_eq!(0, display_row_to_logical_line(&origins, 2));
    assert_eq!(1, display_row_to_logical_line(&origins, 3));
}

#[test]
fn should_fall_back_to_the_last_origin_when_display_row_is_past_the_end() {
    let origins = vec![0, 1, 2];

    let actual = display_row_to_logical_line(&origins, 100);

    assert_eq!(2, actual);
}

// --- round trip ---

#[test]
fn should_round_trip_a_logical_line_through_display_row_conversion_when_it_wraps() {
    let lines = vec![Line::raw("abcdefghij"), Line::raw("k"), Line::raw("lmno")];
    let (_wrapped, origins) = wrap_lines_with_origins(&lines, 4);

    let display_row = logical_line_to_display_row(&origins, 2);
    let actual = display_row_to_logical_line(&origins, display_row);

    assert_eq!(2, actual);
}

// --- resolve_folded_back_logical_line: fold-back that never undoes a
// request that landed inside a preceding wrapped span ---

// 48 display rows of logical line 0 (a huge wrapped signature), followed
// by three unwrapped short lines.
fn origins_with_one_huge_wrapped_line_then_three_short_lines() -> Vec<usize> {
    let mut origins = vec![0; 48];
    origins.extend([1, 2, 3]);
    origins
}

#[test]
fn should_return_the_requested_logical_line_when_the_clamped_display_row_lands_inside_its_own_wrapped_span()
 {
    let origins = origins_with_one_huge_wrapped_line_then_three_short_lines();

    let actual = resolve_folded_back_logical_line(&origins, 0, 0);

    assert_eq!(0, actual);
}

#[test]
fn should_return_the_requested_logical_line_when_it_resolves_past_the_wrapped_span_into_a_short_line()
 {
    let origins = origins_with_one_huge_wrapped_line_then_three_short_lines();

    let actual = resolve_folded_back_logical_line(&origins, 0, 1);

    assert_eq!(1, actual);
}

#[test]
fn should_return_the_last_logical_line_when_the_clamped_display_row_lands_on_its_last_wrapped_row()
{
    let origins = origins_with_one_huge_wrapped_line_then_three_short_lines();

    let actual = resolve_folded_back_logical_line(&origins, 47, 3);

    assert_eq!(3, actual);
}

#[test]
fn should_cap_an_overscrolled_request_at_the_last_logical_line() {
    let origins = origins_with_one_huge_wrapped_line_then_three_short_lines();

    let actual = resolve_folded_back_logical_line(&origins, 47, 9);

    assert_eq!(3, actual);
}

#[test]
fn should_fall_back_to_zero_when_origins_is_empty() {
    let actual = resolve_folded_back_logical_line(&[], 0, 5);

    assert_eq!(0, actual);
}
