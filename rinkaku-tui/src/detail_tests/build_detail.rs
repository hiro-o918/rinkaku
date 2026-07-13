use super::*;
use pretty_assertions::assert_eq;

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
    // fan-in of exactly 1 is below FanIn's >= 2 threshold, so
    // `report.fan_ins` has nothing for "callee" — used_by must still
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
        fan_ins: vec![],
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
fn should_list_every_referrer_as_used_by_when_symbol_has_high_fan_in() {
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
        fan_ins: vec![FanIn {
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
// (`compute_fan_ins`'s own doc comment in rinkaku-core::graph notes
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
            // `compute_fan_ins`'s own doc comment) might one day
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
