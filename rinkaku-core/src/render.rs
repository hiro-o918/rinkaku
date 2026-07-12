//! Rendering the extraction pipeline's results into an output format.
//!
//! [`Report`] is the pipeline-wide result shape produced by
//! [`crate::pipeline::analyze_diff`]: per-file extracted symbols plus the
//! files that were skipped (unsupported language, binary, or deleted), plus
//! the [`crate::graph::SymbolGraph`] built over those symbols (ADR 0008).
//! This module turns a `Report` into either Markdown (the default, meant
//! for humans and LLMs) or JSON (`serde`-derived, for machine consumption).
//!
//! Markdown renders as two sections: a "Change graph" tree (names only,
//! rooted at the graph's auto-detected entry points) giving the reader a
//! call-hierarchy reading order, followed by "Definitions" — the full
//! signature of every changed symbol, in the same tree order, each shown
//! exactly once (ADR 0008's decision to avoid duplicating a symbol
//! reachable from multiple roots).
//!
//! Skipped files are always listed, never silently dropped — a reviewer
//! or LLM consuming the output needs to know what rinkaku didn't look at.

use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::graph::{NodeId, SymbolGraph};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use thiserror::Error;

/// The result of running the extraction pipeline over a whole diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub files: Vec<FileReport>,
    pub skipped: Vec<SkippedFile>,
    /// The dependency graph over `files`' symbols (ADR 0008): edges and
    /// entry points used to render "Change graph" in Markdown, exposed here
    /// too so JSON consumers get the same structure without recomputing it.
    pub graph: SymbolGraph,
}

/// Extracted symbols for a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReport {
    pub path: String,
    pub symbols: Vec<ExtractedSymbol>,
}

/// A file the pipeline did not extract symbols from, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: SkipReason,
}

/// Why a changed file was skipped rather than analyzed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// No registered [`crate::language::LanguageSupport`] for this file's
    /// extension.
    UnsupportedLanguage,
    /// Git reported this as a binary file patch.
    Binary,
    /// The file was deleted; there is no new-side content to extract from.
    Deleted,
}

/// Supported output formats for a [`Report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
}

/// Errors that can occur while rendering a [`Report`].
#[derive(Debug, Error)]
pub enum RenderError {
    /// Writing to the in-memory `String` buffer failed. This only happens
    /// on allocation failure, which `std::fmt::Write` reports as `Err(())`
    /// with no further detail; kept as a typed error (rather than
    /// `.unwrap()`) so the fallible write calls in `render_markdown` can
    /// use `?` instead of panicking.
    #[error("failed to write Markdown output")]
    Fmt(#[from] std::fmt::Error),
    #[error("failed to serialize report as JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Renders a [`Report`] in the requested [`OutputFormat`].
pub fn render(report: &Report, format: OutputFormat) -> Result<String, RenderError> {
    match format {
        OutputFormat::Markdown => render_markdown(report),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(report)?),
    }
}

/// A changed symbol paired with the path of the file it lives in, keyed by
/// [`NodeId`] — the lookup table rendering needs to go from a graph node
/// back to the full [`ExtractedSymbol`] (signature, container,
/// dependencies) it represents.
struct SymbolLookup<'a> {
    by_id: HashMap<&'a str, (&'a str, &'a ExtractedSymbol)>,
}

impl<'a> SymbolLookup<'a> {
    fn build(files: &'a [FileReport]) -> Self {
        let mut by_id = HashMap::new();
        for file in files {
            for symbol in &file.symbols {
                by_id.insert(symbol.id.as_str(), (file.path.as_str(), symbol));
            }
        }
        Self { by_id }
    }

    fn get(&self, id: &str) -> Option<(&'a str, &'a ExtractedSymbol)> {
        self.by_id.get(id).copied()
    }
}

/// Renders a [`Report`] as Markdown: a "Change graph" tree of entry points
/// (ADR 0008), a "Definitions" section with each changed symbol's signature
/// in the same tree order, an "Other changed files" section for files that
/// were analyzed but contributed no symbol (e.g. a pure rename — see
/// `pipeline::analyze_diff`'s doc comment), and a list of skipped files.
///
/// Path headings and tree labels (`{prefix} {name} ({path})`) do not escape
/// Markdown special characters (`#`, `[`, `]`, `_`, ...). A path containing
/// them can distort the rendered heading; this is accepted rather than
/// escaped, since file paths with Markdown-significant characters are rare
/// in practice and the fenced code blocks (the part that matters for
/// correctness) are already hardened against content-driven breakage.
fn render_markdown(report: &Report) -> Result<String, RenderError> {
    let files_with_no_symbols: Vec<&str> = report
        .files
        .iter()
        .filter(|file| file.symbols.is_empty())
        .map(|file| file.path.as_str())
        .collect();

    if report.graph.nodes.is_empty()
        && files_with_no_symbols.is_empty()
        && report.skipped.is_empty()
    {
        return Ok(String::new());
    }

    let lookup = SymbolLookup::build(&report.files);
    let children = children_by_node(&report.graph);
    let visit_order = dfs_pre_order(&report.graph, &children);

    let mut out = String::new();

    if !report.graph.nodes.is_empty() {
        writeln!(out, "## Change graph")?;
        writeln!(out)?;
        render_change_graph(&mut out, &report.graph, &children, &lookup)?;
        writeln!(out)?;

        writeln!(out, "## Definitions")?;
        writeln!(out)?;
        for id in &visit_order {
            let Some((path, symbol)) = lookup.get(id) else {
                continue;
            };
            render_definition(&mut out, path, symbol)?;
        }
    }

    if !files_with_no_symbols.is_empty() {
        writeln!(out, "## Other changed files")?;
        writeln!(out)?;
        for path in &files_with_no_symbols {
            writeln!(out, "- {path}")?;
        }
        writeln!(out)?;
    }

    if !report.skipped.is_empty() {
        writeln!(out, "## Skipped files")?;
        writeln!(out)?;
        for skipped in &report.skipped {
            writeln!(
                out,
                "- {} ({})",
                skipped.path,
                skip_reason_label(skipped.reason)
            )?;
        }
    }

    Ok(out)
}

/// Renders the "Change graph" section: an indented, names-only tree rooted
/// at `graph.roots`, in root order. Each root starts its own top-level DFS;
/// a node already printed earlier in the tree is re-shown by name only,
/// suffixed `(see above)`, and not expanded again — this is what keeps a
/// symbol reachable from multiple roots from being duplicated in full (ADR
/// 0008). Cycle edges (`Edge::is_cycle`) are rendered as an explicit
/// warning line instead of being walked into (walking into one would loop
/// forever, since a cycle edge points back to an ancestor already on the
/// current path).
fn render_change_graph(
    out: &mut String,
    graph: &SymbolGraph,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    lookup: &SymbolLookup,
) -> Result<(), RenderError> {
    let mut printed: HashSet<String> = HashSet::new();

    for root in &graph.roots {
        render_tree_node(out, root, children, lookup, &mut printed, 0)?;
    }
    Ok(())
}

/// Groups `graph.edges` by their `from` node, each target annotated with
/// whether reaching it crosses a cycle edge. Preserves `edges`' own order
/// (source appearance order, per `graph::collect_edges`'s doc comment), so
/// a node's children are visited in a deterministic, diff-order-derived
/// sequence.
fn children_by_node(graph: &SymbolGraph) -> HashMap<&str, Vec<(&str, bool)>> {
    let mut children: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for edge in &graph.edges {
        children
            .entry(edge.from.as_str())
            .or_default()
            .push((edge.to.as_str(), edge.is_cycle));
    }
    children
}

/// Writes one tree line for `id` at `depth`, then recurses into its
/// children (unless `id` was already printed earlier in the tree, in which
/// case it is shown as a `(see above)` reference and not expanded).
fn render_tree_node(
    out: &mut String,
    id: &str,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    lookup: &SymbolLookup,
    printed: &mut HashSet<String>,
    depth: usize,
) -> Result<(), RenderError> {
    let indent = "  ".repeat(depth);
    let Some((path, symbol)) = lookup.get(id) else {
        return Ok(());
    };
    let label = tree_label(path, symbol);

    if !printed.insert(id.to_string()) {
        writeln!(out, "{indent}- {label} (see above)")?;
        return Ok(());
    }
    writeln!(out, "{indent}- {label}")?;

    if let Some(kids) = children.get(id) {
        for &(child_id, is_cycle) in kids {
            if is_cycle {
                let Some((child_path, child_symbol)) = lookup.get(child_id) else {
                    continue;
                };
                let child_label = tree_label(child_path, child_symbol);
                writeln!(
                    out,
                    "{}  - ⚠️ {child_label} — dependency cycle, see above",
                    indent
                )?;
                continue;
            }
            render_tree_node(out, child_id, children, lookup, printed, depth + 1)?;
        }
    }
    Ok(())
}

/// Renders one symbol's "Definitions" entry: a `###` heading using the same
/// label as the tree, the fenced signature (container comment included, as
/// in the pre-ADR-0008 rendering), and its unchanged 1-hop `dependencies`
/// under "Depends on:".
fn render_definition(
    out: &mut String,
    path: &str,
    symbol: &ExtractedSymbol,
) -> Result<(), RenderError> {
    writeln!(out, "### {}", tree_label(path, symbol))?;
    writeln!(out)?;

    let container_line = symbol.container.as_deref().map(|c| format!("// {c}"));
    let fence = fence_for(container_line.as_deref(), &symbol.signature);
    writeln!(out, "{fence}")?;
    if let Some(container_line) = &container_line {
        writeln!(out, "{container_line}")?;
    }
    writeln!(out, "{}", symbol.signature)?;
    writeln!(out, "{fence}")?;
    writeln!(out)?;

    if !symbol.dependencies.is_empty() || symbol.omitted_dependency_matches > 0 {
        writeln!(out, "Depends on:")?;
        for dependency in &symbol.dependencies {
            // Inline code spans (not a fenced block): a dependency list
            // entry is one line per dependency, so a fence per entry would
            // be noisy. Path and signature are not hardened against
            // embedded backticks the way the fenced blocks above are — a
            // signature is unlikely to contain a backtick run long enough
            // to break out of a single backtick span, and this is a
            // cosmetic-only failure mode (unlike the fenced blocks, which
            // without widening could make later content render as code).
            writeln!(out, "- `{}`: `{}`", dependency.path, dependency.signature)?;
        }
        if symbol.omitted_dependency_matches > 0 {
            writeln!(
                out,
                "- (+{} more definitions matched by name)",
                symbol.omitted_dependency_matches
            )?;
        }
        writeln!(out)?;
    }

    Ok(())
}

/// Builds a tree/heading label for `symbol`: `{prefix} {name} ({path})`,
/// e.g. `fn handle_pr (src/main.rs)`. The prefix comes from
/// [`symbol_kind_prefix`], fixed per [`SymbolKind`] rather than derived
/// from the signature text, so it stays stable across languages.
///
/// When `symbol.id` was disambiguated by line number (`graph::collect_nodes`
/// appends `@{start_line}` whenever a report contains more than one symbol
/// sharing the same `(path, name)` pair, e.g. two overloaded free
/// functions), the label includes that line number too —
/// `{prefix} {name} ({path}:{start_line})` — so the otherwise-identical
/// entries stay distinguishable in "Change graph"/"Definitions". Detected
/// by comparing `symbol.id` against the plain (non-disambiguated) form
/// rather than parsing the id string, since `symbol.range.start` is the
/// exact same line number `collect_nodes` used to build it.
fn tree_label(path: &str, symbol: &ExtractedSymbol) -> String {
    let plain_id = format!("{path}::{}", symbol.name);
    let location = if symbol.id == plain_id {
        path.to_string()
    } else {
        format!("{path}:{}", symbol.range.start)
    };
    format!(
        "{} {} ({})",
        symbol_kind_prefix(symbol.kind),
        symbol.name,
        location
    )
}

/// Maps a [`SymbolKind`] to the short, lowercase word used as a tree/heading
/// label prefix. Not tied to any one language's keyword set (`SymbolKind`
/// is already language-neutral, see its own doc comment) — chosen to read
/// naturally for the concept each variant represents.
fn symbol_kind_prefix(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::TypeAlias => "type",
    }
}

/// Pre-order DFS over `graph` starting at `roots`, in root order, following
/// non-cycle edges only (a cycle edge is rendered as a warning reference in
/// `render_change_graph`, never walked into) and never revisiting a node.
/// Used by `render_markdown` to decide "Definitions"' order, matching the
/// order symbols first appear in the "Change graph" tree above it. Takes
/// the already-built `children` lookup (built once by `render_markdown` and
/// shared with `render_change_graph`) rather than rebuilding it from
/// `graph.edges` itself.
///
/// Falls back to visiting any node no root reaches (defensive: `roots` is
/// documented to always cover every node via SCC condensation, see
/// `graph::find_roots`, so this branch is not expected to trigger in
/// practice, only guarding against that guarantee being broken later).
fn dfs_pre_order(graph: &SymbolGraph, children: &HashMap<&str, Vec<(&str, bool)>>) -> Vec<NodeId> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut order: Vec<NodeId> = Vec::new();

    for root in &graph.roots {
        visit_from(root, children, &mut visited, &mut order);
    }
    for node in &graph.nodes {
        visit_from(node.id.as_str(), children, &mut visited, &mut order);
    }

    order
}

/// Iterative pre-order DFS from `start`, appending every newly-visited node
/// to `order` in true pre-order (a node is only appended once every node
/// before it in DFS order has already been fully appended — i.e. the same
/// order `render_tree_node`'s recursive walk visits nodes in) and following
/// non-cycle edges only. Shared by `dfs_pre_order` for both its `roots` pass
/// and its defensive fallback pass over every node.
///
/// Explicit work-stack of `(node, remaining children)` rather than a plain
/// node stack: a plain node stack that appends to `order` when a node is
/// *pushed* (rather than when it is fully descended into) produces the
/// wrong order whenever a node has more than one child and the first child
/// itself has children — e.g. `A -> B, A -> C, B -> D` would wrongly order
/// `A, C, B, D` (C, pushed right after A, gets appended before B's subtree
/// is even explored) instead of the correct `A, B, D, C`. Tracking each
/// stack frame's own child cursor mirrors `render_tree_node`'s recursion
/// without recursing (unbounded-depth walks in this module use an explicit
/// stack, see `tarjan_sccs`/`dfs_mark_back_edges` in `graph.rs`).
fn visit_from(
    start: &str,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    visited: &mut HashSet<String>,
    order: &mut Vec<NodeId>,
) {
    if !visited.insert(start.to_string()) {
        return;
    }
    order.push(start.to_string());

    // Each frame is (node, index of the next child to consider).
    let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
    while let Some(frame) = stack.last_mut() {
        let (node, child_i) = (frame.0, frame.1);
        let kids = children.get(node).map(Vec::as_slice).unwrap_or(&[]);
        let Some(&(child, is_cycle)) = kids.get(child_i) else {
            stack.pop();
            continue;
        };
        frame.1 += 1;

        if is_cycle || !visited.insert(child.to_string()) {
            continue;
        }
        order.push(child.to_string());
        stack.push((child, 0));
    }
}

/// Picks a fence long enough that it cannot be closed early by a backtick
/// run inside the fenced content: one backtick longer than the longest run
/// of consecutive backticks in `content`, with a floor of 3 (the minimum
/// valid Markdown fence).
fn fence_for(container_line: Option<&str>, signature: &str) -> String {
    let longest_run = [container_line.unwrap_or(""), signature]
        .iter()
        .flat_map(|text| longest_backtick_run(text))
        .max()
        .unwrap_or(0);
    "`".repeat((longest_run + 1).max(3))
}

/// Length of the longest run of consecutive `` ` `` characters in `text`.
fn longest_backtick_run(text: &str) -> Option<usize> {
    text.split(|c| c != '`')
        .map(str::len)
        .filter(|&len| len > 0)
        .max()
}

fn skip_reason_label(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::UnsupportedLanguage => "unsupported language",
        SkipReason::Binary => "binary",
        SkipReason::Deleted => "deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::SymbolKind;
    use crate::graph::{Edge, Node};
    use pretty_assertions::assert_eq;

    /// Builds an `ExtractedSymbol` for rendering tests, with `id` set (the
    /// graph-building pipeline stage this module assumes already ran) and
    /// every other field defaulted to something inert unless overridden via
    /// struct-update syntax at the call site.
    fn symbol(id: &str, name: &str, kind: SymbolKind, signature: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            signature: signature.to_string(),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }
    }

    fn node(id: &str, path: &str, name: &str) -> Node {
        Node {
            id: id.to_string(),
            path: path.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn should_render_empty_markdown_when_report_has_no_files_and_no_skips() {
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
        };

        let expected = "".to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Regression test: a pure rename (or mode-change-only diff) is reported
    // as a `FileReport` with an empty `symbols` list (see
    // `pipeline::analyze_diff`'s doc comment) rather than a `SkippedFile` —
    // the file *was* looked at, it just had no symbol-level changes. Before
    // this fix, such a file was silently dropped from Markdown output
    // entirely (the empty-output guard fired because `graph.nodes` and
    // `skipped` were both empty, even though `files` was not), which is a
    // regression from the pre-ADR-0008 renderer that always emitted a `##
    // {path}` heading for every entry in `report.files`.
    #[test]
    fn should_list_file_with_no_symbols_under_other_changed_files_when_report_has_no_graph_nodes() {
        let report = Report {
            files: vec![FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
        };

        let expected = "\
## Other changed files

- src/new_name.rs

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_list_file_with_no_symbols_after_definitions_when_report_has_graph_nodes_too() {
        // A diff with one file that has a changed symbol (feeds the
        // "Change graph"/"Definitions" sections) alongside a pure-rename
        // file with no symbols at all — the pure rename must still show up,
        // in its own section after "Definitions".
        let report = Report {
            files: vec![
                FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo()",
                    )],
                },
                FileReport {
                    path: "src/new_name.rs".to_string(),
                    symbols: vec![],
                },
            ],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Other changed files

- src/new_name.rs

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_other_changed_files_before_skipped_files_when_report_has_both() {
        let report = Report {
            files: vec![FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
        };

        let expected = "\
## Other changed files

- src/new_name.rs

## Skipped files

- assets/logo.png (binary)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_change_graph_and_definitions_when_report_has_one_symbol() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo(a: i32) -> i32",
                )],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(a: i32) -> i32
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_include_start_line_in_label_when_node_id_is_disambiguated_by_line() {
        // `graph::collect_nodes` appends `@{start_line}` to a node's id only
        // when its `(path, name)` pair is not unique in the report (e.g.
        // two overloaded free functions sharing a name). Without a visible
        // line number, "Change graph"/"Definitions" would show two
        // identical-looking `fn foo (src/lib.rs)` entries with no way to
        // tell them apart.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    ExtractedSymbol {
                        range: LineRange { start: 1, end: 3 },
                        ..symbol(
                            "src/lib.rs::foo@1",
                            "foo",
                            SymbolKind::Function,
                            "fn foo(a: i32)",
                        )
                    },
                    ExtractedSymbol {
                        range: LineRange { start: 10, end: 12 },
                        ..symbol(
                            "src/lib.rs::foo@10",
                            "foo",
                            SymbolKind::Function,
                            "fn foo(a: i32, b: i32)",
                        )
                    },
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo@1", "src/lib.rs", "foo"),
                    node("src/lib.rs::foo@10", "src/lib.rs", "foo"),
                ],
                edges: vec![],
                roots: vec![
                    "src/lib.rs::foo@1".to_string(),
                    "src/lib.rs::foo@10".to_string(),
                ],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs:1)
- fn foo (src/lib.rs:10)

## Definitions

### fn foo (src/lib.rs:1)

```
fn foo(a: i32)
```

### fn foo (src/lib.rs:10)

```
fn foo(a: i32, b: i32)
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_nest_callee_under_caller_in_change_graph_when_symbol_references_another() {
        let report = Report {
            files: vec![FileReport {
                path: "src/main.rs".to_string(),
                symbols: vec![
                    symbol(
                        "src/main.rs::handle_pr",
                        "handle_pr",
                        SymbolKind::Function,
                        "fn handle_pr(args: PrArgs) -> Result<()>",
                    ),
                    symbol(
                        "src/main.rs::resolve_pr_base_sha",
                        "resolve_pr_base_sha",
                        SymbolKind::Function,
                        "fn resolve_pr_base_sha() -> Result<String>",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/main.rs::handle_pr", "src/main.rs", "handle_pr"),
                    node(
                        "src/main.rs::resolve_pr_base_sha",
                        "src/main.rs",
                        "resolve_pr_base_sha",
                    ),
                ],
                edges: vec![Edge {
                    from: "src/main.rs::handle_pr".to_string(),
                    to: "src/main.rs::resolve_pr_base_sha".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["src/main.rs::handle_pr".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn handle_pr (src/main.rs)
  - fn resolve_pr_base_sha (src/main.rs)

## Definitions

### fn handle_pr (src/main.rs)

```
fn handle_pr(args: PrArgs) -> Result<()>
```

### fn resolve_pr_base_sha (src/main.rs)

```
fn resolve_pr_base_sha() -> Result<String>
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_order_definitions_in_true_pre_order_when_first_child_has_its_own_child() {
        // A -> B, A -> C (B before C in edge order), B -> D. True pre-order
        // DFS visits A, then descends fully into B's subtree (B, D) before
        // moving on to C: A, B, D, C. A naive "append to order when a node
        // is pushed onto the stack" (rather than when it is actually
        // visited/popped) would instead produce A, C, B, D, because C gets
        // pushed onto the stack right after A even though B is visited
        // first — this test pins the correct DFS order down as the full
        // rendered string so both the "Change graph" tree and "Definitions"
        // order are asserted together.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::a", "a", SymbolKind::Function, "fn a()"),
                    symbol("src/lib.rs::b", "b", SymbolKind::Function, "fn b()"),
                    symbol("src/lib.rs::c", "c", SymbolKind::Function, "fn c()"),
                    symbol("src/lib.rs::d", "d", SymbolKind::Function, "fn d()"),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::a", "src/lib.rs", "a"),
                    node("src/lib.rs::b", "src/lib.rs", "b"),
                    node("src/lib.rs::c", "src/lib.rs", "c"),
                    node("src/lib.rs::d", "src/lib.rs", "d"),
                ],
                edges: vec![
                    Edge {
                        from: "src/lib.rs::a".to_string(),
                        to: "src/lib.rs::b".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::a".to_string(),
                        to: "src/lib.rs::c".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::b".to_string(),
                        to: "src/lib.rs::d".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["src/lib.rs::a".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn a (src/lib.rs)
  - fn b (src/lib.rs)
    - fn d (src/lib.rs)
  - fn c (src/lib.rs)

## Definitions

### fn a (src/lib.rs)

```
fn a()
```

### fn b (src/lib.rs)

```
fn b()
```

### fn d (src/lib.rs)

```
fn d()
```

### fn c (src/lib.rs)

```
fn c()
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_see_above_when_symbol_reachable_from_multiple_roots() {
        // Both "foo" and "bar" reference "shared": it must be rendered in
        // full once (under "foo", the first root in source order) and
        // referenced by name only under "bar" (ADR 0008).
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                    symbol("src/lib.rs::bar", "bar", SymbolKind::Function, "fn bar()"),
                    symbol(
                        "src/lib.rs::shared",
                        "shared",
                        SymbolKind::Function,
                        "fn shared()",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo", "src/lib.rs", "foo"),
                    node("src/lib.rs::bar", "src/lib.rs", "bar"),
                    node("src/lib.rs::shared", "src/lib.rs", "shared"),
                ],
                edges: vec![
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
                ],
                roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)
  - fn shared (src/lib.rs)
- fn bar (src/lib.rs)
  - fn shared (src/lib.rs) (see above)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### fn shared (src/lib.rs)

```
fn shared()
```

### fn bar (src/lib.rs)

```
fn bar()
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_cycle_warning_when_edge_is_marked_as_cycle() {
        let report = Report {
            files: vec![FileReport {
                path: "src/git.rs".to_string(),
                symbols: vec![symbol(
                    "src/git.rs::resolve_pr_base_sha",
                    "resolve_pr_base_sha",
                    SymbolKind::Function,
                    "fn resolve_pr_base_sha()",
                )],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node(
                    "src/git.rs::resolve_pr_base_sha",
                    "src/git.rs",
                    "resolve_pr_base_sha",
                )],
                edges: vec![Edge {
                    from: "src/git.rs::resolve_pr_base_sha".to_string(),
                    to: "src/git.rs::resolve_pr_base_sha".to_string(),
                    is_cycle: true,
                }],
                roots: vec!["src/git.rs::resolve_pr_base_sha".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn resolve_pr_base_sha (src/git.rs)
  - ⚠️ fn resolve_pr_base_sha (src/git.rs) — dependency cycle, see above

## Definitions

### fn resolve_pr_base_sha (src/git.rs)

```
fn resolve_pr_base_sha()
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_full_cycle_example_with_two_root_functions_and_a_dependency_cycle() {
        // The scenario from the ADR walkthrough: `handle_pr` calls
        // `resolve_pr_base_sha`, which calls `fetch_base_branch` and also
        // (a design smell the tool should surface) calls back into
        // itself. `Config` is an unrelated, independent root.
        let report = Report {
            files: vec![
                FileReport {
                    path: "src/main.rs".to_string(),
                    symbols: vec![symbol(
                        "src/main.rs::handle_pr",
                        "handle_pr",
                        SymbolKind::Function,
                        "fn handle_pr(args: PrArgs) -> Result<()>",
                    )],
                },
                FileReport {
                    path: "src/git.rs".to_string(),
                    symbols: vec![
                        symbol(
                            "src/git.rs::resolve_pr_base_sha",
                            "resolve_pr_base_sha",
                            SymbolKind::Function,
                            "fn resolve_pr_base_sha() -> Result<String>",
                        ),
                        symbol(
                            "src/git.rs::fetch_base_branch",
                            "fetch_base_branch",
                            SymbolKind::Function,
                            "fn fetch_base_branch() -> Result<()>",
                        ),
                    ],
                },
                FileReport {
                    path: "src/config.rs".to_string(),
                    symbols: vec![symbol(
                        "src/config.rs::Config",
                        "Config",
                        SymbolKind::Struct,
                        "struct Config { path: String }",
                    )],
                },
            ],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/main.rs::handle_pr", "src/main.rs", "handle_pr"),
                    node(
                        "src/git.rs::resolve_pr_base_sha",
                        "src/git.rs",
                        "resolve_pr_base_sha",
                    ),
                    node(
                        "src/git.rs::fetch_base_branch",
                        "src/git.rs",
                        "fetch_base_branch",
                    ),
                    node("src/config.rs::Config", "src/config.rs", "Config"),
                ],
                edges: vec![
                    Edge {
                        from: "src/main.rs::handle_pr".to_string(),
                        to: "src/git.rs::resolve_pr_base_sha".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/git.rs::resolve_pr_base_sha".to_string(),
                        to: "src/git.rs::fetch_base_branch".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/git.rs::resolve_pr_base_sha".to_string(),
                        to: "src/git.rs::resolve_pr_base_sha".to_string(),
                        is_cycle: true,
                    },
                ],
                roots: vec![
                    "src/main.rs::handle_pr".to_string(),
                    "src/config.rs::Config".to_string(),
                ],
            },
        };

        let expected = "\
## Change graph

- fn handle_pr (src/main.rs)
  - fn resolve_pr_base_sha (src/git.rs)
    - fn fetch_base_branch (src/git.rs)
    - ⚠️ fn resolve_pr_base_sha (src/git.rs) — dependency cycle, see above
- struct Config (src/config.rs)

## Definitions

### fn handle_pr (src/main.rs)

```
fn handle_pr(args: PrArgs) -> Result<()>
```

### fn resolve_pr_base_sha (src/git.rs)

```
fn resolve_pr_base_sha() -> Result<String>
```

### fn fetch_base_branch (src/git.rs)

```
fn fetch_base_branch() -> Result<()>
```

### struct Config (src/config.rs)

```
struct Config { path: String }
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_container_comment_when_symbol_has_container() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    container: Some("impl Foo".to_string()),
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(&self) -> i32",
                    )
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
                edges: vec![],
                roots: vec!["src/lib.rs::bar".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn bar (src/lib.rs)

## Definitions

### fn bar (src/lib.rs)

```
// impl Foo
fn bar(&self) -> i32
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_depends_on_list_when_symbol_has_dependencies() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    dependencies: vec![crate::deps::ResolvedSymbol {
                        signature: "struct Point { x: i32, y: i32 }".to_string(),
                        path: "src/point.rs".to_string(),
                    }],
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32, y: i32 }`

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_multiple_depends_on_entries_when_symbol_has_several_dependencies() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    dependencies: vec![
                        crate::deps::ResolvedSymbol {
                            signature: "struct Point { x: i32 }".to_string(),
                            path: "src/a.rs".to_string(),
                        },
                        crate::deps::ResolvedSymbol {
                            signature: "struct Point { y: i32 }".to_string(),
                            path: "src/b.rs".to_string(),
                        },
                    ],
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/a.rs`: `struct Point { x: i32 }`
- `src/b.rs`: `struct Point { y: i32 }`

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_omitted_matches_note_when_dependency_matches_were_capped() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    dependencies: vec![crate::deps::ResolvedSymbol {
                        signature: "struct Point { x: i32 }".to_string(),
                        path: "src/a.rs".to_string(),
                    }],
                    omitted_dependency_matches: 2,
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/a.rs`: `struct Point { x: i32 }`
- (+2 more definitions matched by name)

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Regression test: a signature containing a backtick code fence (e.g. a
    // doc comment example embedded in a macro invocation) used to break out
    // of the surrounding Markdown fence because it was always rendered with
    // exactly 3 backticks. The fence length must be at least one longer
    // than the longest run of backticks appearing in the rendered content.
    #[test]
    fn should_widen_fence_when_signature_contains_a_backtick_run() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::example_macro",
                    "example_macro",
                    SymbolKind::Function,
                    "fn example_macro() { let s = \"```rust\\nfn f() {}\\n```\"; }",
                )],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node(
                    "src/lib.rs::example_macro",
                    "src/lib.rs",
                    "example_macro",
                )],
                edges: vec![],
                roots: vec!["src/lib.rs::example_macro".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn example_macro (src/lib.rs)

## Definitions

### fn example_macro (src/lib.rs)

````
fn example_macro() { let s = \"```rust\\nfn f() {}\\n```\"; }
````

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Regression test: the container comment is part of the fenced block
    // too, so a backtick run inside the container name must also widen the
    // fence.
    #[test]
    fn should_widen_fence_when_container_contains_a_backtick_run() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    container: Some("impl Foo /* ```` */".to_string()),
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(&self) -> i32",
                    )
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
                edges: vec![],
                roots: vec!["src/lib.rs::bar".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn bar (src/lib.rs)

## Definitions

### fn bar (src/lib.rs)

`````
// impl Foo /* ```` */
fn bar(&self) -> i32
`````

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_skipped_files_section_when_report_has_skips() {
        let report = Report {
            files: vec![],
            skipped: vec![
                SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: SkipReason::Binary,
                },
                SkippedFile {
                    path: "src/main.py".to_string(),
                    reason: SkipReason::UnsupportedLanguage,
                },
                SkippedFile {
                    path: "src/old.rs".to_string(),
                    reason: SkipReason::Deleted,
                },
            ],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
        };

        let expected = "\
## Skipped files

- assets/logo.png (binary)
- src/main.py (unsupported language)
- src/old.rs (deleted)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_change_graph_then_skipped_section_when_report_has_both() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo()",
                )],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
## Change graph

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Skipped files

- assets/logo.png (binary)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_json_with_graph_files_and_skipped_when_report_has_all_three() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo()",
                )],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
        };

        let expected = "\
{
  \"files\": [
    {
      \"path\": \"src/lib.rs\",
      \"symbols\": [
        {
          \"id\": \"src/lib.rs::foo\",
          \"name\": \"foo\",
          \"kind\": \"Function\",
          \"signature\": \"fn foo()\",
          \"range\": {
            \"start\": 1,
            \"end\": 1
          },
          \"container\": null,
          \"dependencies\": [],
          \"omitted_matches\": 0
        }
      ]
    }
  ],
  \"skipped\": [
    {
      \"path\": \"assets/logo.png\",
      \"reason\": \"binary\"
    }
  ],
  \"graph\": {
    \"nodes\": [
      {
        \"id\": \"src/lib.rs::foo\",
        \"path\": \"src/lib.rs\",
        \"name\": \"foo\"
      }
    ],
    \"edges\": [],
    \"roots\": [
      \"src/lib.rs::foo\"
    ]
  }
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }
}
