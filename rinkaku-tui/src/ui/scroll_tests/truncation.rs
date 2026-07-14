use super::*;
use ratatui::style::{Color, Modifier, Style};

// --- truncate_to_width (pure helper, jump popup one-row-per-candidate fix) ---

#[test]
fn should_return_text_unchanged_when_it_fits_exactly_within_width() {
    let actual = truncate_to_width("abcde", 5);

    assert_eq!("abcde".to_string(), actual);
}

#[test]
fn should_return_text_unchanged_when_it_is_shorter_than_width() {
    let actual = truncate_to_width("abc", 10);

    assert_eq!("abc".to_string(), actual);
}

#[test]
fn should_return_empty_string_when_width_is_zero() {
    let actual = truncate_to_width("abcdef", 0);

    assert_eq!("".to_string(), actual);
}

#[test]
fn should_truncate_long_ascii_text_with_trailing_marker_when_it_overflows_width() {
    // width=5 -> 4 chars of budget + 1 column for the trailing "…".
    let actual = truncate_to_width("abcdefgh", 5);

    assert_eq!("abcd…".to_string(), actual);
}

#[test]
fn should_not_split_a_double_width_character_when_truncating() {
    // "あ" is 2 columns; a width-4 budget after reserving 1 column for
    // "…" leaves 3 columns, which fits "a" (1) + "あ" (2) exactly but
    // not a second "あ" (would need 5) — so the second "あ" and
    // everything after it is dropped rather than sliced in half.
    let actual = truncate_to_width("aああb", 4);

    assert_eq!("aあ…".to_string(), actual);
}

#[test]
fn should_truncate_mixed_cjk_and_ascii_label_when_it_overflows_width() {
    let actual = truncate_to_width("シンボル名 (path/to/very/long/file.rs)", 10);

    assert_eq!("シンボル…".to_string(), actual);
}

#[test]
fn should_return_only_the_marker_when_width_is_one() {
    // width=1 leaves a budget of 0 after reserving 1 column for "…", so
    // every character of the input is dropped and only the marker
    // itself remains — the narrowest width at which truncation still
    // produces non-empty output (width=0 is the separate empty-string
    // case covered above).
    let actual = truncate_to_width("abcdef", 1);

    assert_eq!("…".to_string(), actual);
}

#[test]
fn should_return_only_the_marker_when_width_is_one_and_first_char_is_double_width() {
    // Same width=1 boundary, but the first character of the input is a
    // 2-column CJK character that would not fit in the 0-column budget
    // either — makes sure the double-width guard and the width=1
    // budget-exhaustion guard compose correctly instead of one
    // masking a bug in the other.
    let actual = truncate_to_width("あいう", 1);

    assert_eq!("…".to_string(), actual);
}

// --- truncate_to_width_keeping_tail (pure helper, Diff pane header) ---

#[test]
fn should_return_text_unchanged_when_it_fits_within_width_keeping_tail() {
    let actual = truncate_to_width_keeping_tail("abcde", 5);

    assert_eq!("abcde".to_string(), actual);
}

#[test]
fn should_return_empty_string_when_width_is_zero_keeping_tail() {
    let actual = truncate_to_width_keeping_tail("abcdef", 0);

    assert_eq!("".to_string(), actual);
}

#[test]
fn should_truncate_long_ascii_text_with_leading_marker_when_it_overflows_width() {
    // width=5 -> 4 chars of budget + 1 column for the leading "…",
    // keeping the tail ("efgh") rather than the head.
    let actual = truncate_to_width_keeping_tail("abcdefgh", 5);

    assert_eq!("…efgh".to_string(), actual);
}

#[test]
fn should_not_split_a_double_width_character_when_truncating_keeping_tail() {
    let actual = truncate_to_width_keeping_tail("aああb", 4);

    assert_eq!("…あb".to_string(), actual);
}

#[test]
fn should_return_only_the_marker_when_width_is_one_keeping_tail() {
    let actual = truncate_to_width_keeping_tail("abcdef", 1);

    assert_eq!("…".to_string(), actual);
}

// --- truncate_line_to_width (styled, multi-span counterpart) ---

#[test]
fn should_return_line_unchanged_when_it_fits_within_width() {
    let line = Line::from(vec![Span::raw("ab"), Span::styled("cde", Style::default())]);

    let actual = truncate_line_to_width(&line, 5);

    assert_eq!(line, actual);
}

#[test]
fn should_truncate_single_span_line_with_trailing_marker_when_it_overflows_width() {
    let line = Line::raw("abcdefgh");

    let actual = truncate_line_to_width(&line, 5);

    assert_eq!(Line::raw("abcd…"), actual);
}

#[test]
fn should_preserve_each_surviving_spans_own_style_when_truncating_a_multi_span_line() {
    let red = Style::default().fg(Color::Red);
    let line = Line::from(vec![Span::raw("ab"), Span::styled("cdef", red)]);

    let actual = truncate_line_to_width(&line, 4);

    assert_eq!(
        Line::from(vec![Span::raw("ab"), Span::styled("c…", red)]),
        actual
    );
}

#[test]
fn should_preserve_line_level_selected_style_when_truncating_an_overflowing_line() {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let line = Line::from(vec![Span::raw("abcdefgh")]).style(selected_style);

    let actual = truncate_line_to_width(&line, 5);

    assert_eq!(
        Line::from(vec![Span::raw("abcd…")]).style(selected_style),
        actual
    );
}

#[test]
fn should_not_split_a_double_width_character_when_truncating_a_line() {
    let line = Line::raw("aああb");

    let actual = truncate_line_to_width(&line, 4);

    assert_eq!(Line::raw("aあ…"), actual);
}

#[test]
fn should_return_only_the_marker_line_when_width_is_one() {
    let line = Line::raw("abcdef");

    let actual = truncate_line_to_width(&line, 1);

    assert_eq!(Line::raw("…"), actual);
}

#[test]
fn should_return_only_the_marker_line_when_width_is_one_and_first_char_is_double_width() {
    let line = Line::raw("あいう");

    let actual = truncate_line_to_width(&line, 1);

    assert_eq!(Line::raw("…"), actual);
}

#[test]
fn should_return_empty_line_when_width_is_zero() {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let line = Line::from(vec![Span::raw("abcdef")]).style(selected_style);

    let actual = truncate_line_to_width(&line, 0);

    assert_eq!(Line::default().style(selected_style), actual);
}
