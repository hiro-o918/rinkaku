use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_none_for_symbol_start_when_content_is_empty() {
    let actual = section_start_line_for_symbol(
        &DiffPaneContent::Empty,
        "lib.rs::foo",
        DiffViewMode::Unified,
    );

    assert_eq!(None, actual);
}

#[test]
fn should_return_zero_for_symbol_start_when_content_has_a_single_matching_section() {
    // Only section: its title is at line 0, so the section starts at 0.
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);

    let actual = section_start_line_for_symbol(&content, "lib.rs::foo", DiffViewMode::Unified);

    assert_eq!(Some(0), actual);
}

#[test]
fn should_return_second_section_start_when_symbol_id_matches_the_second_section() {
    // Section 0 layout: title(0), blank(1), header(2), body(3) — 4 lines.
    // Blank separator between sections at line 4, so section 1 starts at 5.
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

    let actual = section_start_line_for_symbol(&content, "lib.rs::bar", DiffViewMode::Unified);

    assert_eq!(Some(5), actual);
}

#[test]
fn should_point_at_section_anchor_line_in_unified_view_when_section_has_a_contract_header() {
    // The section start is the anchor row(s) — the 2-line old/new
    // signature pair standing in for the title, *before* the hunks (ADR
    // 0027 decision 3: the reviewer wants the section's changed signature
    // first, not the hunks below it).
    // Section 0: 2 signature lines(0,1), blank before hunk(2), hunk
    // header(3), 1 body line(4) — 5 lines. Blank between sections at line
    // 5, section 1 title at line 6.
    let content = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo(a, b)".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a)".to_string(),
                signature: "fn foo(a, b)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
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

    let actual = section_start_line_for_symbol(&content, "lib.rs::bar", DiffViewMode::Unified);

    assert_eq!(Some(6), actual);
}

#[test]
fn should_point_at_single_anchor_row_in_split_view_when_section_has_a_contract_header() {
    // Split view always pairs the anchor onto exactly 1 row: signature
    // row(0), blank before hunk(1), hunk header(2), 1 body line(3) — 4
    // lines. Blank between sections at line 4, section 1 title at line 5
    // (one line earlier than the unified-view case above).
    let content = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo(a, b)".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a)".to_string(),
                signature: "fn foo(a, b)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
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

    let actual = section_start_line_for_symbol(&content, "lib.rs::bar", DiffViewMode::Split);

    assert_eq!(Some(5), actual);
}

#[test]
fn should_return_none_for_module_level_bucket_when_asked_by_any_symbol_id() {
    // The module-level bucket has `symbol_id: None`, so no real symbol id
    // lookup can accidentally match it. Even passing the literal
    // `MODULE_LEVEL_TITLE` as a symbol id (which is not a valid symbol id
    // shape but is the closest a caller could get to "aim at the bucket")
    // must not match.
    let content = DiffPaneContent::File(vec![DiffSection {
        title: MODULE_LEVEL_TITLE.to_string(),
        symbol_id: None,
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
        )],
    }]);

    let actual = section_start_line_for_symbol(&content, MODULE_LEVEL_TITLE, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_symbol_id_matches_no_section() {
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);

    let actual =
        section_start_line_for_symbol(&content, "lib.rs::nonexistent", DiffViewMode::Unified);

    assert_eq!(None, actual);
}
