use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_no_fan_ins_when_every_node_has_fan_in_below_two() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec![])],
    }];
    let graph = build_graph(&files);

    let expected: Vec<FanIn> = vec![];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_count_fan_in_and_sort_referrer_names_when_two_symbols_reference_one_target() {
    // "zoo" and "alpha" both reference "shared" — fan-in 2 qualifies as
    // a high-fan-in symbol, and `used_by` must come back name-sorted
    // ("alpha" before "zoo") regardless of edge order.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("zoo", vec!["shared"]),
            symbol("alpha", vec!["shared"]),
            symbol("shared", vec![]),
        ],
    }];
    let graph = build_graph(&files);

    let expected = vec![FanIn {
        id: "src/lib.rs::shared".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared".to_string(),
        used_by: vec!["alpha".to_string(), "zoo".to_string()],
    }];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_count_cycle_edge_toward_fan_in_when_referrer_is_part_of_a_cycle() {
    // "foo" and "bar" reference each other (mutual recursion, so one of
    // the two edges gets marked `is_cycle: true` by `build_graph`) and
    // "baz" independently references "bar" too — "bar" ends up with
    // fan-in 2 (from "foo" and "baz"), and the cycle edge must count
    // toward that just like a non-cycle edge would.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("foo", vec!["bar"]),
            symbol("bar", vec!["foo"]),
            symbol("baz", vec!["bar"]),
        ],
    }];
    let graph = build_graph(&files);
    // Sanity check on the fixture: confirm build_graph actually marked
    // one of foo<->bar's edges as a cycle, since this test's premise
    // depends on that.
    assert!(graph.edges.iter().any(|e| e.is_cycle));

    let expected = vec![FanIn {
        id: "src/lib.rs::bar".to_string(),
        path: "src/lib.rs".to_string(),
        name: "bar".to_string(),
        used_by: vec!["baz".to_string(), "foo".to_string()],
    }];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_dedup_referrer_when_same_node_references_target_more_than_once() {
    // `collect_edges` cannot currently produce two edges from the same
    // referrer to the same target (referenced_names de-dups upstream),
    // but `compute_fan_ins` must not over-count fan-in if it ever did
    // — constructing the graph by hand here rather than through
    // `build_graph` to exercise that defensively.
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::foo".to_string(),
                path: "src/lib.rs".to_string(),
                name: "foo".to_string(),
            },
            Node {
                id: "src/lib.rs::bar".to_string(),
                path: "src/lib.rs".to_string(),
                name: "bar".to_string(),
            },
            Node {
                id: "src/lib.rs::shared".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared".to_string(),
            },
        ],
        edges: vec![
            Edge {
                from: "src/lib.rs::foo".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::foo".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::bar".to_string(),
                to: "src/lib.rs::shared".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
    };

    let expected = vec![FanIn {
        id: "src/lib.rs::shared".to_string(),
        path: "src/lib.rs".to_string(),
        name: "shared".to_string(),
        used_by: vec!["bar".to_string(), "foo".to_string()],
    }];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_sort_fan_ins_by_fan_in_descending_when_multiple_fan_ins_exist() {
    // "low" has fan-in 2 ("a", "b"); "high" has fan-in 3 ("c", "d",
    // "e") — "high" must sort first despite "low" being discovered
    // first in edge order.
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::a".to_string(),
                path: "src/lib.rs".to_string(),
                name: "a".to_string(),
            },
            Node {
                id: "src/lib.rs::b".to_string(),
                path: "src/lib.rs".to_string(),
                name: "b".to_string(),
            },
            Node {
                id: "src/lib.rs::low".to_string(),
                path: "src/lib.rs".to_string(),
                name: "low".to_string(),
            },
            Node {
                id: "src/lib.rs::c".to_string(),
                path: "src/lib.rs".to_string(),
                name: "c".to_string(),
            },
            Node {
                id: "src/lib.rs::d".to_string(),
                path: "src/lib.rs".to_string(),
                name: "d".to_string(),
            },
            Node {
                id: "src/lib.rs::e".to_string(),
                path: "src/lib.rs".to_string(),
                name: "e".to_string(),
            },
            Node {
                id: "src/lib.rs::high".to_string(),
                path: "src/lib.rs".to_string(),
                name: "high".to_string(),
            },
        ],
        edges: vec![
            Edge {
                from: "src/lib.rs::a".to_string(),
                to: "src/lib.rs::low".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::b".to_string(),
                to: "src/lib.rs::low".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::c".to_string(),
                to: "src/lib.rs::high".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::d".to_string(),
                to: "src/lib.rs::high".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/lib.rs::e".to_string(),
                to: "src/lib.rs::high".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec![
            "src/lib.rs::a".to_string(),
            "src/lib.rs::b".to_string(),
            "src/lib.rs::c".to_string(),
            "src/lib.rs::d".to_string(),
            "src/lib.rs::e".to_string(),
        ],
    };

    let expected = vec![
        FanIn {
            id: "src/lib.rs::high".to_string(),
            path: "src/lib.rs".to_string(),
            name: "high".to_string(),
            used_by: vec!["c".to_string(), "d".to_string(), "e".to_string()],
        },
        FanIn {
            id: "src/lib.rs::low".to_string(),
            path: "src/lib.rs".to_string(),
            name: "low".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        },
    ];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_break_fan_in_tie_by_id_when_path_and_name_are_also_equal() {
    // Two distinct symbols share the same (path, name) — e.g. two
    // overloaded `helper` functions in the same file, disambiguated only
    // by `@line` in `id` (see `collect_nodes`'s doc comment) — and both
    // reach fan-in 2. Without an id-based tie-break, `HashMap` iteration
    // order in `compute_fan_ins` decides the order non-deterministically
    // across runs; the id ("a.rs::helper@1" before "a.rs::helper@9")
    // must fix it instead.
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "a.rs::helper@9".to_string(),
                path: "a.rs".to_string(),
                name: "helper".to_string(),
            },
            Node {
                id: "a.rs::helper@1".to_string(),
                path: "a.rs".to_string(),
                name: "helper".to_string(),
            },
            Node {
                id: "a.rs::x".to_string(),
                path: "a.rs".to_string(),
                name: "x".to_string(),
            },
            Node {
                id: "a.rs::y".to_string(),
                path: "a.rs".to_string(),
                name: "y".to_string(),
            },
            Node {
                id: "a.rs::m".to_string(),
                path: "a.rs".to_string(),
                name: "m".to_string(),
            },
            Node {
                id: "a.rs::n".to_string(),
                path: "a.rs".to_string(),
                name: "n".to_string(),
            },
        ],
        edges: vec![
            Edge {
                from: "a.rs::x".to_string(),
                to: "a.rs::helper@9".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "a.rs::y".to_string(),
                to: "a.rs::helper@9".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "a.rs::m".to_string(),
                to: "a.rs::helper@1".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "a.rs::n".to_string(),
                to: "a.rs::helper@1".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec![
            "a.rs::x".to_string(),
            "a.rs::y".to_string(),
            "a.rs::m".to_string(),
            "a.rs::n".to_string(),
        ],
    };

    let expected = vec![
        FanIn {
            id: "a.rs::helper@1".to_string(),
            path: "a.rs".to_string(),
            name: "helper".to_string(),
            used_by: vec!["m".to_string(), "n".to_string()],
        },
        FanIn {
            id: "a.rs::helper@9".to_string(),
            path: "a.rs".to_string(),
            name: "helper".to_string(),
            used_by: vec!["x".to_string(), "y".to_string()],
        },
    ];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}

#[test]
fn should_break_fan_in_tie_by_path_then_name_when_counts_are_equal() {
    // Both "b_target" (path b.rs) and "a_target" (path a.rs) have
    // fan-in 2 — must sort a.rs before b.rs by path, not by discovery
    // order in the edges list.
    let graph = SymbolGraph {
        nodes: vec![
            Node {
                id: "b.rs::b_target".to_string(),
                path: "b.rs".to_string(),
                name: "b_target".to_string(),
            },
            Node {
                id: "a.rs::a_target".to_string(),
                path: "a.rs".to_string(),
                name: "a_target".to_string(),
            },
            Node {
                id: "b.rs::x".to_string(),
                path: "b.rs".to_string(),
                name: "x".to_string(),
            },
            Node {
                id: "b.rs::y".to_string(),
                path: "b.rs".to_string(),
                name: "y".to_string(),
            },
            Node {
                id: "a.rs::m".to_string(),
                path: "a.rs".to_string(),
                name: "m".to_string(),
            },
            Node {
                id: "a.rs::n".to_string(),
                path: "a.rs".to_string(),
                name: "n".to_string(),
            },
        ],
        edges: vec![
            Edge {
                from: "b.rs::x".to_string(),
                to: "b.rs::b_target".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "b.rs::y".to_string(),
                to: "b.rs::b_target".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "a.rs::m".to_string(),
                to: "a.rs::a_target".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "a.rs::n".to_string(),
                to: "a.rs::a_target".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec![
            "b.rs::x".to_string(),
            "b.rs::y".to_string(),
            "a.rs::m".to_string(),
            "a.rs::n".to_string(),
        ],
    };

    let expected = vec![
        FanIn {
            id: "a.rs::a_target".to_string(),
            path: "a.rs".to_string(),
            name: "a_target".to_string(),
            used_by: vec!["m".to_string(), "n".to_string()],
        },
        FanIn {
            id: "b.rs::b_target".to_string(),
            path: "b.rs".to_string(),
            name: "b_target".to_string(),
            used_by: vec!["x".to_string(), "y".to_string()],
        },
    ];
    let actual = compute_fan_ins(&graph);

    assert_eq!(expected, actual);
}
