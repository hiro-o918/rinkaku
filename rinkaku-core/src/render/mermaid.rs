//! Mermaid `flowchart` rendering (ADR 0021, amended by ADR 0037, ADR 0038).
//!
//! The `--format mermaid` output path: a human-oriented call/dependency
//! graph aimed at GitHub's native mermaid rendering in PR comments/
//! descriptions, separate from the machine-facing Markdown/JSON paths.
//! Falls back to a file-level aggregation when the symbol-level graph
//! would exceed `MERMAID_NODE_BUDGET`, so the output stays legible instead
//! of degrading into a hairball. `report.removed` (ADR 0014) renders as
//! `removed`-classed nodes in the same graph — see ADR 0037 for why a
//! merged graph rather than a separate before/after diagram. A trailing
//! `Legend` subgraph (ADR 0038) renders one real, styled node per class so
//! the diagram explains its own color/style vocabulary without a separate
//! prose legend.

use crate::extract::Classification;
use crate::graph::Node;
use crate::render::report::Report;
use crate::render::shared::SymbolLookup;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// Node count above which [`render_mermaid`] falls back to a file-level
/// graph (ADR 0021) instead of one node per symbol. Chosen as a size a
/// `flowchart` still renders legibly in a PR comment's viewport; see the
/// ADR's Consequences for the judgment-call caveat. Measured against
/// `graph.nodes.len() + removed.len()` (ADR 0037) so a bulk-deletion diff
/// can't dodge the fallback merely because a deleted symbol has no
/// head-side node.
const MERMAID_NODE_BUDGET: usize = 30;

/// Renders a [`Report`] as a mermaid `flowchart LR` document (ADR 0021): a
/// human-oriented call/dependency graph, opt-in via `--format mermaid`,
/// separate from the machine-facing Markdown/JSON paths (`render_markdown`
/// is untouched by this function).
///
/// Falls back to [`render_mermaid_file_level`] past [`MERMAID_NODE_BUDGET`]
/// — the hairball concern ADR 0013 raised against a symbol-level
/// flowchart, addressed by demoting to file granularity rather than
/// rendering every node anyway.
///
/// Infallible (`Result` in [`crate::render::render`]'s signature is uniform
/// across formats, not because this can fail): unlike `render_markdown`'s
/// `std::fmt::Write` calls, building a `String` via `push_str`/`write!`
/// into an owned buffer that is only ever handed back to the caller cannot
/// error the way a fallible `io::Write` sink could.
pub(super) fn render_mermaid(report: &Report) -> String {
    if report.graph.nodes.len() + report.removed.len() > MERMAID_NODE_BUDGET {
        return render_mermaid_file_level(report);
    }

    let mut out = String::new();
    out.push_str("flowchart LR\n");

    if report.graph.nodes.is_empty() && report.removed.is_empty() {
        out.push_str("%% no symbols\n");
        write_legend_and_class_defs(&mut out);
        return out;
    }

    let lookup = SymbolLookup::build(&report.files);
    // Fan-in count (`used_by.len()`) per node id, used both for class
    // selection and the `(in:N)` label suffix (ADR 0038) — a node present
    // here is by definition a high-fan-in symbol (`compute_fan_ins` only
    // includes nodes with fan-in >= 2, per ADR 0013).
    let fan_in_counts: HashMap<&str, usize> = report
        .fan_ins
        .iter()
        .map(|h| (h.id.as_str(), h.used_by.len()))
        .collect();

    // Sequential, mermaid-safe node ids (`n0`, `n1`, ...), mapped from the
    // original `NodeId` — a `NodeId` like `src/lib.rs::foo@10` contains
    // characters (`/`, `:`, `@`, `.`) mermaid does not accept in a bare
    // node id.
    let mut safe_id_by_node: HashMap<&str, String> = HashMap::new();
    for (i, n) in report.graph.nodes.iter().enumerate() {
        safe_id_by_node.insert(n.id.as_str(), format!("n{i}"));
    }
    // A `RemovedSymbol` has no `NodeId` (ADR 0014: no head-side symbol to
    // derive one from), so it gets its own id space, continuing the `n{i}`
    // sequence past every head-side node.
    let removed_offset = report.graph.nodes.len();
    let safe_id_by_removed: Vec<String> = (0..report.removed.len())
        .map(|i| format!("n{}", removed_offset + i))
        .collect();

    // Group nodes by path, preserving first-seen order (same convention as
    // `change_graph_summary`'s path tie-break) — this is what produces one
    // `subgraph` per file, in source order. Removed symbols join the same
    // grouping below so a removed-only file still gets a subgraph (ADR
    // 0037).
    let mut path_order: Vec<&str> = Vec::new();
    let mut nodes_by_path: HashMap<&str, Vec<&Node>> = HashMap::new();
    for n in &report.graph.nodes {
        let path = n.path.as_str();
        if !nodes_by_path.contains_key(path) {
            path_order.push(path);
        }
        nodes_by_path.entry(path).or_default().push(n);
    }
    let mut removed_by_path: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, removed) in report.removed.iter().enumerate() {
        let path = removed.path.as_str();
        if !nodes_by_path.contains_key(path) && !removed_by_path.contains_key(path) {
            path_order.push(path);
        }
        removed_by_path.entry(path).or_default().push(i);
    }

    for (subgraph_i, path) in path_order.iter().enumerate() {
        writeln!(
            out,
            "  subgraph sub{subgraph_i}[\"{}\"]",
            escape_mermaid_label(path)
        )
        .expect("writing to a String cannot fail");
        if let Some(nodes) = nodes_by_path.get(path) {
            for n in nodes {
                let safe_id = &safe_id_by_node[n.id.as_str()];
                let label = match fan_in_counts.get(n.id.as_str()) {
                    Some(count) => format!("{} (in:{count})", n.name),
                    None => n.name.clone(),
                };
                writeln!(out, "    {safe_id}[\"{}\"]", escape_mermaid_label(&label))
                    .expect("writing to a String cannot fail");
            }
        }
        if let Some(indices) = removed_by_path.get(path) {
            for &i in indices {
                let safe_id = &safe_id_by_removed[i];
                let name = &report.removed[i].name;
                writeln!(out, "    {safe_id}[\"{}\"]", escape_mermaid_label(name))
                    .expect("writing to a String cannot fail");
            }
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
    // `changed`) and a high-fan-in symbol gets `fan-in` styling, checked
    // first — see this function's doc comment / ADR 0021's Decision on
    // precedence. This overlap is real, not just theoretical: fan-in
    // (`fan_in_counts`, from `compute_fan_ins`) counts referrers among
    // *changed* symbols regardless of the target's own classification, so
    // a brand-new (`Added`) symbol referenced by two or more other
    // changed symbols is a perfectly ordinary high-fan-in symbol too —
    // e.g. a new helper function two other new/changed call sites both
    // use in the same diff. `fan-in` wins because fan-in ("how many other
    // changed symbols depend on this") is the more decision-relevant
    // signal for a reviewer skimming the graph than "this particular node
    // is new" — the node's own classification is still visible in the
    // companion Markdown/JSON output's Definitions section either way.
    for n in &report.graph.nodes {
        let safe_id = &safe_id_by_node[n.id.as_str()];
        let class = if fan_in_counts.contains_key(n.id.as_str()) {
            Some("fan-in")
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
    // No precedence conflict with the loop above: a `RemovedSymbol` never
    // has a `graph.nodes` entry, so `removed` is unconditional here.
    for safe_id in &safe_id_by_removed {
        writeln!(out, "  class {safe_id} removed").expect("writing to a String cannot fail");
    }

    write_legend_and_class_defs(&mut out);
    out
}

/// [`render_mermaid`]'s fallback for a graph over [`MERMAID_NODE_BUDGET`]
/// nodes (ADR 0021): one node per file rather than per symbol, edges
/// aggregated between files and deduplicated with a count label, so the
/// output stays legible instead of degrading into a hairball. A leading
/// `%% aggregated to file level` comment marks that this fallback fired.
/// Also folds in `report.removed` so a removed-only file still gets a node
/// (ADR 0037).
fn render_mermaid_file_level(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("flowchart LR\n");
    writeln!(
        out,
        "%% aggregated to file level ({} symbols > budget)",
        report.graph.nodes.len() + report.removed.len()
    )
    .expect("writing to a String cannot fail");

    let path_by_node: HashMap<&str, &str> = report
        .graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.path.as_str()))
        .collect();

    // First-seen path order, matching `render_mermaid`'s subgraph order
    // convention. A removed-only path is appended after every head-side
    // path — a `RemovedSymbol` has no `graph.nodes` entry to interleave it
    // by.
    let mut path_order: Vec<&str> = Vec::new();
    let mut seen_paths: HashSet<&str> = HashSet::new();
    for n in &report.graph.nodes {
        if seen_paths.insert(n.path.as_str()) {
            path_order.push(n.path.as_str());
        }
    }
    for removed in &report.removed {
        if seen_paths.insert(removed.path.as_str()) {
            path_order.push(removed.path.as_str());
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

    // File-level counterpart to `render_mermaid`'s per-node `removed` class
    // (ADR 0037).
    let removed_only_paths: HashSet<&str> = report
        .removed
        .iter()
        .map(|r| r.path.as_str())
        .filter(|path| !changed_paths.contains(path))
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
        let safe_id = &safe_id_by_path[path];
        if changed_paths.contains(path) {
            writeln!(out, "  class {safe_id} changed").expect("writing to a String cannot fail");
        } else if removed_only_paths.contains(path) {
            writeln!(out, "  class {safe_id} removed").expect("writing to a String cannot fail");
        }
    }

    write_legend_and_class_defs(&mut out);
    out
}

/// `classDef` lines shared by [`render_mermaid`] and
/// [`render_mermaid_file_level`]. Colors are chosen with explicit
/// dark-on-light text (rather than relying on mermaid's theme defaults) so
/// they stay legible under both GitHub's light and dark PR-comment themes
/// (ADR 0021). `removed` (ADR 0037, recolored red by ADR 0038) is dashed
/// rather than solid-bordered, echoing the cycle-edge convention (`-.->`)
/// for "no longer normal." `fan-in` (ADR 0038) uses a violet/blue stroke
/// distinct from `removed`'s red, plus its own heavier `stroke-width`, so
/// the two classes cannot be confused for each other at a glance — a node
/// that is both `changed`/`added` and high-fan-in still gets `fan-in`
/// styling (see `render_mermaid`'s class-assignment comment for the
/// precedence rule), and its label additionally carries a `(in:N)` suffix
/// so the signal survives even without color (ADR 0038).
const MERMAID_CLASS_DEFS: &str = concat!(
    "  classDef added fill:#c6f6d5,stroke:#276749,color:#1a202c;\n",
    "  classDef changed fill:#feebc8,stroke:#9c4221,color:#1a202c;\n",
    "  classDef fan-in fill:#e9d8fd,stroke:#553c9a,stroke-width:3px,color:#1a202c;\n",
    "  classDef removed fill:#fed7d7,stroke:#9b2c2c,color:#1a202c,stroke-dasharray: 5 5;\n",
);

/// Fixed-id, real-node `subgraph Legend` block appended by
/// [`write_legend_and_class_defs`] (ADR 0038): one node per class, styled
/// with the same `classDef` a real graph node of that class would use.
/// Always emitted — including the empty-graph and file-level-fallback
/// paths — so a reader never sees the diagram without its own key, and a
/// future `classDef` change re-styles the legend automatically instead of
/// risking drift against a hand-written prose description (the problem ADR
/// 0038 replaces). Node ids are fixed rather than drawn from the `n{i}`
/// sequence since the legend has no corresponding `report.graph.nodes`
/// entry to derive one from.
const MERMAID_LEGEND: &str = concat!(
    "  subgraph Legend\n",
    "    legend_added[\"added\"]\n",
    "    legend_changed[\"API changed\"]\n",
    "    legend_removed[\"removed\"]\n",
    "    legend_fan_in[\"fan-in (in:N)\"]\n",
    "  end\n",
    "  class legend_added added\n",
    "  class legend_changed changed\n",
    "  class legend_removed removed\n",
    "  class legend_fan_in fan-in\n",
);

/// Appends [`MERMAID_LEGEND`] followed by [`MERMAID_CLASS_DEFS`] — every
/// [`render_mermaid`]/[`render_mermaid_file_level`] return path ends with
/// this same pair, in this order, so the legend's `class` assignments
/// above always have a matching `classDef` below them.
fn write_legend_and_class_defs(out: &mut String) {
    out.push_str(MERMAID_LEGEND);
    out.push_str(MERMAID_CLASS_DEFS);
}

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
#[path = "mermaid_tests/mod.rs"]
mod tests;
