use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_when_target_is_none() {
    let report = empty_report();

    let actual = build_diff_pane_content(&report, &[], None);

    assert_eq!(DiffPaneContent::Empty, actual);
}

// ADR 0072: the diff pane no longer groups hunks by symbol — a file
// selection shows every hunk in the file, in the exact order
// `crate::diff_view::parse_diff_hunks` produced them, regardless of how
// many symbols each hunk's range does or does not intersect.
#[test]
fn should_return_every_hunk_in_original_order_for_a_file_selection() {
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
            hunk("@@ -5,1 +5,1 @@", Some((5, 5)), vec!["use foo::bar;"]),
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ],
    }];
    let target = DiffTarget::File {
        path: "lib.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    let expected = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(
            1,
            hunk("@@ -5,1 +5,1 @@", Some((5, 5)), vec!["use foo::bar;"]),
        ),
        attributed(
            2,
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ),
    ]);
    assert_eq!(expected, actual);
}

// Regression coverage carried over from ADR 0029's "brand-new file" case
// (PR #86 dogfooding): a brand-new file's diff is always exactly one hunk
// spanning the whole file. Under ADR 0072 there is no per-symbol
// attribution left to fail — the single hunk is simply returned once,
// regardless of how many symbols the file defines.
#[test]
fn should_return_new_files_single_hunk_unsplit_regardless_of_symbol_count() {
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
    let added_lines = vec![
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
    ];
    let whole_file_hunk = Hunk {
        header: "@@ -0,0 +1,11 @@".to_string(),
        new_range: Some((1, 11)),
        lines: added_lines
            .into_iter()
            .map(|content| crate::diff_view::DiffLine {
                kind: crate::diff_view::DiffLineKind::Added,
                content: content.to_string(),
            })
            .collect(),
    };
    let diff_files = vec![FileHunks {
        path: "file_size.rs".to_string(),
        hunks: vec![whole_file_hunk.clone()],
    }];
    let target = DiffTarget::File {
        path: "file_size.rs".to_string(),
    };

    let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

    assert_eq!(
        DiffPaneContent::File(vec![attributed(0, whole_file_hunk)]),
        actual
    );
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

// Regression test (post-rebase integration check, PR #58): a skipped or
// whole-test-file row has no `FileReport` at all in `report.files`. Under
// ADR 0072 the shaping no longer reads `report.files` at all for content
// (only for the auto-scroll target, computed separately), so this must
// still return every hunk rather than silently dropping the file.
#[test]
fn should_return_every_hunk_when_file_selection_has_no_symbols_at_all() {
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

    let expected = DiffPaneContent::File(vec![attributed(
        0,
        hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["binary blob line"]),
    )]);
    assert_eq!(expected, actual);
}

// Regression test (post-rebase integration check, PR #58): a binary
// skipped file has a `FileHunks` entry (git still reports the path
// touched a diff) but zero `@@` hunks in it ("Binary files ... differ"
// has no hunk syntax for `crate::diff_view::parse_diff_hunks` to parse)
// — the pane must degrade to `Empty` rather than panicking.
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
