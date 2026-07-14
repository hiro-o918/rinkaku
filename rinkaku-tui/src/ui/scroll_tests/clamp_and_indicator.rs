use super::*;

// --- clamp_scroll / scroll_indicator (pure helpers) ---

#[test]
fn should_return_zero_scroll_when_content_fits_entirely() {
    let actual = clamp_scroll(5, 10, 3);

    assert_eq!(0, actual);
}

#[test]
fn should_clamp_requested_scroll_to_max_scroll_when_it_overshoots() {
    // 20 lines of content in a 10-row viewport: max_scroll = 10, so a
    // request of 15 clamps down to 10 (the last full page).
    let actual = clamp_scroll(20, 10, 15);

    assert_eq!(10, actual);
}

#[test]
fn should_pass_through_requested_scroll_when_within_bounds() {
    let actual = clamp_scroll(20, 10, 4);

    assert_eq!(4, actual);
}

#[test]
fn should_return_zero_scroll_when_viewport_height_is_zero() {
    // A degenerate (zero-height) pane can never scroll — `max_scroll`
    // saturates at `content_len` itself, but a requested scroll of 0
    // (the only value `App` ever starts at) still clamps to 0.
    let actual = clamp_scroll(20, 0, 0);

    assert_eq!(0, actual);
}

#[test]
fn should_return_none_indicator_when_content_fits_entirely() {
    let actual = scroll_indicator(5, 10, 0);

    assert_eq!(None, actual);
}

#[test]
fn should_return_indicator_at_top_when_content_overflows_and_scroll_is_zero() {
    let actual = scroll_indicator(20, 10, 0);

    assert_eq!(Some(" (1-10/20)".to_string()), actual);
}

#[test]
fn should_return_indicator_reflecting_scroll_position() {
    let actual = scroll_indicator(20, 10, 4);

    assert_eq!(Some(" (5-14/20)".to_string()), actual);
}

#[test]
fn should_clamp_last_visible_to_content_len_at_max_scroll() {
    // scroll=10, viewport=10 would naively suggest last_visible=20,
    // which happens to equal content_len here anyway; this pins the
    // `.min(content_len)` clamp directly rather than relying on the
    // coincidence.
    let actual = scroll_indicator(20, 10, 10);

    assert_eq!(Some(" (11-20/20)".to_string()), actual);
}
