//! Tests pinning [`super::tidy_signature_lines`]: the transform that turns
//! a raw, range-stripped declaration slice into the multi-line text
//! actually stored on [`super::ExtractedSymbol::signature`] (ADR 0060) —
//! dedenting continuation lines relative to the node's real starting
//! column (`first_line_column`), trimming trailing whitespace per line,
//! and collapsing the blank-line runs a removed comment/body leaves
//! behind.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_unchanged_when_text_is_a_single_line() {
    let actual = tidy_signature_lines("fn foo(a: i32) -> i32", 0);

    assert_eq!("fn foo(a: i32) -> i32", actual);
}

#[test]
fn should_dedent_continuation_lines_relative_to_common_indent() {
    let raw = "struct Point {\n    x: i32,\n    y: i32,\n}";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!("struct Point {\n    x: i32,\n    y: i32,\n}", actual);
}

#[test]
fn should_collapse_blank_line_left_by_a_stripped_body() {
    // The body-removal hole after `def __init__(self, x, y):` leaves a
    // trailing blank line before the closing dedent — that residue must
    // not appear in the tidied signature.
    let raw = "class Point:\n    x: int\n    y: int\n\n    def __init__(self, x, y):\n";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!(
        "class Point:\n    x: int\n    y: int\n\n    def __init__(self, x, y):",
        actual
    );
}

#[test]
fn should_collapse_multiple_consecutive_blank_lines_into_one() {
    let raw = "class Foo:\n    a: int\n\n\n\n    def bar(self):\n";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!("class Foo:\n    a: int\n\n    def bar(self):", actual);
}

#[test]
fn should_trim_leading_and_trailing_blank_lines() {
    let raw = "\n\nstruct Foo {\n    a: i32,\n}\n\n";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!("struct Foo {\n    a: i32,\n}", actual);
}

#[test]
fn should_trim_trailing_whitespace_from_each_line() {
    let raw = "struct Foo {   \n    a: i32,  \n}";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!("struct Foo {\n    a: i32,\n}", actual);
}

#[test]
fn should_return_empty_string_when_text_is_only_whitespace() {
    let actual = tidy_signature_lines("   \n\n  \n", 0);

    assert_eq!("", actual);
}

#[test]
fn should_dedent_continuation_lines_relative_to_nesting_depth_when_first_line_column_is_nonzero() {
    // Mirrors what a nested definition's node text actually looks like:
    // tree-sitter's node span starts mid-line, so the first line (`fn
    // bar(`) carries none of the source's leading indentation in its own
    // stored text, while continuation lines still hold their absolute
    // source column (8 spaces for the params, 4 for the closing paren,
    // matching `fn`'s own column inside an `impl` block, passed here as
    // `first_line_column`). Folding that column into the dedent baseline
    // is what tells this apart from a top-level definition whose
    // continuation lines must NOT be dedented (see the sibling tests
    // above, all passing `first_line_column: 0`).
    let raw = "fn bar(\n        &self,\n        extra: i32,\n    ) -> i32";

    let actual = tidy_signature_lines(raw, 4);

    assert_eq!("fn bar(\n    &self,\n    extra: i32,\n) -> i32", actual);
}

#[test]
fn should_not_dedent_top_level_class_body_when_its_last_line_is_still_indented() {
    // Unlike a brace-delimited struct (whose closing `}` sits back at
    // column 0), a Python class's last kept line stays indented — every
    // non-blank continuation line here is at column 4, which must NOT be
    // read as "this nested definition needs 4 dedented off", since
    // `first_line_column` (0) already says this definition is top-level.
    let raw = "class Foo:\n    a: int\n    def bar(self):";

    let actual = tidy_signature_lines(raw, 0);

    assert_eq!("class Foo:\n    a: int\n    def bar(self):", actual);
}
