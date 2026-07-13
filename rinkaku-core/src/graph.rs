//! Directed graph over changed symbols, for entry-point tree rendering
//! (ADR 0008).
//!
//! [`build_graph`] takes every changed symbol across a diff (already
//! extracted by [`crate::extract`], with `referenced_names` still intact)
//! and builds a [`SymbolGraph`]: nodes are changed symbols, edges are
//! name-matches from one symbol's `referenced_names` to another changed
//! symbol's name. Entry points ("roots") are auto-detected rather than
//! user-specified — see the module-level rationale in the ADR for why.
//!
//! Cycles among changed symbols are expected (e.g. mutual recursion) and
//! must not prevent every node from having *some* root to hang off of, so
//! roots are computed on the strongly-connected-component (SCC) condensation
//! of the graph rather than the raw graph: an SCC with in-degree 0 always
//! exists in a finite DAG (the condensation is always a DAG), so at least
//! one root always exists, even when literally every node participates in
//! some cycle.

use crate::render::FileReport;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Stable identifier for a graph node: `{path}::{name}`, disambiguated with
/// `@{start_line}` only when a report contains more than one symbol sharing
/// that `(path, name)` pair (e.g. overloaded free functions in a language
/// that allows them, or two same-named symbols in different containers).
pub type NodeId = String;

/// One changed symbol, as a graph node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub path: String,
    pub name: String,
}

/// A directed edge from one changed symbol to another, both identified by
/// [`NodeId`]. `is_cycle` marks a back edge discovered by the DFS used to
/// detect cycles (see `build_graph`'s doc comment) — a cycle edge is still
/// a real edge (kept in `SymbolGraph::edges`), just annotated so rendering
/// can show a warning instead of infinitely recursing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub is_cycle: bool,
}

/// A directed graph over a diff's changed symbols, plus its auto-detected
/// entry points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Entry points: nodes with no incoming non-cycle edge from another
    /// changed symbol, chosen so that a DFS from `roots` reaches every node
    /// (see `build_graph`'s doc comment on SCC-based root selection).
    pub roots: Vec<NodeId>,
}

/// Line-count-style eligibility threshold (ADR 0033, mirroring
/// `file_size.rs`'s `WARN_LINE_THRESHOLD`/`SPLIT_LINE_THRESHOLD`
/// convention): a node with `used_by.len() >= HIGH_FAN_IN_THRESHOLD`
/// qualifies as a [`FanIn`] entry. This is a judgment call, not derived
/// from data (ADR 0013's Consequences section flagged the same caveat
/// before this constant existed as a name) — revisit if dogfooding on
/// real diffs shows fan-in == 1 entries carry useful signal. Changing
/// this value is an ADR amendment, same as the file-size thresholds.
pub const HIGH_FAN_IN_THRESHOLD: usize = 2;

/// A changed symbol with two or more distinct referrers (ADR 0013, named
/// "fan-in" per ADR 0033): a symbol a reviewer should pay extra attention
/// to, since changing its signature has a wider blast radius than a symbol
/// only one other changed symbol depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FanIn {
    pub id: NodeId,
    pub path: String,
    pub name: String,
    /// Names of every changed symbol referencing this node, sorted
    /// ascending. Deduplication happens per referrer *node*, not per name,
    /// so two distinct referrers sharing a name both appear (see
    /// [`compute_fan_ins`]'s doc comment). Fan-in count is
    /// `used_by.len()` — no separate count field, since the list already
    /// carries it and a reader who wants the number can just count entries
    /// or check the (short) list itself.
    pub used_by: Vec<String>,
}

/// Aggregates `graph.edges` by target node into [`FanIn`]s: nodes
/// referenced by two or more distinct changed symbols (fan-in >=
/// [`HIGH_FAN_IN_THRESHOLD`]). A cycle edge (`Edge::is_cycle`) still counts
/// as a real reference — the referrer really does depend on the target's
/// signature, cycle or not — so cycle and non-cycle edges are aggregated
/// together without distinction.
///
/// Multiple edges from the same referrer node to the same target
/// (`collect_edges` cannot currently produce these, since a symbol's
/// `referenced_names` come from a `HashSet` upstream in extraction, but nothing
/// in this function's contract depends on that) are deduplicated by
/// referrer *id*, not name, before names are collected — this matters when
/// two distinct referrer nodes happen to share a name (e.g. two overloaded
/// functions both named `helper` referencing the same target): both are
/// kept as separate fan-in contributors (they are genuinely different
/// symbols), so `used_by` can contain the same name twice in that case
/// rather than silently under-counting fan-in.
///
/// Results are sorted by fan-in descending, ties broken by `(path, name,
/// id)` ascending for determinism independent of edge/node iteration
/// order. `id` is the final tie-break rather than `path`/`name` alone
/// because two distinct symbols can share both — e.g. two overloaded
/// functions named `helper` in the same file, disambiguated only by the
/// `@{start_line}` suffix `collect_nodes` gives their `id` — and without
/// it, `referrers_by_target`'s `HashMap` iteration order (which varies
/// run to run under Rust's randomized `HashMap` seed) decided the order
/// between them (see ADR 0013's amendment).
pub fn compute_fan_ins(graph: &SymbolGraph) -> Vec<FanIn> {
    let node_by_id: HashMap<&str, &Node> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Dedup by (target, referrer id) first, so a target with multiple edges
    // from the same referrer (however unlikely today) is not over-counted.
    let mut referrers_by_target: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        let referrers = referrers_by_target.entry(edge.to.as_str()).or_default();
        if !referrers.contains(&edge.from.as_str()) {
            referrers.push(edge.from.as_str());
        }
    }

    let mut fan_ins: Vec<FanIn> = referrers_by_target
        .into_iter()
        .filter(|(_, referrers)| referrers.len() >= HIGH_FAN_IN_THRESHOLD)
        .filter_map(|(target_id, referrer_ids)| {
            let node = node_by_id.get(target_id)?;
            let mut used_by: Vec<String> = referrer_ids
                .iter()
                .filter_map(|id| node_by_id.get(id))
                .map(|n| n.name.clone())
                .collect();
            used_by.sort();
            Some(FanIn {
                id: node.id.clone(),
                path: node.path.clone(),
                name: node.name.clone(),
                used_by,
            })
        })
        .collect();

    fan_ins.sort_by(|a, b| {
        b.used_by
            .len()
            .cmp(&a.used_by.len())
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });

    fan_ins
}

/// Builds a [`SymbolGraph`] over every symbol in `files`.
///
/// Node order (and therefore `nodes`, tie-breaks in `roots`, and DFS
/// exploration order downstream in rendering) follows `files`' own order,
/// then each file's symbol order — i.e. source appearance order, which is
/// already how `pipeline::analyze_diff` assembles `files`. Determinism
/// downstream (rendering) depends on this.
pub fn build_graph(files: &[FileReport]) -> SymbolGraph {
    let nodes = collect_nodes(files);
    let mut edges = collect_edges(files, &nodes);
    let roots = find_roots(&nodes, &edges);
    mark_cycle_edges(&nodes, &roots, &mut edges);

    SymbolGraph {
        nodes,
        edges,
        roots,
    }
}

/// Re-roots `graph` at `path_prefix` (ADR 0019): a viewpoint change, not a
/// re-analysis. `nodes` and `edges` are untouched — every dependency stays
/// reachable exactly as before — only `roots` (and, since cycle marking
/// depends on where DFS starts, `edges[..].is_cycle`) are recomputed so
/// rendering can walk the tree from the chosen path outward.
///
/// The new roots are [`pivot_roots`]'s result: nodes under `path_prefix`
/// that no *other node under the same prefix* depends on. Cycle edges are
/// then re-marked by the same back-edge DFS `build_graph` uses
/// ([`mark_cycle_edges`]), started from these new roots instead of the
/// whole-graph ones — a cycle that closes back to a pivot root is exactly
/// as much a cycle from this viewpoint as from the default one, so it must
/// still render as a warning rather than being silently walked past.
///
/// Returns `graph` unchanged (roots become empty) when no node's path is
/// under `path_prefix` — the caller (CLI/TUI) is responsible for showing
/// "no symbols under `<path>`" in that case, since this function has no
/// rendering concern of its own.
pub fn pivot_graph(graph: &SymbolGraph, path_prefix: &str) -> SymbolGraph {
    let roots = pivot_roots(graph, path_prefix);
    let mut edges = graph.edges.clone();
    mark_cycle_edges(&graph.nodes, &roots, &mut edges);

    SymbolGraph {
        nodes: graph.nodes.clone(),
        edges,
        roots,
    }
}

/// Computes the pivoted root set for `path_prefix` (ADR 0019): among nodes
/// whose `path` is under `path_prefix` (directory-boundary-respecting, see
/// [`path_under_prefix`]), the roots are those with no incoming edge *from
/// another node in that same subset* — an edge arriving from outside the
/// subset does not disqualify a root, since the pivot's whole point is to
/// ignore the outside-in direction and look outward from the chosen path
/// instead.
///
/// Applies ADR 0008's SCC-based root rule to the subset rather than the
/// whole graph: a cycle entirely within the subset must still yield at
/// least one representative root (same rationale as `find_roots`), so this
/// delegates to it after restricting both the node list and the edge list
/// to the subset. Root order follows subset-relative source order, which
/// is the same relative order the nodes already have in `graph.nodes`
/// (`collect_nodes`'s doc comment on source-order determinism).
///
/// Returns an empty `Vec` when no node's path matches `path_prefix` at all.
pub fn pivot_roots(graph: &SymbolGraph, path_prefix: &str) -> Vec<NodeId> {
    let subset_nodes: Vec<Node> = graph
        .nodes
        .iter()
        .filter(|node| path_under_prefix(&node.path, path_prefix))
        .cloned()
        .collect();

    if subset_nodes.is_empty() {
        return Vec::new();
    }

    let subset_ids: HashSet<&str> = subset_nodes.iter().map(|n| n.id.as_str()).collect();
    let subset_edges: Vec<Edge> = graph
        .edges
        .iter()
        .filter(|edge| {
            subset_ids.contains(edge.from.as_str()) && subset_ids.contains(edge.to.as_str())
        })
        .cloned()
        .collect();

    find_roots(&subset_nodes, &subset_edges)
}

/// Whether `path` is under `path_prefix`, respecting directory boundaries:
/// `path_prefix == "src/api"` matches `"src/api/handler.rs"` and
/// `"src/api"` itself (an exact match — the prefix can name a file
/// directly, not just a directory), but not `"src/api2/handler.rs"`. A
/// plain `str::starts_with` would wrongly match that last case, since
/// `"src/api2"` starts with the byte sequence `"src/api"`.
///
/// Public (rather than a private helper of [`pivot_roots`] alone) because
/// `rinkaku-tui`'s pivot pane (ADR 0019) needs the exact same
/// directory-boundary test to tell which lines of a rendered pivot tree
/// fall inside the pivoted prefix vs. were only reached by expanding a
/// dependency edge outward past it — recomputing a subtly different
/// `starts_with` there would risk the two disagreeing on an edge case like
/// `"src/api2"`.
pub fn path_under_prefix(path: &str, path_prefix: &str) -> bool {
    let prefix = path_prefix.trim_end_matches('/');
    if prefix.is_empty() {
        // An empty prefix (after trimming trailing slashes) means "the
        // whole repository root" — every path matches, same as ADR 0019's
        // whole-repo mode having no meaningful narrower prefix to filter by.
        return true;
    }
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Assigns each symbol in `files` its [`ExtractedSymbol::id`] to match the
/// [`NodeId`] `build_graph` gave its corresponding [`Node`] (ADR 0008: JSON
/// consumers need to correlate a symbol with `SymbolGraph`'s
/// `nodes`/`edges`/`roots` without recomputing the `{path}::{name}` scheme
/// themselves). Iterates `files` in the same source order `collect_nodes`
/// used to build `graph.nodes`, so `graph.nodes[i]` and the i-th symbol
/// encountered here always correspond to the same definition — this
/// ordering coupling is why `stamp_ids` takes the already-built `graph`
/// rather than recomputing IDs independently, which could drift out of
/// sync if the two ID-assignment schemes were ever edited separately.
pub fn stamp_ids(files: &mut [FileReport], graph: &SymbolGraph) {
    let mut node_index = 0;
    for file in files.iter_mut() {
        for symbol in file.symbols.iter_mut() {
            if let Some(node) = graph.nodes.get(node_index) {
                symbol.id = node.id.clone();
            }
            node_index += 1;
        }
    }
}

/// Assigns a stable [`NodeId`] to every symbol in source order. A `(path,
/// name)` pair disambiguates with `@{start_line}` only when it is not
/// already unique within `files` — the common case (no duplicate names)
/// gets the short `{path}::{name}` form.
fn collect_nodes(files: &[FileReport]) -> Vec<Node> {
    let mut name_counts: HashMap<(&str, &str), usize> = HashMap::new();
    for file in files {
        for symbol in &file.symbols {
            *name_counts
                .entry((file.path.as_str(), symbol.name.as_str()))
                .or_default() += 1;
        }
    }

    let mut nodes = Vec::new();
    for file in files {
        for symbol in &file.symbols {
            let is_duplicate = name_counts[&(file.path.as_str(), symbol.name.as_str())] > 1;
            let id = if is_duplicate {
                format!("{}::{}@{}", file.path, symbol.name, symbol.range.start)
            } else {
                format!("{}::{}", file.path, symbol.name)
            };
            nodes.push(Node {
                id,
                path: file.path.clone(),
                name: symbol.name.clone(),
            });
        }
    }
    nodes
}

/// Builds edges from each symbol's `referenced_names` to every changed
/// symbol whose name matches, across the whole diff (not just within one
/// file) — mirroring how `deps::resolve_dependencies` matches by name
/// alone. Self-references (a node referencing its own name, e.g. a
/// struct's name appearing inside its own definition — see
/// `extract::collect_referenced_names`'s doc comment) are excluded, same
/// rationale as `deps::resolve_dependencies`'s self-reference exclusion.
/// A referenced name matching more than one changed symbol (possible once
/// duplicate `(path, name)` pairs get distinct node IDs) produces an edge
/// to each match, not just one — the caller has no way to disambiguate
/// under v1's name-only matching (ADR 0003), so all plausible edges are
/// kept rather than arbitrarily picking one.
fn collect_edges(files: &[FileReport], nodes: &[Node]) -> Vec<Edge> {
    let mut nodes_by_name: HashMap<&str, Vec<&Node>> = HashMap::new();
    for node in nodes {
        nodes_by_name
            .entry(node.name.as_str())
            .or_default()
            .push(node);
    }

    let mut edges = Vec::new();
    let mut node_index = 0;
    for file in files {
        for symbol in &file.symbols {
            let from = &nodes[node_index];
            for referenced_name in &symbol.referenced_names {
                if let Some(targets) = nodes_by_name.get(referenced_name.as_str()) {
                    for target in targets {
                        if target.id != from.id {
                            edges.push(Edge {
                                from: from.id.clone(),
                                to: target.id.clone(),
                                is_cycle: false,
                            });
                        }
                    }
                }
            }
            node_index += 1;
        }
    }
    edges
}

/// Maps each node's [`NodeId`] to its position in `nodes`, letting the
/// index-based algorithms below (`find_roots`'s SCC condensation,
/// `mark_cycle_edges`'s back-edge DFS) translate an [`Edge`]'s
/// string-keyed `from`/`to` into array indices without a linear scan per
/// lookup. Shared by both rather than each rebuilding its own copy from
/// `nodes`.
fn node_index_map(nodes: &[Node]) -> HashMap<&str, usize> {
    nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect()
}

/// Computes entry points as the representative nodes of in-degree-0 SCCs in
/// the condensation graph, so at least one root always exists even if every
/// node participates in some cycle (a pure raw in-degree-0 test would find
/// none in that case). Representative = the SCC member that appears first
/// in `nodes`' source order.
///
/// Uses Tarjan's algorithm (`tarjan_sccs`) rather than pulling in a graph
/// crate: this is the only graph algorithm the codebase needs today (see
/// CLAUDE.md's guidance against speculative shared abstractions), and a
/// from-scratch implementation keeps the core dependency-free.
fn find_roots(nodes: &[Node], edges: &[Edge]) -> Vec<NodeId> {
    if nodes.is_empty() {
        return Vec::new();
    }

    let index_of = node_index_map(nodes);

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for edge in edges {
        if let (Some(&from), Some(&to)) = (
            index_of.get(edge.from.as_str()),
            index_of.get(edge.to.as_str()),
        ) {
            adjacency[from].push(to);
        }
    }

    let sccs = tarjan_sccs(&adjacency);
    // scc_of[node_index] = index into `sccs` of the SCC containing it.
    let mut scc_of = vec![0usize; nodes.len()];
    for (scc_index, scc) in sccs.iter().enumerate() {
        for &node_index in scc {
            scc_of[node_index] = scc_index;
        }
    }

    let mut scc_has_incoming = vec![false; sccs.len()];
    for edge in edges {
        if let (Some(&from), Some(&to)) = (
            index_of.get(edge.from.as_str()),
            index_of.get(edge.to.as_str()),
        ) {
            let (from_scc, to_scc) = (scc_of[from], scc_of[to]);
            if from_scc != to_scc {
                scc_has_incoming[to_scc] = true;
            }
        }
    }

    let mut roots = Vec::new();
    for (scc_index, scc) in sccs.iter().enumerate() {
        if scc_has_incoming[scc_index] {
            continue;
        }
        // Representative = earliest in source order (smallest node index).
        // `tarjan_sccs` never produces an empty component (each `component`
        // it pushes starts from a freshly-visited node and is only pushed
        // after collecting at least that node), so `min()` always finds a
        // value in practice; `if let` avoids asserting that invariant via
        // `.expect()` in library code and simply skips a component that
        // somehow turned out empty rather than panicking.
        if let Some(&representative) = scc.iter().min() {
            roots.push(nodes[representative].id.clone());
        }
    }

    // sccs are discovered in reverse-postorder-ish order depending on the
    // implementation; sort roots by node source order for deterministic
    // output regardless of that internal detail.
    roots.sort_by_key(|id| index_of[id.as_str()]);
    roots
}

/// Marks every back edge discovered by a pre-order DFS starting at `roots`
/// (in order) as `is_cycle = true`. A back edge — one pointing to an
/// ancestor still on the current DFS path — is the standard graph-theory
/// signature of a cycle; marking it (rather than the whole SCC) pinpoints
/// exactly which edge closes the loop, which is what rendering needs to
/// show a warning at the right spot in the tree (ADR 0008).
///
/// DFS starts from `roots` so cycle marking agrees with the traversal
/// order `render.rs` will use to walk the tree. `roots` is guaranteed to
/// reach every node (see `find_roots`'s doc comment), so the fallback loop
/// over all remaining unvisited nodes only guards against future changes
/// to `find_roots` breaking that guarantee — not expected to trigger in
/// practice.
fn mark_cycle_edges(nodes: &[Node], roots: &[NodeId], edges: &mut [Edge]) {
    if nodes.is_empty() {
        return;
    }

    let index_of = node_index_map(nodes);

    let mut adjacency: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nodes.len()]; // (target, edge_index)
    for (edge_index, edge) in edges.iter().enumerate() {
        if let (Some(&from), Some(&to)) = (
            index_of.get(edge.from.as_str()),
            index_of.get(edge.to.as_str()),
        ) {
            adjacency[from].push((to, edge_index));
        }
    }

    let mut visited = vec![false; nodes.len()];
    let mut on_path = vec![false; nodes.len()];
    let mut cycle_edge_indices: Vec<usize> = Vec::new();

    let start_indices: Vec<usize> = roots
        .iter()
        .filter_map(|id| index_of.get(id.as_str()).copied())
        .chain((0..nodes.len()).filter(|i| !visited[*i]))
        .collect();

    for start in start_indices {
        if visited[start] {
            continue;
        }
        dfs_mark_back_edges(
            start,
            &adjacency,
            &mut visited,
            &mut on_path,
            &mut cycle_edge_indices,
        );
    }

    for edge_index in cycle_edge_indices {
        edges[edge_index].is_cycle = true;
    }
}

/// Iterative pre-order DFS from `start`, recording the edge index of every
/// back edge (an edge to a node currently `on_path`, i.e. an ancestor in
/// the DFS tree) into `cycle_edge_indices`. Iterative rather than recursive
/// for the same stack-depth reason as `tarjan_sccs`.
fn dfs_mark_back_edges(
    start: usize,
    adjacency: &[Vec<(usize, usize)>],
    visited: &mut [bool],
    on_path: &mut [bool],
    cycle_edge_indices: &mut Vec<usize>,
) {
    // Explicit work-stack entries: (node, next child index to visit).
    let mut work: Vec<(usize, usize)> = vec![(start, 0)];
    visited[start] = true;
    on_path[start] = true;

    while let Some(&mut (v, ref mut child_i)) = work.last_mut() {
        if *child_i < adjacency[v].len() {
            let (w, edge_index) = adjacency[v][*child_i];
            *child_i += 1;

            if on_path[w] {
                cycle_edge_indices.push(edge_index);
            } else if !visited[w] {
                visited[w] = true;
                on_path[w] = true;
                work.push((w, 0));
            }
            // `visited[w] && !on_path[w]`: a cross/forward edge to an
            // already-fully-explored node — not a cycle, nothing to mark.
        } else {
            on_path[v] = false;
            work.pop();
        }
    }
}

/// Tarjan's strongly-connected-components algorithm, iterative (not
/// recursive) to avoid stack overflow on pathologically deep call chains in
/// a large diff. Returns each SCC as a `Vec<usize>` of node indices;
/// singleton SCCs (a node with no cycle through itself) are included too,
/// same as any other SCC — the caller does not need to special-case them.
fn tarjan_sccs(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut index_counter = 0usize;
    let mut indices: Vec<Option<usize>> = vec![None; n];
    let mut lowlink: Vec<usize> = vec![0; n];
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    // Explicit work-stack entries: (node, next child index to visit).
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
                // Unreachable in practice (the `while let Some(...) =
                // work.last()` guard above just matched this same frame),
                // but avoiding `.unwrap()` here keeps this library function
                // panic-free even if that invariant is ever broken by a
                // future edit — an empty `work` simply ends the loop for
                // this `start` rather than panicking.
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

                // `indices[v]` was set to `Some(_)` unconditionally above
                // (at `child_i == 0`, the first time `v` was visited) and
                // is never cleared afterward, so it is always `Some` here;
                // `if let` reads that invariant without asserting it via
                // `.expect()`. A `None` (unreachable in practice) simply
                // skips emitting an SCC for `v` instead of panicking.
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
#[path = "graph_tests/mod.rs"]
mod tests;
