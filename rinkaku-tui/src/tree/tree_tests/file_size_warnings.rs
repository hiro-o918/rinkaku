use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_populate_own_file_line_count_when_report_has_band_for_that_path() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/big.rs".to_string(),
            symbols: vec![],
        }],
        file_size_bands: vec![rinkaku_core::file_size::FileSizeEntry {
            path: "src/big.rs".to_string(),
            line_count: 1734,
            band: FileSizeBand::Warn,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Root is the "src" Dir; file node is its only child.
    let file_node = &tree.roots[0].children[0];
    let expected = Badges {
        own_file_size_band: Some(FileSizeBand::Warn),
        own_file_line_count: Some(1734),
        file_size_warn_count: 1,
        file_size_split_count: 0,
        ..Badges::default()
    };
    assert_eq!(expected, file_node.badges);
}

#[test]
fn should_aggregate_warn_and_split_counts_separately_on_parent_dir() {
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
        file_size_bands: vec![
            rinkaku_core::file_size::FileSizeEntry {
                path: "src/warn.rs".to_string(),
                line_count: 1600,
                band: FileSizeBand::Warn,
            },
            rinkaku_core::file_size::FileSizeEntry {
                path: "src/split.rs".to_string(),
                line_count: 2500,
                band: FileSizeBand::Split,
            },
            rinkaku_core::file_size::FileSizeEntry {
                path: "src/ok.rs".to_string(),
                line_count: 10,
                band: FileSizeBand::Normal,
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Root is the "src" Dir.
    let expected = Badges {
        file_size_warn_count: 1,
        file_size_split_count: 1,
        own_file_size_band: None,
        own_file_line_count: None,
        ..Badges::default()
    };
    assert_eq!(expected, tree.roots[0].badges);
}

#[test]
fn should_leave_own_file_size_band_none_on_dir_node() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/big.rs".to_string(),
            symbols: vec![],
        }],
        file_size_bands: vec![rinkaku_core::file_size::FileSizeEntry {
            path: "src/big.rs".to_string(),
            line_count: 3000,
            band: FileSizeBand::Split,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let dir_node = &tree.roots[0];
    assert_eq!(None, dir_node.badges.own_file_size_band);
    assert_eq!(None, dir_node.badges.own_file_line_count);
    assert_eq!(1, dir_node.badges.file_size_split_count);
}

#[test]
fn should_not_aggregate_warn_or_split_count_when_band_is_normal_or_watch() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/normal.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "src/watch.rs".to_string(),
                symbols: vec![],
            },
        ],
        file_size_bands: vec![
            rinkaku_core::file_size::FileSizeEntry {
                path: "src/normal.rs".to_string(),
                line_count: 10,
                band: FileSizeBand::Normal,
            },
            rinkaku_core::file_size::FileSizeEntry {
                path: "src/watch.rs".to_string(),
                line_count: 700,
                band: FileSizeBand::Watch,
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let expected = Badges {
        own_file_size_band: None,
        own_file_line_count: None,
        file_size_warn_count: 0,
        file_size_split_count: 0,
        ..Badges::default()
    };
    assert_eq!(expected, tree.roots[0].badges);
}
