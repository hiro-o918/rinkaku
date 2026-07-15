use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_when_target_is_none() {
    let report = empty_report();

    let actual = build_diff_pane_content(&report, &[], None);

    assert_eq!(DiffPaneContent::Empty, actual);
}

#[test]
fn should_group_file_selection_hunks_under_per_symbol_sections() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 }),
                symbol("lib.rs::bar", "bar", LineRange { start: 10, end: 11 }),
            ],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![
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
    assert_eq!(expected, actual);
}

#[test]
fn should_attribute_pure_deletion_hunk_to_owning_symbol_instead_of_module_level() {
    // Finding-2 regression: `hunk_intersects` always returning `false`
    // for a pure-deletion hunk meant `build_file_content`'s owner lookup
    // (`symbols.iter().position(...)`) never matched, so every deletion
    // hunk landed in the `MODULE_LEVEL_TITLE` bucket regardless of which
    // symbol's body it actually came from.
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 4 })],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![hunk(
            "@@ -4 +3,0 @@",
            Some((3, 2)),
            vec!["println!(\"removed\");"],
        )],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk(
                "@@ -4 +3,0 @@",
                Some((3, 2)),
                vec!["println!(\"removed\");"],
            ),
        )],
    }]);
    assert_eq!(expected, actual);
}

#[test]
fn should_bucket_hunk_under_module_level_when_it_intersects_no_symbol() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol(
                "lib.rs::foo",
                "foo",
                LineRange { start: 10, end: 11 },
            )],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn foo() {}"]),
        ],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                1,
                hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn foo() {}"]),
            )],
        },
        DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
            )],
        },
    ]);
    assert_eq!(expected, actual);
}

#[test]
fn should_omit_module_level_section_when_every_hunk_is_attributed_to_a_symbol() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"])],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        )],
    }]);
    assert_eq!(expected, actual);
}

#[test]
fn should_split_overlapping_hunk_into_a_sub_hunk_per_symbol_it_intersects() {
    // Two symbols with adjacent, overlapping ranges (a pathological
    // input a real extractor would not normally produce, but the
    // shaping function's contract must still resolve deterministically).
    // ADR 0029 established "attribute to every intersecting symbol, not
    // just the first"; ADR 0053 amends the attribution step itself to
    // split the hunk into a per-symbol sub-hunk instead of cloning the
    // whole hunk into every owning section. Here both symbols' ranges
    // genuinely overlap at the one shared line, so both sub-hunks still
    // carry that same line (ADR 0053's own doc comment on
    // `hunk_split::split_hunk`) — this is the one case a split does not
    // eliminate duplication, because the line truly belongs to both.
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

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected_hunk = AttributedHunk {
        source_index: 0,
        hunk: hunk("@@ -1,1 +3,1 @@", Some((3, 3)), vec!["shared line"]),
        origin_offset: 0,
    };
    let expected = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![expected_hunk.clone()],
        },
        DiffSection {
            title: "fn bar()".to_string(),
            symbol_id: Some("lib.rs::bar".to_string()),
            contract_header: None,
            hunks: vec![expected_hunk],
        },
    ]);
    assert_eq!(expected, actual);
}

#[test]
fn should_attribute_new_file_single_hunk_to_every_symbol_it_defines() {
    // Regression test (PR #86 dogfooding, ADR 0029, amended by ADR 0053):
    // a brand-new file's diff is always exactly one hunk spanning the
    // whole file (`@@ -0,0 +1,N @@`), so every symbol the file defines
    // has a range inside that one hunk. Before ADR 0029, only the first
    // symbol in source order (`foo`) ever got a section; `bar` and
    // `baz` were silently dropped, breaking their diff-pane auto-scroll
    // (ADR 0027 decision 2) with no error or indicator. ADR 0053 further
    // amends the attribution step to split this one hunk into a sub-hunk
    // per symbol (plus a module-level sub-hunk for the blank separator
    // lines between them) instead of cloning the whole hunk into every
    // section.
    let report = Report {
        files: vec![FileReport {
            path: "file_size.rs".to_string(),
            symbols: vec![
                symbol("file_size.rs::foo", "foo", LineRange { start: 1, end: 3 }),
                symbol("file_size.rs::bar", "bar", LineRange { start: 5, end: 7 }),
                symbol("file_size.rs::baz", "baz", LineRange { start: 9, end: 11 }),
            ],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "file_size.rs".to_string(),
        hunks: vec![hunk(
            "@@ -0,0 +1,11 @@",
            Some((1, 11)),
            vec![
                "fn foo() {",
                "    body();",
                "}",
                "",
                "fn bar() {",
                "    body();",
                "}",
                "",
                "fn baz() {",
                "    body();",
                "}",
            ],
        )],
    }];
    let target = DiffTarget::File {
        path: "file_size.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![
        DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("file_size.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![AttributedHunk {
                source_index: 0,
                hunk: hunk(
                    "@@ -0,3 +1,3 @@",
                    Some((1, 3)),
                    vec!["fn foo() {", "    body();", "}"],
                ),
                origin_offset: 0,
            }],
        },
        DiffSection {
            title: "fn bar()".to_string(),
            symbol_id: Some("file_size.rs::bar".to_string()),
            contract_header: None,
            hunks: vec![AttributedHunk {
                source_index: 0,
                hunk: hunk(
                    "@@ -4,3 +5,3 @@",
                    Some((5, 7)),
                    vec!["fn bar() {", "    body();", "}"],
                ),
                origin_offset: 4,
            }],
        },
        DiffSection {
            title: "fn baz()".to_string(),
            symbol_id: Some("file_size.rs::baz".to_string()),
            contract_header: None,
            hunks: vec![AttributedHunk {
                source_index: 0,
                hunk: hunk(
                    "@@ -8,3 +9,3 @@",
                    Some((9, 11)),
                    vec!["fn baz() {", "    body();", "}"],
                ),
                origin_offset: 8,
            }],
        },
        DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![
                AttributedHunk {
                    source_index: 0,
                    hunk: hunk("@@ -3,1 +4,1 @@", Some((4, 4)), vec![""]),
                    origin_offset: 3,
                },
                AttributedHunk {
                    source_index: 0,
                    hunk: hunk("@@ -7,1 +8,1 @@", Some((8, 8)), vec![""]),
                    origin_offset: 7,
                },
            ],
        },
    ]);
    assert_eq!(expected, actual);

    // Every symbol now resolves an auto-scroll target (ADR 0027
    // decision 2 / decision 4) — not just the first.
    assert_eq!(
        Some(0),
        section_start_line_for_symbol(&actual, "file_size.rs::foo", DiffViewMode::Unified)
    );
    assert!(
        section_start_line_for_symbol(&actual, "file_size.rs::bar", DiffViewMode::Unified)
            .is_some()
    );
    assert!(
        section_start_line_for_symbol(&actual, "file_size.rs::baz", DiffViewMode::Unified)
            .is_some()
    );
}

#[test]
fn should_include_contract_header_on_the_owning_section_in_a_file_selection() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::SignatureChanged),
                previous_signature: Some("fn foo(a: i32)".to_string()),
                signature: "fn foo(a: i32, b: i32)".to_string(),
                ..symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })
            }],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![hunk(
            "@@ -1,1 +1,2 @@",
            Some((1, 2)),
            vec!["fn foo(a, b) {}"],
        )],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![DiffSection {
        title: "fn foo(a: i32, b: i32)".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: Some(ContractHeader {
            previous_signature: "fn foo(a: i32)".to_string(),
            signature: "fn foo(a: i32, b: i32)".to_string(),
        }),
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
        )],
    }]);
    assert_eq!(expected, actual);
}

// Regression test (post-rebase integration check, PR #58): a skipped or
// whole-test-file row (ADR: "show skipped and test-only files in the
// entry tree") has no `FileReport` at all in `report.files`, so
// `build_file_content`'s `symbols` lookup falls back to `&[]` — every
// hunk must still land somewhere rather than being silently dropped or
// panicking on an out-of-bounds `sections` index.
#[test]
fn should_bucket_every_hunk_under_module_level_when_file_selection_has_no_symbols_at_all() {
    // `report.files` has no entry for "assets/logo.png" at all — the
    // exact shape of a skipped/whole-test-file row, which is tracked in
    // `report.skipped`/`report.tests` instead of `report.files`.
    let report = empty_report();
    let diff_files = vec![FileHunks {
        path: "assets/logo.png".to_string(),
        hunks: vec![hunk(
            "@@ -1,1 +1,2 @@",
            Some((1, 2)),
            vec!["binary blob line"],
        )],
    }];
    let target = DiffTarget::File {
        path: "assets/logo.png".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![DiffSection {
        title: MODULE_LEVEL_TITLE.to_string(),
        symbol_id: None,
        contract_header: None,
        hunks: vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["binary blob line"]),
        )],
    }]);
    assert_eq!(expected, actual);
}

#[test]
fn should_return_empty_when_file_has_no_hunks_at_all() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
        }],
        ..empty_report()
    };
    let diff_files = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    assert_eq!(DiffPaneContent::Empty, actual);
}

#[test]
fn should_return_empty_when_diff_has_no_entry_for_the_selected_file() {
    let report = Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
        }],
        ..empty_report()
    };
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &[], Some(&target));

    assert_eq!(DiffPaneContent::Empty, actual);
}

// Regression test (post-rebase integration check, PR #58): a binary
// skipped file has a `FileHunks` entry (git still reports the path
// touched a diff) but zero `@@` hunks in it ("Binary files ... differ"
// has no hunk syntax for `crate::diff_view::parse_diff_hunks` to parse)
// and no `FileReport`/symbols at all — the pane must degrade to `Empty`
// (rendered by `crate::ui::draw_diff_pane` as its own placeholder text)
// rather than panicking or fabricating a module-level section with no
// hunks in it.
#[test]
fn should_return_empty_when_skipped_file_has_no_symbols_and_no_hunks() {
    let report = empty_report();
    let diff_files = vec![FileHunks {
        path: "assets/logo.png".to_string(),
        hunks: vec![],
    }];
    let target = DiffTarget::File {
        path: "assets/logo.png".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    assert_eq!(DiffPaneContent::Empty, actual);
}
