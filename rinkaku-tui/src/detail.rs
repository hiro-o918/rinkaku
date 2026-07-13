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
    /// (every incoming edge), not from `report.hotspots` — `Hotspot` only
    /// aggregates fan-in >= 2 (see `compute_hotspots`'s doc comment in
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
    // computed above — `report.hotspots` is not consulted here because it
    // only aggregates fan-in >= 2 (see `compute_hotspots`'s doc comment in
    // `rinkaku-core::graph`) and would under-report a fan-in of 0 or 1;
    // reading `graph.edges` directly covers every fan-in count uniformly,
    // `Hotspot` included (a hotspot's referrers are exactly its incoming
    // edges). `used_by` is kept as its own field distinct from `callers`
    // for the same forward-compatibility reason documented on
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
/// `compute_hotspots`'s own doc comment in `rinkaku-core::graph` notes that
/// a repeated edge between the same pair of nodes is not something
/// `build_graph` can currently produce, but nothing in this function's
/// contract depends on that staying true either. Deduping by the other
/// endpoint's id before collecting mentions keeps a duplicate edge from
/// making the same caller/callee show up twice, mirroring
/// `compute_hotspots`'s own dedup-by-referrer-id.
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
    /// This directory's own hotspot symbols (fan-in >= 2), sorted by
    /// fan-in descending then `(path, name)` ascending for determinism —
    /// mirrors `compute_hotspots`'s own tie-break in `rinkaku-core::graph`.
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
/// piece of information) and the hotspot lookup for `top_fan_in` — both
/// computed fresh per call, same "recompute rather than cache" philosophy
/// the rest of this view-model layer already follows (ADR 0016 decision 1).
pub fn build_dir_detail(tree: &Tree, report: &Report, path: &str) -> Option<DirDetail> {
    let node = find_dir_node(tree, path)?;

    let hotspot_by_id: std::collections::HashMap<&str, &rinkaku_core::graph::Hotspot> = report
        .hotspots
        .iter()
        .map(|hotspot| (hotspot.id.as_str(), hotspot))
        .collect();

    let mut symbol_ids = Vec::new();
    collect_symbol_ids(node, &mut symbol_ids);

    let mut top_fan_in: Vec<SymbolMention> = symbol_ids
        .iter()
        .filter_map(|id| hotspot_by_id.get(id.as_str()).map(|h| (*h, id)))
        .map(|(hotspot, id)| SymbolMention {
            id: id.clone(),
            name: hotspot.name.clone(),
            path: hotspot.path.clone(),
        })
        .collect();
    // Sort by fan-in descending (looked up again per entry rather than
    // carried alongside — the list is small, capped at 5 below, so a
    // second map lookup per comparison is not worth avoiding via a tuple),
    // ties broken by (path, name) ascending, mirroring
    // `compute_hotspots`'s own tie-break in `rinkaku-core::graph`.
    top_fan_in.sort_by(|a, b| {
        let fan_in_of = |mention: &SymbolMention| {
            hotspot_by_id
                .get(mention.id.as_str())
                .map(|h| h.used_by.len())
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
        .hotspots
        .iter()
        .map(|hotspot| (hotspot.id.as_str(), hotspot.used_by.len()))
        .collect();

    let symbols: Vec<FileSymbolSummary> = node
        .children
        .iter()
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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::ExtractedSymbol;
    use rinkaku_core::graph::{Edge, Hotspot, Node, SymbolGraph};
    use rinkaku_core::render::FileReport;

    fn symbol(id: &str, name: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn node(id: &str, path: &str, name: &str) -> Node {
        Node {
            id: id.to_string(),
            path: path.to_string(),
            name: name.to_string(),
        }
    }

    fn empty_report() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_return_none_when_symbol_id_is_not_found() {
        let report = empty_report();

        let actual = build_detail(&report, "missing::id");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_build_current_signature_when_classification_is_not_signature_changed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::BodyOnly),
                    ..symbol("lib.rs::foo", "foo")
                }],
            }],
            ..empty_report()
        };

        let expected = DetailView {
            id: "lib.rs::foo".to_string(),
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            path: "lib.rs".to_string(),
            container: None,
            signature: SignatureView::Current("fn foo()".to_string()),
            classification: Some(Classification::BodyOnly),
            used_by: vec![],
            callees: vec![],
            callers: vec![],
        };
        let actual = build_detail(&report, "lib.rs::foo");

        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_build_changed_signature_when_classification_is_signature_changed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    previous_signature: Some("fn foo(a: i32)".to_string()),
                    signature: "fn foo(a: i32, b: i32)".to_string(),
                    ..symbol("lib.rs::foo", "foo")
                }],
            }],
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::foo").expect("symbol found");

        let expected_signature = SignatureView::Changed {
            previous: "fn foo(a: i32)".to_string(),
            current: "fn foo(a: i32, b: i32)".to_string(),
        };
        assert_eq!(expected_signature, actual.signature);
    }

    #[test]
    fn should_fall_back_to_current_signature_when_previous_signature_is_missing() {
        // Defensive: SignatureChanged without a previous_signature (should
        // not happen per `classify_symbols`'s contract) still renders
        // something sane rather than panicking.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    previous_signature: None,
                    ..symbol("lib.rs::foo", "foo")
                }],
            }],
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::foo").expect("symbol found");

        assert_eq!(
            SignatureView::Current("fn foo()".to_string()),
            actual.signature
        );
    }

    #[test]
    fn should_list_single_caller_as_used_by_when_fan_in_is_one() {
        // fan-in of exactly 1 is below Hotspot's >= 2 threshold, so
        // `report.hotspots` has nothing for "callee" — used_by must still
        // show the one caller by reading `graph.edges` directly.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::caller", "caller"),
                    symbol("lib.rs::callee", "callee"),
                ],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::caller", "lib.rs", "caller"),
                    node("lib.rs::callee", "lib.rs", "callee"),
                ],
                edges: vec![Edge {
                    from: "lib.rs::caller".to_string(),
                    to: "lib.rs::callee".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::caller".to_string()],
            },
            hotspots: vec![],
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::callee").expect("symbol found");

        let expected_used_by = vec![SymbolMention {
            id: "lib.rs::caller".to_string(),
            name: "caller".to_string(),
            path: "lib.rs".to_string(),
        }];
        assert_eq!(expected_used_by, actual.used_by);
    }

    #[test]
    fn should_list_every_referrer_as_used_by_when_symbol_is_a_hotspot() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::a", "a"),
                    symbol("lib.rs::b", "b"),
                    symbol("lib.rs::shared", "shared"),
                ],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::a", "lib.rs", "a"),
                    node("lib.rs::b", "lib.rs", "b"),
                    node("lib.rs::shared", "lib.rs", "shared"),
                ],
                edges: vec![
                    Edge {
                        from: "lib.rs::a".to_string(),
                        to: "lib.rs::shared".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "lib.rs::b".to_string(),
                        to: "lib.rs::shared".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["lib.rs::a".to_string(), "lib.rs::b".to_string()],
            },
            hotspots: vec![Hotspot {
                id: "lib.rs::shared".to_string(),
                path: "lib.rs".to_string(),
                name: "shared".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            }],
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::shared").expect("symbol found");

        let mut used_by_names: Vec<&str> = actual.used_by.iter().map(|m| m.name.as_str()).collect();
        used_by_names.sort();
        assert_eq!(vec!["a", "b"], used_by_names);
    }

    #[test]
    fn should_list_outgoing_edges_as_callees() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::a", "a"), symbol("lib.rs::b", "b")],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::a", "lib.rs", "a"),
                    node("lib.rs::b", "lib.rs", "b"),
                ],
                edges: vec![Edge {
                    from: "lib.rs::a".to_string(),
                    to: "lib.rs::b".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::a".to_string()],
            },
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::a").expect("symbol found");

        let expected_callees = vec![SymbolMention {
            id: "lib.rs::b".to_string(),
            name: "b".to_string(),
            path: "lib.rs".to_string(),
        }];
        assert_eq!(expected_callees, actual.callees);
        assert_eq!(Vec::<SymbolMention>::new(), actual.callers);
    }

    #[test]
    fn should_have_empty_callers_and_callees_when_symbol_has_no_edges() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::solo", "solo")],
            }],
            graph: SymbolGraph {
                nodes: vec![node("lib.rs::solo", "lib.rs", "solo")],
                edges: vec![],
                roots: vec!["lib.rs::solo".to_string()],
            },
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::solo").expect("symbol found");

        assert_eq!(Vec::<SymbolMention>::new(), actual.callers);
        assert_eq!(Vec::<SymbolMention>::new(), actual.callees);
        assert_eq!(Vec::<SymbolMention>::new(), actual.used_by);
    }

    // SHOULD-FIX 5: `graph.edges` uniqueness is not a contractual guarantee
    // (`compute_hotspots`'s own doc comment in rinkaku-core::graph notes
    // this, and defends against it by deduping referrers per target) —
    // `build_detail` must apply the same defensive dedup to `callees`,
    // `callers`, and `used_by` rather than assume `graph.edges` never
    // repeats an edge, or a duplicate edge would silently double-count a
    // caller/callee in the detail pane's pivots.
    #[test]
    fn should_dedup_duplicate_edges_between_the_same_pair_of_symbols() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::caller", "caller"),
                    symbol("lib.rs::callee", "callee"),
                ],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::caller", "lib.rs", "caller"),
                    node("lib.rs::callee", "lib.rs", "callee"),
                ],
                // Same caller -> callee edge listed twice — a hand-built
                // graph standing in for whatever upstream circumstance
                // (not currently reachable through `build_graph`, per
                // `compute_hotspots`'s own doc comment) might one day
                // produce a repeated edge.
                edges: vec![
                    Edge {
                        from: "lib.rs::caller".to_string(),
                        to: "lib.rs::callee".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "lib.rs::caller".to_string(),
                        to: "lib.rs::callee".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["lib.rs::caller".to_string()],
            },
            ..empty_report()
        };

        let callee_detail = build_detail(&report, "lib.rs::callee").expect("symbol found");
        let caller_detail = build_detail(&report, "lib.rs::caller").expect("symbol found");

        let expected_caller_mention = vec![SymbolMention {
            id: "lib.rs::caller".to_string(),
            name: "caller".to_string(),
            path: "lib.rs".to_string(),
        }];
        let expected_callee_mention = vec![SymbolMention {
            id: "lib.rs::callee".to_string(),
            name: "callee".to_string(),
            path: "lib.rs".to_string(),
        }];
        assert_eq!(expected_caller_mention, callee_detail.callers);
        assert_eq!(expected_caller_mention, callee_detail.used_by);
        assert_eq!(expected_callee_mention, caller_detail.callees);
    }

    // SHOULD-FIX 5: `rinkaku-core::graph::collect_edges` explicitly excludes
    // self-edges (`if target.id != from.id`), so a self-edge cannot occur
    // through the normal pipeline today — but `build_detail`'s own contract
    // should not rely on that staying true forever, same reasoning as the
    // duplicate-edge dedup above. This pins the defensive filter with a
    // hand-built graph containing a self-edge no real `build_graph` call
    // would currently produce.
    #[test]
    fn should_exclude_self_edge_from_callers_callees_and_used_by() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::recursive", "recursive")],
            }],
            graph: SymbolGraph {
                nodes: vec![node("lib.rs::recursive", "lib.rs", "recursive")],
                edges: vec![Edge {
                    from: "lib.rs::recursive".to_string(),
                    to: "lib.rs::recursive".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::recursive".to_string()],
            },
            ..empty_report()
        };

        let actual = build_detail(&report, "lib.rs::recursive").expect("symbol found");

        assert_eq!(Vec::<SymbolMention>::new(), actual.callees);
        assert_eq!(Vec::<SymbolMention>::new(), actual.callers);
        assert_eq!(Vec::<SymbolMention>::new(), actual.used_by);
    }

    // symbol_mentions tests (ADR 0022): direct coverage of the function
    // extracted out of `build_detail`'s own callee/caller computation, so
    // jump navigation's use of it is pinned independently of the Detail
    // pane's own tests above.

    #[test]
    fn should_return_outgoing_edges_as_callees_via_symbol_mentions() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::a", "a"), symbol("lib.rs::b", "b")],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::a", "lib.rs", "a"),
                    node("lib.rs::b", "lib.rs", "b"),
                ],
                edges: vec![Edge {
                    from: "lib.rs::a".to_string(),
                    to: "lib.rs::b".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::a".to_string()],
            },
            ..empty_report()
        };

        let actual = symbol_mentions(&report, "lib.rs::a", MentionDirection::Callees);

        assert_eq!(
            vec![SymbolMention {
                id: "lib.rs::b".to_string(),
                name: "b".to_string(),
                path: "lib.rs".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_return_incoming_edges_as_callers_via_symbol_mentions() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::a", "a"), symbol("lib.rs::b", "b")],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::a", "lib.rs", "a"),
                    node("lib.rs::b", "lib.rs", "b"),
                ],
                edges: vec![Edge {
                    from: "lib.rs::a".to_string(),
                    to: "lib.rs::b".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::a".to_string()],
            },
            ..empty_report()
        };

        let actual = symbol_mentions(&report, "lib.rs::b", MentionDirection::Callers);

        assert_eq!(
            vec![SymbolMention {
                id: "lib.rs::a".to_string(),
                name: "a".to_string(),
                path: "lib.rs".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_return_empty_mentions_when_symbol_has_no_edges() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::solo", "solo")],
            }],
            graph: SymbolGraph {
                nodes: vec![node("lib.rs::solo", "lib.rs", "solo")],
                edges: vec![],
                roots: vec!["lib.rs::solo".to_string()],
            },
            ..empty_report()
        };

        let callees = symbol_mentions(&report, "lib.rs::solo", MentionDirection::Callees);
        let callers = symbol_mentions(&report, "lib.rs::solo", MentionDirection::Callers);

        assert_eq!(Vec::<SymbolMention>::new(), callees);
        assert_eq!(Vec::<SymbolMention>::new(), callers);
    }

    // build_dir_detail / build_file_detail tests (TUI iteration 2).

    #[test]
    fn should_return_none_when_dir_path_is_not_found() {
        let report = empty_report();
        let tree = crate::tree::build_tree(&report);

        let actual = build_dir_detail(&tree, &report, "missing");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_build_dir_detail_with_badges_and_no_cycle_when_directory_is_not_in_a_cycle() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    ..symbol("src/lib.rs::foo", "foo")
                }],
            }],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

        let expected = DirDetail {
            path: "src".to_string(),
            badges: crate::tree::Badges {
                changed_symbols: 1,
                contract_changes: 1,
                fan_in: 0,
                ..crate::tree::Badges::default()
            },
            top_fan_in: vec![],
            cycle_partners: vec![],
            cycle_edges: vec![],
        };
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_list_top_fan_in_symbols_sorted_by_fan_in_descending() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::a", "a"),
                    symbol("src/lib.rs::b", "b"),
                    symbol("src/lib.rs::shared_low", "shared_low"),
                    symbol("src/lib.rs::shared_high", "shared_high"),
                ],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::a", "src/lib.rs", "a"),
                    node("src/lib.rs::b", "src/lib.rs", "b"),
                    node("src/lib.rs::shared_low", "src/lib.rs", "shared_low"),
                    node("src/lib.rs::shared_high", "src/lib.rs", "shared_high"),
                ],
                edges: vec![],
                roots: vec![],
            },
            hotspots: vec![
                Hotspot {
                    id: "src/lib.rs::shared_low".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "shared_low".to_string(),
                    used_by: vec!["a".to_string(), "b".to_string()],
                },
                Hotspot {
                    id: "src/lib.rs::shared_high".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "shared_high".to_string(),
                    used_by: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                },
            ],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

        let expected_top_fan_in = vec![
            SymbolMention {
                id: "src/lib.rs::shared_high".to_string(),
                name: "shared_high".to_string(),
                path: "src/lib.rs".to_string(),
            },
            SymbolMention {
                id: "src/lib.rs::shared_low".to_string(),
                name: "shared_low".to_string(),
                path: "src/lib.rs".to_string(),
            },
        ];
        assert_eq!(expected_top_fan_in, actual.top_fan_in);
    }

    #[test]
    fn should_truncate_top_fan_in_to_five_entries() {
        let symbols: Vec<ExtractedSymbol> = (0..7)
            .map(|i| symbol(&format!("src/lib.rs::s{i}"), &format!("s{i}")))
            .collect();
        let nodes: Vec<Node> = (0..7)
            .map(|i| node(&format!("src/lib.rs::s{i}"), "src/lib.rs", &format!("s{i}")))
            .collect();
        let hotspots: Vec<Hotspot> = (0..7)
            .map(|i| Hotspot {
                id: format!("src/lib.rs::s{i}"),
                path: "src/lib.rs".to_string(),
                name: format!("s{i}"),
                used_by: vec!["x".to_string(), "y".to_string()],
            })
            .collect();
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols,
            }],
            graph: SymbolGraph {
                nodes,
                edges: vec![],
                roots: vec![],
            },
            hotspots,
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_dir_detail(&tree, &report, "src").expect("dir found");

        assert_eq!(5, actual.top_fan_in.len());
    }

    #[test]
    fn should_explain_cycle_partners_and_edges_when_directory_participates_in_a_cycle() {
        // api/ and store/ depend on each other — a directory-level cycle
        // (mirrors crate::order's own cycle test fixtures).
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "api/handler.rs".to_string(),
                    symbols: vec![symbol("api/handler.rs::handle", "handle")],
                },
                FileReport {
                    path: "store/db.rs".to_string(),
                    symbols: vec![symbol("store/db.rs::save", "save")],
                },
            ],
            graph: SymbolGraph {
                nodes: vec![
                    node("api/handler.rs::handle", "api/handler.rs", "handle"),
                    node("store/db.rs::save", "store/db.rs", "save"),
                ],
                edges: vec![
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
                roots: vec![],
            },
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_dir_detail(&tree, &report, "api").expect("dir found");

        assert_eq!(vec!["store".to_string()], actual.cycle_partners);
        // Both directed edges touch "api" as an endpoint (api -> store and
        // store -> api), so both are part of the cycle explanation shown
        // for "api" — not just the one where "api" is the source.
        let expected_edges = vec![
            CycleEdgeView {
                from: "api/handler.rs::handle".to_string(),
                to: "store/db.rs::save".to_string(),
            },
            CycleEdgeView {
                from: "store/db.rs::save".to_string(),
                to: "api/handler.rs::handle".to_string(),
            },
        ];
        assert_eq!(expected_edges, actual.cycle_edges);
    }

    #[test]
    fn should_return_none_when_file_path_is_not_found() {
        let report = empty_report();
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "missing.rs");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_build_file_detail_with_symbol_summaries_and_fan_in() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    ExtractedSymbol {
                        classification: Some(Classification::Added),
                        ..symbol("lib.rs::foo", "foo")
                    },
                    symbol("lib.rs::bar", "bar"),
                ],
            }],
            graph: SymbolGraph {
                nodes: vec![
                    node("lib.rs::foo", "lib.rs", "foo"),
                    node("lib.rs::bar", "lib.rs", "bar"),
                ],
                edges: vec![],
                roots: vec![],
            },
            hotspots: vec![Hotspot {
                id: "lib.rs::bar".to_string(),
                path: "lib.rs".to_string(),
                name: "bar".to_string(),
                used_by: vec!["foo".to_string(), "baz".to_string()],
            }],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "lib.rs").expect("file found");

        let expected = FileDetail {
            path: "lib.rs".to_string(),
            symbols: vec![
                FileSymbolSummary {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    classification: Some(Classification::Added),
                    removed: false,
                    fan_in: 0,
                },
                FileSymbolSummary {
                    name: "bar".to_string(),
                    kind: SymbolKind::Function,
                    classification: None,
                    removed: false,
                    fan_in: 2,
                },
            ],
            skip_reason: None,
            test_symbol_count: None,
            size_warning: None,
        };
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_include_removed_symbol_in_file_detail_summary() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            removed: vec![rinkaku_core::extract::RemovedSymbol {
                name: "gone".to_string(),
                kind: SymbolKind::Function,
                path: "lib.rs".to_string(),
                signature: "fn gone()".to_string(),
            }],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "lib.rs").expect("file found");

        let expected = FileDetail {
            path: "lib.rs".to_string(),
            symbols: vec![FileSymbolSummary {
                name: "gone".to_string(),
                kind: SymbolKind::Function,
                classification: None,
                removed: true,
                fan_in: 0,
            }],
            skip_reason: None,
            test_symbol_count: None,
            size_warning: None,
        };
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_carry_skip_reason_into_file_detail_when_file_row_is_skipped() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "assets/logo.png").expect("file found");

        let expected = FileDetail {
            path: "assets/logo.png".to_string(),
            symbols: vec![],
            skip_reason: Some(rinkaku_core::render::SkipReason::Binary),
            test_symbol_count: None,
            size_warning: None,
        };
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_carry_test_symbol_count_into_file_detail_when_file_row_is_a_whole_test_file() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            tests: vec![rinkaku_core::render::TestFileSummary {
                path: "src/lib_test.go".to_string(),
                symbol_count: 4,
            }],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "src/lib_test.go").expect("file found");

        let expected = FileDetail {
            path: "src/lib_test.go".to_string(),
            symbols: vec![],
            skip_reason: None,
            test_symbol_count: Some(4),
            size_warning: None,
        };
        assert_eq!(expected, actual);
    }

    // Regression test (post-rebase integration check): a mixed file — real
    // (non-test) symbols in `report.files` *and* a test-symbol count in
    // `report.tests` for the same path, which `pipeline::partition_test_symbols`
    // legitimately produces for a file with both production and
    // `#[cfg(test)]`-style code changed in one diff — must keep both halves
    // on the built `FileDetail` rather than one silently dropping the other.
    // This is exactly the shape that caused a live panic when running the
    // TUI against this repo's own diff (`rinkaku-tui/src/app.rs` has both
    // real and test symbols changed), before `TreeBuilder::insert_test_file`
    // stopped asserting the file's `symbols` were empty.
    #[test]
    fn should_keep_real_symbols_alongside_test_symbol_count_when_file_is_mixed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "app.rs".to_string(),
                symbols: vec![symbol("app.rs::handle_key", "handle_key")],
            }],
            tests: vec![rinkaku_core::render::TestFileSummary {
                path: "app.rs".to_string(),
                symbol_count: 5,
            }],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "app.rs").expect("file found");

        let expected = FileDetail {
            path: "app.rs".to_string(),
            symbols: vec![FileSymbolSummary {
                name: "handle_key".to_string(),
                kind: SymbolKind::Function,
                classification: None,
                removed: false,
                fan_in: 0,
            }],
            skip_reason: None,
            test_symbol_count: Some(5),
            size_warning: None,
        };
        assert_eq!(expected, actual);
    }

    // ADR 0028: a file whose path shows up in `report.file_size_warnings`
    // must carry the matching warning onto its `FileDetail` so the detail
    // pane can render the "1734 lines — consider splitting" hint above
    // the symbols listing without re-scanning the report itself.
    #[test]
    fn should_populate_size_warning_on_file_detail_when_report_has_warning_for_that_path() {
        let warning = rinkaku_core::file_size::FileSizeWarning {
            path: "src/big.rs".to_string(),
            line_count: 1734,
            severity: rinkaku_core::file_size::FileSizeSeverity::Warn,
        };
        let report = Report {
            files: vec![FileReport {
                path: "src/big.rs".to_string(),
                symbols: vec![symbol("src/big.rs::foo", "foo")],
            }],
            file_size_warnings: vec![warning.clone()],
            ..empty_report()
        };
        let tree = crate::tree::build_tree(&report);

        let actual = build_file_detail(&tree, &report, "src/big.rs").expect("file found");

        let expected = FileDetail {
            path: "src/big.rs".to_string(),
            symbols: vec![FileSymbolSummary {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                classification: None,
                removed: false,
                fan_in: 0,
            }],
            skip_reason: None,
            test_symbol_count: None,
            size_warning: Some(warning),
        };
        assert_eq!(expected, actual);
    }
}
