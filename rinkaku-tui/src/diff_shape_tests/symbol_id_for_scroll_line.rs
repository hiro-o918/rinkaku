//! `symbol_id_for_scroll_line` tests (ADR 0030, ADR 0072, ADR 0074):
//! reverse-lookup from a scroll offset back to whichever symbol's line
//! range contains the *row* at that offset. Powers the diff -> tree cursor
//! auto-sync (`lib::sync_target_for_scroll` from the caller side).

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
fn should_return_the_symbol_covering_the_hunk_header_row() {
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
fn should_return_the_symbol_covering_the_body_row_at_the_scroll_line() {
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
fn should_return_none_when_the_row_at_the_scroll_line_belongs_to_no_symbol() {
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
    // ADR 0030 decision 3 (carried over by ADR 0072/0074): an overscroll
    // about to be clamped by `crate::ui::clamp_scroll` next frame clamps
    // to the last rendered row and still resolves to that row's own
    // symbol rather than `None`.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 100, &symbols);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_each_symbol_in_turn_for_rows_of_one_hunk_covering_several_symbols() {
    // ADR 0074's regression pin, the mirror image of
    // `scroll_target_line::should_return_the_symbols_own_row_when_it_starts_inside_a_hunk`:
    // scrolling down through one hunk that covers several symbols must
    // hand the tree cursor each of them in turn, not pin it to whichever
    // symbol happens to come first in source order.
    // Rows: header(0) at line 1, then new-side lines 1..=4 at rows 1..=4.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -0,0 +1,4 @@",
            Some((1, 4)),
            vec!["fn foo() {}", "", "fn bar() {}", ""],
        ),
    )]);
    let symbols = vec![
        ("lib.rs::foo".to_string(), LineRange { start: 1, end: 2 }),
        ("lib.rs::bar".to_string(), LineRange { start: 3, end: 4 }),
    ];

    let actual: Vec<Option<&str>> = (0..=4)
        .map(|line| symbol_id_for_scroll_line(&content, line, &symbols))
        .collect();

    assert_eq!(
        vec![
            Some("lib.rs::foo"),
            Some("lib.rs::foo"),
            Some("lib.rs::foo"),
            Some("lib.rs::bar"),
            Some("lib.rs::bar"),
        ],
        actual
    );
}

#[test]
fn should_round_trip_the_scroll_target_of_every_symbol_under_one_hunk() {
    // The two directions must agree: whatever row a symbol's selection
    // scrolls to must resolve back to that same symbol, or the tree cursor
    // and the diff pane disagree about what is on screen the moment the
    // reviewer touches either one.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -0,0 +1,6 @@",
            Some((1, 6)),
            vec!["fn foo() {}", "", "fn bar() {}", "", "fn baz() {}", ""],
        ),
    )]);
    let symbols = vec![
        ("lib.rs::foo".to_string(), LineRange { start: 1, end: 2 }),
        ("lib.rs::bar".to_string(), LineRange { start: 3, end: 4 }),
        ("lib.rs::baz".to_string(), LineRange { start: 5, end: 6 }),
    ];

    let actual: Vec<Option<&str>> = symbols
        .iter()
        .map(|(_, range)| {
            let target = scroll_target_line_for_symbol(&content, *range)
                .expect("every symbol here has a covering row");
            symbol_id_for_scroll_line(&content, target, &symbols)
        })
        .collect();

    assert_eq!(
        vec![
            Some("lib.rs::foo"),
            Some("lib.rs::bar"),
            Some("lib.rs::baz"),
        ],
        actual
    );
}
