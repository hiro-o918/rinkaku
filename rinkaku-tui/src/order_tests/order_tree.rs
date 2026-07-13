use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_order_directories_before_files_at_the_same_level() {
    let mut tree = crate::tree::Tree {
        roots: vec![file_node("z.rs"), dir_node("a", vec![])],
    };

    order_tree(&mut tree, &HashMap::new(), OrderMode::AlphaNumeric);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["a", "z.rs"], paths);
}

#[test]
fn should_order_files_alphabetically_regardless_of_mode() {
    let mut tree = crate::tree::Tree {
        roots: vec![file_node("z.rs"), file_node("a.rs")],
    };

    order_tree(&mut tree, &HashMap::new(), OrderMode::Topological);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["a.rs", "z.rs"], paths);
}

#[test]
fn should_order_directories_by_rank_ascending_when_mode_is_topological() {
    let mut tree = crate::tree::Tree {
        roots: vec![dir_node("store", vec![]), dir_node("api", vec![])],
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        "api".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );
    ranks.insert(
        "store".to_string(),
        DirRank {
            rank: 1,
            in_cycle: false,
        },
    );

    order_tree(&mut tree, &ranks, OrderMode::Topological);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["api", "store"], paths);
}

#[test]
fn should_order_directories_alphabetically_when_mode_is_alpha_numeric_even_with_ranks_present() {
    let mut tree = crate::tree::Tree {
        roots: vec![dir_node("store", vec![]), dir_node("api", vec![])],
    };
    let mut ranks = HashMap::new();
    // Ranks say "store first" but AlphaNumeric mode must ignore them.
    ranks.insert(
        "store".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );
    ranks.insert(
        "api".to_string(),
        DirRank {
            rank: 1,
            in_cycle: false,
        },
    );

    order_tree(&mut tree, &ranks, OrderMode::AlphaNumeric);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["api", "store"], paths);
}

#[test]
fn should_sort_unranked_directory_after_ranked_ones() {
    let mut tree = crate::tree::Tree {
        roots: vec![dir_node("unranked", vec![]), dir_node("api", vec![])],
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        "api".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );
    // "unranked" has no entry in `ranks` at all.

    order_tree(&mut tree, &ranks, OrderMode::Topological);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["api", "unranked"], paths);
}

#[test]
fn should_order_nested_directory_children_recursively() {
    let mut tree = crate::tree::Tree {
        roots: vec![dir_node(
            "src",
            vec![dir_node("src/store", vec![]), dir_node("src/api", vec![])],
        )],
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        "src/api".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );
    ranks.insert(
        "src/store".to_string(),
        DirRank {
            rank: 1,
            in_cycle: false,
        },
    );

    order_tree(&mut tree, &ranks, OrderMode::Topological);

    let paths: Vec<&str> = tree.roots[0]
        .children
        .iter()
        .map(|n| n.path.as_str())
        .collect();
    assert_eq!(vec!["src/api", "src/store"], paths);
}

// INTEGRATION test: rank_directories -> order_tree end to end, on a
// real 3-level branching tree — the shape the unit test above (which
// hand-builds the `ranks` map) cannot catch. `rank_directories` only
// emits an entry for a *leaf* directory (the direct parent of a graph
// node's path); an intermediate/branching directory like "zzz" here
// owns no node directly (only its descendants "zzz/api"/"zzz/service"
// do). Without effective-rank propagation up through ancestors, "zzz"
// would have no rank at all and sort A-Z against "aaa" (i.e. after
// it, since "aaa" < "zzz") regardless of dependency direction.
//
// The whole graph here is a single connected chain (zzz/api ->
// zzz/service -> aaa/store) precisely to avoid any ambiguity from
// independent/unrelated SCCs sharing in-degree 0 — this test's only
// job is to pin down propagation through a branching intermediate
// directory, not `topological_scc_order`'s tie-break among unrelated
// components (covered separately, see the `tarjan_sccs`/
// `topological_scc_order` unit tests below). Package names are chosen
// so the correct topological order ("zzz" first, "aaa" last) disagrees
// with alphabetical order, making a regression to A-Z observable.

#[test]
fn should_order_full_tree_by_effective_rank_through_branching_intermediate_directories() {
    // zzz/api/handler.rs -> zzz/service/logic.rs -> aaa/store/db.rs:
    // "zzz" contains both the entry point (api, rank 0) and a middle
    // link (service, rank 1) — its effective rank must become 0 (the
    // minimum of its descendants) so it still sorts first despite
    // "aaa" < "zzz" alphabetically.
    let report = report_with_graph(
        vec![
            node("zzz/api/handler.rs::handle", "zzz/api/handler.rs", "handle"),
            node("zzz/service/logic.rs::run", "zzz/service/logic.rs", "run"),
            node("aaa/store/db.rs::save", "aaa/store/db.rs", "save"),
        ],
        vec![
            Edge {
                from: "zzz/api/handler.rs::handle".to_string(),
                to: "zzz/service/logic.rs::run".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "zzz/service/logic.rs::run".to_string(),
                to: "aaa/store/db.rs::save".to_string(),
                is_cycle: false,
            },
        ],
    );
    let ranks = rank_directories(&report);

    // Build the corresponding tree by hand (mirrors what
    // `tree::build_tree` would produce for this Report's files, minus
    // badges which this test doesn't care about): two top-level
    // packages, "zzz" branching into two subdirectories, "aaa" with
    // one.
    let mut tree = crate::tree::Tree {
        roots: vec![
            dir_node(
                "aaa",
                vec![dir_node(
                    "aaa/store",
                    vec![file_node_with_children("aaa/store/db.rs", vec![])],
                )],
            ),
            dir_node(
                "zzz",
                vec![
                    dir_node(
                        "zzz/service",
                        vec![file_node_with_children("zzz/service/logic.rs", vec![])],
                    ),
                    dir_node(
                        "zzz/api",
                        vec![file_node_with_children("zzz/api/handler.rs", vec![])],
                    ),
                ],
            ),
        ],
    };

    order_tree(&mut tree, &ranks, OrderMode::Topological);

    let expected = crate::tree::Tree {
        roots: vec![
            dir_node(
                "zzz",
                vec![
                    dir_node(
                        "zzz/api",
                        vec![file_node_with_children("zzz/api/handler.rs", vec![])],
                    ),
                    dir_node(
                        "zzz/service",
                        vec![file_node_with_children("zzz/service/logic.rs", vec![])],
                    ),
                ],
            ),
            dir_node(
                "aaa",
                vec![dir_node(
                    "aaa/store",
                    vec![file_node_with_children("aaa/store/db.rs", vec![])],
                )],
            ),
        ],
    };
    assert_eq!(expected, tree);
}
