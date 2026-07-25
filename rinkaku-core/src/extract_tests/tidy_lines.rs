//! Tests pinning [`super::tidy_signature_lines`]: the transform that turns
//! a raw, range-stripped declaration slice into the multi-line text
//! actually stored on [`super::ExtractedSymbol::signature`] (ADR 0060) —
//! dedenting relative to the first line, trimming trailing whitespace per
//! line, and collapsing the blank-line runs a removed comment/body leaves
//! behind.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_unchanged_when_text_is_a_single_line() {
    let actual = tidy_signature_lines("fn foo(a: i32) -> i32");

    assert_eq!("fn foo(a: i32) -> i32", actual);
}

#[test]
fn should_dedent_continuation_lines_relative_to_common_indent() {
    let raw = "struct Point {\n    x: i32,\n    y: i32,\n}";

    let actual = tidy_signature_lines(raw);

    assert_eq!("struct Point {\n    x: i32,\n    y: i32,\n}", actual);
}

#[test]
fn should_collapse_blank_line_left_by_a_stripped_body() {
    // The body-removal hole after `def __init__(self, x, y):` leaves a
    // trailing blank line before the closing dedent — that residue must
    // not appear in the tidied signature.
    let raw = "class Point:\n    x: int\n    y: int\n\n    def __init__(self, x, y):\n";

    let actual = tidy_signature_lines(raw);

    assert_eq!(
        "class Point:\n    x: int\n    y: int\n\n    def __init__(self, x, y):",
        actual
    );
}

#[test]
fn should_collapse_multiple_consecutive_blank_lines_into_one() {
    let raw = "class Foo:\n    a: int\n\n\n\n    def bar(self):\n";

    let actual = tidy_signature_lines(raw);

    assert_eq!("class Foo:\n    a: int\n\n    def bar(self):", actual);
}

#[test]
fn should_trim_leading_and_trailing_blank_lines() {
    let raw = "\n\nstruct Foo {\n    a: i32,\n}\n\n";

    let actual = tidy_signature_lines(raw);

    assert_eq!("struct Foo {\n    a: i32,\n}", actual);
}

#[test]
fn should_trim_trailing_whitespace_from_each_line() {
    let raw = "struct Foo {   \n    a: i32,  \n}";

    let actual = tidy_signature_lines(raw);

    assert_eq!("struct Foo {\n    a: i32,\n}", actual);
}

#[test]
fn should_return_empty_string_when_text_is_only_whitespace() {
    let actual = tidy_signature_lines("   \n\n  \n");

    assert_eq!("", actual);
}
