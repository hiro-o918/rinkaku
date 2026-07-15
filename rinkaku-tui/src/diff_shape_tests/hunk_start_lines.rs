use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_hunk_starts_when_content_is_empty() {
    let actual = hunk_start_lines(&DiffPaneContent::Empty, DiffViewMode::Unified);

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_offset_first_hunk_start_by_title_and_blank_when_file_has_a_single_section_without_contract_header()
 {
    // ADR 0027 unified layout: the section title is always shown, and
    // every hunk (including the section's first) gets a blank line
    // before its header. So: title(0), blank(1), header(2) — the hunk
    // starts at line 2.
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk(
                "@@ -1,1 +1,2 @@",
                Some((1, 2)),
                vec!["fn a() {}", "fn foo() {}"],
            ),
        )],
    }]);

    let actual = hunk_start_lines(&content, DiffViewMode::Unified);

    assert_eq!(vec![2], actual);
}

#[test]
fn should_offset_hunk_start_by_contract_header_lines_in_unified_view_when_section_has_one() {
    // The contract header replaces the section's plain title (not an
    // addition to it): 2 signature lines (0, 1), blank before the hunk
    // (2), hunk header (3) — hunk starts at line 3.
    let content = DiffPaneContent::File(vec![DiffSection {
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
    }]);

    let actual = hunk_start_lines(&content, DiffViewMode::Unified);

    assert_eq!(vec![3], actual);
}

#[test]
fn should_offset_hunk_start_by_a_single_anchor_row_in_split_view_when_section_has_a_contract_header()
 {
    // Split view always pairs the anchor onto exactly 1 row regardless of
    // whether the signature changed: signature row(0), blank before the
    // hunk(1), hunk header(2) — hunk starts at line 2 (one line earlier
    // than the unified-view case above).
    let content = DiffPaneContent::File(vec![DiffSection {
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
    }]);

    let actual = hunk_start_lines(&content, DiffViewMode::Split);

    assert_eq!(vec![2], actual);
}

#[test]
fn should_offset_second_hunk_start_by_first_hunk_header_and_body_length() {
    // Section title(0), blank(1), first hunk header(2), 2 body lines(3,4),
    // blank before second hunk(5), second hunk header(6) — starts at 2, 6.
    let content = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![
            attributed(
                0,
                hunk(
                    "@@ -1,1 +1,2 @@",
                    Some((1, 2)),
                    vec!["fn a() {}", "fn b() {}"],
                ),
            ),
            attributed(
                1,
                hunk("@@ -10,1 +11,1 @@", Some((11, 11)), vec!["fn c() {}"]),
            ),
        ],
    }]);

    let actual = hunk_start_lines(&content, DiffViewMode::Unified);

    assert_eq!(vec![2, 6], actual);
}

#[test]
fn should_offset_hunk_start_by_section_header_and_separator_lines_for_a_file_selection() {
    // Section 0: title(0), blank(1), header(2), 1 body line(3) — hunk
    // starts at line 2. Section 1: blank separator between sections(4),
    // title(5), blank before its hunk(6), header(7) — hunk starts at 7.
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

    let actual = hunk_start_lines(&content, DiffViewMode::Unified);

    assert_eq!(vec![2, 7], actual);
}

#[test]
fn should_emit_one_hunk_jump_stop_per_section_when_a_hunk_is_shared_by_two_symbols() {
    // ADR 0029 consequence: a hunk attributed to more than one symbol
    // (an overlapping-range case) is rendered once per owning section,
    // so `]c`/`[c` (backed by this table) must stop once per rendered
    // occurrence — matching what is actually on screen — rather than
    // deduplicating by the shared `source_index` down to one stop.
    // Built straight from `build_diff_pane_content`'s real output
    // (not a hand-built `DiffPaneContent`) so this test exercises the
    // same shape the overlapping-hunk attribution test above produces.
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 5 }),
                symbol("lib.rs::bar", "bar", LineRange { start: 3, end: 8 }),
            ],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"])],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };
    let content = build_diff_pane_content(&report, &diff_files, Some(&target));

    let actual = hunk_start_lines(&content, DiffViewMode::Unified);

    // Section 0 (`foo`): title(0), blank(1), hunk header(2), 1 body
    // line(3) — 4 lines. Blank separator(4), section 1 (`bar`)
    // title(5), blank(6), hunk header(7) — stop at 7. Two stops for
    // the one underlying hunk, one per section it was duplicated into.
    assert_eq!(vec![2, 7], actual);
}
