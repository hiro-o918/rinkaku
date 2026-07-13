use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_count_contract_change_for_signature_changed_symbol() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::SignatureChanged),
                ..symbol("lib.rs::foo", "foo", SymbolKind::Function)
            }],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let expected = Badges {
        changed_symbols: 1,
        contract_changes: 1,
        fan_in: 0,
        ..Badges::default()
    };
    let actual = tree.roots[0].badges;

    assert_eq!(expected, actual);
}

#[test]
fn should_not_count_contract_change_for_body_only_symbol() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::BodyOnly),
                ..symbol("lib.rs::foo", "foo", SymbolKind::Function)
            }],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let expected = Badges {
        changed_symbols: 1,
        contract_changes: 0,
        fan_in: 0,
        ..Badges::default()
    };
    let actual = tree.roots[0].badges;

    assert_eq!(expected, actual);
}

#[test]
fn should_add_removed_symbol_as_marked_leaf_under_its_file_without_counting_as_changed() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![],
        removed: vec![RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::File,
            path: "lib.rs".to_string(),
            badges: Badges {
                changed_symbols: 0,
                contract_changes: 1,
                fan_in: 0,
                ..Badges::default()
            },
            children: vec![TreeNode {
                kind: NodeKind::Symbol(SymbolRef {
                    id: "lib.rs::gone".to_string(),
                    name: "gone".to_string(),
                    kind: SymbolKind::Function,
                    classification: None,
                    removed: true,
                }),
                path: "lib.rs".to_string(),
                badges: Badges {
                    changed_symbols: 0,
                    contract_changes: 1,
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
fn should_merge_removed_symbol_into_existing_file_with_present_symbols() {
    // A file with one present (unchanged classification-wise) symbol
    // and one removed symbol must land under the same File node, not
    // create two separate entries for "lib.rs".
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
        }],
        removed: vec![RemovedSymbol {
            name: "gone".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            signature: "fn gone()".to_string(),
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::File,
            path: "lib.rs".to_string(),
            badges: Badges {
                changed_symbols: 1,
                contract_changes: 1,
                fan_in: 0,
                ..Badges::default()
            },
            children: vec![
                TreeNode {
                    kind: NodeKind::Symbol(SymbolRef {
                        id: "lib.rs::foo".to_string(),
                        name: "foo".to_string(),
                        kind: SymbolKind::Function,
                        classification: None,
                        removed: false,
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
                },
                TreeNode {
                    kind: NodeKind::Symbol(SymbolRef {
                        id: "lib.rs::gone".to_string(),
                        name: "gone".to_string(),
                        kind: SymbolKind::Function,
                        classification: None,
                        removed: true,
                    }),
                    path: "lib.rs".to_string(),
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
            ],
            skip_reason: None,
            test_symbol_count: None,
        }],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_set_fan_in_badge_from_matching_fan_in_and_aggregate_upward() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("src/lib.rs::shared", "shared", SymbolKind::Function)],
        }],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    // Fan-in of 2 (two referrers) must show on the symbol leaf and
    // aggregate up through File and Dir.
    let src = &tree.roots[0];
    assert_eq!("src", src.path);
    assert_eq!(2, src.badges.fan_in);
    let file_node = &src.children[0];
    assert_eq!(2, file_node.badges.fan_in);
    let symbol_node = &file_node.children[0];
    assert_eq!(2, symbol_node.badges.fan_in);
}

#[test]
fn should_leave_fan_in_at_zero_when_symbol_has_no_matching_fan_in_entry() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::solo", "solo", SymbolKind::Function)],
        }],
        fan_ins: vec![],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(0, tree.roots[0].badges.fan_in);
}
