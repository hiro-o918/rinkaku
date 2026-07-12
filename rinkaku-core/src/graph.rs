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

    let index_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

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
        let representative = *scc.iter().min().expect("an SCC always has >=1 member");
        roots.push(nodes[representative].id.clone());
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

    let index_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

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

    while let Some(&(v, child_i)) = work.last() {
        if child_i < adjacency[v].len() {
            let (w, edge_index) = adjacency[v][child_i];
            work.last_mut().unwrap().1 += 1;

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

            if child_i < adjacency[v].len() {
                let w = adjacency[v][child_i];
                work.last_mut().unwrap().1 += 1;

                if indices[w].is_none() {
                    work.push((w, 0));
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(indices[w].unwrap());
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    lowlink[parent] = lowlink[parent].min(lowlink[v]);
                }

                if lowlink[v] == indices[v].unwrap() {
                    let mut component = Vec::new();
                    loop {
                        let w = stack.pop().expect("stack must contain v's SCC members");
                        on_stack[w] = false;
                        component.push(w);
                        if w == v {
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
