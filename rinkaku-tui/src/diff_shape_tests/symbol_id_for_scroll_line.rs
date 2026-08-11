//! `symbol_id_for_scroll_line` tests (ADR 0030, ADR 0072): reverse-lookup
//! from a scroll offset back to whichever symbol's line range the hunk at
//! that offset intersects. Powers the diff -> tree cursor auto-sync
//! (`lib::sync_target_for_scroll` from the caller side).

use super::*;

fn foo_bar_symbols() -> Vec<(String, LineRange)> {
    vec![
        ("lib.rs::foo".to_string(), LineRange { start: 1, end: 2 }),
        ("lib.rs::bar".to_string(), LineRange { start: 10, end: 11 }),
    ]
}

#[test]
fn should_return_none_for_scroll_line_when_content_is_empty() {
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&DiffPaneContent::Empty, 0, &symbols);

    assert_eq!(None, actual);
}

#[test]
fn should_return_the_symbol_whose_range_intersects_the_hunk_at_the_header_row() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -1,1 +1,2 @@",
            Some((1, 2)),
            vec!["fn a() {}", "fn foo() {}"],
        ),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 0, &symbols);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_the_symbol_whose_range_intersects_the_hunk_at_a_body_row() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -1,1 +1,2 @@",
            Some((1, 2)),
            vec!["fn a() {}", "fn foo() {}"],
        ),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 1, &symbols);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_the_second_symbol_when_scroll_line_falls_inside_the_second_hunk() {
    // Hunk 0: header(0), 1 body line(1) — 2 lines. Blank(2), hunk 1
    // header(3), 1 body line(4).
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(
            1,
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ),
    ]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 4, &symbols);

    assert_eq!(Some("lib.rs::bar"), actual);
}

#[test]
fn should_return_none_when_scroll_line_falls_inside_a_hunk_intersecting_no_symbol() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -20,1 +20,2 @@", Some((20, 21)), vec!["use foo::bar;"]),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 1, &symbols);

    assert_eq!(None, actual);
}

#[test]
fn should_return_the_first_hunks_symbol_when_scroll_line_is_the_separator_before_the_next_hunk() {
    // Hunk 0: header(0), 1 body line(1) — 2 lines. Blank(2) is the last
    // line still owned by hunk 0 before hunk 1 starts at line 3 (sibling
    // of `should_return_the_second_symbol_when_scroll_line_falls_inside_the_second_hunk`,
    // which pins the boundary's other side).
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(
            1,
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ),
    ]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 2, &symbols);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_the_last_hunks_symbol_when_scroll_line_is_past_every_hunk() {
    // ADR 0030 decision 3 (carried over by ADR 0072): the last hunk's span
    // is open-ended — an overscroll about to be clamped by
    // `crate::ui::clamp_scroll` next frame still resolves to the last
    // hunk's own symbol rather than `None`.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 100, &symbols);

    assert_eq!(Some("lib.rs::foo"), actual);
}
