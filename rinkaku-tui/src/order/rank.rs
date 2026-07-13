//! Directory ranking and cycle analysis for the entry view (ADR 0016
//! decision 4): SCC-condenses `report.graph` down to directories and
//! runs Kahn's algorithm over the condensation, so a directory's rank
//! reflects its position in the production dependency order.
//!
//! Reimplements `rinkaku-core::graph`'s SCC-condensation approach locally
//! (CLAUDE.md: no shared abstraction without a concrete second use case
//! argued in an ADR) rather than reusing its private helpers, since this
//! module operates over directories, a concept `rinkaku-core`'s graph
//! module has no notion of.

use rinkaku_core::render::Report;
use std::collections::{HashMap, HashSet};

/// One directory's computed rank: its position in topological order, plus
/// whether it participates in a dependency cycle with at least one other
/// directory (a design-warning signal, ADR 0016 decision 4 / ADR 0008's
/// existing symbol-level cycle warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirRank {
    pub rank: usize,
    pub in_cycle: bool,
}

/// The directory-level condensation shared by [`rank_directories`],
/// [`cycle_partners`], [`cycle_edges`], and [`cycle_explanation`]: every
/// directory that owns at least one graph node, the inter-directory
/// adjacency derived from `report.graph.edges`, and the Tarjan SCC
/// grouping over that adjacency. [`cycle_explanation`] builds exactly one
/// `DirCondensation` and reads both partners and edges off it, rather
/// than re-running Tarjan and re-deriving the directory index twice.
struct DirCondensation<'a> {
    /// Directory paths, sorted, indexed by position.
    dirs: Vec<&'a str>,
    adjacency: Vec<Vec<usize>>,
    sccs: Vec<Vec<usize>>,
    /// `scc_of[i]` is the SCC index (into `sccs`) directory `dirs[i]`
    /// belongs to.
    scc_of: Vec<usize>,
}

impl<'a> DirCondensation<'a> {
    /// Drops every node/edge whose id names a test symbol (ADR 0035)
    /// before condensing to directories, so test code cannot pull a
    /// production directory's rank around — see [`test_node_ids`].
    /// Remaining nodes/edges are remapped to their endpoint's parent
    /// directory (the empty string for a root-level file), dropping any
    /// edge whose two endpoints condense to the same directory.
    fn build(report: &'a Report) -> Self {
        let test_ids = test_node_ids(report);

        let dir_of_node: HashMap<&str, &str> = report
            .graph
            .nodes
            .iter()
            .filter(|node| !test_ids.contains(node.id.as_str()))
            .map(|node| (node.id.as_str(), parent_dir(&node.path)))
            .collect();

        let mut dirs: Vec<&str> = dir_of_node
            .values()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        dirs.sort_unstable();
        let dir_index: HashMap<&str, usize> =
            dirs.iter().enumerate().map(|(i, &d)| (d, i)).collect();

        let mut adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); dirs.len()];
        for edge in &report.graph.edges {
            if test_ids.contains(edge.from.as_str()) || test_ids.contains(edge.to.as_str()) {
                continue;
            }
            let (Some(&from_dir), Some(&to_dir)) = (
                dir_of_node.get(edge.from.as_str()),
                dir_of_node.get(edge.to.as_str()),
            ) else {
                continue;
            };
            if from_dir == to_dir {
                continue;
            }
            let (from_i, to_i) = (dir_index[from_dir], dir_index[to_dir]);
            adjacency[from_i].insert(to_i);
        }
        let adjacency: Vec<Vec<usize>> = adjacency
            .into_iter()
            .map(|targets| targets.into_iter().collect())
            .collect();

        let sccs = tarjan_sccs(&adjacency);
        let mut scc_of = vec![0usize; dirs.len()];
        for (scc_index, scc) in sccs.iter().enumerate() {
            for &node_index in scc {
                scc_of[node_index] = scc_index;
            }
        }

        Self {
            dirs,
            adjacency,
            sccs,
            scc_of,
        }
    }
}

/// Every graph node/edge id whose owning symbol is test code (ADR 0035).
///
/// Relies on `graph::stamp_ids` having already run before `Report` is
/// built (`pipeline::analyze_diff`/`analyze_repo`): `ExtractedSymbol::id`
/// and `graph::Node::id` are the same stable string by construction, so
/// matching on `id` here needs no re-derivation of
/// `graph::collect_nodes`'s id-uniqueness algorithm.
///
/// This is a cross-crate invariant with no compile-time enforcement: if
/// `rinkaku-core::graph::stamp_ids`'s id-assignment order ever changes,
/// this function silently starts filtering the wrong nodes/edges rather
/// than failing to compile (`HashSet::contains` on a mismatched id is
/// just `false`, not an error). A change there must re-check this
/// function and `rank_tests/rank_directories.rs` by hand.
fn test_node_ids(report: &Report) -> HashSet<&str> {
    report
        .files
        .iter()
        .flat_map(|file| &file.symbols)
        .filter(|symbol| symbol.is_test)
        .map(|symbol| symbol.id.as_str())
        .collect()
}

/// Computes each directory's [`DirRank`] from `report.graph`'s edges,
/// condensed from symbol-level to directory-level.
///
/// Ranking: Tarjan SCCs the condensed directory graph, then orders SCCs
/// by a Kahn topological sort starting from in-degree-0 SCCs (the same
/// direction `graph::find_roots` uses), so entry-point directories rank
/// lowest and directories heavily depended upon rank highest. Every
/// directory inside one SCC shares that SCC's rank and is marked
/// `in_cycle` when the SCC has more than one member.
///
/// Returns an entry only for a *leaf* directory: one that is the direct
/// parent of at least one `report.graph.nodes` path. A branching
/// directory that owns no node directly (e.g. `"src"` when only its
/// subdirectories do) gets no entry here — `order::sort::effective_ranks`
/// is the caller that walks the built `Tree` bottom-up to give such a
/// directory its descendants' minimum rank instead.
pub fn rank_directories(report: &Report) -> HashMap<String, DirRank> {
    let condensation = DirCondensation::build(report);
    let DirCondensation {
        dirs,
        adjacency,
        sccs,
        scc_of,
    } = condensation;

    let scc_order = topological_scc_order(&sccs, &scc_of, &adjacency);

    let mut rank_of_scc = vec![0usize; sccs.len()];
    for (rank, &scc_index) in scc_order.iter().enumerate() {
        rank_of_scc[scc_index] = rank;
    }

    let mut result = HashMap::new();
    for (scc_index, scc) in sccs.iter().enumerate() {
        let in_cycle = scc.len() > 1;
        for &node_index in scc {
            result.insert(
                dirs[node_index].to_string(),
                DirRank {
                    rank: rank_of_scc[scc_index],
                    in_cycle,
                },
            );
        }
    }
    result
}

/// For every directory that participates in a directory-level cycle, the
/// sorted list of *other* directories sharing that cycle — so a
/// directory marked `(cycle)` in the entry view can name its actual
/// partners rather than leaving the reviewer to guess. A directory with
/// no cycle membership is absent from the returned map entirely (not
/// present with an empty `Vec`).
///
/// Kept as its own public function (rather than folded into
/// [`cycle_explanation`]) since it has independent unit test coverage
/// and callers that only need partners, not edges too. A caller that
/// needs both should use [`cycle_explanation`] instead, which builds the
/// condensation once rather than once per function.
pub fn cycle_partners(report: &Report) -> HashMap<String, Vec<String>> {
    let condensation = DirCondensation::build(report);
    partners_from_condensation(&condensation)
}

fn partners_from_condensation(condensation: &DirCondensation) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();
    for scc in &condensation.sccs {
        if scc.len() < 2 {
            continue;
        }
        let members: Vec<String> = scc
            .iter()
            .map(|&i| condensation.dirs[i].to_string())
            .collect();
        for &node_index in scc {
            let this_dir = condensation.dirs[node_index];
            let mut partners: Vec<String> = members
                .iter()
                .filter(|m| m.as_str() != this_dir)
                .cloned()
                .collect();
            partners.sort_unstable();
            result.insert(this_dir.to_string(), partners);
        }
    }
    result
}

/// One directed cross-directory edge forming part of a cycle — the
/// concrete `path::name -> path::name` line the dir detail view renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleEdge {
    pub from_path: String,
    pub from_name: String,
    pub to_path: String,
    pub to_name: String,
}

/// Every `report.graph.edges` entry whose two endpoints' parent
/// directories are distinct members of the same cycle-forming SCC (per
/// [`cycle_partners`]) — the edges that concretely make up a directory
/// cycle. An edge within a single directory, or between two directories
/// that merely both exist without forming a cycle together, is excluded.
///
/// Builds its own [`DirCondensation`] — see [`cycle_explanation`] for the
/// shared-build alternative.
pub fn cycle_edges(report: &Report) -> Vec<CycleEdge> {
    let condensation = DirCondensation::build(report);
    edges_from_condensation(report, &condensation)
}

fn edges_from_condensation(report: &Report, condensation: &DirCondensation) -> Vec<CycleEdge> {
    let node_by_id: HashMap<&str, &rinkaku_core::graph::Node> = report
        .graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    let dir_index: HashMap<&str, usize> = condensation
        .dirs
        .iter()
        .enumerate()
        .map(|(i, &d)| (d, i))
        .collect();
    let scc_of_dir = |dir: &str| dir_index.get(dir).map(|&i| condensation.scc_of[i]);

    let mut edges = Vec::new();
    for edge in &report.graph.edges {
        let (Some(from_node), Some(to_node)) = (
            node_by_id.get(edge.from.as_str()),
            node_by_id.get(edge.to.as_str()),
        ) else {
            continue;
        };
        let from_dir = parent_dir(&from_node.path);
        let to_dir = parent_dir(&to_node.path);
        if from_dir == to_dir {
            continue;
        }
        let (Some(from_scc), Some(to_scc)) = (scc_of_dir(from_dir), scc_of_dir(to_dir)) else {
            continue;
        };
        if from_scc != to_scc {
            continue;
        }
        edges.push(CycleEdge {
            from_path: from_node.path.clone(),
            from_name: from_node.name.clone(),
            to_path: to_node.path.clone(),
            to_name: to_node.name.clone(),
        });
    }
    edges
}

/// One directory's cycle explanation: the other directories it cycles
/// with, and the concrete cross-directory edges forming that cycle —
/// exactly what [`crate::detail::build_dir_detail`] needs. Builds
/// [`DirCondensation`] exactly once and derives both results from it,
/// unlike calling [`cycle_partners`] and [`cycle_edges`] separately.
pub fn cycle_explanation(report: &Report, path: &str) -> (Vec<String>, Vec<CycleEdge>) {
    let condensation = DirCondensation::build(report);
    let partners = partners_from_condensation(&condensation)
        .remove(path)
        .unwrap_or_default();
    let edges = if partners.is_empty() {
        Vec::new()
    } else {
        edges_from_condensation(report, &condensation)
            .into_iter()
            .filter(|edge| parent_dir(&edge.from_path) == path || parent_dir(&edge.to_path) == path)
            .collect()
    };
    (partners, edges)
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Orders SCC indices via Kahn's algorithm on the condensation DAG, ties
/// broken by each SCC's smallest member index — mirrors
/// `graph::find_roots`'s own tie-break ("earliest in source order").
fn topological_scc_order(
    sccs: &[Vec<usize>],
    scc_of: &[usize],
    adjacency: &[Vec<usize>],
) -> Vec<usize> {
    let scc_count = sccs.len();
    let mut scc_adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); scc_count];
    let mut in_degree = vec![0usize; scc_count];

    for (from_node, targets) in adjacency.iter().enumerate() {
        let from_scc = scc_of[from_node];
        for &to_node in targets {
            let to_scc = scc_of[to_node];
            if from_scc != to_scc && scc_adjacency[from_scc].insert(to_scc) {
                in_degree[to_scc] += 1;
            }
        }
    }

    // `tarjan_sccs` never produces an empty component, so `min()` always
    // finds a value in practice; `if let` avoids asserting that
    // invariant via `.expect()` in library code and falls back to
    // `usize::MAX` for a component that somehow turned out empty, which
    // sorts it last rather than panicking.
    let scc_min_member: Vec<usize> = sccs
        .iter()
        .map(|scc| scc.iter().min().copied().unwrap_or(usize::MAX))
        .collect();

    let mut frontier: Vec<usize> = (0..scc_count).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(scc_count);
    let mut visited = vec![false; scc_count];

    while !frontier.is_empty() {
        frontier.sort_by_key(|&i| scc_min_member[i]);
        let scc_index = frontier.remove(0);
        if visited[scc_index] {
            continue;
        }
        visited[scc_index] = true;
        order.push(scc_index);

        for &neighbor in &scc_adjacency[scc_index] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                frontier.push(neighbor);
            }
        }
    }

    // Defensive: Kahn's algorithm always terminates on a DAG, and the SCC
    // condensation is always a DAG by construction, so this should never
    // execute. Append any stragglers in source order rather than
    // silently dropping them if it somehow does.
    for (scc_index, &was_visited) in visited.iter().enumerate() {
        if !was_visited {
            order.push(scc_index);
        }
    }

    order
}

/// Tarjan's strongly-connected-components algorithm, iterative to avoid
/// stack overflow on a large directory graph — same shape as
/// `rinkaku-core::graph`'s private `tarjan_sccs` (reimplemented here per
/// this module's own doc comment on not sharing an abstraction across
/// the crate boundary for one use case). The two copies do not stay in
/// sync automatically: a change to one (e.g. `find_roots`'s
/// empty-component handling) should be checked against the other.
fn tarjan_sccs(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut index_counter = 0usize;
    let mut indices: Vec<Option<usize>> = vec![None; n];
    let mut lowlink: Vec<usize> = vec![0; n];
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        if indices[start].is_some() {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(start, 0)];

        while let Some(&(v, child_i)) = work.last() {
            if child_i == 0 {
                indices[v] = Some(index_counter);
                lowlink[v] = index_counter;
                index_counter += 1;
                stack.push(v);
                on_stack[v] = true;
            }

            let Some(frame) = work.last_mut() else {
                break;
            };

            if child_i < adjacency[v].len() {
                let w = adjacency[v][child_i];
                frame.1 += 1;

                match indices[w] {
                    None => work.push((w, 0)),
                    Some(w_index) if on_stack[w] => {
                        lowlink[v] = lowlink[v].min(w_index);
                    }
                    Some(_) => {}
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    lowlink[parent] = lowlink[parent].min(lowlink[v]);
                }

                if let Some(v_index) = indices[v]
                    && lowlink[v] == v_index
                {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        let is_v = w == v;
                        component.push(w);
                        if is_v {
                            break;
                        }
                    }
                    sccs.push(component);
                }
            }
        }
    }

    sccs
}

#[cfg(test)]
#[path = "rank_tests/mod.rs"]
mod tests;
