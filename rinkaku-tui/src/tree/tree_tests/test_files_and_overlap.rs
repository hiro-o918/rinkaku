use super::*;
use pretty_assertions::assert_eq;

// Whole-test-file tests: a file whose changed symbols were *all* test
// code has no `FileReport` in `report.files` at all
// (`pipeline::partition_test_symbols`'s doc comment) — only a
// `TestFileSummary` in `report.tests`. Without surfacing that summary
// into the tree, such a file is invisible to a reviewer, the same gap
// as a skipped file.

#[test]
fn should_add_whole_test_file_as_childless_file_node_with_symbol_count() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        tests: vec![TestFileSummary {
            path: "src/lib_test.go".to_string(),
            symbol_count: 3,
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::Dir,
            path: "src".to_string(),
            badges: Badges::default(),
            children: vec![TreeNode {
                kind: NodeKind::File,
                path: "src/lib_test.go".to_string(),
                badges: Badges::default(),
                children: vec![],
                skip_reason: None,
                test_symbol_count: Some(3),
            }],
            skip_reason: None,
            test_symbol_count: None,
        }],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_merge_test_file_into_existing_dir_alongside_analyzed_files() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        tests: vec![TestFileSummary {
            path: "src/lib_test.rs".to_string(),
            symbol_count: 2,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    let src = &tree.roots[0];
    assert_eq!("src", src.path);
    assert_eq!(2, src.children.len());
    let paths: Vec<&str> = src.children.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["src/lib.rs", "src/lib_test.rs"], paths);
}

#[test]
fn should_not_set_test_symbol_count_or_skip_reason_on_an_ordinary_file() {
    // Regression guard: an ordinary analyzed file must keep both new
    // fields at `None`, not accidentally inherit a stale default from
    // whatever `FileBuilder` construction path is taken.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(None, tree.roots[0].skip_reason);
    assert_eq!(None, tree.roots[0].test_symbol_count);
}

// `pipeline::analyze_diff` never produces a `Report` where the same path
// appears in both `files`/`skipped` or both `tests`/`skipped` (see
// `TreeBuilder::insert_file`/`insert_test_file`/`insert_skipped`'s own
// doc comments), so `#[cfg(debug_assertions)]` keeps these panic-path
// tests out of release builds, matching the `debug_assert!`s themselves
// — they only guard a caller contract, not a condition `build_tree`
// needs to handle gracefully at runtime. `files`/`tests` overlapping on
// the same path, in contrast, is a *valid* `analyze_diff` output (a
// mixed file) — see `should_keep_real_symbols_when_file_is_also_in_tests`
// below, not a panic case.
// `build_tree` visits `report.files` before `report.skipped` (its own
// doc comment's discovery order), so this hits `insert_skipped`'s own
// assert, not `insert_file`'s — `insert_file` only guards against a
// path *already* marked skipped when files runs, which is not yet true
// here.

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "report.skipped must not overlap report.files/report.tests")]
fn should_panic_when_the_same_path_appears_in_files_and_skipped() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        skipped: vec![SkippedFile {
            path: "lib.rs".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
        ..empty_report()
    };

    build_tree(&report);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "report.skipped must not overlap report.files/report.tests")]
fn should_panic_when_the_same_path_appears_in_tests_and_skipped() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        tests: vec![TestFileSummary {
            path: "lib.rs".to_string(),
            symbol_count: 1,
        }],
        skipped: vec![SkippedFile {
            path: "lib.rs".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
        ..empty_report()
    };

    build_tree(&report);
}

// Regression test (post-rebase integration check): `lib.rs` in both
// `report.files` (some real symbols) and `report.tests` (a test count
// for the rest) is exactly what `pipeline::partition_test_symbols`
// produces for a mixed file — e.g. a Rust file with production
// functions changed alongside its own `#[cfg(test)] mod tests` in the
// same diff (this crate's own `rinkaku-tui/src/app.rs` hit this in a
// live dogfood run). `build_tree` must keep both pieces of information
// on the one `TreeNode` rather than panicking or silently dropping
// either half.

#[test]
fn should_keep_real_symbols_when_file_is_also_in_tests() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        tests: vec![TestFileSummary {
            path: "lib.rs".to_string(),
            symbol_count: 3,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots[0].children.len());
    assert_eq!(Some(3), tree.roots[0].test_symbol_count);
    assert_eq!(None, tree.roots[0].skip_reason);
}
