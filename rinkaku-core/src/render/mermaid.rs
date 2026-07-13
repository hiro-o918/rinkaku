//! Mermaid `flowchart` rendering (ADR 0021).
//!
//! The `--format mermaid` output path: a human-oriented call/dependency
//! graph aimed at GitHub's native mermaid rendering in PR comments/
//! descriptions, separate from the machine-facing Markdown/JSON paths.
//! Falls back to a file-level aggregation when the symbol-level graph
//! would exceed `MERMAID_NODE_BUDGET`, so the output stays legible instead
//! of degrading into a hairball.

use crate::extract::Classification;
use crate::graph::Node;
use crate::render::report::Report;
use crate::render::shared::SymbolLookup;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// Node count above which [`render_mermaid`] falls back to a file-level
/// graph (ADR 0021) instead of one node per symbol. Chosen as a size a
/// `flowchart` still renders legibly in a PR comment's viewport; see the
/// ADR's Consequences for the judgment-call caveat.
const MERMAID_NODE_BUDGET: usize = 30;

/// Renders a [`Report`] as a mermaid `flowchart LR` document (ADR 0021): a
/// human-oriented call/dependency graph, opt-in via `--format mermaid`,
/// separate from the machine-facing Markdown/JSON paths (`render_markdown`
/// is untouched by this function).
///
/// Falls back to [`render_mermaid_file_level`] when `graph.nodes.len()`
/// exceeds [`MERMAID_NODE_BUDGET`] — the hairball concern ADR 0013 raised
/// against a symbol-level flowchart, addressed by demoting to file
/// granularity rather than rendering every node anyway.
///
/// Infallible (`Result` in [`crate::render::render`]'s signature is uniform
/// across formats, not because this can fail): unlike `render_markdown`'s
/// `std::fmt::Write` calls, building a `String` via `push_str`/`write!`
/// into an owned buffer that is only ever handed back to the caller cannot
/// error the way a fallible `io::Write` sink could.
pub(super) fn render_mermaid(report: &Report) -> String {
    if report.graph.nodes.len() > MERMAID_NODE_BUDGET {
        return render_mermaid_file_level(report);
    }

    let mut out = String::new();
    out.push_str("flowchart LR\n");

    if report.graph.nodes.is_empty() {
        out.push_str("%% no symbols\n");
        return out;
    }

    let lookup = SymbolLookup::build(&report.files);
    let hotspot_ids: HashSet<&str> = report.hotspots.iter().map(|h| h.id.as_str()).collect();

    // Sequential, mermaid-safe node ids (`n0`, `n1`, ...), mapped from the
    // original `NodeId` — a `NodeId` like `src/lib.rs::foo@10` contains
    // characters (`/`, `:`, `@`, `.`) mermaid does not accept in a bare
    // node id.
    let mut safe_id_by_node: HashMap<&str, String> = HashMap::new();
    for (i, n) in report.graph.nodes.iter().enumerate() {
        safe_id_by_node.insert(n.id.as_str(), format!("n{i}"));
    }

    // Group nodes by path, preserving first-seen order (same convention as
    // `change_graph_summary`'s path tie-break) — this is what produces one
    // `subgraph` per file, in source order.
    let mut path_order: Vec<&str> = Vec::new();
    let mut nodes_by_path: HashMap<&str, Vec<&Node>> = HashMap::new();
    for n in &report.graph.nodes {
        let path = n.path.as_str();
        if !nodes_by_path.contains_key(path) {
            path_order.push(path);
        }
        nodes_by_path.entry(path).or_default().push(n);
    }

    for (subgraph_i, path) in path_order.iter().enumerate() {
        writeln!(
            out,
            "  subgraph sub{subgraph_i}[\"{}\"]",
            escape_mermaid_label(path)
        )
        .expect("writing to a String cannot fail");
        for n in &nodes_by_path[path] {
            let safe_id = &safe_id_by_node[n.id.as_str()];
            writeln!(out, "    {safe_id}[\"{}\"]", escape_mermaid_label(&n.name))
                .expect("writing to a String cannot fail");
        }
        out.push_str("  end\n");
    }

    for edge in &report.graph.edges {
        let (Some(from), Some(to)) = (
            safe_id_by_node.get(edge.from.as_str()),
            safe_id_by_node.get(edge.to.as_str()),
        ) else {
            continue;
        };
        let arrow = if edge.is_cycle { "-.->" } else { "-->" };
        writeln!(out, "  {from} {arrow} {to}").expect("writing to a String cannot fail");
    }

    // Class assignment: a node that is both classified (`added` or
    // `changed`) and a hotspot gets `hotspot` styling, checked first —
    // see this function's doc comment / ADR 0021's Decision on
    // precedence. This overlap is real, not just theoretical: fan-in
    // (`hotspot_ids`, from `compute_hotspots`) counts referrers among
    // *changed* symbols regardless of the target's own classification, so
    // a brand-new (`Added`) symbol referenced by two or more other
    // changed symbols is a perfectly ordinary hotspot too — e.g. a new
    // helper function two other new/changed call sites both use in the
    // same diff. `hotspot` wins because fan-in ("how many other changed
    // symbols depend on this") is the more decision-relevant signal for a
    // reviewer skimming the graph than "this particular node is new" —
    // the node's own classification is still visible in the companion
    // Markdown/JSON output's Definitions section either way.
    for n in &report.graph.nodes {
        let safe_id = &safe_id_by_node[n.id.as_str()];
        let class = if hotspot_ids.contains(n.id.as_str()) {
            Some("hotspot")
        } else {
            match lookup.get(&n.id).and_then(|(_, s)| s.classification) {
                Some(Classification::Added) => Some("added"),
                Some(Classification::SignatureChanged) => Some("changed"),
                _ => None,
            }
        };
        if let Some(class) = class {
            writeln!(out, "  class {safe_id} {class}").expect("writing to a String cannot fail");
        }
    }

    out.push_str(MERMAID_CLASS_DEFS);
    out
}

/// [`render_mermaid`]'s fallback for a graph over [`MERMAID_NODE_BUDGET`]
/// nodes (ADR 0021): one node per file rather than per symbol, edges
/// aggregated between files and deduplicated with a count label, so the
/// output stays legible instead of degrading into a hairball. A leading
/// `%% aggregated to file level` comment marks that this fallback fired.
fn render_mermaid_file_level(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("flowchart LR\n");
    writeln!(
        out,
        "%% aggregated to file level ({} symbols > budget)",
        report.graph.nodes.len()
    )
    .expect("writing to a String cannot fail");

    let path_by_node: HashMap<&str, &str> = report
        .graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.path.as_str()))
        .collect();

    // First-seen path order, matching `render_mermaid`'s subgraph order
    // convention.
    let mut path_order: Vec<&str> = Vec::new();
    let mut seen_paths: HashSet<&str> = HashSet::new();
    for n in &report.graph.nodes {
        if seen_paths.insert(n.path.as_str()) {
            path_order.push(n.path.as_str());
        }
    }

    let mut safe_id_by_path: HashMap<&str, String> = HashMap::new();
    for (i, path) in path_order.iter().enumerate() {
        safe_id_by_path.insert(path, format!("n{i}"));
    }

    let changed_paths: HashSet<&str> = report
        .files
        .iter()
        .filter(|f| {
            f.symbols.iter().any(|s| {
                matches!(
                    s.classification,
                    Some(Classification::Added) | Some(Classification::SignatureChanged)
                )
            })
        })
        .map(|f| f.path.as_str())
        .collect();

    for path in &path_order {
        let safe_id = &safe_id_by_path[path];
        writeln!(out, "  {safe_id}[\"{}\"]", escape_mermaid_label(path))
            .expect("writing to a String cannot fail");
    }

    // Aggregate edges by (from_path, to_path), deduped/counted, first-seen
    // order — an intra-file edge (from_path == to_path) is dropped, since a
    // self-loop at file granularity carries no information a reader can act
    // on (unlike a symbol-level cycle edge, which pinpoints a real
    // dependency cycle).
    let mut pair_order: Vec<(&str, &str)> = Vec::new();
    let mut counts: HashMap<(&str, &str), usize> = HashMap::new();
    for edge in &report.graph.edges {
        let (Some(&from_path), Some(&to_path)) = (
            path_by_node.get(edge.from.as_str()),
            path_by_node.get(edge.to.as_str()),
        ) else {
            continue;
        };
        if from_path == to_path {
            continue;
        }
        let key = (from_path, to_path);
        if !counts.contains_key(&key) {
            pair_order.push(key);
        }
        *counts.entry(key).or_insert(0) += 1;
    }

    for (from_path, to_path) in &pair_order {
        let from = &safe_id_by_path[from_path];
        let to = &safe_id_by_path[to_path];
        let count = counts[&(*from_path, *to_path)];
        writeln!(out, "  {from} -- {count} --> {to}").expect("writing to a String cannot fail");
    }

    for path in &path_order {
        if changed_paths.contains(path) {
            let safe_id = &safe_id_by_path[path];
            writeln!(out, "  class {safe_id} changed").expect("writing to a String cannot fail");
        }
    }

    out.push_str(MERMAID_CLASS_DEFS);
    out
}

/// `classDef` lines shared by [`render_mermaid`] and
/// [`render_mermaid_file_level`]. Colors are chosen with explicit
/// dark-on-light text (rather than relying on mermaid's theme defaults) so
/// they stay legible under both GitHub's light and dark PR-comment themes
/// (ADR 0021) — `stroke-width` on `hotspot` gives it a heavier outline on
/// top of its own fill, in addition to `changed`'s (SignatureChanged)
/// styling, since `hotspot` styling takes precedence over `changed` for a
/// node that qualifies as both (see `render_mermaid`'s class-assignment
/// comment).
const MERMAID_CLASS_DEFS: &str = concat!(
    "  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;\n",
    "  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;\n",
    "  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;\n",
);

/// Escapes text embedded in a quoted mermaid node/subgraph label
/// (`id["text"]`): `&` first (so the escape sequences below aren't
/// themselves re-escaped), then `"` and `[`/`]`, any of which would
/// otherwise prematurely close or corrupt the quoted label. Embedded
/// newlines are replaced with a space rather than escaped — a path or
/// symbol name is not expected to legitimately contain one, but a literal
/// `\n` inside a quoted label would break the single-line label syntax
/// (and, worse, could start a new line mermaid tries to parse as its own
/// statement), so this is a defensive normalization rather than a
/// meaning-preserving escape the way the others are.
fn escape_mermaid_label(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::{Classification, ExtractedSymbol, SymbolKind};
    use crate::graph::{Edge, Hotspot, Node, SymbolGraph};
    use crate::render::report::{FileReport, ReportOrigin};
    use crate::render::{OutputFormat, render};
    use pretty_assertions::assert_eq;

    /// Same shape as the sibling `tests::symbol` helper, plus a
    /// `classification` parameter these tests need to exercise
    /// added/changed/hotspot styling.
    fn symbol(
        id: &str,
        name: &str,
        kind: SymbolKind,
        classification: Option<Classification>,
    ) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            signature: format!("{name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification,
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

    fn empty_report(graph: SymbolGraph, files: Vec<FileReport>) -> Report {
        Report {
            origin: ReportOrigin::Diff,
            files,
            skipped: vec![],
            graph,
            tests: vec![],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_render_minimal_valid_document_when_graph_is_empty() {
        let report = empty_report(
            SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            vec![],
        );

        let expected = "\
flowchart LR
%% no symbols
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_subgraph_per_file_with_class_assignments_when_report_has_classified_symbols() {
        // "foo" is Added, "bar" is SignatureChanged, both in src/lib.rs;
        // "baz" (unclassified/body-only) lives in src/other.rs and depends
        // on "foo" — pins subgraph grouping, node labels (name only, no
        // kind prefix), the edge, and the `added`/`changed` class
        // assignments together in one full-string comparison.
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

        let expected = "\
flowchart LR
  subgraph sub0[\"src/lib.rs\"]
    n0[\"foo\"]
    n1[\"bar\"]
  end
  subgraph sub1[\"src/other.rs\"]
    n2[\"baz\"]
  end
  n2 --> n0
  class n0 added
  class n1 changed
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
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

        let expected = "\
flowchart LR
  subgraph sub0[\"src/lib.rs\"]
    n0[\"foo\"]
    n1[\"bar\"]
  end
  n0 --> n1
  n1 -.-> n0
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_escape_label_when_name_contains_quote_and_bracket() {
        let report = empty_report(
            SymbolGraph {
                nodes: vec![node(
                    "src/lib.rs::weird",
                    "src/lib.rs",
                    "weird\"name[with]brackets",
                )],
                edges: vec![],
                roots: vec!["src/lib.rs::weird".to_string()],
            },
            vec![],
        );

        let expected = "\
flowchart LR
  subgraph sub0[\"src/lib.rs\"]
    n0[\"weird&quot;name&#91;with&#93;brackets\"]
  end
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_replace_embedded_newline_with_space_when_path_contains_one() {
        // A path/name is not expected to legitimately contain a newline,
        // but nothing upstream guarantees it can't — an unescaped `\n`
        // inside a quoted mermaid label would break the single-line label
        // syntax, so it is normalized to a space defensively rather than
        // left as-is or escaped like the other special characters.
        let report = empty_report(
            SymbolGraph {
                nodes: vec![node("src/lib.rs::weird", "src/li\nb.rs", "weird")],
                edges: vec![],
                roots: vec!["src/lib.rs::weird".to_string()],
            },
            vec![],
        );

        let expected = "\
flowchart LR
  subgraph sub0[\"src/li b.rs\"]
    n0[\"weird\"]
  end
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_prefer_hotspot_class_over_changed_class_when_node_is_both() {
        // "shared" is SignatureChanged *and* referenced by two other
        // symbols (fan-in >= 2, so it's also a hotspot) — precedence goes
        // to `hotspot` styling per this module's documented choice.
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
            hotspots: vec![Hotspot {
                id: "src/lib.rs::shared".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            }],
            file_size_warnings: vec![],
            removed: vec![],
        };

        let expected = "\
flowchart LR
  subgraph sub0[\"src/lib.rs\"]
    n0[\"shared\"]
  end
  class n0 hotspot
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_prefer_hotspot_class_over_added_class_when_a_new_symbol_is_also_a_hotspot() {
        // Fan-in (`compute_hotspots`) counts referrers among *changed*
        // symbols regardless of the referenced node's own classification —
        // a brand-new ("added") symbol referenced by two or more other
        // changed symbols in the same diff (e.g. a new helper two other
        // new/changed call sites both use) is a perfectly ordinary
        // hotspot too, not a case that can't occur. Same precedence as
        // the SignatureChanged sibling test above: `hotspot` wins.
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
            hotspots: vec![Hotspot {
                id: "src/lib.rs::new_helper".to_string(),
                path: "src/lib.rs".to_string(),
                name: "new_helper".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            }],
            file_size_warnings: vec![],
            removed: vec![],
        };

        let expected = "\
flowchart LR
  subgraph sub0[\"src/lib.rs\"]
    n0[\"new_helper\"]
  end
  class n0 hotspot
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_stay_symbol_level_when_node_count_equals_budget_exactly() {
        // Exactly MERMAID_NODE_BUDGET (30) nodes: the fallback condition
        // is `> budget`, so this boundary case must still render one
        // subgraph/node per symbol, not the file-level aggregation — pins
        // the off-by-one the sibling over-budget test alone can't rule
        // out (that test only proves 31 falls back, not that 30 doesn't).
        let mut nodes = Vec::new();
        let mut symbols = Vec::new();
        for i in 0..30 {
            let id = format!("src/lib.rs::s{i}");
            nodes.push(node(&id, "src/lib.rs", &format!("s{i}")));
            symbols.push(symbol(&id, &format!("s{i}"), SymbolKind::Function, None));
        }
        assert_eq!(30, nodes.len());

        let report = empty_report(
            SymbolGraph {
                nodes,
                edges: vec![],
                roots: vec!["src/lib.rs::s0".to_string()],
            },
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols,
            }],
        );

        let mut expected = String::from("flowchart LR\n  subgraph sub0[\"src/lib.rs\"]\n");
        for i in 0..30 {
            expected.push_str(&format!("    n{i}[\"s{i}\"]\n"));
        }
        expected.push_str("  end\n");
        expected.push_str(MERMAID_CLASS_DEFS);

        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_fall_back_to_file_level_graph_when_node_count_exceeds_budget() {
        // 31 nodes (one over MERMAID_NODE_BUDGET's 30) across two files:
        // 16 in src/a.rs (one classified Added, so a.rs is "changed"), 15
        // in src/b.rs. Two edges cross from a.rs to b.rs (aggregated with
        // count 2); one edge stays within a.rs (dropped: an intra-file
        // edge carries no file-level signal).
        let mut nodes = Vec::new();
        let mut files_a_symbols = Vec::new();
        for i in 0..16 {
            let id = format!("src/a.rs::a{i}");
            nodes.push(node(&id, "src/a.rs", &format!("a{i}")));
            let classification = if i == 0 {
                Some(Classification::Added)
            } else {
                None
            };
            files_a_symbols.push(symbol(
                &id,
                &format!("a{i}"),
                SymbolKind::Function,
                classification,
            ));
        }
        let mut files_b_symbols = Vec::new();
        for i in 0..15 {
            let id = format!("src/b.rs::b{i}");
            nodes.push(node(&id, "src/b.rs", &format!("b{i}")));
            files_b_symbols.push(symbol(&id, &format!("b{i}"), SymbolKind::Function, None));
        }
        assert_eq!(31, nodes.len());

        let edges = vec![
            Edge {
                from: "src/a.rs::a0".to_string(),
                to: "src/b.rs::b0".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/a.rs::a1".to_string(),
                to: "src/b.rs::b1".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "src/a.rs::a0".to_string(),
                to: "src/a.rs::a1".to_string(),
                is_cycle: false,
            },
        ];
        let roots = vec!["src/a.rs::a0".to_string(), "src/b.rs::b0".to_string()];

        let report = empty_report(
            SymbolGraph {
                nodes,
                edges,
                roots,
            },
            vec![
                FileReport {
                    path: "src/a.rs".to_string(),
                    symbols: files_a_symbols,
                },
                FileReport {
                    path: "src/b.rs".to_string(),
                    symbols: files_b_symbols,
                },
            ],
        );

        let expected = "\
flowchart LR
%% aggregated to file level (31 symbols > budget)
  n0[\"src/a.rs\"]
  n1[\"src/b.rs\"]
  n0 -- 2 --> n1
  class n0 changed
  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;
  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;
  classDef hotspot fill:#fed7d7,stroke:#9b2c2c,stroke-width:3px,color:#1a202c;
"
        .to_string();
        let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

        assert_eq!(expected, actual);
    }
}
