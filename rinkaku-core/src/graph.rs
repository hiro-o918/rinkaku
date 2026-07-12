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
use std::collections::HashMap;

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

/// A changed symbol with two or more distinct referrers (ADR 0013): a
/// "fan-in hotspot" a reviewer should pay extra attention to, since changing
/// its signature has a wider blast radius than a symbol only one other
/// changed symbol depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hotspot {
    pub id: NodeId,
    pub path: String,
    pub name: String,
    /// Names of every changed symbol referencing this node, sorted
    /// ascending and deduplicated (see [`compute_hotspots`]'s doc comment).
    /// Fan-in count is `used_by.len()` — no separate count field, since the
    /// list already carries it and a reader who wants the number can just
    /// count entries or check the (short) list itself.
    pub used_by: Vec<String>,
}

/// Aggregates `graph.edges` by target node into [`Hotspot`]s: nodes
/// referenced by two or more distinct changed symbols (fan-in >= 2). A
/// cycle edge (`Edge::is_cycle`) still counts as a real reference — the
/// referrer really does depend on the target's signature, cycle or not — so
/// cycle and non-cycle edges are aggregated together without distinction.
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
/// Results are sorted by fan-in descending, ties broken by `(path, name)`
/// ascending for determinism independent of edge/node iteration order.
pub fn compute_hotspots(graph: &SymbolGraph) -> Vec<Hotspot> {
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

    let mut hotspots: Vec<Hotspot> = referrers_by_target
        .into_iter()
        .filter(|(_, referrers)| referrers.len() >= 2)
        .filter_map(|(target_id, referrer_ids)| {
            let node = node_by_id.get(target_id)?;
            let mut used_by: Vec<String> = referrer_ids
                .iter()
                .filter_map(|id| node_by_id.get(id))
                .map(|n| n.name.clone())
                .collect();
            used_by.sort();
            Some(Hotspot {
                id: node.id.clone(),
                path: node.path.clone(),
                name: node.name.clone(),
                used_by,
            })
        })
        .collect();

    hotspots.sort_by(|a, b| {
        b.used_by
            .len()
            .cmp(&a.used_by.len())
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.name.cmp(&b.name))
    });

    hotspots
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
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::{ExtractedSymbol, SymbolKind};
    use pretty_assertions::assert_eq;

    /// Builds an `ExtractedSymbol` with a given `name`/`referenced_names`,
    /// filling every other field with a fixed placeholder — these tests
    /// only care about the graph-building fields.
    fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
        ExtractedSymbol {
            id: String::new(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
        }
    }

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

    #[test]
    fn should_stamp_each_symbol_id_when_graph_has_no_duplicate_names() {
        let mut files = vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec![])],
        }];
        let graph = build_graph(&files);

        stamp_ids(&mut files, &graph);

        let expected = vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    id: "src/lib.rs::foo".to_string(),
                    ..symbol("foo", vec!["bar"])
                },
                ExtractedSymbol {
                    id: "src/lib.rs::bar".to_string(),
                    ..symbol("bar", vec![])
                },
            ],
        }];

        assert_eq!(expected, files);
    }

    #[test]
    fn should_return_no_hotspots_when_every_node_has_fan_in_below_two() {
        let files = vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec![])],
        }];
        let graph = build_graph(&files);

        let expected: Vec<Hotspot> = vec![];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_count_fan_in_and_sort_referrer_names_when_two_symbols_reference_one_target() {
        // "zoo" and "alpha" both reference "shared" — fan-in 2 qualifies as
        // a hotspot, and `used_by` must come back name-sorted ("alpha"
        // before "zoo") regardless of edge order.
        let files = vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("zoo", vec!["shared"]),
                symbol("alpha", vec!["shared"]),
                symbol("shared", vec![]),
            ],
        }];
        let graph = build_graph(&files);

        let expected = vec![Hotspot {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["alpha".to_string(), "zoo".to_string()],
        }];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_count_cycle_edge_toward_fan_in_when_referrer_is_part_of_a_cycle() {
        // "foo" and "bar" reference each other (mutual recursion, so one of
        // the two edges gets marked `is_cycle: true` by `build_graph`) and
        // "baz" independently references "bar" too — "bar" ends up with
        // fan-in 2 (from "foo" and "baz"), and the cycle edge must count
        // toward that just like a non-cycle edge would.
        let files = vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("foo", vec!["bar"]),
                symbol("bar", vec!["foo"]),
                symbol("baz", vec!["bar"]),
            ],
        }];
        let graph = build_graph(&files);
        // Sanity check on the fixture: confirm build_graph actually marked
        // one of foo<->bar's edges as a cycle, since this test's premise
        // depends on that.
        assert!(graph.edges.iter().any(|e| e.is_cycle));

        let expected = vec![Hotspot {
            id: "src/lib.rs::bar".to_string(),
            path: "src/lib.rs".to_string(),
            name: "bar".to_string(),
            used_by: vec!["baz".to_string(), "foo".to_string()],
        }];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_dedup_referrer_when_same_node_references_target_more_than_once() {
        // `collect_edges` cannot currently produce two edges from the same
        // referrer to the same target (referenced_names de-dups upstream),
        // but `compute_hotspots` must not over-count fan-in if it ever did
        // — constructing the graph by hand here rather than through
        // `build_graph` to exercise that defensively.
        let graph = SymbolGraph {
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
                Node {
                    id: "src/lib.rs::shared".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "shared".to_string(),
                },
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::shared".to_string(),
                    is_cycle: false,
                },
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
            ],
            roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
        };

        let expected = vec![Hotspot {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["bar".to_string(), "foo".to_string()],
        }];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_sort_hotspots_by_fan_in_descending_when_multiple_hotspots_exist() {
        // "low" has fan-in 2 ("a", "b"); "high" has fan-in 3 ("c", "d",
        // "e") — "high" must sort first despite "low" being discovered
        // first in edge order.
        let graph = SymbolGraph {
            nodes: vec![
                Node {
                    id: "src/lib.rs::a".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "a".to_string(),
                },
                Node {
                    id: "src/lib.rs::b".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "b".to_string(),
                },
                Node {
                    id: "src/lib.rs::low".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "low".to_string(),
                },
                Node {
                    id: "src/lib.rs::c".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "c".to_string(),
                },
                Node {
                    id: "src/lib.rs::d".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "d".to_string(),
                },
                Node {
                    id: "src/lib.rs::e".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "e".to_string(),
                },
                Node {
                    id: "src/lib.rs::high".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "high".to_string(),
                },
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::a".to_string(),
                    to: "src/lib.rs::low".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::b".to_string(),
                    to: "src/lib.rs::low".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::c".to_string(),
                    to: "src/lib.rs::high".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::d".to_string(),
                    to: "src/lib.rs::high".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::e".to_string(),
                    to: "src/lib.rs::high".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "src/lib.rs::a".to_string(),
                "src/lib.rs::b".to_string(),
                "src/lib.rs::c".to_string(),
                "src/lib.rs::d".to_string(),
                "src/lib.rs::e".to_string(),
            ],
        };

        let expected = vec![
            Hotspot {
                id: "src/lib.rs::high".to_string(),
                path: "src/lib.rs".to_string(),
                name: "high".to_string(),
                used_by: vec!["c".to_string(), "d".to_string(), "e".to_string()],
            },
            Hotspot {
                id: "src/lib.rs::low".to_string(),
                path: "src/lib.rs".to_string(),
                name: "low".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            },
        ];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_break_fan_in_tie_by_path_then_name_when_counts_are_equal() {
        // Both "b_target" (path b.rs) and "a_target" (path a.rs) have
        // fan-in 2 — must sort a.rs before b.rs by path, not by discovery
        // order in the edges list.
        let graph = SymbolGraph {
            nodes: vec![
                Node {
                    id: "b.rs::b_target".to_string(),
                    path: "b.rs".to_string(),
                    name: "b_target".to_string(),
                },
                Node {
                    id: "a.rs::a_target".to_string(),
                    path: "a.rs".to_string(),
                    name: "a_target".to_string(),
                },
                Node {
                    id: "b.rs::x".to_string(),
                    path: "b.rs".to_string(),
                    name: "x".to_string(),
                },
                Node {
                    id: "b.rs::y".to_string(),
                    path: "b.rs".to_string(),
                    name: "y".to_string(),
                },
                Node {
                    id: "a.rs::m".to_string(),
                    path: "a.rs".to_string(),
                    name: "m".to_string(),
                },
                Node {
                    id: "a.rs::n".to_string(),
                    path: "a.rs".to_string(),
                    name: "n".to_string(),
                },
            ],
            edges: vec![
                Edge {
                    from: "b.rs::x".to_string(),
                    to: "b.rs::b_target".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "b.rs::y".to_string(),
                    to: "b.rs::b_target".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "a.rs::m".to_string(),
                    to: "a.rs::a_target".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "a.rs::n".to_string(),
                    to: "a.rs::a_target".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "b.rs::x".to_string(),
                "b.rs::y".to_string(),
                "a.rs::m".to_string(),
                "a.rs::n".to_string(),
            ],
        };

        let expected = vec![
            Hotspot {
                id: "a.rs::a_target".to_string(),
                path: "a.rs".to_string(),
                name: "a_target".to_string(),
                used_by: vec!["m".to_string(), "n".to_string()],
            },
            Hotspot {
                id: "b.rs::b_target".to_string(),
                path: "b.rs".to_string(),
                name: "b_target".to_string(),
                used_by: vec!["x".to_string(), "y".to_string()],
            },
        ];
        let actual = compute_hotspots(&graph);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_stamp_disambiguated_id_when_duplicate_path_and_name() {
        let mut files = vec![FileReport {
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
        let graph = build_graph(&files);

        stamp_ids(&mut files, &graph);

        let expected_ids = vec![
            "src/lib.rs::foo@1".to_string(),
            "src/lib.rs::foo@10".to_string(),
        ];
        let actual_ids: Vec<String> = files[0].symbols.iter().map(|s| s.id.clone()).collect();

        assert_eq!(expected_ids, actual_ids);
    }
}
