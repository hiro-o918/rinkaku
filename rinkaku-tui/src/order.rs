//! Topological directory ordering for the entry view (ADR 0016 decision
//! 4): by default, sibling directories are ordered so that outermost
//! (least-depended-on) directories show first and foundational
//! (most-depended-on) directories show last — the same shape
//! `rinkaku-core`'s `graph::find_roots` already computes at the symbol
//! level, condensed here to directories instead.
//!
//! This module reimplements the SCC-condensation approach locally rather
//! than reusing `rinkaku-core::graph`'s private helpers (CLAUDE.md: no
//! shared abstraction without a concrete second use case argued in an
//! ADR) — the algorithm is the same shape, but it operates over
//! directories, a concept `rinkaku-core`'s graph module has no notion of.

use rinkaku_core::render::Report;
use std::collections::{HashMap, HashSet};

/// How sibling directories/files are ordered in the entry view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderMode {
    /// Topological: least-depended-on directories first, foundations last
    /// (ADR 0016 decision 4, this module's default per that ADR).
    #[default]
    Topological,
    /// Plain alphabetical ordering, the toggle ADR 0016 keeps available.
    AlphaNumeric,
}

/// One directory's computed rank: its position in topological order, plus
/// whether it participates in a dependency cycle with at least one other
/// directory (a design-warning signal, ADR 0016 decision 4 / ADR 0008's
/// existing symbol-level cycle warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirRank {
    /// Ascending sort key: directories sharing an SCC share the same rank.
    /// Directories absent from the change graph entirely (e.g. a directory
    /// whose only content is removed symbols, which never enter
    /// `graph.edges`/`graph.nodes`) get no `DirRank` at all — see
    /// `rank_directories`'s doc comment — rather than an arbitrary rank
    /// value, so callers cannot accidentally compare "no data" against a
    /// real rank.
    pub rank: usize,
    pub in_cycle: bool,
}

/// The directory-level condensation used by [`rank_directories`],
/// [`cycle_partners`], [`cycle_edges`], and [`cycle_explanation`]: every
/// directory that owns at least one graph node, the inter-directory
/// adjacency derived from `report.graph.edges`, and the Tarjan SCC grouping
/// over that adjacency. Each of the four public functions above builds its
/// own `DirCondensation` from a fresh `report` — the type itself doesn't
/// cache across calls — but a single call that needs more than one piece of
/// derived information (partners *and* edges, [`cycle_explanation`]'s case)
/// builds exactly one `DirCondensation` and reads both off it, rather than
/// re-running Tarjan and re-deriving the same directory index twice within
/// that one call.
struct DirCondensation<'a> {
    /// Directory paths, sorted, indexed by position — `dirs[i]` is the
    /// directory `scc_of[i]`/`adjacency[i]` refer to by index `i`.
    dirs: Vec<&'a str>,
    /// Adjacency between directories (by index into `dirs`), edges within
    /// the same directory already dropped (see `build`'s doc comment).
    adjacency: Vec<Vec<usize>>,
    sccs: Vec<Vec<usize>>,
    /// `scc_of[i]` is the SCC index (into `sccs`) directory `dirs[i]`
    /// belongs to.
    scc_of: Vec<usize>,
}

impl<'a> DirCondensation<'a> {
    /// Builds the condensation from `report.graph`, first dropping every
    /// node/edge whose id names a test symbol (ADR 0035): a test-only
    /// directory (e.g. `tests/`, or a whole test file) then has no
    /// [`DirRank`] entry at all — same as today's existing "no graph
    /// presence" case (a directory whose only content is removed
    /// symbols) — and an edge from/to a test symbol cannot pull a
    /// *production* directory's rank around merely because a test
    /// happens to reference it. See [`test_node_ids`] for how test ids
    /// are found.
    ///
    /// Remaining nodes/edges are remapped from their endpoints' node ids
    /// to the parent directory of each endpoint's file path (the empty
    /// string for a root-level file, e.g. `"lib.rs"` condenses to `""`),
    /// dropping any edge whose two endpoints condense to the *same*
    /// directory — it says nothing about inter-directory dependency, only
    /// intra-directory structure (a directory doesn't depend on itself).
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

/// Every `id` in `report.files`/`report.removed` whose owning symbol is
/// test code (ADR 0035): `ExtractedSymbol::is_test` for a present symbol
/// (`report.files`), or every id in `report.removed` — a removed symbol
/// carries no `is_test` flag of its own (see `RemovedSymbol`'s shape),
/// but it also never appears in `report.graph.nodes` in the first place
/// (`tree.rs`'s own doc comment: a removed symbol is never a graph
/// node), so `report.removed` contributes nothing here in practice
/// either way — included only so a future change that *did* start giving
/// removed symbols graph ids would not silently need to revisit this.
///
/// Relies on `graph::stamp_ids` having already run before `Report` is
/// built (`pipeline::analyze_diff`/`analyze_repo`'s own doc comments):
/// `ExtractedSymbol::id` and `graph::Node::id` are the *same* stable
/// string by the time a `Report` exists, so matching on `id` here needs
/// no re-derivation of `graph::collect_nodes`'s own id-uniqueness
/// algorithm.
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
/// Ranking: Tarjan SCCs the condensed directory graph, then orders SCCs by
/// a Kahn topological sort starting from in-degree-0 SCCs — the same
/// direction `graph::find_roots` uses (an edge's `from` depends on/
/// references its `to`, so a 0-indegree SCC is reached by nobody, i.e. an
/// entry point) — so entry-point directories rank lowest (shown first) and
/// directories heavily depended upon by others rank highest (shown last,
/// as foundations). Every directory inside one SCC shares that SCC's rank
/// and is marked `in_cycle` when the SCC has more than one member.
///
/// Returns an entry only for a *leaf* directory: one that is the direct
/// parent of at least one `report.graph.nodes` path (e.g. `"src/api"` for
/// a node at `"src/api/handler.rs"`). A branching/intermediate directory
/// that owns no node directly — e.g. `"src"` itself, when only its
/// subdirectories do — is deliberately **not** given an entry here; that
/// would require walking the whole subtree per directory, which this
/// function has no tree structure to do (it only sees `report.graph`, not
/// `crate::tree::Tree`). [`effective_ranks`] is "the caller accounting for
/// nesting" this comment used to gesture at without naming: it walks the
/// built `Tree` bottom-up and gives every ancestor directory the minimum
/// rank of its descendants, so `order_tree` (which calls it internally)
/// still ranks intermediate directories correctly despite this function's
/// leaf-only contract. A directory whose entire subtree has no graph
/// presence at all (removed symbols only, or files with no changed-symbol
/// nodes) still ends up with no effective rank either, and
/// `order_tree`/`order_siblings` sort those after every ranked directory,
/// A-Z (ADR 0016 decision 4).
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

/// For every directory that participates in a directory-level cycle (an
/// SCC of size > 1 in the condensation [`rank_directories`] already
/// computes), the sorted list of *other* directories sharing that cycle —
/// the answer to "cycle と言われても何が cycle してるか分からない" (the dir
/// detail view's own motivating complaint): a directory marked `(cycle)`
/// in the entry view names its actual partners here, rather than leaving
/// the reviewer to guess. A directory with no `ranks` entry, or whose SCC
/// has only itself as a member, is simply absent from the returned map
/// (not present with an empty `Vec`) — "not in a cycle" and "in a
/// one-member cycle" (impossible, since Tarjan's own SCCs are only >1
/// member when there's an actual back edge) are the same "nothing to
/// report" case either way.
///
/// Builds its own [`DirCondensation`] — kept as its own public function
/// (rather than folded away entirely) since it has independent unit test
/// coverage and callers that only need partners, not edges too. A caller
/// that needs both (`build_dir_detail`) should use [`cycle_explanation`]
/// instead, which builds the condensation once rather than once per
/// function.
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
/// concrete `path::name -> path::name` line the dir detail view renders,
/// derived from `report.graph.edges` rather than the directory-level
/// condensation directly, since a reviewer wants to see the actual symbols
/// involved, not just "these two directories cycle".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleEdge {
    pub from_path: String,
    pub from_name: String,
    pub to_path: String,
    pub to_name: String,
}

/// Every `report.graph.edges` entry whose two endpoints' parent
/// directories are distinct members of the same cycle-forming SCC (per
/// [`cycle_partners`]) — the edges that concretely *make up* a directory
/// cycle, for the dir detail view to render as `path::name -> path::name`
/// lines. An edge within a single directory, or between two directories
/// that merely both happen to exist without forming a cycle together, is
/// excluded — only edges between two SCC-mates count.
///
/// Builds its own [`DirCondensation`], same "kept as its own function"
/// reasoning as [`cycle_partners`]'s doc comment — see [`cycle_explanation`]
/// for the shared-build alternative.
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
/// exactly the two pieces [`crate::detail::build_dir_detail`] needs.
/// Builds [`DirCondensation`] (a Tarjan SCC run over the whole directory
/// graph) exactly once and derives both results from that single build,
/// unlike calling [`cycle_partners`] and [`cycle_edges`] separately, which
/// would each re-run the same condensation from scratch.
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

/// The parent directory of a slash-separated `path` — everything before
/// the last `/`, or the empty string when `path` has no `/` at all (a
/// root-level file).
fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Orders SCC indices via Kahn's algorithm on the condensation DAG, ties
/// broken by each SCC's smallest member index (stable, deterministic
/// regardless of `tarjan_sccs`' own discovery order) — mirrors
/// `graph::find_roots`'s own tie-break ("earliest in source order").
/// Starting the frontier at in-degree-0 SCCs and always picking the
/// lowest-index-available candidate next produces entry points first,
/// foundations last, same direction `find_roots` establishes for roots.
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

    // `tarjan_sccs` never produces an empty component (each `component` it
    // pushes starts from a freshly-visited node and is only pushed after
    // collecting at least that node), so `min()` always finds a value in
    // practice; `if let` avoids asserting that invariant via `.expect()` in
    // library code (mirrors `rinkaku-core::graph::find_roots`'s identical
    // defensive handling of the same invariant) and falls back to
    // `usize::MAX` for a component that somehow turned out empty, which
    // sorts it last in the frontier rather than panicking.
    let scc_min_member: Vec<usize> = sccs
        .iter()
        .map(|scc| scc.iter().min().copied().unwrap_or(usize::MAX))
        .collect();

    let mut frontier: Vec<usize> = (0..scc_count).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(scc_count);
    let mut visited = vec![false; scc_count];

    while !frontier.is_empty() {
        // Deterministic regardless of HashSet/discovery order: always
        // advance whichever ready SCC has the earliest source-order
        // member.
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

    // Defensive: a bug in in-degree bookkeeping could in principle leave an
    // SCC unvisited (Kahn's algorithm always terminates on a DAG, and the
    // SCC condensation is always a DAG by construction). Append any
    // stragglers in source order rather than silently dropping them.
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
/// this module's own doc comment on not sharing an abstraction across the
/// crate boundary for one use case).
///
/// Because this is a deliberate duplication rather than a shared
/// dependency, the two copies do not stay in sync automatically: a bugfix
/// or algorithmic change made to one (e.g. the empty-component handling in
/// `find_roots`/`topological_scc_order`) should be checked against the
/// other and mirrored by hand if it applies there too.
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

/// Reorders every level of `tree` in place according to `mode`, using
/// `ranks` (from [`rank_directories`]) when `mode` is
/// [`OrderMode::Topological`].
///
/// Ordering rule at each level (ADR 0016 decision 4): directory children
/// sort first, file children after — files never carry a rank of their
/// own (only directories do), so keeping the two groups separate avoids
/// interleaving "ranked" and "unrankable" siblings in a way that would be
/// hard to read. Symbol children (leaves under a `File`) are never
/// reordered; they stay in the extraction/graph order `crate::tree`
/// already gave them, since ADR 0016 only asks for directory ordering.
///
/// Within the directory group: [`OrderMode::Topological`] sorts by each
/// directory's *effective* rank ascending (see [`effective_ranks`] — a
/// branching/intermediate directory that owns no graph node directly,
/// e.g. "src" when only "src/api"/"src/store" have nodes, inherits the
/// minimum rank of its descendant directories rather than being treated
/// as unranked), with directories that have no effective rank at all (no
/// ranked directory anywhere in their subtree — e.g. a directory
/// containing only removed symbols) sorted after every ranked directory,
/// then A-Z among themselves and among ties on the same rank (a stable,
/// readable tie-break absent from `rank_directories`' own contract, which
/// only promises *a* deterministic rank, not a name-based tie-break).
/// [`OrderMode::AlphaNumeric`] ignores `ranks` entirely and sorts every
/// directory A-Z. Either way, the file group is always A-Z regardless of
/// `mode` — files have no rank concept to toggle between.
pub fn order_tree(tree: &mut crate::tree::Tree, ranks: &HashMap<String, DirRank>, mode: OrderMode) {
    // Computed once up front (rather than re-derived per sort comparison)
    // since it requires a full bottom-up subtree walk per directory node —
    // doing that inside the `Ord::cmp` closure `order_siblings` uses would
    // recompute the same descendants' minimum rank on every comparison.
    let effective = effective_ranks(tree, ranks);
    order_siblings(&mut tree.roots, &effective, mode);
}

/// For every directory node in `tree`, its effective rank: the minimum
/// (outermost/least-depended-on-first) [`DirRank::rank`] across the
/// directory itself and every directory nested under it, or `None` when
/// neither the directory nor any descendant directory has a `ranks` entry
/// at all.
///
/// This is what makes `rank_directories`' contract true in practice: that
/// function only promises ranks for *leaf* directories (the direct parent
/// of a graph node's path, see its own doc comment), and this function is
/// "the caller accounting for nesting" its doc comment refers to. Without
/// this propagation, a branching intermediate directory that owns no node
/// of its own (e.g. "src" when only its subdirectories do) would have no
/// `ranks` entry and silently sort as unranked, degrading topological
/// order back to alphabetical among top-level entries — exactly the bug
/// this function exists to close.
///
/// Taking the *minimum* (rather than e.g. an average) is what preserves
/// "entry points first": if any descendant is an entry point (rank 0),
/// the ancestor directory containing it should show early too, since a
/// reviewer scanning top-to-bottom expects to reach that entry point
/// promptly rather than have it buried under unrelated higher-ranked
/// siblings.
fn effective_ranks(
    tree: &crate::tree::Tree,
    ranks: &HashMap<String, DirRank>,
) -> HashMap<String, usize> {
    let mut effective = HashMap::new();
    for root in &tree.roots {
        compute_effective_rank(root, ranks, &mut effective);
    }
    effective
}

/// Post-order walk: computes every descendant directory's effective rank
/// first, then this node's own as `min(own direct rank, min of children's
/// effective ranks)`. Returns this node's effective rank (`None` when
/// nothing in its own subtree is ranked) so the caller (a parent
/// directory) can fold it into its own minimum without re-reading the map.
fn compute_effective_rank(
    node: &crate::tree::TreeNode,
    ranks: &HashMap<String, DirRank>,
    effective: &mut HashMap<String, usize>,
) -> Option<usize> {
    if !matches!(node.kind, crate::tree::NodeKind::Dir) {
        // Files/symbols never carry a rank; nothing to fold into a parent.
        return None;
    }

    let own_rank = ranks.get(&node.path).map(|r| r.rank);
    let min_child_rank = node
        .children
        .iter()
        .filter_map(|child| compute_effective_rank(child, ranks, effective))
        .min();

    let resolved = match (own_rank, min_child_rank) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    if let Some(rank) = resolved {
        effective.insert(node.path.clone(), rank);
    }
    resolved
}

fn order_siblings(
    nodes: &mut [crate::tree::TreeNode],
    effective_ranks: &HashMap<String, usize>,
    mode: OrderMode,
) {
    for node in nodes.iter_mut() {
        if matches!(node.kind, crate::tree::NodeKind::Dir) {
            order_siblings(&mut node.children, effective_ranks, mode);
        }
    }

    nodes.sort_by(|a, b| {
        let a_is_dir = matches!(a.kind, crate::tree::NodeKind::Dir);
        let b_is_dir = matches!(b.kind, crate::tree::NodeKind::Dir);
        // Directories before files, regardless of mode.
        b_is_dir.cmp(&a_is_dir).then_with(|| match mode {
            OrderMode::Topological if a_is_dir && b_is_dir => {
                // `None` (unranked: no ranked directory anywhere in this
                // subtree) must sort after every `Some` rank — the
                // opposite of `Option<usize>`'s derived `Ord`, which puts
                // `None` first — so rank is compared via an explicit
                // "unranked last" key (`usize::MAX` standing in for "no
                // rank") rather than relying on `Option`'s own ordering.
                // Ties (same rank, or both unranked) break A-Z on path.
                let rank_key =
                    |path: &str| effective_ranks.get(path).copied().unwrap_or(usize::MAX);
                rank_key(&a.path)
                    .cmp(&rank_key(&b.path))
                    .then_with(|| a.path.cmp(&b.path))
            }
            _ => a.path.cmp(&b.path),
        })
    });
}

#[cfg(test)]
#[path = "order_tests/mod.rs"]
mod tests;
