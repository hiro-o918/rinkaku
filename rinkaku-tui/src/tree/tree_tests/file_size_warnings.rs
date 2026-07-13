use super::*;
use pretty_assertions::assert_eq;

// File-size warning badge tests (ADR 0028): a file rinkaku measured as
// oversized must carry its own severity + line count on the file node,
// and its containing directories must aggregate the count split by
// severity so `row_view` can render the pair (`warn:N split:M`).

#[test]
fn should_populate_own_file_line_count_when_report_has_warning_for_that_path() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/big.rs".to_string(),
            symbols: vec![],
        }],
        file_size_warnings: vec![rinkaku_core::file_size::FileSizeWarning {
            path: "src/big.rs".to_string(),
            line_count: 1734,
            severity: FileSizeSeverity::Warn,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Root is the "src" Dir; file node is its only child.
    let file_node = &tree.roots[0].children[0];
    let expected = Badges {
        own_file_size_severity: Some(FileSizeSeverity::Warn),
        own_file_line_count: Some(1734),
        file_size_warn_count: 1,
        file_size_split_count: 0,
        ..Badges::default()
    };
    assert_eq!(expected, file_node.badges);
}

#[test]
fn should_aggregate_warn_and_split_counts_separately_on_parent_dir() {
    // src/warn.rs = Warn, src/split.rs = Split, src/ok.rs = under
    // threshold. The parent "src" dir must show warn_count=1,
    // split_count=1 — kept split by severity so the row can render
    // `warn:1 split:1` rather than merging into one meaningless total.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/warn.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "src/split.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "src/ok.rs".to_string(),
                symbols: vec![],
            },
        ],
        file_size_warnings: vec![
            rinkaku_core::file_size::FileSizeWarning {
                path: "src/warn.rs".to_string(),
                line_count: 1600,
                severity: FileSizeSeverity::Warn,
            },
            rinkaku_core::file_size::FileSizeWarning {
                path: "src/split.rs".to_string(),
                line_count: 2500,
                severity: FileSizeSeverity::Split,
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Root is the "src" Dir.
    let expected = Badges {
        file_size_warn_count: 1,
        file_size_split_count: 1,
        // Directories deliberately never carry per-node severity /
        // line_count of their own — those live only on file rows.
        own_file_size_severity: None,
        own_file_line_count: None,
        ..Badges::default()
    };
    assert_eq!(expected, tree.roots[0].badges);
}

#[test]
fn should_leave_own_file_size_severity_none_on_dir_node() {
    // Even when a directory aggregates a nonzero warn/split count
    // from below, its own_file_size_severity / own_file_line_count
    // must stay None — those two fields are per-file attributes only.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/big.rs".to_string(),
            symbols: vec![],
        }],
        file_size_warnings: vec![rinkaku_core::file_size::FileSizeWarning {
            path: "src/big.rs".to_string(),
            line_count: 3000,
            severity: FileSizeSeverity::Split,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let dir_node = &tree.roots[0];
    assert_eq!(None, dir_node.badges.own_file_size_severity);
    assert_eq!(None, dir_node.badges.own_file_line_count);
    // Aggregates still count the descendant.
    assert_eq!(1, dir_node.badges.file_size_split_count);
}
