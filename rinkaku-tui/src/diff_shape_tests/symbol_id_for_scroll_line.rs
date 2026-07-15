//! `symbol_id_for_scroll_line` tests (ADR 0030): reverse-lookup from a
//! scroll offset back to whichever symbol section it falls inside.
//! Powers the diff → tree cursor auto-sync (`lib::sync_target_for_scroll`
//! from the caller side).

use super::*;

#[test]
fn should_return_none_for_scroll_line_when_content_is_empty() {
    let actual = symbol_id_for_scroll_line(&DiffPaneContent::Empty, 0, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_the_only_symbol_when_scroll_line_is_its_title_line() {
    // Only section: title(0), blank(1), header(2), body(3).
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);

    let actual = symbol_id_for_scroll_line(&content, 0, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_the_only_symbol_when_scroll_line_is_inside_its_hunk_body_not_just_its_title() {
    // Same layout as above; scroll_line 3 (the hunk body line, not the
    // title at 0) must still resolve to the same symbol — a section's
    // span covers every line inside it, not just its first.
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);

    let actual = symbol_id_for_scroll_line(&content, 3, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::foo"), actual);
}

#[test]
fn should_return_the_second_symbol_when_scroll_line_falls_inside_its_section() {
    // Section 0 (`foo`): title(0), blank(1), header(2), body(3) — 4
    // lines. Blank separator(4), section 1 (`bar`) title(5), blank(6),
    // header(7). scroll_line 5 (bar's own title) and 6/7 must all
    // resolve to `bar`, not `foo`.
    let content = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        },
        DiffSection {
            title: "fn bar()".to_string(),
            symbol_id: Some("lib.rs::bar".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                1,
                hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
            )],
        },
    ]);

    assert_eq!(
        Some("lib.rs::bar"),
        symbol_id_for_scroll_line(&content, 5, DiffViewMode::Unified)
    );
    assert_eq!(
        Some("lib.rs::bar"),
        symbol_id_for_scroll_line(&content, 7, DiffViewMode::Unified)
    );
    // The boundary immediately before section 1 still belongs to
    // section 0.
    assert_eq!(
        Some("lib.rs::foo"),
        symbol_id_for_scroll_line(&content, 4, DiffViewMode::Unified)
    );
}

#[test]
fn should_return_none_when_scroll_line_falls_inside_the_module_level_bucket() {
    // ADR 0030 decision 3: the module-level bucket has `symbol_id:
    // None` by construction, so a scroll line landing there must not
    // resolve to any symbol — not even the nearest one.
    let content = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        },
        DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![attributed(
                1,
                hunk("@@ -20,1 +20,2 @@", Some((20, 21)), vec!["use foo::bar;"]),
            )],
        },
    ]);

    // Module-level section starts at line 5 (same layout math as the
    // two-symbol test above).
    let actual = symbol_id_for_scroll_line(&content, 5, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_the_last_symbol_when_scroll_line_is_past_every_section_start_but_still_within_it()
{
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);

    // Line 100 is past the section's actual rendered content (4 lines
    // total) but there is no *next* section to bound it — the last
    // section's span is open-ended, matching an overscroll that is
    // about to be clamped by `crate::ui::clamp_scroll` next frame
    // rather than a position inside some other symbol.
    let actual = symbol_id_for_scroll_line(&content, 100, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::foo"), actual);
}
