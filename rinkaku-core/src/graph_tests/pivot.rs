use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_roots_when_no_node_matches_the_prefix() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec![])],
    }];
    let graph = build_graph(&files);

    let expected: Vec<NodeId> = vec![];
    let actual = pivot_roots(&graph, "other/path");

    assert_eq!(expected, actual);
}

#[test]
fn should_pivot_root_at_prefix_node_when_prefix_node_has_no_in_subset_referrer() {
    // "api" (under src/api) references "helper" (under src/util) and
    // "helper" is also referenced from outside src/api by "caller"
    // (src/other.rs). Pivoting at "src/api" must yield "api" as the
    // sole root: it has no referrer *within* src/api, and the
    // outside-in edge from "caller" must not disqualify "helper" either
    // way since "helper" itself is outside the subset and therefore
    // excluded from the pivoted root candidates entirely.
    let files = vec![
        FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("api", vec!["helper"])],
        },
        FileReport {
            path: "src/util.rs".to_string(),
            symbols: vec![symbol("helper", vec![])],
        },
        FileReport {
            path: "src/other.rs".to_string(),
            symbols: vec![symbol("caller", vec!["helper"])],
        },
    ];
    let graph = build_graph(&files);

    let expected = vec!["src/api/handler.rs::api".to_string()];
    let actual = pivot_roots(&graph, "src/api");

    assert_eq!(expected, actual);
}

#[test]
fn should_drop_node_from_pivot_roots_when_it_is_depended_on_by_another_node_in_the_same_prefix() {
    // Both "outer" and "inner" live under "src/api"; "outer" references
    // "inner", so "inner" must not be a pivot root (it has an in-subset
    // referrer) even though nothing *outside* src/api depends on it.
    let files = vec![FileReport {
        path: "src/api/handler.rs".to_string(),
        symbols: vec![symbol("outer", vec!["inner"]), symbol("inner", vec![])],
    }];
    let graph = build_graph(&files);

    let expected = vec!["src/api/handler.rs::outer".to_string()];
    let actual = pivot_roots(&graph, "src/api");

    assert_eq!(expected, actual);
}

#[test]
fn should_respect_directory_boundary_when_pivoting_at_a_prefix() {
    // "src/api" must match "src/api/handler.rs" but not
    // "src/api2/other.rs" — a naive `starts_with` would wrongly include
    // the latter.
    let files = vec![
        FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("api", vec![])],
        },
        FileReport {
            path: "src/api2/other.rs".to_string(),
            symbols: vec![symbol("other", vec![])],
        },
    ];
    let graph = build_graph(&files);

    let expected = vec!["src/api/handler.rs::api".to_string()];
    let actual = pivot_roots(&graph, "src/api");

    assert_eq!(expected, actual);
}

#[test]
fn should_match_exact_file_path_when_prefix_names_a_file_directly() {
    let files = vec![FileReport {
        path: "src/api/handler.rs".to_string(),
        symbols: vec![symbol("api", vec![])],
    }];
    let graph = build_graph(&files);

    let expected = vec!["src/api/handler.rs::api".to_string()];
    let actual = pivot_roots(&graph, "src/api/handler.rs");

    assert_eq!(expected, actual);
}

#[test]
fn should_find_scc_representative_as_pivot_root_when_two_prefix_nodes_form_a_cycle() {
    // "foo" and "bar" (both under src/api) reference each other: the
    // SCC condensation rule (ADR 0008, applied to the subset per this
    // ADR) must still surface one representative rather than zero
    // roots, matching find_roots's whole-graph behavior.
    let files = vec![FileReport {
        path: "src/api/handler.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec!["foo"])],
    }];
    let graph = build_graph(&files);

    let expected = vec!["src/api/handler.rs::foo".to_string()];
    let actual = pivot_roots(&graph, "src/api");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_same_nodes_and_edges_with_new_roots_when_pivoting_graph() {
    let files = vec![
        FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("api", vec!["helper"])],
        },
        FileReport {
            path: "src/util.rs".to_string(),
            symbols: vec![symbol("helper", vec![])],
        },
    ];
    let graph = build_graph(&files);

    let actual = pivot_graph(&graph, "src/api");

    let expected = SymbolGraph {
        nodes: graph.nodes.clone(),
        edges: graph.edges.clone(),
        roots: vec!["src/api/handler.rs::api".to_string()],
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_mark_cycle_edge_relative_to_pivot_root_when_re_rooting_across_a_cycle() {
    // foo -> bar -> baz -> foo, all under src/api. The whole-graph DFS
    // (rooted at "foo", its only default root) marks baz -> foo as the
    // cycle edge. Pivoting at "src/api/other.rs" — a sibling file with
    // its own entry "baz2" that also references "foo" — changes which
    // node the pivot DFS starts from ("baz2"), so cycle marking must be
    // recomputed relative to the *new* root rather than reusing the
    // whole-graph graph's stale cycle marks.
    let files = vec![
        FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![
                symbol("foo", vec!["bar"]),
                symbol("bar", vec!["baz"]),
                symbol("baz", vec!["foo"]),
            ],
        },
        FileReport {
            path: "src/api/other.rs".to_string(),
            symbols: vec![symbol("baz2", vec!["foo"])],
        },
    ];
    let graph = build_graph(&files);

    let actual = pivot_graph(&graph, "src/api");

    // "baz2" is now the sole pivot root (nothing in src/api references
    // it), so the DFS walks baz2 -> foo -> bar -> baz -> foo, and the
    // *last* foo edge (baz -> foo) is still the one that closes the
    // loop and must be marked as a cycle.
    let expected = SymbolGraph {
        nodes: graph.nodes.clone(),
        edges: vec![
            Edge {
                from: "src/api/handler.rs::foo".to_string(),
                to: "src/api/handler.rs::bar".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/api/handler.rs::bar".to_string(),
                to: "src/api/handler.rs::baz".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/api/handler.rs::baz".to_string(),
                to: "src/api/handler.rs::foo".to_string(),
                is_cycle: true,
            },
            Edge {
                from: "src/api/other.rs::baz2".to_string(),
                to: "src/api/handler.rs::foo".to_string(),
                is_cycle: false,
            },
        ],
        roots: vec!["src/api/other.rs::baz2".to_string()],
    };
    assert_eq!(expected, actual);
}

#[test]
fn should_match_every_path_when_prefix_is_empty() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec![])],
    }];
    let graph = build_graph(&files);

    let expected = vec!["src/lib.rs::foo".to_string()];
    let actual = pivot_roots(&graph, "");

    assert_eq!(expected, actual);
}
