use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_graph_when_no_files() {
    let files: Vec<FileReport> = vec![];

    let expected = SymbolGraph {
        nodes: vec![],
        edges: vec![],
        roots: vec![],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_single_node_graph_with_itself_as_root_when_one_symbol_and_no_references() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec![])],
    }];

    let expected = SymbolGraph {
        nodes: vec![Node {
            id: "src/lib.rs::foo".to_string(),
            path: "src/lib.rs".to_string(),
            name: "foo".to_string(),
        }],
        edges: vec![],
        roots: vec!["src/lib.rs::foo".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_symbol_references_another_changed_symbol_by_name() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec![])],
    }];

    let expected = SymbolGraph {
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
        ],
        edges: vec![Edge {
            from: "src/lib.rs::foo".to_string(),
            to: "src/lib.rs::bar".to_string(),
            is_cycle: false,
        }],
        roots: vec!["src/lib.rs::foo".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_self_reference_edge_when_symbol_references_its_own_name() {
    // A struct's own name is captured as a `referenced_names` entry by
    // the extractor (see `extract::collect_referenced_names`'s doc
    // comment on self-references) — this must not produce a self-loop
    // edge.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("Point", vec!["Point"])],
    }];

    let expected = SymbolGraph {
        nodes: vec![Node {
            id: "src/lib.rs::Point".to_string(),
            path: "src/lib.rs".to_string(),
            name: "Point".to_string(),
        }],
        edges: vec![],
        roots: vec!["src/lib.rs::Point".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_disambiguate_node_id_with_start_line_when_duplicate_path_and_name() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            ExtractedSymbol {
                range: LineRange { start: 1, end: 2 },
                ..symbol("foo", vec![])
            },
            ExtractedSymbol {
                range: LineRange { start: 10, end: 12 },
                ..symbol("foo", vec![])
            },
        ],
    }];

    let expected_ids = vec![
        "src/lib.rs::foo@1".to_string(),
        "src/lib.rs::foo@10".to_string(),
    ];
    let actual = build_graph(&files);
    let actual_ids: Vec<NodeId> = actual.nodes.iter().map(|n| n.id.clone()).collect();

    assert_eq!(expected_ids, actual_ids);
}

#[test]
fn should_disambiguate_every_node_id_when_three_symbols_share_path_and_name() {
    // Guards against an off-by-one in the "more than 2" case: a naive
    // implementation could special-case pairs (e.g. compare only the
    // first two) and mishandle a third or later duplicate. All three
    // must get a distinct `@{start_line}`-suffixed id.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            ExtractedSymbol {
                range: LineRange { start: 1, end: 2 },
                ..symbol("foo", vec![])
            },
            ExtractedSymbol {
                range: LineRange { start: 10, end: 12 },
                ..symbol("foo", vec![])
            },
            ExtractedSymbol {
                range: LineRange { start: 20, end: 22 },
                ..symbol("foo", vec![])
            },
        ],
    }];

    let expected_ids = vec![
        "src/lib.rs::foo@1".to_string(),
        "src/lib.rs::foo@10".to_string(),
        "src/lib.rs::foo@20".to_string(),
    ];
    let actual = build_graph(&files);
    let actual_ids: Vec<NodeId> = actual.nodes.iter().map(|n| n.id.clone()).collect();

    assert_eq!(expected_ids, actual_ids);
}
