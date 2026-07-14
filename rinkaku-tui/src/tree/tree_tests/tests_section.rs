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

fn symbol_ref(id: &str, name: &str) -> SymbolRef {
    SymbolRef {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        classification: None,
        removed: false,
        is_test: false,
    }
}

fn one_symbol_badges() -> Badges {
    Badges {
        changed_symbols: 1,
        contract_changes: 0,
        fan_in: 0,
        ..Badges::default()
    }
}

fn symbol_leaf(path: &str, name: &str) -> TreeNode {
    TreeNode {
        kind: NodeKind::Symbol(symbol_ref(&format!("{path}::{name}"), name)),
        path: path.to_string(),
        badges: one_symbol_badges(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

fn file_with_one_symbol(path: &str, name: &str) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges: one_symbol_badges(),
        children: vec![symbol_leaf(path, name)],
        skip_reason: None,
        test_symbol_count: None,
    }
}

fn dir_wrapping(path: &str, badges: Badges, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::Dir,
        path: path.to_string(),
        badges,
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

fn tests_section(badges: Badges, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        kind: NodeKind::Section(SectionKind::Tests),
        path: crate::tree::TESTS_SECTION_PATH.to_string(),
        badges,
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

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

    // `tree.roots` holds "src" (only the non-test file left under it) plus
    // the trailing Tests section as a sibling root, itself nesting its
    // own "src" dir for the whole-test file (a section keeps directory
    // nesting, per ADR 0035).
    let expected = Tree {
        roots: vec![
            dir_wrapping(
                "src",
                one_symbol_badges(),
                vec![file_with_one_symbol("src/lib.rs", "foo")],
            ),
            tests_section(
                one_symbol_badges(),
                vec![dir_wrapping(
                    "src",
                    one_symbol_badges(),
                    vec![file_with_one_symbol("src/lib_test.go", "TestFoo")],
                )],
            ),
        ],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
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

    // Only the Tests section itself — no production root, since the
    // only file in this report is a whole test file.
    let expected = Tree {
        roots: vec![tests_section(
            one_symbol_badges(),
            vec![dir_wrapping(
                "src",
                one_symbol_badges(),
                vec![TreeNode {
                    kind: NodeKind::File,
                    path: "src/only_tests.rs".to_string(),
                    badges: one_symbol_badges(),
                    children: vec![TreeNode {
                        kind: NodeKind::Symbol(SymbolRef {
                            is_test: true,
                            ..symbol_ref("src/only_tests.rs::test_it", "test_it")
                        }),
                        path: "src/only_tests.rs".to_string(),
                        badges: one_symbol_badges(),
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    }],
                    skip_reason: None,
                    test_symbol_count: None,
                }],
            )],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_keep_a_mixed_file_in_the_production_tree_and_omit_the_tests_section() {
    // A mixed file (real symbol + test symbol in the same file) is never
    // moved — see this file's header comment. Its test symbol nests under
    // a synthetic `TestGroup` child (visual-encoding prototype) rather
    // than sitting flat alongside `real_fn`.
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

    let two_symbol_badges = Badges {
        changed_symbols: 2,
        contract_changes: 0,
        fan_in: 0,
        ..Badges::default()
    };
    let expected = Tree {
        roots: vec![dir_wrapping(
            "src",
            two_symbol_badges,
            vec![TreeNode {
                kind: NodeKind::File,
                path: "src/mixed.rs".to_string(),
                badges: two_symbol_badges,
                children: vec![
                    symbol_leaf("src/mixed.rs", "real_fn"),
                    TreeNode {
                        kind: NodeKind::TestGroup { count: 1 },
                        path: "src/mixed.rs::tests".to_string(),
                        badges: one_symbol_badges(),
                        children: vec![TreeNode {
                            kind: NodeKind::Symbol(SymbolRef {
                                is_test: true,
                                ..symbol_ref("src/mixed.rs::test_it", "test_it")
                            }),
                            path: "src/mixed.rs".to_string(),
                            badges: one_symbol_badges(),
                            children: vec![],
                            skip_reason: None,
                            test_symbol_count: None,
                        }],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_place_test_group_at_earliest_test_symbol_line_when_production_symbols_surround_it() {
    // ADR 0045: production symbols both before and after the test
    // block must not push TestGroup to the fixed last position —
    // it inserts immediately before the first production symbol whose
    // line comes after the earliest test symbol's line.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 1, end: 5 },
                    ..symbol("src/mixed.rs::before_fn", "before_fn", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 15 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_it", "test_it", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 20, end: 25 },
                    ..symbol("src/mixed.rs::after_fn", "after_fn", SymbolKind::Function)
                },
            ],
        }],
        ..empty_report()
    };

    let three_symbol_badges = Badges {
        changed_symbols: 3,
        contract_changes: 0,
        fan_in: 0,
        ..Badges::default()
    };
    let expected = Tree {
        roots: vec![dir_wrapping(
            "src",
            three_symbol_badges,
            vec![TreeNode {
                kind: NodeKind::File,
                path: "src/mixed.rs".to_string(),
                badges: three_symbol_badges,
                children: vec![
                    symbol_leaf("src/mixed.rs", "before_fn"),
                    TreeNode {
                        kind: NodeKind::TestGroup { count: 1 },
                        path: "src/mixed.rs::tests".to_string(),
                        badges: one_symbol_badges(),
                        children: vec![TreeNode {
                            kind: NodeKind::Symbol(SymbolRef {
                                is_test: true,
                                ..symbol_ref("src/mixed.rs::test_it", "test_it")
                            }),
                            path: "src/mixed.rs".to_string(),
                            badges: one_symbol_badges(),
                            children: vec![],
                            skip_reason: None,
                            test_symbol_count: None,
                        }],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                    symbol_leaf("src/mixed.rs", "after_fn"),
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_append_test_group_last_when_every_test_symbol_trails_every_production_symbol() {
    // The common case (a trailing `#[cfg(test)] mod tests` block): with
    // no production symbol positioned after the earliest test symbol,
    // placement degrades to the same "append last" behavior as before
    // ADR 0045.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 1, end: 5 },
                    ..symbol("src/mixed.rs::real_fn", "real_fn", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 15 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_it", "test_it", SymbolKind::Function)
                },
            ],
        }],
        ..empty_report()
    };

    let two_symbol_badges = Badges {
        changed_symbols: 2,
        contract_changes: 0,
        fan_in: 0,
        ..Badges::default()
    };
    let expected = Tree {
        roots: vec![dir_wrapping(
            "src",
            two_symbol_badges,
            vec![TreeNode {
                kind: NodeKind::File,
                path: "src/mixed.rs".to_string(),
                badges: two_symbol_badges,
                children: vec![
                    symbol_leaf("src/mixed.rs", "real_fn"),
                    TreeNode {
                        kind: NodeKind::TestGroup { count: 1 },
                        path: "src/mixed.rs::tests".to_string(),
                        badges: one_symbol_badges(),
                        children: vec![TreeNode {
                            kind: NodeKind::Symbol(SymbolRef {
                                is_test: true,
                                ..symbol_ref("src/mixed.rs::test_it", "test_it")
                            }),
                            path: "src/mixed.rs".to_string(),
                            badges: one_symbol_badges(),
                            children: vec![],
                            skip_reason: None,
                            test_symbol_count: None,
                        }],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_let_a_removed_symbol_be_mistaken_for_a_later_production_symbol() {
    // ADR 0045 regression: a removed symbol carries no line of its own
    // (`RemovedSymbol` has no `range`), so it must never be treated as
    // "positioned after the test block" — TestGroup still appends last,
    // the removed symbol keeps its own insertion-order position ahead of it.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 1, end: 5 },
                    ..symbol("src/mixed.rs::real_fn", "real_fn", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 15 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_it", "test_it", SymbolKind::Function)
                },
            ],
        }],
        removed: vec![RemovedSymbol {
            name: "removed_fn".to_string(),
            kind: SymbolKind::Function,
            path: "src/mixed.rs".to_string(),
            signature: "fn removed_fn()".to_string(),
        }],
        ..empty_report()
    };

    let badges = Badges {
        changed_symbols: 2,
        contract_changes: 1,
        fan_in: 0,
        ..Badges::default()
    };
    let expected = Tree {
        roots: vec![dir_wrapping(
            "src",
            badges,
            vec![TreeNode {
                kind: NodeKind::File,
                path: "src/mixed.rs".to_string(),
                badges,
                children: vec![
                    symbol_leaf("src/mixed.rs", "real_fn"),
                    TreeNode {
                        kind: NodeKind::Symbol(SymbolRef {
                            id: "src/mixed.rs::removed_fn".to_string(),
                            name: "removed_fn".to_string(),
                            kind: SymbolKind::Function,
                            classification: None,
                            removed: true,
                            is_test: false,
                        }),
                        path: "src/mixed.rs".to_string(),
                        badges: Badges {
                            changed_symbols: 0,
                            contract_changes: 1,
                            fan_in: 0,
                            ..Badges::default()
                        },
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                    TreeNode {
                        kind: NodeKind::TestGroup { count: 1 },
                        path: "src/mixed.rs::tests".to_string(),
                        badges: one_symbol_badges(),
                        children: vec![TreeNode {
                            kind: NodeKind::Symbol(SymbolRef {
                                is_test: true,
                                ..symbol_ref("src/mixed.rs::test_it", "test_it")
                            }),
                            path: "src/mixed.rs".to_string(),
                            badges: one_symbol_badges(),
                            children: vec![],
                            skip_reason: None,
                            test_symbol_count: None,
                        }],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_break_ties_toward_production_when_a_test_symbol_shares_its_start_line() {
    // Strict `>` comparison: a production symbol sharing the test block's
    // exact start line is not treated as "after" it, so it stays before
    // TestGroup in the fixed insertion-order tie case.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 15 },
                    ..symbol(
                        "src/mixed.rs::same_line_fn",
                        "same_line_fn",
                        SymbolKind::Function,
                    )
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 15 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_it", "test_it", SymbolKind::Function)
                },
            ],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);
    let src = &tree.roots[0];
    let file = &src.children[0];
    let child_kinds: Vec<&str> = file
        .children
        .iter()
        .map(|child| match &child.kind {
            NodeKind::Symbol(symbol_ref) => symbol_ref.name.as_str(),
            NodeKind::TestGroup { .. } => "TestGroup",
            _ => "other",
        })
        .collect();

    // NOTE: partial assert (child kind sequence only) — the full node
    // shape for a single production symbol plus a one-member TestGroup is
    // already pinned by
    // `should_append_test_group_last_when_every_test_symbol_trails_every_production_symbol`;
    // this test's only concern is the tie-break order itself.
    assert_eq!(vec!["same_line_fn", "TestGroup"], child_kinds);
}

#[test]
fn should_group_scattered_test_symbols_at_the_earliest_ones_line() {
    // Multiple, non-contiguous test symbols still fold into one
    // TestGroup (ADR 0045 rejected per-block splitting) positioned at
    // the earliest test symbol's line, not the last one's.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/mixed.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    range: LineRange { start: 1, end: 5 },
                    ..symbol("src/mixed.rs::before_fn", "before_fn", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 12 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_a", "test_a", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 20, end: 25 },
                    ..symbol("src/mixed.rs::middle_fn", "middle_fn", SymbolKind::Function)
                },
                ExtractedSymbol {
                    range: LineRange { start: 30, end: 32 },
                    is_test: true,
                    ..symbol("src/mixed.rs::test_b", "test_b", SymbolKind::Function)
                },
            ],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);
    let src = &tree.roots[0];
    let file = &src.children[0];
    let child_kinds: Vec<&str> = file
        .children
        .iter()
        .map(|child| match &child.kind {
            NodeKind::Symbol(symbol_ref) => symbol_ref.name.as_str(),
            NodeKind::TestGroup { .. } => "TestGroup",
            _ => "other",
        })
        .collect();

    // NOTE: partial assert (child kind/name sequence only) — this test's
    // sole concern is ordering among four children whose full badge/path
    // shape is already pinned by the other tests in this module; a full
    // `Tree` comparison here would restate that shape without
    // strengthening the ordering assertion.
    assert_eq!(vec!["before_fn", "TestGroup", "middle_fn"], child_kinds);
    assert_eq!(NodeKind::TestGroup { count: 2 }, file.children[1].kind);
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

    let expected = Tree {
        roots: vec![file_with_one_symbol("lib.rs", "foo")],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
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

    let expected = Tree {
        roots: vec![tests_section(
            Badges {
                changed_symbols: 2,
                contract_changes: 0,
                fan_in: 0,
                ..Badges::default()
            },
            vec![
                file_with_one_symbol("a_test.go", "TestA"),
                file_with_one_symbol("b_test.go", "TestB"),
            ],
        )],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
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

    let removed_badges = Badges {
        changed_symbols: 0,
        contract_changes: 1,
        fan_in: 0,
        ..Badges::default()
    };
    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::File,
            path: "lib.rs".to_string(),
            badges: removed_badges,
            children: vec![TreeNode {
                kind: NodeKind::Symbol(SymbolRef {
                    id: "lib.rs::gone".to_string(),
                    name: "gone".to_string(),
                    kind: SymbolKind::Function,
                    classification: None,
                    removed: true,
                    is_test: false,
                }),
                path: "lib.rs".to_string(),
                badges: removed_badges,
                children: vec![],
                skip_reason: None,
                test_symbol_count: None,
            }],
            skip_reason: None,
            test_symbol_count: None,
        }],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}
