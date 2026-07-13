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

// Uses `symbol` to document that `rank_directories` only reads
// `report.graph`, not `report.files`, even though a realistic `Report`
// always carries matching `files` alongside its `graph` — this pins
// that `rank_directories`' contract is graph-only, matching its own doc
// comment.

#[test]
fn should_ignore_files_field_and_rank_from_graph_alone() {
    let mut report = report_with_graph(vec![node("api/a.rs::a", "api/a.rs", "a")], vec![]);
    report.files = vec![FileReport {
        path: "api/a.rs".to_string(),
        symbols: vec![symbol("api/a.rs::a", "a")],
    }];

    let ranks = rank_directories(&report);

    assert_eq!(1, ranks.len());
    assert_eq!(0, ranks["api"].rank);
}
