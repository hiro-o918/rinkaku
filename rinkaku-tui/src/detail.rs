//! Detail-pane view-model (ADR 0015): given a selected symbol, produces
//! the plain data a detail pane shows — signature (old/new when the
//! contract changed), classification, who depends on it ("used by"), and
//! a pivot to callers/callees for call-graph reading order (ADR 0008's
//! tree, reached on demand rather than as the entry view's spine).
//!
//! [`build_detail`] is a pure function of a symbol id plus [`Report`]: no
//! IO, no `ratatui` types.

use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::render::Report;

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

    let callees: Vec<SymbolMention> = report
        .graph
        .edges
        .iter()
        .filter(|edge| edge.from == id)
        .filter_map(|edge| mentions_by_id(&edge.to))
        .collect();

    let callers: Vec<SymbolMention> = report
        .graph
        .edges
        .iter()
        .filter(|edge| edge.to == id)
        .filter_map(|edge| mentions_by_id(&edge.from))
        .collect();

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
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
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
}
