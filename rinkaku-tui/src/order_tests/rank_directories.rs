use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_ranking_when_graph_has_no_nodes() {
    let report = report_with_graph(vec![], vec![]);

    let expected: HashMap<String, DirRank> = HashMap::new();
    let actual = rank_directories(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_rank_directory_with_no_edges_at_rank_zero_and_not_in_cycle() {
    let report = report_with_graph(vec![node("api/lib.rs::foo", "api/lib.rs", "foo")], vec![]);

    let mut expected = HashMap::new();
    expected.insert(
        "api".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );
    let actual = rank_directories(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_rank_caller_directory_before_callee_directory() {
    // api/ calls store/: api is the entry point (0 incoming
    // inter-directory edges), store is the foundation.
    let report = report_with_graph(
        vec![
            node("api/handler.rs::handle", "api/handler.rs", "handle"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![Edge {
            from: "api/handler.rs::handle".to_string(),
            to: "store/db.rs::save".to_string(),
            is_cycle: false,
        }],
    );

    let ranks = rank_directories(&report);

    let api_rank = ranks["api"].rank;
    let store_rank = ranks["store"].rank;
    assert!(
        api_rank < store_rank,
        "expected api ({api_rank}) to rank before store ({store_rank})"
    );
    assert_eq!(false, ranks["api"].in_cycle);
    assert_eq!(false, ranks["store"].in_cycle);
}

#[test]
fn should_drop_edge_and_still_rank_zero_when_both_endpoints_share_a_directory() {
    // Both symbols live in the same directory ("api"), so the edge
    // between them condenses to a self-loop and must be dropped —
    // otherwise "api" would wrongly show up as depending on itself.
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("api/b.rs::b", "api/b.rs", "b"),
        ],
        vec![Edge {
            from: "api/a.rs::a".to_string(),
            to: "api/b.rs::b".to_string(),
            is_cycle: false,
        }],
    );

    let expected = {
        let mut m = HashMap::new();
        m.insert(
            "api".to_string(),
            DirRank {
                rank: 0,
                in_cycle: false,
            },
        );
        m
    };
    let actual = rank_directories(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_mark_directories_in_cycle_and_give_them_the_same_rank() {
    // api/ and store/ depend on each other — a directory-level cycle.
    let report = report_with_graph(
        vec![
            node("api/handler.rs::handle", "api/handler.rs", "handle"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![
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
    );

    let ranks = rank_directories(&report);

    assert_eq!(ranks["api"].rank, ranks["store"].rank);
    assert_eq!(true, ranks["api"].in_cycle);
    assert_eq!(true, ranks["store"].in_cycle);
}

#[test]
fn should_rank_root_level_file_directory_as_empty_string() {
    let report = report_with_graph(vec![node("lib.rs::foo", "lib.rs", "foo")], vec![]);

    let expected = {
        let mut m = HashMap::new();
        m.insert(
            String::new(),
            DirRank {
                rank: 0,
                in_cycle: false,
            },
        );
        m
    };
    let actual = rank_directories(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_chain_three_directories_in_dependency_order() {
    // api -> service -> store: three distinct ranks, strictly
    // increasing in that dependency order.
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("service/s.rs::s", "service/s.rs", "s"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![
            Edge {
                from: "api/a.rs::a".to_string(),
                to: "service/s.rs::s".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "service/s.rs::s".to_string(),
                to: "store/db.rs::save".to_string(),
                is_cycle: false,
            },
        ],
    );

    let ranks = rank_directories(&report);

    assert_eq!(0, ranks["api"].rank);
    assert_eq!(1, ranks["service"].rank);
    assert_eq!(2, ranks["store"].rank);
}

// Uses `symbol` (non-test) to document that `rank_directories` produces
// the same ranking whether or not `report.files` carries a matching,
// non-test entry for a graph node — `files` is only ever consulted to
// find *test*-flagged symbols to exclude (ADR 0035), never to influence
// the ranking of an ordinary, non-test node. A `files` entry for a node
// that is not test code is therefore a no-op input as far as
// `rank_directories` is concerned, matching its pre-ADR-0035 contract of
// ranking from `report.graph` alone in that case.

#[test]
fn should_rank_the_same_whether_or_not_files_has_a_matching_non_test_entry() {
    let mut report = report_with_graph(vec![node("api/a.rs::a", "api/a.rs", "a")], vec![]);
    report.files = vec![FileReport {
        path: "api/a.rs".to_string(),
        symbols: vec![symbol("api/a.rs::a", "a")],
    }];

    let ranks = rank_directories(&report);

    assert_eq!(1, ranks.len());
    assert_eq!(0, ranks["api"].rank);
}

// The following tests pin ADR 0035's rank-exclusion behavior: a node or
// edge whose owning symbol is test code (`ExtractedSymbol::is_test`,
// looked up via `report.files` by matching `graph::Node::id` against
// `ExtractedSymbol::id` — the two are the same stable id after
// `graph::stamp_ids` runs, per `graph.rs`'s own doc comment) is dropped
// from the condensation before Tarjan/Kahn ever see it, so it cannot
// affect any *production* directory's rank.

#[test]
fn should_omit_directory_whose_only_graph_presence_is_a_test_symbol() {
    let report = report_with_graph_and_files(
        vec![node(
            "api/handler_test.rs::test_it",
            "api/handler_test.rs",
            "test_it",
        )],
        vec![],
        vec![FileReport {
            path: "api/handler_test.rs".to_string(),
            symbols: vec![test_symbol("api/handler_test.rs::test_it", "test_it")],
        }],
    );

    let ranks = rank_directories(&report);

    assert_eq!(HashMap::new(), ranks);
}

#[test]
fn should_not_let_inbound_test_edge_affect_production_directory_rank() {
    // A test in `api/` references a production symbol in `store/`. Under
    // the pre-ADR-0035 behavior this edge would give `store` an inbound
    // reference from `api`, making `api` (in-degree 0) rank before
    // `store` (in-degree 1) even though no *production* code in `api`
    // depends on anything — purely a test-authored dependency. After
    // exclusion, the test node/edge are dropped entirely, `store` has no
    // remaining inbound edges either, and each directory ranks
    // independently at rank 0 (no relative order asserted between them,
    // since with the test edge gone there is nothing left to order them
    // by).
    let report = report_with_graph_and_files(
        vec![
            node(
                "api/handler_test.rs::test_it",
                "api/handler_test.rs",
                "test_it",
            ),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![Edge {
            from: "api/handler_test.rs::test_it".to_string(),
            to: "store/db.rs::save".to_string(),
            is_cycle: false,
        }],
        vec![
            FileReport {
                path: "api/handler_test.rs".to_string(),
                symbols: vec![test_symbol("api/handler_test.rs::test_it", "test_it")],
            },
            FileReport {
                path: "store/db.rs".to_string(),
                symbols: vec![symbol("store/db.rs::save", "save")],
            },
        ],
    );

    let ranks = rank_directories(&report);

    let expected = {
        let mut m = HashMap::new();
        m.insert(
            "store".to_string(),
            DirRank {
                rank: 0,
                in_cycle: false,
            },
        );
        m
    };
    assert_eq!(expected, ranks);
}

#[test]
fn should_rank_production_directory_by_production_edges_alone_when_mixed_with_test_edges() {
    // `service/` has a real production dependency on `store/`, plus a
    // test-authored edge from `api/`'s test file into `service/`. The
    // production ranking (`api` semantics aside — `api` here has no
    // production node at all) must come out identical to the
    // test-free case: `service` before `store`.
    let report = report_with_graph_and_files(
        vec![
            node(
                "api/handler_test.rs::test_it",
                "api/handler_test.rs",
                "test_it",
            ),
            node("service/s.rs::s", "service/s.rs", "s"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![
            Edge {
                from: "api/handler_test.rs::test_it".to_string(),
                to: "service/s.rs::s".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "service/s.rs::s".to_string(),
                to: "store/db.rs::save".to_string(),
                is_cycle: false,
            },
        ],
        vec![
            FileReport {
                path: "api/handler_test.rs".to_string(),
                symbols: vec![test_symbol("api/handler_test.rs::test_it", "test_it")],
            },
            FileReport {
                path: "service/s.rs".to_string(),
                symbols: vec![symbol("service/s.rs::s", "s")],
            },
            FileReport {
                path: "store/db.rs".to_string(),
                symbols: vec![symbol("store/db.rs::save", "save")],
            },
        ],
    );

    let ranks = rank_directories(&report);

    assert_eq!(2, ranks.len());
    assert_eq!(0, ranks["service"].rank);
    assert_eq!(1, ranks["store"].rank);
}
