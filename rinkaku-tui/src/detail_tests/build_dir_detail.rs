// build_dir_detail / build_file_detail tests (TUI iteration 2).

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_none_when_dir_path_is_not_found() {
    let report = empty_report();
    let tree = crate::tree::build_tree(&report);

    let actual = build_dir_detail(&tree, &report, "missing");

    assert_eq!(None, actual);
}

#[test]
fn should_build_dir_detail_with_badges_and_no_cycle_when_directory_is_not_in_a_cycle() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::SignatureChanged),
                ..symbol("src/lib.rs::foo", "foo")
            }],
        }],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

    let expected = DirDetail {
        path: "src".to_string(),
        badges: crate::tree::Badges {
            changed_symbols: 1,
            contract_changes: 1,
            fan_in: 0,
            ..crate::tree::Badges::default()
        },
        top_fan_in: vec![],
        cycle_partners: vec![],
        cycle_edges: vec![],
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_list_top_fan_in_symbols_sorted_by_fan_in_descending() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::a", "a"),
                symbol("src/lib.rs::b", "b"),
                symbol("src/lib.rs::shared_low", "shared_low"),
                symbol("src/lib.rs::shared_high", "shared_high"),
            ],
        }],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::a", "src/lib.rs", "a"),
                node("src/lib.rs::b", "src/lib.rs", "b"),
                node("src/lib.rs::shared_low", "src/lib.rs", "shared_low"),
                node("src/lib.rs::shared_high", "src/lib.rs", "shared_high"),
            ],
            edges: vec![],
            roots: vec![],
        },
        hotspots: vec![
            Hotspot {
                id: "src/lib.rs::shared_low".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared_low".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            },
            Hotspot {
                id: "src/lib.rs::shared_high".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared_high".to_string(),
                used_by: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            },
        ],
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

    let expected_top_fan_in = vec![
        SymbolMention {
            id: "src/lib.rs::shared_high".to_string(),
            name: "shared_high".to_string(),
            path: "src/lib.rs".to_string(),
        },
        SymbolMention {
            id: "src/lib.rs::shared_low".to_string(),
            name: "shared_low".to_string(),
            path: "src/lib.rs".to_string(),
        },
    ];
    assert_eq!(expected_top_fan_in, actual.top_fan_in);
}

#[test]
fn should_truncate_top_fan_in_to_five_entries() {
    let symbols: Vec<ExtractedSymbol> = (0..7)
        .map(|i| symbol(&format!("src/lib.rs::s{i}"), &format!("s{i}")))
        .collect();
    let nodes: Vec<Node> = (0..7)
        .map(|i| node(&format!("src/lib.rs::s{i}"), "src/lib.rs", &format!("s{i}")))
        .collect();
    let hotspots: Vec<Hotspot> = (0..7)
        .map(|i| Hotspot {
            id: format!("src/lib.rs::s{i}"),
            path: "src/lib.rs".to_string(),
            name: format!("s{i}"),
            used_by: vec!["x".to_string(), "y".to_string()],
        })
        .collect();
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols,
        }],
        graph: SymbolGraph {
            nodes,
            edges: vec![],
            roots: vec![],
        },
        hotspots,
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

    assert_eq!(5, actual.top_fan_in.len());
}

#[test]
fn should_explain_cycle_partners_and_edges_when_directory_participates_in_a_cycle() {
    // api/ and store/ depend on each other — a directory-level cycle
    // (mirrors crate::order's own cycle test fixtures).
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "api/handler.rs".to_string(),
                symbols: vec![symbol("api/handler.rs::handle", "handle")],
            },
            FileReport {
                path: "store/db.rs".to_string(),
                symbols: vec![symbol("store/db.rs::save", "save")],
            },
        ],
        graph: SymbolGraph {
            nodes: vec![
                node("api/handler.rs::handle", "api/handler.rs", "handle"),
                node("store/db.rs::save", "store/db.rs", "save"),
            ],
            edges: vec![
                Edge {
                    from: "api/handler.rs::handle".to_string(),
                    to: "store/db.rs::save".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "store/db.rs::save".to_string(),
                    to: "api/handler.rs::handle".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![],
        },
        ..empty_report()
    };
    let tree = crate::tree::build_tree(&report);

    let actual = build_dir_detail(&tree, &report, "api").expect("dir found");

    assert_eq!(vec!["store".to_string()], actual.cycle_partners);
    // Both directed edges touch "api" as an endpoint (api -> store and
    // store -> api), so both are part of the cycle explanation shown
    // for "api" — not just the one where "api" is the source.
    let expected_edges = vec![
        CycleEdgeView {
            from: "api/handler.rs::handle".to_string(),
            to: "store/db.rs::save".to_string(),
        },
        CycleEdgeView {
            from: "store/db.rs::save".to_string(),
            to: "api/handler.rs::handle".to_string(),
        },
    ];
    assert_eq!(expected_edges, actual.cycle_edges);
}
