//! `symbol_id_for_scroll_line` tests (ADR 0030, ADR 0072, ADR 0074):
//! reverse-lookup from a scroll offset back to whichever symbol's line
//! range contains the *row* at that offset. Powers the diff -> tree cursor
//! auto-sync (`lib::sync_target_for_scroll` from the caller side).

use super::*;
use crate::app::DiffViewMode;

fn foo_bar_symbols() -> Vec<(String, LineRange)> {
    vec![
        ("lib.rs::foo".to_string(), LineRange { start: 1, end: 2 }),
        ("lib.rs::bar".to_string(), LineRange { start: 10, end: 11 }),
    ]
}

#[test]
fn should_return_none_for_scroll_line_when_content_is_empty() {
    let symbols = foo_bar_symbols();

    let actual =
        symbol_id_for_scroll_line(&DiffPaneContent::Empty, 0, &symbols, DiffViewMode::Unified);

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

    let actual = symbol_id_for_scroll_line(&content, 0, &symbols, DiffViewMode::Unified);

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

    let actual = symbol_id_for_scroll_line(&content, 1, &symbols, DiffViewMode::Unified);

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

    let actual = symbol_id_for_scroll_line(&content, 4, &symbols, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::bar"), actual);
}

#[test]
fn should_return_none_when_the_row_at_the_scroll_line_belongs_to_no_symbol() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -20,1 +20,2 @@", Some((20, 21)), vec!["use foo::bar;"]),
    )]);
    let symbols = foo_bar_symbols();

    let actual = symbol_id_for_scroll_line(&content, 1, &symbols, DiffViewMode::Unified);

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

    let actual = symbol_id_for_scroll_line(&content, 2, &symbols, DiffViewMode::Unified);

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

    let actual = symbol_id_for_scroll_line(&content, 100, &symbols, DiffViewMode::Unified);

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
        .map(|line| symbol_id_for_scroll_line(&content, line, &symbols, DiffViewMode::Unified))
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
            let target = scroll_target_line_for_symbol(&content, *range, DiffViewMode::Unified)
                .expect("every symbol here has a covering row");
            symbol_id_for_scroll_line(&content, target, &symbols, DiffViewMode::Unified)
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

#[test]
fn should_resolve_split_view_rows_by_where_each_symbol_actually_renders() {
    // The mirror image of
    // `scroll_target_line::should_target_the_row_a_symbol_actually_renders_on_in_split_view`:
    // in split view body row 3 shows `gamma`'s own `-`/`+` pair, so parking
    // the scroll there must resolve to `gamma` — under the unified-order
    // walk it resolves to `alpha`, whose `-` line is the third source line.
    use crate::diff_view::{DiffLine, DiffLineKind};

    let replaced = |content: &str, kind| DiffLine {
        kind,
        content: content.to_string(),
    };
    let content = DiffPaneContent::File(vec![attributed(
        0,
        Hunk {
            header: "@@ -10,3 +10,3 @@".to_string(),
            new_range: Some((10, 12)),
            lines: vec![
                replaced("fn alpha(x: u32) {}", DiffLineKind::Removed),
                replaced("fn beta(x: u32) {}", DiffLineKind::Removed),
                replaced("fn gamma(x: u32) {}", DiffLineKind::Removed),
                replaced("fn alpha(x: u64) {}", DiffLineKind::Added),
                replaced("fn beta(x: u64) {}", DiffLineKind::Added),
                replaced("fn gamma(x: u64) {}", DiffLineKind::Added),
            ],
        },
    )]);
    let symbols = vec![
        (
            "lib.rs::alpha".to_string(),
            LineRange { start: 10, end: 10 },
        ),
        ("lib.rs::beta".to_string(), LineRange { start: 11, end: 11 }),
        (
            "lib.rs::gamma".to_string(),
            LineRange { start: 12, end: 12 },
        ),
    ];

    let actual: Vec<Option<&str>> = (1..=3)
        .map(|line| symbol_id_for_scroll_line(&content, line, &symbols, DiffViewMode::Split))
        .collect();

    assert_eq!(
        vec![
            Some("lib.rs::alpha"),
            Some("lib.rs::beta"),
            Some("lib.rs::gamma"),
        ],
        actual
    );
}

#[test]
fn should_walk_back_to_the_previous_hunk_when_the_hunk_at_the_scroll_line_has_no_readable_header() {
    // A hunk whose header this parser could not read carries no coordinate
    // on any of its rows, so a scroll position inside it resolves to the
    // nearest preceding row that has one — the previous hunk's symbol —
    // rather than to `None`. Documented on `symbol_id_for_scroll_line`
    // alongside the separator case, but only the separator case was pinned.
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(1, hunk("@@ garbled @@", None, vec!["fn bar() {}"])),
    ]);
    let symbols = foo_bar_symbols();

    // Rows: header(0), body(1), separator(2), header(3), body(4).
    let actual = symbol_id_for_scroll_line(&content, 4, &symbols, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::foo"), actual);
}
