use super::*;
use pretty_assertions::assert_eq;

// ADR 0035 Phase B: a *whole* test file — either matched by
// `LanguageSupport::is_test_path` (e.g. Go's `*_test.go` convention) or
// one whose every changed symbol has `ExtractedSymbol::is_test == true`
// (Rust's `#[cfg(test)]` convention, when the file has no production
// symbols left over) — is lifted out of the production tree into a
// trailing `NodeKind::Section(SectionKind::Tests)` node, keeping its
// directory nesting. A *mixed* file (some non-test symbols alongside
// some test symbols) stays in the production tree untouched — only a
// whole-test file moves.

#[test]
fn should_append_a_trailing_tests_section_for_a_whole_test_file_by_path_convention() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo", SymbolKind::Function)],
            },
            FileReport {
                path: "src/lib_test.go".to_string(),
                symbols: vec![symbol(
                    "src/lib_test.go::TestFoo",
                    "TestFoo",
                    SymbolKind::Function,
                )],
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Production tree: `tree.roots` has "src" (only the non-test file
    // left under it) plus the trailing Tests section as its second,
    // sibling root — the section is a top-level sibling of every
    // production root, not nested inside one.
    assert_eq!(2, tree.roots.len());
    let src = &tree.roots[0];
    assert_eq!(NodeKind::Dir, src.kind);
    assert_eq!(1, src.children.len());
    assert_eq!("src/lib.rs", src.children[0].path);

    // The whole-test file is nested *inside* the section, under its own
    // "src" dir node (a section keeps directory nesting, per ADR 0035).
    let section = find_tests_section(&tree).expect("expected a Tests section");
    assert_eq!(1, section.children.len());
    let section_src = &section.children[0];
    assert_eq!(NodeKind::Dir, section_src.kind);
    assert_eq!("src", section_src.path);
    assert_eq!(1, section_src.children.len());
    assert_eq!("src/lib_test.go", section_src.children[0].path);
}

#[test]
fn should_append_a_trailing_tests_section_for_a_whole_test_file_by_all_symbols_being_test() {
    // No `_test.go`/`_test.rs`-style path convention here — every symbol
    // in the file is individually flagged `is_test` (Rust's
    // `#[cfg(test)] mod tests` convention, when nothing production-side
    // remains in the same file).
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/only_tests.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                is_test: true,
                ..symbol(
                    "src/only_tests.rs::test_it",
                    "test_it",
                    SymbolKind::Function,
                )
            }],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(
        1,
        tree.roots.len(),
        "only the Tests section itself — no production root, since the \
         only file in this report is a whole test file"
    );
    let section = find_tests_section(&tree).expect("expected a Tests section");
    assert_eq!(1, section.children.len());
    let section_src = &section.children[0];
    assert_eq!("src", section_src.path);
    assert_eq!("src/only_tests.rs", section_src.children[0].path);
}

#[test]
fn should_keep_a_mixed_file_in_the_production_tree_and_omit_the_tests_section() {
    // A mixed file (real symbol + test symbol in the same file) is never
    // moved — see this file's header comment.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                symbol("src/mixed.rs::real_fn", "real_fn", SymbolKind::Function),
                ExtractedSymbol {
                    is_test: true,
                    ..symbol("src/mixed.rs::test_it", "test_it", SymbolKind::Function)
                },
            ],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    let src = &tree.roots[0];
    assert_eq!(1, src.children.len());
    assert_eq!("src/mixed.rs", src.children[0].path);
    assert_eq!(2, src.children[0].children.len());

    assert_eq!(
        None,
        find_tests_section(&tree),
        "no Tests section at all when every file is production or mixed"
    );
}

#[test]
fn should_not_append_a_tests_section_when_there_are_no_test_files_at_all() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    assert_eq!(None, find_tests_section(&tree));
}

#[test]
fn should_sort_tests_section_children_alphabetically_regardless_of_discovery_order() {
    // Discovery order here is intentionally "b" before "a" — the Tests
    // section is always A-Z internally (ADR 0035), independent of
    // source/discovery order, unlike the production tree's default
    // source-order-then-reorder split (`crate::order`).
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "b_test.go".to_string(),
                symbols: vec![symbol("b_test.go::TestB", "TestB", SymbolKind::Function)],
            },
            FileReport {
                path: "a_test.go".to_string(),
                symbols: vec![symbol("a_test.go::TestA", "TestA", SymbolKind::Function)],
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let section = find_tests_section(&tree).expect("expected a Tests section");
    let paths: Vec<&str> = section.children.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["a_test.go", "b_test.go"], paths);
}

#[test]
fn should_treat_removed_symbols_file_as_production_never_moved_to_tests_section() {
    // A `RemovedSymbol` carries no `is_test` of its own (ADR 0035's
    // Consequences: no head-side AST context to classify it by), and a
    // file whose only content is a removed symbol is not itself
    // `is_test_path`-flagged here, so it stays in the production tree.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        removed: vec![RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    assert_eq!("lib.rs", tree.roots[0].path);
    assert_eq!(None, find_tests_section(&tree));
}

fn find_tests_section(tree: &Tree) -> Option<&TreeNode> {
    tree.roots
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Section(SectionKind::Tests)))
}
