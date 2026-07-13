use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_none_when_file_path_is_not_found() {
    let report = empty_report();
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "missing.rs");

    assert_eq!(None, actual);
}

#[test]
fn should_build_file_detail_with_symbol_summaries_and_fan_in() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    classification: Some(Classification::Added),
                    ..symbol("lib.rs::foo", "foo")
                },
                symbol("lib.rs::bar", "bar"),
            ],
        }],
        graph: SymbolGraph {
            nodes: vec![
                node("lib.rs::foo", "lib.rs", "foo"),
                node("lib.rs::bar", "lib.rs", "bar"),
            ],
            edges: vec![],
            roots: vec![],
        },
        fan_ins: vec![FanIn {
            id: "lib.rs::bar".to_string(),
            path: "lib.rs".to_string(),
            name: "bar".to_string(),
            used_by: vec!["foo".to_string(), "baz".to_string()],
        }],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "lib.rs").expect("file found");

    let expected = FileDetail {
        path: "lib.rs".to_string(),
        symbols: vec![
            FileSymbolSummary {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                classification: Some(Classification::Added),
                removed: false,
                fan_in: 0,
            },
            FileSymbolSummary {
                name: "bar".to_string(),
                kind: SymbolKind::Function,
                classification: None,
                removed: false,
                fan_in: 2,
            },
        ],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: None,
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_include_removed_symbol_in_file_detail_summary() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        removed: vec![rinkaku_core::extract::RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "lib.rs").expect("file found");

    let expected = FileDetail {
        path: "lib.rs".to_string(),
        symbols: vec![FileSymbolSummary {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: true,
            fan_in: 0,
        }],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: None,
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_carry_skip_reason_into_file_detail_when_file_row_is_skipped() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![rinkaku_core::render::SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "assets/logo.png").expect("file found");

    let expected = FileDetail {
        path: "assets/logo.png".to_string(),
        symbols: vec![],
        skip_reason: Some(rinkaku_core::render::SkipReason::Binary),
        test_symbol_count: None,
        size_warning: None,
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_carry_test_symbol_count_into_file_detail_when_file_row_is_a_whole_test_file() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        tests: vec![rinkaku_core::render::TestFileSummary {
            path: "src/lib_test.go".to_string(),
            symbol_count: 4,
        }],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "src/lib_test.go").expect("file found");

    let expected = FileDetail {
        path: "src/lib_test.go".to_string(),
        symbols: vec![],
        skip_reason: None,
        test_symbol_count: Some(4),
        size_warning: None,
    };
    assert_eq!(expected, actual);
}

// Regression test (post-rebase integration check): a mixed file — real
// (non-test) symbols in `report.files` *and* a test-symbol count in
// `report.tests` for the same path, which `pipeline::partition_test_symbols`
// legitimately produces for a file with both production and
// `#[cfg(test)]`-style code changed in one diff — must keep both halves
// on the built `FileDetail` rather than one silently dropping the other.
// This is exactly the shape that caused a live panic when running the
// TUI against this repo's own diff (`rinkaku-tui/src/app.rs` has both
// real and test symbols changed), before `TreeBuilder::insert_test_file`
// stopped asserting the file's `symbols` were empty.
#[test]
fn should_keep_real_symbols_alongside_test_symbol_count_when_file_is_mixed() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "app.rs".to_string(),
            symbols: vec![symbol("app.rs::handle_key", "handle_key")],
        }],
        tests: vec![rinkaku_core::render::TestFileSummary {
            path: "app.rs".to_string(),
            symbol_count: 5,
        }],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "app.rs").expect("file found");

    let expected = FileDetail {
        path: "app.rs".to_string(),
        symbols: vec![FileSymbolSummary {
            name: "handle_key".to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: false,
            fan_in: 0,
        }],
        skip_reason: None,
        test_symbol_count: Some(5),
        size_warning: None,
    };
    assert_eq!(expected, actual);
}

// ADR 0028: a file whose path shows up in `report.file_size_warnings`
// must carry the matching warning onto its `FileDetail` so the detail
// pane can render the "1734 lines — consider splitting" hint above
// the symbols listing without re-scanning the report itself.
#[test]
fn should_populate_size_warning_on_file_detail_when_report_has_warning_for_that_path() {
    let warning = rinkaku_core::file_size::FileSizeWarning {
        path: "src/big.rs".to_string(),
        line_count: 1734,
        severity: rinkaku_core::file_size::FileSizeSeverity::Warn,
    };
    let report = Report {
        files: vec![FileReport {
            path: "src/big.rs".to_string(),
            symbols: vec![symbol("src/big.rs::foo", "foo")],
        }],
        file_size_warnings: vec![warning.clone()],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_file_detail(&tree, &report, "src/big.rs").expect("file found");

    let expected = FileDetail {
        path: "src/big.rs".to_string(),
        symbols: vec![FileSymbolSummary {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: false,
            fan_in: 0,
        }],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: Some(warning),
    };
    assert_eq!(expected, actual);
}
