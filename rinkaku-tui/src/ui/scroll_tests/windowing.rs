use super::*;

// --- visible_index_window / window_overflow_indicators (pure helpers,
// #61-review fix: cursor-follow scroll for the tree pane and jump popup)

#[test]
fn should_return_empty_window_when_total_items_is_zero() {
    let actual = visible_index_window(0, 0, 10);

    assert_eq!((0, 0), actual);
}

#[test]
fn should_return_empty_window_when_viewport_height_is_zero() {
    let actual = visible_index_window(20, 5, 0);

    assert_eq!((0, 0), actual);
}

#[test]
fn should_show_whole_list_when_it_fits_entirely_within_viewport() {
    let actual = visible_index_window(5, 2, 10);

    assert_eq!((0, 5), actual);
}

#[test]
fn should_center_window_around_cursor_when_list_exceeds_viewport() {
    // 100 items, cursor at index 49, viewport 10 -> half=5,
    // ideal_start=44, max_start=90 (not clamped) -> (44, 54).
    let actual = visible_index_window(100, 49, 10);

    assert_eq!((44, 54), actual);
}

#[test]
fn should_clamp_window_to_start_of_list_when_cursor_is_near_the_top() {
    let actual = visible_index_window(100, 1, 10);

    assert_eq!((0, 10), actual);
}

#[test]
fn should_clamp_window_to_end_of_list_when_cursor_is_near_the_bottom() {
    let actual = visible_index_window(100, 98, 10);

    assert_eq!((90, 100), actual);
}

#[test]
fn should_keep_cursor_row_inside_the_window_when_jumping_far_from_the_current_position() {
    // The exact scenario the #61-review finding describes: a cursor
    // that jumps from near the top of a long list straight to near the
    // bottom (e.g. a gd/gr jump) must land inside the returned window,
    // not leave it showing the same rows as before the jump.
    let (start, end) = visible_index_window(200, 180, 20);

    assert!(
        start <= 180 && 180 < end,
        "cursor 180 not in [{start}, {end})"
    );
}

#[test]
fn should_return_no_indicators_when_window_covers_the_whole_list() {
    let actual = window_overflow_indicators(5, 0, 5);

    assert_eq!((None, None), actual);
}

#[test]
fn should_return_above_indicator_only_when_window_starts_past_the_top() {
    let actual = window_overflow_indicators(100, 10, 20);

    assert_eq!(
        (
            Some("… 10 more above".to_string()),
            Some("… 80 more below".to_string())
        ),
        actual
    );
}

#[test]
fn should_return_no_above_indicator_when_window_starts_at_the_top() {
    let actual = window_overflow_indicators(100, 0, 20);

    assert_eq!((None, Some("… 80 more below".to_string())), actual);
}

#[test]
fn should_return_no_below_indicator_when_window_reaches_the_end() {
    let actual = window_overflow_indicators(100, 80, 100);

    assert_eq!((Some("… 80 more above".to_string()), None), actual);
}

// --- windowed_rows_with_indicators (pure helper) ---
//
// Regression coverage for the reserved-row bug found while writing
// `should_window_candidates_around_cursor_when_popup_has_more_candidates_than_fit`:
// naively computing the content window against the full viewport height
// and then unconditionally prepending/appending indicator lines
// overflows the viewport by up to 2 rows, clipping the cursor row
// itself off the bottom in the worst case.

#[test]
fn should_return_whole_list_with_no_indicators_when_it_fits_the_viewport() {
    let actual = windowed_rows_with_indicators(5, 2, 10);

    assert_eq!((0, 5, None, None), actual);
}

#[test]
fn should_reserve_a_row_for_the_below_indicator_when_cursor_is_near_the_top() {
    // 100 items, cursor at 0, viewport 10: without reservation the
    // content window alone would be (0, 10), needing a "below"
    // indicator — reserving 1 row for it must shrink the content
    // window to (0, 9) so the indicator line has room without pushing
    // total rows past the viewport.
    let (start, end, above, below) = windowed_rows_with_indicators(100, 0, 10);

    assert_eq!((0, 9), (start, end));
    assert_eq!(None, above);
    assert_eq!(Some("… 91 more below".to_string()), below);
    // The rendered row count (indicator + content) must never exceed
    // the viewport.
    let below_rows = below.is_some() as usize;
    assert!(end - start + below_rows <= 10);
}

#[test]
fn should_reserve_a_row_for_the_above_indicator_when_cursor_is_near_the_bottom() {
    let (start, end, above, below) = windowed_rows_with_indicators(100, 99, 10);

    assert_eq!((91, 100), (start, end));
    assert_eq!(Some("… 91 more above".to_string()), above);
    assert_eq!(None, below);
    let above_rows = above.is_some() as usize;
    assert!(end - start + above_rows <= 10);
}

#[test]
fn should_reserve_rows_for_both_indicators_when_cursor_is_in_the_middle() {
    let (start, end, above, below) = windowed_rows_with_indicators(100, 50, 10);

    assert!(above.is_some());
    assert!(below.is_some());
    // The cursor must still be inside the (possibly shrunk) content
    // window — the entire point of this function.
    assert!(start <= 50 && 50 < end, "cursor 50 not in [{start}, {end})");
    // Total rendered rows (2 indicators + content) must never exceed
    // the viewport.
    assert!(end - start + 2 <= 10);
}

#[test]
fn should_keep_cursor_visible_after_reserving_indicator_rows_at_a_small_viewport() {
    // A tight viewport (3 rows) where reserving rows for indicators
    // could plausibly starve the content window down to nothing —
    // pins that the cursor row itself is never sacrificed.
    let (start, end, _above, below) = windowed_rows_with_indicators(50, 49, 3);

    assert!(start <= 49 && 49 < end, "cursor 49 not in [{start}, {end})");
    let below_rows = below.is_some() as usize;
    assert!(end - start + below_rows <= 3);
}
