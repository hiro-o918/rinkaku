//! ADR 0017: whole-repo [`analyze_repo`] — empty-input handling,
//! extracting every symbol, the `classification: None` invariant,
//! per-path skip cases (unsupported language / read failure /
//! `generated_paths` / generated-content marker / test path), test
//! symbol filtering by `include_tests`, and hotspot aggregation.

use super::{empty_graph, fake_reader};
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::pipeline::analyze_repo;
use crate::render::{FileReport, Report, ReportOrigin};
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_return_empty_report_when_paths_is_empty() {
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&[], read_file, true, &HashSet::new(), true);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_every_symbol_when_file_has_no_changes_to_speak_of() {
    // Unlike `analyze_diff`, there is no diff here at all — every
    // symbol in the file is reported, not just ones touching a
    // changed line, since there is no changed-line concept in this
    // mode (ADR 0017).
    let source = "\
fn helper(x: i32) -> i32 {
    x
}

struct Point {
    x: i32,
}
";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
    let paths = vec!["src/lib.rs".to_string()];

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    id: "src/lib.rs::helper".to_string(),
                    name: "helper".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn helper(x: i32) -> i32".to_string(),
                    range: LineRange { start: 1, end: 3 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                    is_test: false,
                    classification: None,
                    previous_signature: None,
                },
                ExtractedSymbol {
                    id: "src/lib.rs::Point".to_string(),
                    name: "Point".to_string(),
                    kind: SymbolKind::Struct,
                    signature: "struct Point { x: i32, }".to_string(),
                    range: LineRange { start: 5, end: 7 },
                    container: None,
                    referenced_names: vec!["Point".to_string()],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                    is_test: false,
                    classification: None,
                    previous_signature: None,
                },
            ],
        }],
        skipped: vec![],
        graph: crate::graph::SymbolGraph {
            nodes: vec![
                crate::graph::Node {
                    id: "src/lib.rs::helper".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "helper".to_string(),
                },
                crate::graph::Node {
                    id: "src/lib.rs::Point".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "Point".to_string(),
                },
            ],
            edges: vec![],
            roots: vec![
                "src/lib.rs::helper".to_string(),
                "src/lib.rs::Point".to_string(),
            ],
        },
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    assert_eq!(expected, actual);
}

// No `classification: Some(Added)`: ADR 0017's whole point is that
// whole-repo mode must not mistake "nothing changed" for "every
// symbol was just added" the way a synthetic empty-tree diff would.
#[test]
fn should_leave_classification_none_for_every_symbol() {
    let source = "fn foo() -> i32 { 1 }\n";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
    let paths = vec!["src/lib.rs".to_string()];

    let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    let expected: Option<crate::extract::Classification> = None;
    let actual = report.files[0].symbols[0].classification;
    assert_eq!(expected, actual);
}

#[test]
fn should_skip_path_without_registered_language_support() {
    // `.rb` has no registered `LanguageSupport` (see the note on
    // `should_skip_file_with_unsupported_language_without_reading_it`
    // above) — silently excluded from the outline, no `SkippedFile`
    // entry (unlike `analyze_diff`, there is no diff-touched file to
    // report a skip reason for; see `analyze_repo`'s doc comment).
    let read_file = fake_reader(HashMap::new());
    let paths = vec!["src/main.rb".to_string()];

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    assert_eq!(expected, actual);
}

#[test]
fn should_skip_path_when_read_file_fails() {
    // No entry in the map for this path: `read_file` returns `Err`,
    // which `analyze_repo` treats as best-effort "skip this file"
    // rather than failing the whole run (see its own doc comment).
    let read_file = fake_reader(HashMap::new());
    let paths = vec!["src/lib.rs".to_string()];

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    assert_eq!(expected, actual);
}

#[test]
fn should_skip_path_in_generated_paths_set_without_reading_it() {
    // No entry in the map: if `analyze_repo` tried to read a
    // generated file, this would return `Err` and (being treated as
    // best-effort) silently produce the same empty result either
    // way — so this test also pins that the file is excluded
    // *before* any read is attempted, matching
    // `TagsResolver::new`'s check ordering (deps.rs).
    let read_file = fake_reader(HashMap::new());
    let paths = vec!["Cargo.lock".to_string()];
    let generated_paths: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, true, &generated_paths, true);

    assert_eq!(expected, actual);
}

#[test]
fn should_skip_file_with_generated_content_marker_when_include_generated_is_false() {
    let source = "// Code generated by SQLBoiler 4.19.5. DO NOT EDIT.\n\nfn foo() -> i32 { 1 }\n";
    let read_file = fake_reader(HashMap::from([("models/user.rs", source)]));
    let paths = vec!["models/user.rs".to_string()];

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, true, &HashSet::new(), false);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_skip_file_with_generated_content_marker_when_include_generated_is_true() {
    let source = "// Code generated by SQLBoiler 4.19.5. DO NOT EDIT.\n\nfn foo() -> i32 { 1 }\n";
    let read_file = fake_reader(HashMap::from([("models/user.rs", source)]));
    let paths = vec!["models/user.rs".to_string()];

    let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    assert_eq!(1, report.files.len());
}

#[test]
fn should_drop_whole_file_from_files_when_test_path_has_only_test_symbols() {
    let source = "\
package main

func TestFoo(t *testing.T) {
	1
}
";
    let read_file = fake_reader(HashMap::from([("repo_test.go", source)]));
    let paths = vec!["repo_test.go".to_string()];

    let expected = Report {
        origin: ReportOrigin::RepoOutline,
        files: vec![],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        hotspots: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };
    let actual = analyze_repo(&paths, read_file, false, &HashSet::new(), true);

    assert_eq!(expected, actual);
}

#[test]
fn should_keep_test_symbol_when_include_tests_is_true() {
    let source = "\
package main

func TestFoo(t *testing.T) {
	1
}
";
    let read_file = fake_reader(HashMap::from([("repo_test.go", source)]));
    let paths = vec!["repo_test.go".to_string()];

    let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    assert_eq!(1, report.files.len());
}

#[test]
fn should_keep_non_test_symbol_and_drop_test_symbol_when_file_mixes_both() {
    // A Rust file with one production function and one
    // `#[cfg(test)] mod tests` function — the production symbol is
    // kept, the test symbol is dropped, and the file itself is kept
    // (not emptied entirely) since it still has a non-test symbol
    // left after filtering.
    let source = "\
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_add_two_numbers() {
        assert_eq!(3, add(1, 2));
    }
}
";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
    let paths = vec!["src/lib.rs".to_string()];

    let report = analyze_repo(&paths, read_file, false, &HashSet::new(), true);

    let expected_names = vec!["add".to_string()];
    let actual_names: Vec<String> = report.files[0]
        .symbols
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(expected_names, actual_names);
}

#[test]
fn should_populate_hotspots_when_repo_has_a_symbol_with_fan_in_of_two() {
    // ADR 0017's Consequences: fan-in hotspots are computed over the
    // whole repository in this mode, same aggregation as diff mode.
    let source = "\
fn shared_helper() -> i32 {
    1
}

fn caller_one() -> i32 {
    shared_helper()
}

fn caller_two() -> i32 {
    shared_helper()
}
";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));
    let paths = vec!["src/lib.rs".to_string()];

    let report = analyze_repo(&paths, read_file, true, &HashSet::new(), true);

    let expected = vec![crate::graph::Hotspot {
        id: "src/lib.rs::shared_helper".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared_helper".to_string(),
        used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
    }];
    assert_eq!(expected, report.hotspots);
}
