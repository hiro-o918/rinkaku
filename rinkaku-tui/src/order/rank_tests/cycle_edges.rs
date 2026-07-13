use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_cross_directory_edges_forming_a_two_directory_cycle() {
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

    let mut edges = cycle_edges(&report);
    edges.sort_by(|a, b| a.from_path.cmp(&b.from_path));

    let expected = vec![
        CycleEdge {
            from_path: "api/handler.rs".to_string(),
            from_name: "handle".to_string(),
            to_path: "store/db.rs".to_string(),
            to_name: "save".to_string(),
        },
        CycleEdge {
            from_path: "store/db.rs".to_string(),
            from_name: "save".to_string(),
            to_path: "api/handler.rs".to_string(),
            to_name: "handle".to_string(),
        },
    ];
    assert_eq!(expected, edges);
}

#[test]
fn should_exclude_edge_from_cycle_edges_when_directories_do_not_cycle() {
    // api -> store, but store does not depend back on api: no cycle,
    // so this edge must not show up as a "cycle edge".
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![Edge {
            from: "api/a.rs::a".to_string(),
            to: "store/db.rs::save".to_string(),
            is_cycle: false,
        }],
    );

    let edges = cycle_edges(&report);

    assert_eq!(Vec::<CycleEdge>::new(), edges);
}

#[test]
fn should_exclude_intra_directory_edge_from_cycle_edges() {
    // Both symbols live in "api" itself; even if api participates in a
    // cycle with another directory, an edge fully inside api is not a
    // cross-directory cycle edge.
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("api/b.rs::b", "api/b.rs", "b"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![
            Edge {
                from: "api/a.rs::a".to_string(),
                to: "api/b.rs::b".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "api/a.rs::a".to_string(),
                to: "store/db.rs::save".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "store/db.rs::save".to_string(),
                to: "api/a.rs::a".to_string(),
                is_cycle: false,
            },
        ],
    );

    let edges = cycle_edges(&report);

    // Only the two cross-directory edges (api <-> store) qualify; the
    // intra-"api" edge (a -> b) must be excluded.
    assert_eq!(2, edges.len());
    assert!(edges.iter().all(|e| e.from_path != e.to_path));
}
