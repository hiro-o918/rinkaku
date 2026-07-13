// symbol_mentions tests (ADR 0022): direct coverage of the function
// extracted out of `build_detail`'s own callee/caller computation, so
// jump navigation's use of it is pinned independently of the Detail
// pane's own tests above.

use super::*;
use pretty_assertions::assert_eq;

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
