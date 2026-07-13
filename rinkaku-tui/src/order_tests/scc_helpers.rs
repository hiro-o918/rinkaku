use super::*;
use pretty_assertions::assert_eq;

// The `rank_directories`-level tests above exercise `tarjan_sccs` and
// `topological_scc_order` only indirectly, through a full directory
// condensation built from a `Report`. These tests instead drive the two
// private helpers directly with hand-built adjacency lists, so a future
// change to either algorithm gets pinned down at the shape it actually
// operates on (plain `usize` adjacency), not just observed through the
// directory-name-shaped output several layers up.

#[test]
fn should_split_into_singleton_sccs_when_graph_is_a_diamond() {
    // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3: acyclic diamond, so every node is
    // its own SCC (no back edges to merge any of them together).
    let adjacency = vec![vec![1, 2], vec![3], vec![3], vec![]];

    let mut sccs = tarjan_sccs(&adjacency);
    for scc in &mut sccs {
        scc.sort_unstable();
    }
    sccs.sort_by_key(|scc| scc[0]);

    assert_eq!(vec![vec![0], vec![1], vec![2], vec![3]], sccs);
}

#[test]
fn should_group_mutually_reachable_nodes_into_one_scc_when_graph_has_two_independent_cycles() {
    // 0 <-> 1 and 2 <-> 3, with no edges between the two pairs: two
    // independent 2-node SCCs, neither depending on the other.
    let adjacency = vec![vec![1], vec![0], vec![3], vec![2]];

    let mut sccs = tarjan_sccs(&adjacency);
    for scc in &mut sccs {
        scc.sort_unstable();
    }
    sccs.sort_by_key(|scc| scc[0]);

    assert_eq!(vec![vec![0, 1], vec![2, 3]], sccs);
}

#[test]
fn should_isolate_cycle_from_surrounding_acyclic_nodes_when_graph_mixes_both() {
    // 0 -> 1 -> 2 -> 1 -> 3: nodes 1 and 2 form a cycle (1 -> 2 -> 1),
    // while 0 (entry) and 3 (reached only from the cycle) stay acyclic
    // singletons on either side of it.
    let adjacency = vec![vec![1], vec![2], vec![1, 3], vec![]];

    let mut sccs = tarjan_sccs(&adjacency);
    for scc in &mut sccs {
        scc.sort_unstable();
    }
    sccs.sort_by_key(|scc| scc[0]);

    assert_eq!(vec![vec![0], vec![1, 2], vec![3]], sccs);
}

#[test]
fn should_order_diamond_sccs_with_entry_first_and_join_last() {
    // Same diamond as the tarjan_sccs test above: 0 -> {1, 2} -> 3.
    // Every node is its own singleton SCC; topological order must place
    // 0 first (the only in-degree-0 SCC) and 3 last (depended on by
    // both 1 and 2), with 1 before 2 as the deterministic tie-break
    // (smaller member index first) since neither depends on the other.
    let adjacency = vec![vec![1, 2], vec![3], vec![3], vec![]];
    let sccs: Vec<Vec<usize>> = vec![vec![0], vec![1], vec![2], vec![3]];
    let mut scc_of = vec![0usize; 4];
    for (scc_index, scc) in sccs.iter().enumerate() {
        for &node_index in scc {
            scc_of[node_index] = scc_index;
        }
    }

    let order = topological_scc_order(&sccs, &scc_of, &adjacency);

    assert_eq!(vec![0, 1, 2, 3], order);
}

#[test]
fn should_order_two_independent_sccs_by_smallest_member_when_neither_depends_on_the_other() {
    // {0, 1} and {2, 3} are independent SCCs (no edges between them) —
    // both have in-degree 0 in the condensation, so the tie-break
    // (smallest member index) alone decides that {0, 1} orders first.
    let adjacency = vec![vec![1], vec![0], vec![3], vec![2]];
    let sccs: Vec<Vec<usize>> = vec![vec![0, 1], vec![2, 3]];
    let mut scc_of = vec![0usize; 4];
    for (scc_index, scc) in sccs.iter().enumerate() {
        for &node_index in scc {
            scc_of[node_index] = scc_index;
        }
    }

    let order = topological_scc_order(&sccs, &scc_of, &adjacency);

    assert_eq!(vec![0, 1], order);
}

#[test]
fn should_order_cycle_between_its_acyclic_neighbors_when_graph_mixes_both() {
    // Same mixed graph as the tarjan_sccs test above: 0 -> {1, 2} (a
    // cycle) -> 3. The condensation is 0 -> scc{1,2} -> 3, so the
    // topological order must be exactly [0, scc{1,2}, 3] regardless of
    // which of 1/2 tarjan_sccs happened to list first inside the SCC.
    let adjacency = vec![vec![1], vec![2], vec![1, 3], vec![]];
    let sccs: Vec<Vec<usize>> = vec![vec![0], vec![1, 2], vec![3]];
    let mut scc_of = vec![0usize; 4];
    for (scc_index, scc) in sccs.iter().enumerate() {
        for &node_index in scc {
            scc_of[node_index] = scc_index;
        }
    }

    let order = topological_scc_order(&sccs, &scc_of, &adjacency);

    assert_eq!(vec![0, 1, 2], order);
}
