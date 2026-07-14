use super::*;
use ratatui::style::{Color, Style};

// --- wrap_lines (pure helper) ---

#[test]
fn should_return_lines_unchanged_when_width_is_zero() {
    let lines = vec![Line::raw("hello world")];

    let actual = wrap_lines(&lines, 0);

    assert_eq!(lines, actual);
}

#[test]
fn should_return_one_empty_line_when_input_line_is_blank() {
    let lines = vec![Line::raw("")];

    let actual = wrap_lines(&lines, 10);

    assert_eq!(vec![Line::raw("")], actual);
}

#[test]
fn should_not_wrap_when_line_fits_exactly_within_width() {
    let lines = vec![Line::raw("abcde")];

    let actual = wrap_lines(&lines, 5);

    assert_eq!(vec![Line::raw("abcde")], actual);
}

#[test]
fn should_split_long_ascii_line_into_multiple_lines_at_the_width_boundary() {
    let lines = vec![Line::raw("abcdefghij")];

    let actual = wrap_lines(&lines, 4);

    assert_eq!(
        vec![Line::raw("abcd"), Line::raw("efgh"), Line::raw("ij"),],
        actual
    );
}

#[test]
fn should_wrap_full_width_characters_without_splitting_a_double_width_char_across_lines() {
    // Each "あ" is 2 columns wide; a width-3 pane can fit "あ" (2) plus
    // one more column, but the second "あ" would overflow to column 4,
    // so it wraps onto the next line rather than being sliced in half.
    let lines = vec![Line::raw("ああa")];

    let actual = wrap_lines(&lines, 3);

    assert_eq!(vec![Line::raw("あ"), Line::raw("あa")], actual);
}

#[test]
fn should_preserve_span_style_on_both_fragments_when_a_styled_span_is_split_by_wrapping() {
    let style = Style::default().fg(Color::Red);
    let lines = vec![Line::from(vec![Span::styled("abcdef", style)])];

    let actual = wrap_lines(&lines, 4);

    assert_eq!(
        vec![
            Line::from(vec![Span::styled("abcd", style)]),
            Line::from(vec![Span::styled("ef", style)]),
        ],
        actual
    );
}

#[test]
fn should_preserve_distinct_span_styles_when_a_multi_span_line_wraps_across_span_boundaries() {
    // "ab" (unstyled) + "cdef" (red): a width-3 wrap must split after
    // "abc" (2 unstyled chars + 1 red char) and carry each fragment's
    // own style into the split, not just the first span's.
    let red = Style::default().fg(Color::Red);
    let lines = vec![Line::from(vec![Span::raw("ab"), Span::styled("cdef", red)])];

    let actual = wrap_lines(&lines, 3);

    assert_eq!(
        vec![
            Line::from(vec![Span::raw("ab"), Span::styled("c", red)]),
            Line::from(vec![Span::styled("def", red)]),
        ],
        actual
    );
}

#[test]
fn should_wrap_each_logical_line_independently_when_multiple_lines_are_passed() {
    let lines = vec![Line::raw("abcdef"), Line::raw("xy")];

    let actual = wrap_lines(&lines, 4);

    assert_eq!(
        vec![Line::raw("abcd"), Line::raw("ef"), Line::raw("xy")],
        actual
    );
}
