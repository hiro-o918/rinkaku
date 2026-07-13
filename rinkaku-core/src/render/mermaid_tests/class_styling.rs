//! `added`/`changed`/`fan-in`/`referenced` class assignment, the
//! `+`/`~`/`-` diff-marker label prefixes (ADR 0041), fan-in's `(in:N)`
//! label suffix (ADR 0039), and the fan-in-vs-changed/added precedence
//! rule.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_render_subgraph_per_file_with_class_assignments_when_report_has_classified_symbols() {
    // "foo" is Added, "bar" is SignatureChanged, both in src/lib.rs; "baz"
    // (unclassified/body-only) lives in src/other.rs and depends on "foo"
    // — pins subgraph grouping, the `+`/`~` marker prefixes, the edge, and
    // the `added`/`changed`/`referenced` class assignments together in one
    // full-string comparison.
    let report = empty_report(
        SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
                node("src/other.rs::baz", "src/other.rs", "baz"),
            ],
            edges: vec![Edge {
                from: "src/other.rs::baz".to_string(),
                to: "src/lib.rs::foo".to_string(),
                is_cycle: false,
            }],
            roots: vec![
                "src/lib.rs::foo".to_string(),
                "src/lib.rs::bar".to_string(),
                "src/other.rs::baz".to_string(),
            ],
        },
        vec![
            FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        Some(Classification::Added),
                    ),
                    symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        Some(Classification::SignatureChanged),
                    ),
                ],
            },
            FileReport {
                path: "src/other.rs".to_string(),
                symbols: vec![symbol(
                    "src/other.rs::baz",
                    "baz",
                    SymbolKind::Function,
                    Some(Classification::BodyOnly),
                )],
            },
        ],
    );

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"+ foo\"]\n",
        "    n1[\"~ bar\"]\n",
        "  end\n",
        "  subgraph sub1[\"src/other.rs\"]\n",
        "    n2[\"baz\"]\n",
        "  end\n",
        "  n2 --> n0\n",
        "  class n0 added\n",
        "  class n1 changed\n",
        "  class n2 referenced\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_dashed_arrow_when_edge_is_a_cycle() {
    let report = empty_report(
        SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
            ],
            edges: vec![
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
            ],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        vec![],
    );

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"foo\"]\n",
        "    n1[\"bar\"]\n",
        "  end\n",
        "  n0 --> n1\n",
        "  n1 -.-> n0\n",
        "  class n0 referenced\n",
        "  class n1 referenced\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_append_fan_in_count_suffix_to_label_when_node_is_high_fan_in() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::shared",
                "shared",
                SymbolKind::Function,
                None,
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::shared", "src/lib.rs", "shared")],
            edges: vec![],
            roots: vec!["src/lib.rs::shared".to_string()],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        }],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"shared (in:3)\"]\n",
        "  end\n",
        "  class n0 fan-in\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_prefer_fan_in_class_over_changed_class_when_node_is_both() {
    // "shared" is SignatureChanged *and* referenced by two other symbols
    // (fan-in >= 2, so it's also a high-fan-in symbol) — precedence goes
    // to `fan-in` styling per this module's documented choice.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::shared",
                "shared",
                SymbolKind::Function,
                Some(Classification::SignatureChanged),
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::shared", "src/lib.rs", "shared")],
            edges: vec![],
            roots: vec!["src/lib.rs::shared".to_string()],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::shared".to_string(),
            path: "src/lib.rs".to_string(),
            name: "shared".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        }],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"shared (in:2)\"]\n",
        "  end\n",
        "  class n0 fan-in\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_prefer_fan_in_class_over_added_class_when_a_new_symbol_has_high_fan_in() {
    // Fan-in (`compute_fan_ins`) counts referrers among *changed* symbols
    // regardless of the referenced node's own classification — a
    // brand-new ("added") symbol referenced by two or more other changed
    // symbols in the same diff (e.g. a new helper two other new/changed
    // call sites both use) is a perfectly ordinary high-fan-in symbol too,
    // not a case that can't occur. Same precedence as the SignatureChanged
    // sibling test above: `fan-in` wins.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::new_helper",
                "new_helper",
                SymbolKind::Function,
                Some(Classification::Added),
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::new_helper", "src/lib.rs", "new_helper")],
            edges: vec![],
            roots: vec!["src/lib.rs::new_helper".to_string()],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::new_helper".to_string(),
            path: "src/lib.rs".to_string(),
            name: "new_helper".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        }],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"new_helper (in:2)\"]\n",
        "  end\n",
        "  class n0 fan-in\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_removed_and_fan_in_nodes_distinctly_when_both_present_in_one_report() {
    // Regression pin for the ADR 0039 collision this ADR fixes: before
    // this change, `removed` and `fan-in` shared the same red fill, so a
    // report containing both classes rendered two visually-identical node
    // styles. `hot` (SignatureChanged, fan-in 2) must get the `fan-in`
    // class/violet styling and an `(in:2)` label suffix; `gone` (removed)
    // must get the red-dashed `removed` class and no label suffix —
    // distinct classes, distinct `classDef` colors.
    let mut report = empty_report(
        SymbolGraph {
            nodes: vec![node("src/lib.rs::hot", "src/lib.rs", "hot")],
            edges: vec![],
            roots: vec!["src/lib.rs::hot".to_string()],
        },
        vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::hot",
                "hot",
                SymbolKind::Function,
                Some(Classification::SignatureChanged),
            )],
        }],
    );
    report.fan_ins = vec![FanIn {
        id: "src/lib.rs::hot".to_string(),
        path: "src/lib.rs".to_string(),
        name: "hot".to_string(),
        used_by: vec!["a".to_string(), "b".to_string()],
    }];
    report.removed = vec![removed_symbol("gone", "src/lib.rs")];

    let expected = concat!(
        "flowchart LR\n",
        "  subgraph sub0[\"src/lib.rs\"]\n",
        "    n0[\"hot (in:2)\"]\n",
        "    n1[\"- gone\"]\n",
        "  end\n",
        "  class n0 fan-in\n",
        "  class n1 removed\n",
    )
    .to_string()
        + CLASS_DEFS;
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}
