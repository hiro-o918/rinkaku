use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_find_root_via_scc_representative_when_two_symbols_reference_each_other() {
    // `foo` and `bar` reference each other (mutual recursion): neither
    // has an in-degree-0 raw node, but their SCC as a whole has no
    // incoming edge from outside, so the SCC's first-in-source-order
    // member ("foo") becomes the root.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec!["foo"])],
    }];

    let expected_roots = vec!["src/lib.rs::foo".to_string()];
    let actual = build_graph(&files);

    assert_eq!(expected_roots, actual.roots);
}

#[test]
fn should_mark_back_edge_as_cycle_when_two_symbols_reference_each_other() {
    // "foo" -> "bar" is the forward (tree) edge from the DFS root
    // ("foo"); "bar" -> "foo" is the back edge that closes the loop and
    // must be marked `is_cycle: true`. The forward edge is a normal
    // dependency edge, not a cycle edge itself.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec!["foo"])],
    }];

    let expected_edges = vec![
        Edge {
            from: "src/lib.rs::foo".to_string(),
            to: "src/lib.rs::bar".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/lib.rs::bar".to_string(),
            to: "src/lib.rs::foo".to_string(),
            is_cycle: true,
        },
    ];
    let actual = build_graph(&files);

    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_mark_self_referencing_cycle_edge_when_three_symbols_form_a_cycle() {
    // foo -> bar -> baz -> foo: a 3-node cycle. DFS starts at "foo"
    // (its only root, since the whole SCC has no incoming edge from
    // outside), walks foo -> bar -> baz, and baz -> foo is the back
    // edge.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("foo", vec!["bar"]),
            symbol("bar", vec!["baz"]),
            symbol("baz", vec!["foo"]),
        ],
    }];

    let expected_edges = vec![
        Edge {
            from: "src/lib.rs::foo".to_string(),
            to: "src/lib.rs::bar".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/lib.rs::bar".to_string(),
            to: "src/lib.rs::baz".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/lib.rs::baz".to_string(),
            to: "src/lib.rs::foo".to_string(),
            is_cycle: true,
        },
    ];
    let actual = build_graph(&files);

    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_not_mark_edge_as_cycle_when_two_roots_both_reach_a_shared_node() {
    // "shared" is reachable from both "foo" and "bar" (a diamond, not a
    // cycle): the second edge into "shared" is a cross/forward edge in
    // DFS terms, not a back edge, so it must stay `is_cycle: false`.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("foo", vec!["shared"]),
            symbol("bar", vec!["shared"]),
            symbol("shared", vec![]),
        ],
    }];

    let expected_edges = vec![
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
    ];
    let actual = build_graph(&files);

    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_find_multiple_roots_when_two_independent_entry_points_exist() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            symbol("foo", vec!["shared"]),
            symbol("bar", vec!["shared"]),
            symbol("shared", vec![]),
        ],
    }];

    let expected_roots = vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()];
    let actual = build_graph(&files);

    assert_eq!(expected_roots, actual.roots);
}
