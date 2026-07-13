use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_build_empty_tree_when_report_has_no_files_and_no_removed() {
    let report = empty_report();

    let expected = Tree { roots: vec![] };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_flat_file_node_when_path_has_no_directory() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::File,
            path: "lib.rs".to_string(),
            badges: Badges {
                changed_symbols: 1,
                contract_changes: 0,
                fan_in: 0,
                ..Badges::default()
            },
            children: vec![TreeNode {
                kind: NodeKind::Symbol(SymbolRef {
                    id: "lib.rs::foo".to_string(),
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    classification: None,
                    removed: false,
                    is_test: false,
                }),
                path: "lib.rs".to_string(),
                badges: Badges {
                    changed_symbols: 1,
                    contract_changes: 0,
                    fan_in: 0,
                    ..Badges::default()
                },
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

#[test]
fn should_collapse_single_child_directory_chain_into_one_node() {
    // src/foo/bar/lib.rs — src, foo, bar each have exactly one child,
    // so all three collapse into one Dir node labeled "src/foo/bar".
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/foo/bar/lib.rs".to_string(),
            symbols: vec![],
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::Dir,
            path: "src/foo/bar".to_string(),
            badges: Badges::default(),
            children: vec![TreeNode {
                kind: NodeKind::File,
                path: "src/foo/bar/lib.rs".to_string(),
                badges: Badges::default(),
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

#[test]
fn should_not_collapse_directory_with_two_children() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/a.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "src/b.rs".to_string(),
                symbols: vec![],
            },
        ],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::Dir,
            path: "src".to_string(),
            badges: Badges::default(),
            children: vec![
                TreeNode {
                    kind: NodeKind::File,
                    path: "src/a.rs".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                },
                TreeNode {
                    kind: NodeKind::File,
                    path: "src/b.rs".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                },
            ],
            skip_reason: None,
            test_symbol_count: None,
        }],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_collapse_directory_that_has_own_file_alongside_subdirectory() {
    // src/ has both a direct file (mod.rs) and a subdirectory (foo/) —
    // src is not "just a chain" to reach foo, so it must stay a
    // separate node rather than collapsing with foo.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/mod.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "src/foo/bar.rs".to_string(),
                symbols: vec![],
            },
        ],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::Dir,
            path: "src".to_string(),
            badges: Badges::default(),
            children: vec![
                TreeNode {
                    kind: NodeKind::File,
                    path: "src/mod.rs".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                },
                TreeNode {
                    kind: NodeKind::Dir,
                    path: "src/foo".to_string(),
                    badges: Badges::default(),
                    children: vec![TreeNode {
                        kind: NodeKind::File,
                        path: "src/foo/bar.rs".to_string(),
                        badges: Badges::default(),
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
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

// NOTE: partial assert (root count, path, then only the aggregated
// `Badges`) rather than a whole-`Tree` comparison — this test's only
// concern is that bottom-up aggregation reaches the top of a
// multi-level, multi-file subtree correctly; restating the full
// "src/a/one.rs" and "src/b/two.rs" node structure (already pinned down
// by other tests in this module, e.g.
// `should_build_flat_file_node_when_path_has_no_directory`) would just
// add noise without strengthening what this test is checking.

#[test]
fn should_aggregate_badges_bottom_up_across_nested_directories() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/a/one.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    ..symbol("src/a/one.rs::x", "x", SymbolKind::Function)
                }],
            },
            FileReport {
                path: "src/b/two.rs".to_string(),
                symbols: vec![symbol("src/b/two.rs::y", "y", SymbolKind::Function)],
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    let src = &tree.roots[0];
    assert_eq!("src", src.path);
    let expected = Badges {
        changed_symbols: 2,
        contract_changes: 1,
        fan_in: 0,
        ..Badges::default()
    };
    assert_eq!(expected, src.badges);
}

#[test]
fn should_keep_file_with_no_symbols_as_childless_file_node() {
    // A pure rename (FileReport with empty symbols) must still show up
    // as a File node with zero badges, not be dropped from the tree.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/renamed.rs".to_string(),
            symbols: vec![],
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
                path: "src/renamed.rs".to_string(),
                badges: Badges::default(),
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

#[test]
fn should_preserve_source_order_of_siblings_before_reordering() {
    // Discovery order in `report.files` must be preserved (reordering
    // is a separate concern handled by `crate::order`), even though the
    // builder uses a BTreeMap internally.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "z.rs".to_string(),
                symbols: vec![],
            },
            FileReport {
                path: "a.rs".to_string(),
                symbols: vec![],
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let names: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["z.rs", "a.rs"], names);
}
