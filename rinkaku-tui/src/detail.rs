//! Detail-pane view-model (ADR 0015): given a selected symbol, produces
//! the plain data a detail pane shows — signature (old/new when the
//! contract changed), classification, who depends on it ("used by"), and
//! a pivot to callers/callees for call-graph reading order (ADR 0008's
//! tree, reached on demand rather than as the entry view's spine).
//!
//! [`build_detail`] is a pure function of a symbol id plus [`Report`]: no
//! IO, no `ratatui` types.
//!
//! [`build_dir_detail`]/[`build_file_detail`] (TUI iteration 2) extend the
//! same detail pane to directory and file rows, which previously showed
//! only a placeholder ("select a symbol row to see its detail"). A
//! directory's detail is a badge breakdown plus, when it participates in a
//! directory-level cycle, an explanation of exactly which directories it
//! cycles with and which concrete symbol-to-symbol edges form that cycle —
//! the entry view's `(cycle)` marker on its own only says *that* a
//! directory cycles, not *with what*.

use crate::order::{CycleEdge, cycle_explanation};
use crate::tree::{Badges, NodeKind, SymbolRef, Tree, TreeNode};
use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::render::Report;
use std::collections::HashSet;

/// One symbol referenced from a [`DetailView`] (a caller or callee), named
/// and located but not carrying its full signature — a caller/callee
/// pivot is meant to answer "what else is involved", not duplicate the
/// full detail a reviewer would get by selecting that symbol in turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolMention {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// A symbol's signature, either unchanged/newly-added (`Current`) or shown
/// as an old/new pair (`Changed`) when [`Classification::SignatureChanged`]
/// applies — mirrors `render.rs`'s Markdown ` ```diff ` block decision
/// (`render_definition`), just as plain data instead of formatted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureView {
    Current(String),
    Changed { previous: String, current: String },
}

/// The full detail-pane view-model for one selected, *present* symbol
/// (see [`build_detail`]'s doc comment for why removed symbols are out of
/// scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailView {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    pub container: Option<String>,
    pub signature: SignatureView,
    pub classification: Option<Classification>,
    /// Every changed symbol that references this one, i.e. this symbol's
    /// fan-in (ADR 0013). Computed directly from `report.graph.edges`
    /// (every incoming edge), not from `report.fan_ins` — `FanIn` only
    /// aggregates fan-in >= 2 (see `compute_fan_ins`'s doc comment in
    /// `rinkaku-core::graph`) and would under-report a fan-in of 0 or 1,
    /// so a symbol with exactly one referrer still shows it here instead
    /// of reading as "nobody depends on this".
    pub used_by: Vec<SymbolMention>,
    /// Every symbol this one references, i.e. outgoing edges
    /// (`report.graph.edges` where `from == id`) — the callees pivot ADR
    /// 0015 asks for.
    pub callees: Vec<SymbolMention>,
    /// Every symbol referencing this one, i.e. incoming edges
    /// (`report.graph.edges` where `to == id`) — the callers pivot ADR 0015
    /// asks for. Deliberately kept as its own field even though its
    /// *content* always matches `used_by` (both derive from the same
    /// incoming-edge set): `used_by` is fan-in framing ("who depends on my
    /// signature"), `callers` is call-graph framing ("where do I get
    /// reached from") — ADR 0015 asks for both framings as distinct pivots
    /// even though v1's data happens to make them redundant. A future
    /// resolver-based dependency model (ADR 0015's context on pluggable
    /// `Resolver`s) could pull these apart (e.g. a caller found via a
    /// different mechanism than a fan-in-relevant reference), so keeping
    /// two fields now avoids a breaking rename later.
    pub callers: Vec<SymbolMention>,
}

/// Builds a [`DetailView`] for the *present* (non-removed) symbol
/// identified by `id` in `report`, or `None` when no such symbol exists.
///
/// Removed symbols are out of scope for this function: a
/// [`rinkaku_core::extract::RemovedSymbol`] carries no stable id
/// (`crate::tree::SymbolRef`'s doc comment already notes this), no
/// `graph` presence to pivot callers/callees from, and no dependencies —
/// there is no call-graph detail to show beyond what `Report.removed`
/// already carries directly (name, kind, path, prior signature), which a
/// caller can render straight from that struct without going through this
/// pure function.
pub fn build_detail(report: &Report, id: &str) -> Option<DetailView> {
    let (path, symbol) = report.files.iter().find_map(|file| {
        file.symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| (file.path.as_str(), symbol))
    })?;

    let signature = match (symbol.classification, &symbol.previous_signature) {
        (Some(Classification::SignatureChanged), Some(previous)) => SignatureView::Changed {
            previous: previous.clone(),
            current: symbol.signature.clone(),
        },
        _ => SignatureView::Current(symbol.signature.clone()),
    };

    let callees = symbol_mentions(report, id, MentionDirection::Callees);
    let callers = symbol_mentions(report, id, MentionDirection::Callers);

    // `used_by` is every incoming edge, same set `callers` already
    // computed above — `report.fan_ins` is not consulted here because it
    // only aggregates fan-in >= 2 (see `compute_fan_ins`'s doc comment in
    // `rinkaku-core::graph`) and would under-report a fan-in of 0 or 1;
    // reading `graph.edges` directly covers every fan-in count uniformly,
    // `FanIn` included (a fan-in entry's referrers are exactly its
    // incoming edges). `used_by` is kept as its own field distinct from
    // `callers` for the same forward-compatibility reason documented on
    // `DetailView::callers`.
    let used_by = callers.clone();

    Some(DetailView {
        id: symbol.id.clone(),
        name: symbol.name.clone(),
        kind: symbol.kind,
        path: path.to_string(),
        container: symbol.container.clone(),
        signature,
        classification: symbol.classification,
        used_by,
        callees,
        callers,
    })
}

/// Which side of `report.graph.edges` [`symbol_mentions`] should walk: the
/// symbols `id` references ([`Self::Callees`], outgoing edges) or the
/// symbols referencing `id` ([`Self::Callers`], incoming edges).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionDirection {
    Callees,
    Callers,
}

/// Every present symbol directly connected to `id` in `report.graph.edges`,
/// in the direction `direction` selects — the callee/caller pivot both
/// [`build_detail`] (the Detail pane) and jump navigation (ADR 0022's
/// `gd`/`gr`) need, extracted here rather than duplicated so both callers
/// share one dedup/self-edge-filter contract instead of two independently
/// maintained copies of it.
///
/// Edge uniqueness within `graph.edges` is not a contractual guarantee —
/// `compute_fan_ins`'s own doc comment in `rinkaku-core::graph` notes that
/// a repeated edge between the same pair of nodes is not something
/// `build_graph` can currently produce, but nothing in this function's
/// contract depends on that staying true either. Deduping by the other
/// endpoint's id before collecting mentions keeps a duplicate edge from
/// making the same caller/callee show up twice, mirroring
/// `compute_fan_ins`'s own dedup-by-referrer-id.
///
/// Self-edges (`from == to == id`) are filtered the same defensive way:
/// `collect_edges` in `rinkaku-core::graph` explicitly excludes them (`if
/// target.id != from.id`), so they cannot occur through the normal pipeline
/// today, but — same reasoning as the dedup above — this function's own
/// contract does not want to depend on that staying true forever. Without
/// this filter, a hypothetical self-edge would make a symbol list itself as
/// its own caller/callee, which is never a meaningful pivot to show a
/// reviewer.
pub fn symbol_mentions(
    report: &Report,
    id: &str,
    direction: MentionDirection,
) -> Vec<SymbolMention> {
    let mentions_by_id = |target_id: &str| -> Option<SymbolMention> {
        report.files.iter().find_map(|file| {
            file.symbols
                .iter()
                .find(|s| s.id == target_id)
                .map(|s| SymbolMention {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    path: file.path.clone(),
                })
        })
    };

    let mut seen = HashSet::new();
    match direction {
        MentionDirection::Callees => report
            .graph
            .edges
            .iter()
            .filter(|edge| edge.from == id && edge.to != id)
            .filter(|edge| seen.insert(edge.to.as_str()))
            .filter_map(|edge| mentions_by_id(&edge.to))
            .collect(),
        MentionDirection::Callers => report
            .graph
            .edges
            .iter()
            .filter(|edge| edge.to == id && edge.from != id)
            .filter(|edge| seen.insert(edge.from.as_str()))
            .filter_map(|edge| mentions_by_id(&edge.from))
            .collect(),
    }
}

/// A cross-directory edge forming part of a directory cycle, as displayed
/// text — `crate::order::CycleEdge`'s fields pre-joined into the
/// `path::name -> path::name` shape the detail pane renders directly,
/// keeping `crate::ui` free of string-formatting decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleEdgeView {
    pub from: String,
    pub to: String,
}

impl From<&CycleEdge> for CycleEdgeView {
    fn from(edge: &CycleEdge) -> Self {
        Self {
            from: format!("{}::{}", edge.from_path, edge.from_name),
            to: format!("{}::{}", edge.to_path, edge.to_name),
        }
    }
}

/// The detail-pane view-model for a selected [`NodeKind::Dir`] row
/// (TUI iteration 2): a badge breakdown (already aggregated bottom-up onto
/// the node by `crate::tree::build_tree`), the directory's own top fan-in
/// symbols, and — only when this directory participates in a
/// directory-level cycle — which other directories it cycles with and the
/// concrete symbol-to-symbol edges that make up that cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirDetail {
    pub path: String,
    pub badges: Badges,
    /// This directory's own high-fan-in symbols (fan-in >= 2), sorted by
    /// fan-in descending then `(path, name)` ascending for determinism —
    /// mirrors `compute_fan_ins`'s own tie-break in `rinkaku-core::graph`.
    /// Capped at 5: this is a "what stands out" summary, not an exhaustive
    /// listing (the badge's `fan_in` count already carries the full
    /// aggregate).
    pub top_fan_in: Vec<SymbolMention>,
    /// Other directories sharing this directory's cycle (empty when this
    /// directory is not in a cycle at all), sorted ascending.
    pub cycle_partners: Vec<String>,
    /// Concrete cross-directory edges forming the cycle this directory
    /// participates in, restricted to edges touching this directory as
    /// either endpoint (empty when not in a cycle) — the answer to "cycle
    /// と言われても何が cycle してるか分からない".
    pub cycle_edges: Vec<CycleEdgeView>,
}

/// One symbol summary line for a [`FileDetail`]: enough to render a
/// classification marker and fan-in without duplicating the full
/// [`DetailView`] a reviewer gets by selecting that symbol row directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSymbolSummary {
    pub name: String,
    pub kind: SymbolKind,
    pub classification: Option<Classification>,
    pub removed: bool,
    pub fan_in: usize,
}

/// The detail-pane view-model for a selected [`NodeKind::File`] row (TUI
/// iteration 2): the list of symbols changed in this file, each with its
/// classification marker and fan-in — a compact index of "what changed
/// here" before drilling into an individual symbol row.
///
/// `skip_reason`/`test_symbol_count` mirror `crate::tree::TreeNode`'s own
/// fields of the same name (copied straight from the tree node this detail
/// was built from — see `build_file_detail`) so the detail pane can explain
/// *why* a skipped file has no `symbols` entries instead of silently
/// showing an empty list, which used to read as "this file changed nothing"
/// rather than "rinkaku did not analyze this file's symbols" — and, for a
/// whole/mixed-test file, can additionally note the excluded test-symbol
/// count alongside whatever real `symbols` the file does have (`skip_reason`
/// and `test_symbol_count` are mutually exclusive, but `test_symbol_count`
/// and `symbols` are not — `crate::tree::TreeNode::test_symbol_count`'s own
/// doc comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDetail {
    pub path: String,
    pub symbols: Vec<FileSymbolSummary>,
    pub skip_reason: Option<rinkaku_core::render::SkipReason>,
    pub test_symbol_count: Option<usize>,
    /// Oversized-file warning for this file (ADR 0028), when
    /// `report.file_size_warnings` has an entry whose `path` matches this
    /// file. `None` means the file is under the watch threshold — a
    /// distinct signal from the aggregated total on the status line.
    pub size_warning: Option<rinkaku_core::file_size::FileSizeWarning>,
}

/// Builds a [`DirDetail`] for the directory at `path` in `tree`, or `None`
/// when no such directory node exists. `report` supplies the cycle
/// explanation (`crate::order::cycle_explanation`, which builds the
/// directory-level SCC condensation exactly once rather than once per
/// piece of information) and the fan-in lookup for `top_fan_in` — both
/// computed fresh per call, same "recompute rather than cache" philosophy
/// the rest of this view-model layer already follows (ADR 0016 decision 1).
pub fn build_dir_detail(tree: &Tree, report: &Report, path: &str) -> Option<DirDetail> {
    let node = find_dir_node(tree, path)?;

    let fan_in_by_id: std::collections::HashMap<&str, &rinkaku_core::graph::FanIn> = report
        .fan_ins
        .iter()
        .map(|fan_in| (fan_in.id.as_str(), fan_in))
        .collect();

    let mut symbol_ids = Vec::new();
    collect_symbol_ids(node, &mut symbol_ids);

    let mut top_fan_in: Vec<SymbolMention> = symbol_ids
        .iter()
        .filter_map(|id| fan_in_by_id.get(id.as_str()).map(|f| (*f, id)))
        .map(|(fan_in, id)| SymbolMention {
            id: id.clone(),
            name: fan_in.name.clone(),
            path: fan_in.path.clone(),
        })
        .collect();
    // Sort by fan-in descending (looked up again per entry rather than
    // carried alongside — the list is small, capped at 5 below, so a
    // second map lookup per comparison is not worth avoiding via a tuple),
    // ties broken by (path, name) ascending, mirroring
    // `compute_fan_ins`'s own tie-break in `rinkaku-core::graph`.
    top_fan_in.sort_by(|a, b| {
        let fan_in_of = |mention: &SymbolMention| {
            fan_in_by_id
                .get(mention.id.as_str())
                .map(|f| f.used_by.len())
                .unwrap_or(0)
        };
        fan_in_of(b)
            .cmp(&fan_in_of(a))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.name.cmp(&b.name))
    });
    top_fan_in.truncate(5);

    let (partners, cycle_edges) = cycle_explanation(report, path);
    let edges: Vec<CycleEdgeView> = cycle_edges.iter().map(CycleEdgeView::from).collect();

    Some(DirDetail {
        path: node.path.clone(),
        badges: node.badges,
        top_fan_in,
        cycle_partners: partners,
        cycle_edges: edges,
    })
}

/// Builds a [`FileDetail`] for the file at `path` in `tree`, or `None` when
/// no such file node exists.
pub fn build_file_detail(tree: &Tree, report: &Report, path: &str) -> Option<FileDetail> {
    let node = find_file_node(tree, path)?;

    let fan_in_by_id: std::collections::HashMap<&str, usize> = report
        .fan_ins
        .iter()
        .map(|fan_in| (fan_in.id.as_str(), fan_in.used_by.len()))
        .collect();

    // A mixed file's test symbols are nested one level deeper, under a
    // synthetic `TestGroup` child (visual-encoding prototype) rather than
    // directly under the file — flatten that one level so the detail pane
    // still lists every symbol regardless of the tree's grouping.
    let symbol_source = node.children.iter().flat_map(|child| match &child.kind {
        NodeKind::TestGroup { .. } => child.children.iter(),
        _ => std::slice::from_ref(child).iter(),
    });
    let symbols: Vec<FileSymbolSummary> = symbol_source
        .filter_map(|child| match &child.kind {
            NodeKind::Symbol(symbol_ref) => Some(file_symbol_summary(symbol_ref, &fan_in_by_id)),
            _ => None,
        })
        .collect();

    let size_warning = report
        .file_size_warnings
        .iter()
        .find(|warning| warning.path == node.path)
        .cloned();

    Some(FileDetail {
        path: node.path.clone(),
        symbols,
        skip_reason: node.skip_reason,
        test_symbol_count: node.test_symbol_count,
        size_warning,
    })
}

fn file_symbol_summary(
    symbol_ref: &SymbolRef,
    fan_in_by_id: &std::collections::HashMap<&str, usize>,
) -> FileSymbolSummary {
    FileSymbolSummary {
        name: symbol_ref.name.clone(),
        kind: symbol_ref.kind,
        classification: symbol_ref.classification,
        removed: symbol_ref.removed,
        fan_in: fan_in_by_id
            .get(symbol_ref.id.as_str())
            .copied()
            .unwrap_or(0),
    }
}

fn find_dir_node<'a>(tree: &'a Tree, path: &str) -> Option<&'a TreeNode> {
    tree.roots
        .iter()
        .find_map(|root| find_node(root, path, |kind| matches!(kind, NodeKind::Dir)))
}

fn find_file_node<'a>(tree: &'a Tree, path: &str) -> Option<&'a TreeNode> {
    tree.roots
        .iter()
        .find_map(|root| find_node(root, path, |kind| matches!(kind, NodeKind::File)))
}

fn find_node<'a>(
    node: &'a TreeNode,
    path: &str,
    matches_kind: impl Fn(&NodeKind) -> bool + Copy,
) -> Option<&'a TreeNode> {
    if node.path == path && matches_kind(&node.kind) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, path, matches_kind))
}

fn collect_symbol_ids(node: &TreeNode, ids: &mut Vec<String>) {
    if let NodeKind::Symbol(symbol_ref) = &node.kind {
        ids.push(symbol_ref.id.clone());
    }
    for child in &node.children {
        collect_symbol_ids(child, ids);
    }
}

#[cfg(test)]
#[path = "detail_tests/mod.rs"]
mod tests;
