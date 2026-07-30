//! Tests pinning [`super::normalize_for_comparison`]: the whitespace
//! normalization `classify_symbols` uses to compare two signatures (ADR
//! 0014/0060). Unlike the unconditional single-line collapsing used at
//! display sites (`render::markdown`'s `collapse_to_single_line` and its
//! `rinkaku-tui` counterpart, both unaffected by this change), this
//! variant must not introduce a space at a position the source never had
//! one: a reflow that inserts a newline right after an opening `(` (no
//! space in the original) must normalize identically to the un-reflowed
//! source, or `classify_symbols` reports a false `SignatureChanged`.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_drop_whitespace_run_when_adjacent_to_a_symbol_character() {
    let actual = normalize_for_comparison("func F(\n\talpha string,\n) string");

    assert_eq!("func F(alpha string,)string", actual);
}

#[test]
fn should_collapse_whitespace_run_to_single_space_when_both_sides_are_word_characters() {
    let actual = normalize_for_comparison("a\n   b");

    assert_eq!("a b", actual);
}

#[test]
fn should_treat_underscore_as_a_word_character() {
    let actual = normalize_for_comparison("foo_bar\nbaz_qux");

    assert_eq!("foo_bar baz_qux", actual);
}

#[test]
fn should_normalize_identically_when_reflow_only_moves_a_comma_onto_its_own_line() {
    let with_trailing_comma_on_own_line = normalize_for_comparison("a,\nb");
    let on_one_line = normalize_for_comparison("a, b");

    assert_eq!(with_trailing_comma_on_own_line, on_one_line);
}

#[test]
fn should_distinguish_signatures_that_differ_only_by_a_word_boundary_space() {
    // `F(a b string)` and `F(ab string)` must never normalize to the same
    // string — the space between `a` and `b` is meaningful content, not
    // reflow, so collapsing/removing it here would hide an actual
    // signature change from `classify_symbols`.
    let with_space = normalize_for_comparison("func F(a b string)");
    let without_space = normalize_for_comparison("func F(ab string)");

    assert_eq!("func F(a b string)", with_space);
    assert_eq!("func F(ab string)", without_space);
}
