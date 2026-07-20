use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_report_zero_test_count_when_symbol_has_no_referrers() {
    // A lone symbol with no referrers at all is still "changed and
    // untested" — compute_test_coverage must report it with an empty
    // covering_tests list, not omit it (unlike compute_fan_ins, which
    // only emits nodes that clear a threshold).
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec![])],
    }];
    let graph = build_graph(&files);

    let expected = vec![TestCoverage {
        id: "src/lib.rs::foo".to_string(),
        path: "src/lib.rs".to_string(),
        name: "foo".to_string(),
        covering_tests: vec![],
        test_count: 0,
    }];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_report_test_count_when_a_test_symbol_references_a_production_symbol() {
    let mut spec_shared = symbol("spec_shared", vec!["shared"]);
    spec_shared.is_test = true;
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![spec_shared, symbol("shared", vec![])],
    }];
    let graph = build_graph(&files);

    // "spec_shared" is itself test code, so it does not get its own
    // TestCoverage entry — only "shared" (the production symbol it
    // covers) does.
    let expected = vec![TestCoverage {
        id: "src/lib.rs::shared".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared".to_string(),
        covering_tests: vec!["src/lib.rs::spec_shared".to_string()],
        test_count: 1,
    }];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_production_referrers_from_covering_tests() {
    // "alpha" (production) and "spec" (test) both reference "shared" —
    // only "spec" may count toward "shared"'s test coverage, mirroring
    // compute_fan_ins's inverse filter (ADR 0042).
    let mut spec = symbol("spec", vec!["shared"]);
    spec.is_test = true;
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("alpha", vec!["shared"]),
            spec,
            symbol("shared", vec![]),
        ],
    }];
    let graph = build_graph(&files);

    let expected = vec![
        TestCoverage {
            id: "src/lib.rs::alpha".to_string(),
            path: "src/lib.rs".to_string(),
            name: "alpha".to_string(),
            covering_tests: vec![],
            test_count: 0,
        },
        TestCoverage {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            covering_tests: vec!["src/lib.rs::spec".to_string()],
            test_count: 1,
        },
    ];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_dedup_covering_test_when_same_test_references_target_more_than_once() {
    // `collect_edges` cannot currently produce two edges from the same
    // referrer to the same target (referenced_names de-dups upstream),
    // but compute_test_coverage must not over-count coverage if it ever
    // did — constructing the graph by hand to exercise that
    // defensively, same approach compute_fan_ins's own dedup test uses.
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::spec".to_string(),
                path: "src/lib.rs".to_string(),
                name: "spec".to_string(),
                is_test: true,
            },
            Node {
                id: "src/lib.rs::shared".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared".to_string(),
                is_test: false,
            },
        ],
        edges: vec![
            Edge {
                from: "src/lib.rs::spec".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::spec".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec!["src/lib.rs::spec".to_string()],
    };

    let expected = vec![TestCoverage {
        id: "src/lib.rs::shared".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared".to_string(),
        covering_tests: vec!["src/lib.rs::spec".to_string()],
        test_count: 1,
    }];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_sort_covering_tests_by_id_ascending_regardless_of_edge_order() {
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::spec_z".to_string(),
                path: "src/lib.rs".to_string(),
                name: "spec_z".to_string(),
                is_test: true,
            },
            Node {
                id: "src/lib.rs::spec_a".to_string(),
                path: "src/lib.rs".to_string(),
                name: "spec_a".to_string(),
                is_test: true,
            },
            Node {
                id: "src/lib.rs::shared".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared".to_string(),
                is_test: false,
            },
        ],
        edges: vec![
            Edge {
                from: "src/lib.rs::spec_z".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::spec_a".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec![
            "src/lib.rs::spec_z".to_string(),
            "src/lib.rs::spec_a".to_string(),
        ],
    };

    let expected = vec![TestCoverage {
        id: "src/lib.rs::shared".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared".to_string(),
        covering_tests: vec![
            "src/lib.rs::spec_a".to_string(),
            "src/lib.rs::spec_z".to_string(),
        ],
        test_count: 2,
    }];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_sort_results_by_test_count_ascending_then_path_name_id() {
    // "tested" has coverage (test_count 1); "untested_b" and
    // "untested_a" both have zero coverage — the untested symbols must
    // sort first (that's the whole point of this aggregation), tied
    // among themselves by path/name/id.
    let mut spec = symbol("spec", vec!["tested"]);
    spec.is_test = true;
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("tested", vec![]),
            spec,
            symbol("untested_b", vec![]),
            symbol("untested_a", vec![]),
        ],
    }];
    let graph = build_graph(&files);

    let expected = vec![
        TestCoverage {
            id: "src/lib.rs::untested_a".to_string(),
            path: "src/lib.rs".to_string(),
            name: "untested_a".to_string(),
            covering_tests: vec![],
            test_count: 0,
        },
        TestCoverage {
            id: "src/lib.rs::untested_b".to_string(),
            path: "src/lib.rs".to_string(),
            name: "untested_b".to_string(),
            covering_tests: vec![],
            test_count: 0,
        },
        TestCoverage {
            id: "src/lib.rs::tested".to_string(),
            path: "src/lib.rs".to_string(),
            name: "tested".to_string(),
            covering_tests: vec!["src/lib.rs::spec".to_string()],
            test_count: 1,
        },
    ];
    let actual = compute_test_coverage(&graph);

    assert_eq!(expected, actual);
}
