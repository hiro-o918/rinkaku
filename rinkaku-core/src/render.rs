//! Rendering the extraction pipeline's results into an output format.
//!
//! [`Report`] is the pipeline-wide result shape produced by
//! [`crate::pipeline::analyze_diff`]: per-file extracted symbols plus the
//! files that were skipped (unsupported language, binary, or deleted), plus
//! the [`crate::graph::SymbolGraph`] built over those symbols (ADR 0008).
//! This module turns a `Report` into either Markdown (the default, meant
//! for humans and LLMs) or JSON (`serde`-derived, for machine consumption).
//!
//! Markdown renders as four sections, in this order: a "Change graph" tree
//! (names only, rooted at the graph's auto-detected entry points) giving
//! the reader a call-hierarchy reading order; "Definitions" — the full
//! signature of every changed symbol, in the same tree order, each shown
//! exactly once (ADR 0008's decision to avoid duplicating a symbol
//! reachable from multiple roots); "Tests" — a per-file count of changed
//! test symbols excluded from the graph/definitions above by default (ADR
//! 0009); "Other changed files" — files with no changed-symbol-level
//! content (e.g. pure renames); and "Skipped files".
//!
//! Skipped files are listed, never silently dropped, with one exception:
//! `SkipReason::Generated` entries are omitted from Markdown entirely (ADR
//! 0010/0011) — a `.gitattributes` declaration or a linguist-compatible
//! content marker has already told the repository this file is
//! uninteresting to diff-review, so listing it as something rinkaku
//! "didn't look at" would just be noise. Every other skip reason still
//! always appears, since a reviewer or LLM consuming the output needs to
//! know what rinkaku didn't look at. Test symbols are summarized rather
//! than dropped outright for the same reason: a reviewer still wants to
//! know "did this change come with tests?" even though the individual test
//! signatures are noise (ADR 0009).

use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::graph::{Hotspot, Node, NodeId, SymbolGraph};
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
    /// Per-file counts of changed test symbols excluded from `files` by
    /// default (ADR 0009) — empty when `--include-tests` is given, since
    /// test symbols then stay in `files` like any other symbol instead of
    /// being summarized here. Source order (the order files were first
    /// encountered in the diff), same as `files`.
    pub tests: Vec<TestFileSummary>,
    /// Fan-in hotspots (ADR 0013): changed symbols referenced by two or more
    /// other changed symbols, sorted by fan-in descending. Derived from
    /// `graph` via [`crate::graph::compute_hotspots`] and kept as its own
    /// `Report` field (rather than recomputed at render time) so JSON
    /// consumers get it without recomputing the aggregation themselves,
    /// matching how `graph` itself is already exposed alongside `files`.
    pub hotspots: Vec<Hotspot>,
}

/// Extracted symbols for a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReport {
    pub path: String,
    pub symbols: Vec<ExtractedSymbol>,
}

/// How many changed test symbols were excluded from a given file's
/// `FileReport` (ADR 0009). Kept separate from `FileReport` rather than as
/// an extra field on it, since a file that is *entirely* tests (e.g. a Go
/// `*_test.go` file) would otherwise need an empty `FileReport` just to
/// carry this count — `tests` covers that file on its own instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestFileSummary {
    pub path: String,
    pub symbol_count: usize,
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
    /// `.gitattributes` marks this file `-diff` or `linguist-generated`
    /// (ADR 0010).
    Generated,
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

/// Renders a [`Report`] as Markdown, in this order: a "Change graph" tree
/// of entry points (ADR 0008); a "Definitions" section with each changed
/// symbol's signature in the same tree order; a "Tests" section
/// summarizing excluded test symbols per file (ADR 0009); an "Other changed
/// files" section for files that were analyzed but contributed no symbol
/// (e.g. a pure rename — see `pipeline::analyze_diff`'s doc comment); and a
/// list of skipped files.
///
/// `SkipReason::Generated` entries are omitted from "Skipped files"
/// entirely, however the file was detected as generated — either an
/// explicit `.gitattributes` declaration (ADR 0010) or a linguist-
/// compatible content marker found in the file itself (ADR 0011); both
/// produce the same `SkipReason::Generated`, and this function does not
/// distinguish between them. Either way the repository (or the generating
/// tool) has already marked the file uninteresting to diff-review, so
/// listing it as something rinkaku "didn't look at" would just be noise in
/// output meant for a human/LLM skimming a change. These entries remain in
/// `Report.skipped` and therefore in JSON output (`render`'s
/// `OutputFormat::Json` branch serializes `report` as-is, unfiltered) for
/// machine consumers that want the full picture — this function only
/// affects the Markdown rendering.
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
    let visible_skipped: Vec<&SkippedFile> = report
        .skipped
        .iter()
        .filter(|skipped| !matches!(skipped.reason, SkipReason::Generated))
        .collect();

    if report.graph.nodes.is_empty()
        && report.tests.is_empty()
        && files_with_no_symbols.is_empty()
        && visible_skipped.is_empty()
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
        writeln!(out, "{}", change_graph_summary(&report.graph.nodes))?;
        writeln!(out)?;
        render_change_graph(&mut out, &report.graph, &children, &lookup)?;
        writeln!(out)?;

        if !report.hotspots.is_empty() {
            writeln!(out, "## Hotspots")?;
            writeln!(out)?;
            for hotspot in &report.hotspots {
                writeln!(
                    out,
                    "- {} — used by {}: {}",
                    hotspot_label(hotspot, &lookup),
                    hotspot.used_by.len(),
                    hotspot.used_by.join(", ")
                )?;
            }
            writeln!(out)?;
        }

        writeln!(out, "## Definitions")?;
        writeln!(out)?;
        for id in &visit_order {
            let Some((path, symbol)) = lookup.get(id) else {
                continue;
            };
            render_definition(&mut out, path, symbol)?;
        }
    }

    if !report.tests.is_empty() {
        writeln!(out, "## Tests")?;
        writeln!(out)?;
        for test_file in &report.tests {
            let noun = if test_file.symbol_count == 1 {
                "symbol"
            } else {
                "symbols"
            };
            writeln!(
                out,
                "- {}: {} changed test {noun}",
                test_file.path, test_file.symbol_count
            )?;
        }
        writeln!(out)?;
    }

    if !files_with_no_symbols.is_empty() {
        writeln!(out, "## Other changed files")?;
        writeln!(out)?;
        for path in &files_with_no_symbols {
            writeln!(out, "- {path}")?;
        }
        writeln!(out)?;
    }

    if !visible_skipped.is_empty() {
        writeln!(out, "## Skipped files")?;
        writeln!(out)?;
        for skipped in &visible_skipped {
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

/// Builds the one-line summary shown under the "## Change graph" heading,
/// e.g. `16 changed symbols in 3 files — most in store/items.go (11)`
/// (ADR 0012 decision 3). Computed from `nodes` alone (not `edges`/`roots`),
/// so it stays meaningful even if graph-building changes independently.
///
/// The `— most in ...` suffix is dropped when every node lives in the same
/// file: naming "the file with the most nodes" is redundant when there is
/// only one file to begin with. Ties for "most" go to whichever path's node
/// appears first in `nodes` (stable, diff-derived order), matching the
/// tie-break `render_markdown` already relies on elsewhere (e.g. root
/// order) rather than an arbitrary path-string sort.
///
/// Callers must not call this with an empty `nodes` — `render_markdown`
/// only emits the "Change graph" section (and this summary) when
/// `graph.nodes` is non-empty, matching pre-ADR-0012 behavior for an empty
/// graph.
fn change_graph_summary(nodes: &[Node]) -> String {
    let total = nodes.len();
    let symbol_noun = if total == 1 { "symbol" } else { "symbols" };

    // First-seen order for paths, with per-path counts, so both the file
    // count and the "most changed" tie-break can be read off in one pass.
    let mut path_order: Vec<&str> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for node in nodes {
        let path = node.path.as_str();
        if !counts.contains_key(path) {
            path_order.push(path);
        }
        *counts.entry(path).or_insert(0) += 1;
    }

    let file_count = path_order.len();
    let file_noun = if file_count == 1 { "file" } else { "files" };

    if file_count <= 1 {
        return format!("{total} changed {symbol_noun} in {file_count} {file_noun}");
    }

    // `max_by_key` keeps the *last* maximal element on ties, but the
    // tie-break we want is "first in `nodes` order" — negate the position
    // so an earlier path outranks a later one with the same count.
    let (hotspot_path, hotspot_count) = path_order
        .iter()
        .enumerate()
        .map(|(i, &path)| (path, counts[path], i))
        .max_by_key(|&(_, count, i)| (count, std::cmp::Reverse(i)))
        .map(|(path, count, _)| (path, count))
        .expect("file_count > 1 implies path_order is non-empty");

    format!(
        "{total} changed {symbol_noun} in {file_count} {file_noun} — most in {hotspot_path} ({hotspot_count})"
    )
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
///
/// A child that is both a non-function symbol and childless in the graph
/// (see [`is_foldable`]) is not rendered as its own nested line — its name
/// is folded into its parent's line instead, as an inline `— uses: ...`
/// annotation (ADR 0012 decision 1). `roots` is passed alongside `children`
/// so `render_tree_node` can exempt root nodes from folding even when they
/// would otherwise qualify: a root is always its own top-level DFS, never
/// "just a dependency" of something else.
fn render_change_graph(
    out: &mut String,
    graph: &SymbolGraph,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    lookup: &SymbolLookup,
) -> Result<(), RenderError> {
    let mut printed: HashSet<String> = HashSet::new();
    let roots: HashSet<&str> = graph.roots.iter().map(String::as_str).collect();

    for root in &graph.roots {
        render_tree_node(out, root, children, lookup, &roots, &mut printed, 0)?;
    }
    Ok(())
}

/// A node is foldable — eligible to be inlined into its parent's line
/// rather than rendered as its own nested line (ADR 0012 decision 1) —
/// when both hold:
/// - its symbol's kind is not [`SymbolKind::Function`] (i.e. it is a data
///   shape: struct/enum/trait/interface/class/type-alias), and
/// - it has no outgoing edges at all in the graph, including cycle edges —
///   a node whose only children are cycle edges is *not* foldable, so the
///   cycle warning stays visible rather than being silently swallowed.
///
/// Root nodes are exempt from folding regardless of this check — see
/// `render_change_graph`'s doc comment — so this function only answers "is
/// this node structurally childless and non-function", leaving the root
/// exemption to the caller.
fn is_foldable(
    id: &str,
    lookup: &SymbolLookup,
    children: &HashMap<&str, Vec<(&str, bool)>>,
) -> bool {
    let Some((_, symbol)) = lookup.get(id) else {
        return false;
    };
    symbol.kind != SymbolKind::Function && !children.contains_key(id)
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
/// non-foldable children (unless `id` was already printed earlier in the
/// tree, in which case it is shown as a `(see above)` reference and not
/// expanded).
///
/// Foldable children (see [`is_foldable`]; roots are always exempt, per
/// `render_change_graph`'s doc comment) are not visited recursively at
/// all — they never enter `printed` and never get a `(see above)` line
/// anywhere. Instead their names are collected and appended to *this*
/// line as `— uses: A, B`, in the same order they appear in the edge
/// list. Because folding is a per-call decision (not a global one), the
/// same folded name legitimately repeats verbatim on every parent that
/// references it — that repetition is intentional, not a duplicate-
/// rendering bug.
fn render_tree_node(
    out: &mut String,
    id: &str,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    lookup: &SymbolLookup,
    roots: &HashSet<&str>,
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

    let kids = children.get(id).map(Vec::as_slice).unwrap_or(&[]);
    let folded_names: Vec<String> = kids
        .iter()
        .filter(|&&(child_id, is_cycle)| {
            !is_cycle && !roots.contains(child_id) && is_foldable(child_id, lookup, children)
        })
        .filter_map(|&(child_id, _)| {
            lookup
                .get(child_id)
                .map(|(child_path, sym)| folded_name(child_path, sym))
        })
        .collect();

    if folded_names.is_empty() {
        writeln!(out, "{indent}- {label}")?;
    } else {
        writeln!(out, "{indent}- {label} — uses: {}", folded_names.join(", "))?;
    }

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
        if !roots.contains(child_id) && is_foldable(child_id, lookup, children) {
            continue;
        }
        render_tree_node(out, child_id, children, lookup, roots, printed, depth + 1)?;
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
/// When `symbol.id` was disambiguated by line number (see
/// [`symbol_location`]), the label includes that line number too —
/// `{prefix} {name} ({path}:{start_line})` — so the otherwise-identical
/// entries stay distinguishable in "Change graph"/"Definitions".
fn tree_label(path: &str, symbol: &ExtractedSymbol) -> String {
    format!(
        "{} {} ({})",
        symbol_kind_prefix(symbol.kind),
        symbol.name,
        symbol_location(path, symbol)
    )
}

/// Builds the "Hotspots" line label for a [`Hotspot`], reusing
/// [`tree_label`] via `lookup` so a hotspot's label is identical to how the
/// same symbol is labeled in "Change graph"/"Definitions" (ADR 0013's
/// requirement that labels stay consistent across sections) — including
/// the `:{start_line}` disambiguation suffix when applicable.
///
/// Falls back to a bare `{name} ({path})` (no kind prefix) when `lookup` has
/// no matching `ExtractedSymbol` for `hotspot.id` — defensive, since
/// `pipeline::analyze_diff` always builds `hotspots` from the same `graph`
/// whose node ids match `files`' stamped symbol ids (same invariant
/// `render_tree_node`'s own lookup-miss guards rely on), so this branch is
/// not expected to trigger in practice.
fn hotspot_label(hotspot: &Hotspot, lookup: &SymbolLookup) -> String {
    match lookup.get(&hotspot.id) {
        Some((path, symbol)) => tree_label(path, symbol),
        None => format!("{} ({})", hotspot.name, hotspot.path),
    }
}

/// The `(path)` or `(path:start_line)` portion shared by [`tree_label`] and
/// folded `— uses: ...` annotations (see `render_tree_node`).
///
/// `graph::collect_nodes` appends `@{start_line}` to `symbol.id` whenever a
/// report contains more than one symbol sharing the same `(path, name)`
/// pair (e.g. two overloaded free functions, or two structs named the same
/// in different scopes of one file) — comparing `symbol.id` against the
/// plain (non-disambiguated) form detects that case without parsing the id
/// string, since `symbol.range.start` is the exact same line number
/// `collect_nodes` used to build it. Plain (non-disambiguated) symbols get
/// just `path`.
fn symbol_location(path: &str, symbol: &ExtractedSymbol) -> String {
    let plain_id = format!("{path}::{}", symbol.name);
    if symbol.id == plain_id {
        path.to_string()
    } else {
        format!("{path}:{}", symbol.range.start)
    }
}

/// The name shown for a folded child in a `— uses: ...` annotation (see
/// `render_tree_node`): bare `symbol.name` normally, or `{name}
/// ({path}:{start_line})` — the same disambiguation [`tree_label`] applies
/// to tree lines and "Definitions" headers — when `collect_nodes` had to
/// disambiguate this symbol's id. Without this, two distinct same-named
/// symbols folded under the same parent would both render as the bare
/// name (`— uses: Dup, Dup`), indistinguishable from each other even
/// though "Definitions" shows two different headers for them.
fn folded_name(path: &str, symbol: &ExtractedSymbol) -> String {
    let plain_id = format!("{path}::{}", symbol.name);
    if symbol.id == plain_id {
        symbol.name.clone()
    } else {
        format!("{} ({})", symbol.name, symbol_location(path, symbol))
    }
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
        SkipReason::Generated => "generated",
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
            is_test: false,
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
            tests: vec![],
            hotspots: vec![],
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
            tests: vec![],
            hotspots: vec![],
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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
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
    fn should_render_tests_section_with_singular_symbol_noun_when_count_is_one() {
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![crate::render::TestFileSummary {
                path: "src/lib.rs".to_string(),
                symbol_count: 1,
            }],
            hotspots: vec![],
        };

        let expected = "\
## Tests

- src/lib.rs: 1 changed test symbol

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_tests_section_with_plural_symbols_noun_when_count_is_greater_than_one() {
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![crate::render::TestFileSummary {
                path: "src/lib.rs".to_string(),
                symbol_count: 3,
            }],
            hotspots: vec![],
        };

        let expected = "\
## Tests

- src/lib.rs: 3 changed test symbols

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_tests_section_between_definitions_and_other_changed_files_when_report_has_all_sections()
     {
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
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![crate::render::TestFileSummary {
                path: "src/lib.rs".to_string(),
                symbol_count: 2,
            }],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Tests

- src/lib.rs: 2 changed test symbols

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Regression test: a `Generated` skip entry must not appear in Markdown
    // output at all (not even under "Skipped files") — `.gitattributes`
    // already marks these files as uninteresting to diff-review, so
    // Markdown output (meant for humans/LLMs skimming a change) drops them
    // silently rather than listing them as something rinkaku "didn't look
    // at". They stay visible in JSON (see
    // `should_keep_generated_entry_in_json_output` below) for machine
    // consumers that want the full picture.
    #[test]
    fn should_omit_generated_skip_entry_from_markdown_output() {
        let report = Report {
            files: vec![],
            skipped: vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "".to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Sibling case: when a `Generated` entry is mixed with other skip
    // reasons, only the generated one is dropped from Markdown — the
    // section itself still renders for the remaining, non-generated skips.
    #[test]
    fn should_omit_only_generated_entries_when_skipped_has_other_reasons_too() {
        let report = Report {
            files: vec![],
            skipped: vec![
                SkippedFile {
                    path: "Cargo.lock".to_string(),
                    reason: SkipReason::Generated,
                },
                SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: SkipReason::Binary,
                },
            ],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Skipped files

- assets/logo.png (binary)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // JSON is machine-readable output, not the human-skimmable Markdown
    // rendering — `Generated` entries must stay in `skipped` there for
    // full-fidelity consumers and so `garbage_input_note`'s "did we
    // recognize anything at all" check keeps working for an all-generated
    // diff (see `garbage_input_note_tests` in `rinkaku/src/main.rs`, which
    // reads `report.skipped` directly, not the rendered Markdown).
    #[test]
    fn should_keep_generated_entry_in_json_output() {
        let report = Report {
            files: vec![],
            skipped: vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
{
  \"files\": [],
  \"skipped\": [
    {
      \"path\": \"Cargo.lock\",
      \"reason\": \"generated\"
    }
  ],
  \"graph\": {
    \"nodes\": [],
    \"edges\": [],
    \"roots\": []
  },
  \"tests\": [],
  \"hotspots\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
    fn should_render_summary_with_hotspot_when_report_has_multiple_symbols_and_files() {
        // 5 nodes across store/items.go (3, the hotspot) and store/db.go (2)
        // — pins the plural "changed symbols"/"files" wording together with
        // the "— most in ..." suffix and its count.
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("store/items.go::A", "store/items.go", "A"),
                    node("store/items.go::B", "store/items.go", "B"),
                    node("store/items.go::C", "store/items.go", "C"),
                    node("store/db.go::D", "store/db.go", "D"),
                    node("store/db.go::E", "store/db.go", "E"),
                ],
                edges: vec![],
                roots: vec![
                    "store/items.go::A".to_string(),
                    "store/items.go::B".to_string(),
                    "store/items.go::C".to_string(),
                    "store/db.go::D".to_string(),
                    "store/db.go::E".to_string(),
                ],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

5 changed symbols in 2 files — most in store/items.go (3)


## Definitions

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_omit_hotspot_suffix_when_all_symbols_are_in_one_file() {
        // Every node lives in the same file, so naming "the file with the
        // most nodes" would be redundant — the suffix must be dropped
        // entirely, not degenerate into e.g. "(2)".
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::a", "src/lib.rs", "a"),
                    node("src/lib.rs::b", "src/lib.rs", "b"),
                ],
                edges: vec![],
                roots: vec!["src/lib.rs::a".to_string(), "src/lib.rs::b".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

2 changed symbols in 1 file


## Definitions

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_break_hotspot_tie_by_first_seen_path_order_when_counts_are_equal() {
        // b.rs and a.rs both have 2 nodes each; b.rs's node appears first in
        // `graph.nodes`, so it must win the tie over a.rs despite sorting
        // after it alphabetically — the tie-break is source order, not a
        // path-string comparison.
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("b.rs::x", "b.rs", "x"),
                    node("a.rs::y", "a.rs", "y"),
                    node("b.rs::z", "b.rs", "z"),
                    node("a.rs::w", "a.rs", "w"),
                ],
                edges: vec![],
                roots: vec![
                    "b.rs::x".to_string(),
                    "a.rs::y".to_string(),
                    "b.rs::z".to_string(),
                    "a.rs::w".to_string(),
                ],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

4 changed symbols in 2 files — most in b.rs (2)


## Definitions

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

2 changed symbols in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

2 changed symbols in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

4 changed symbols in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

4 changed symbols in 3 files — most in src/git.rs (2)

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
    fn should_inline_two_leaf_struct_children_as_uses_annotation_on_method_line() {
        // A method referencing two childless, non-function structs (the
        // request/response shape the ADR calls out): both fold into the
        // parent's own line as `— uses: ...` instead of rendering as their
        // own nested lines, but both still get full "### ..." entries
        // under "Definitions" (ADR 0012 decision 1).
        let report = Report {
            files: vec![FileReport {
                path: "store/items.go".to_string(),
                symbols: vec![
                    symbol(
                        "store/items.go::UpsertItems",
                        "UpsertItems",
                        SymbolKind::Function,
                        "func UpsertItems(req UpsertItemsRequest) (UpsertItemsResponse, error)",
                    ),
                    symbol(
                        "store/items.go::UpsertItemsRequest",
                        "UpsertItemsRequest",
                        SymbolKind::Struct,
                        "type UpsertItemsRequest struct { Items []Item }",
                    ),
                    symbol(
                        "store/items.go::UpsertItemsResponse",
                        "UpsertItemsResponse",
                        SymbolKind::Struct,
                        "type UpsertItemsResponse struct { Count int }",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node(
                        "store/items.go::UpsertItems",
                        "store/items.go",
                        "UpsertItems",
                    ),
                    node(
                        "store/items.go::UpsertItemsRequest",
                        "store/items.go",
                        "UpsertItemsRequest",
                    ),
                    node(
                        "store/items.go::UpsertItemsResponse",
                        "store/items.go",
                        "UpsertItemsResponse",
                    ),
                ],
                edges: vec![
                    Edge {
                        from: "store/items.go::UpsertItems".to_string(),
                        to: "store/items.go::UpsertItemsRequest".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "store/items.go::UpsertItems".to_string(),
                        to: "store/items.go::UpsertItemsResponse".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["store/items.go::UpsertItems".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

- fn UpsertItems (store/items.go) — uses: UpsertItemsRequest, UpsertItemsResponse

## Definitions

### fn UpsertItems (store/items.go)

```
func UpsertItems(req UpsertItemsRequest) (UpsertItemsResponse, error)
```

### struct UpsertItemsRequest (store/items.go)

```
type UpsertItemsRequest struct { Items []Item }
```

### struct UpsertItemsResponse (store/items.go)

```
type UpsertItemsResponse struct { Count int }
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_disambiguate_folded_names_when_duplicate_symbols_fold_under_same_parent() {
        // Two distinct `Dup` structs in the same file (mirroring an
        // overloaded/shadowed-name scenario `graph::collect_nodes`
        // disambiguates by appending `@{start_line}` to the node id) both
        // fold under `foo`. Bare `Dup, Dup` would be ambiguous — Definitions
        // shows two distinct `### struct Dup (src/lib.rs:5)` /
        // `(src/lib.rs:10)` headers, so the folded annotation must use the
        // same `Name (path:line)` form `tree_label` already uses for
        // disambiguated symbols, not the bare name.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                    ExtractedSymbol {
                        range: LineRange { start: 5, end: 6 },
                        ..symbol(
                            "src/lib.rs::Dup@5",
                            "Dup",
                            SymbolKind::Struct,
                            "struct Dup { a: i32 }",
                        )
                    },
                    ExtractedSymbol {
                        range: LineRange { start: 10, end: 11 },
                        ..symbol(
                            "src/lib.rs::Dup@10",
                            "Dup",
                            SymbolKind::Struct,
                            "struct Dup { b: i32 }",
                        )
                    },
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo", "src/lib.rs", "foo"),
                    node("src/lib.rs::Dup@5", "src/lib.rs", "Dup"),
                    node("src/lib.rs::Dup@10", "src/lib.rs", "Dup"),
                ],
                edges: vec![
                    Edge {
                        from: "src/lib.rs::foo".to_string(),
                        to: "src/lib.rs::Dup@5".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::foo".to_string(),
                        to: "src/lib.rs::Dup@10".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs) — uses: Dup (src/lib.rs:5), Dup (src/lib.rs:10)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Dup (src/lib.rs:5)

```
struct Dup { a: i32 }
```

### struct Dup (src/lib.rs:10)

```
struct Dup { b: i32 }
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_repeat_folded_struct_annotation_on_every_referencing_parent() {
        // Both `foo` and `bar` reference the same childless struct `Shared`
        // — unlike function children (which get a single full render plus
        // `(see above)` elsewhere, ADR 0008), a folded name has no
        // "see above" tracking: it legitimately repeats verbatim in the
        // `— uses: ...` annotation on every parent that references it, and
        // it must never itself get a `(see above)` line.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                    symbol("src/lib.rs::bar", "bar", SymbolKind::Function, "fn bar()"),
                    symbol(
                        "src/lib.rs::Shared",
                        "Shared",
                        SymbolKind::Struct,
                        "struct Shared { x: i32 }",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo", "src/lib.rs", "foo"),
                    node("src/lib.rs::bar", "src/lib.rs", "bar"),
                    node("src/lib.rs::Shared", "src/lib.rs", "Shared"),
                ],
                edges: vec![
                    Edge {
                        from: "src/lib.rs::foo".to_string(),
                        to: "src/lib.rs::Shared".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::bar".to_string(),
                        to: "src/lib.rs::Shared".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs) — uses: Shared
- fn bar (src/lib.rs) — uses: Shared

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Shared (src/lib.rs)

```
struct Shared { x: i32 }
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
    fn should_render_childless_non_function_root_as_top_level_line_when_it_would_otherwise_be_foldable()
     {
        // `Config` is a childless struct — foldable by the structural
        // criterion — but it is also a root, so it must still render as
        // its own top-level tree line rather than being folded away
        // entirely (roots are always their own top-level DFS start).
        let report = Report {
            files: vec![FileReport {
                path: "src/config.rs".to_string(),
                symbols: vec![symbol(
                    "src/config.rs::Config",
                    "Config",
                    SymbolKind::Struct,
                    "struct Config { path: String }",
                )],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/config.rs::Config", "src/config.rs", "Config")],
                edges: vec![],
                roots: vec!["src/config.rs::Config".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

- struct Config (src/config.rs)

## Definitions

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
    fn should_render_nested_line_when_non_function_child_has_its_own_children() {
        // `Wrapper` is a non-function child of `foo`, but it is not
        // foldable because it has an outgoing edge of its own (to `Inner`)
        // — the structural criterion is "childless", not "non-function",
        // so `Wrapper` itself still renders as a nested line exactly as
        // before this feature. `Inner`, in turn, *is* childless and
        // non-function, so it folds into `Wrapper`'s own line instead of
        // getting a third nesting level.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                    symbol(
                        "src/lib.rs::Wrapper",
                        "Wrapper",
                        SymbolKind::Struct,
                        "struct Wrapper { inner: Inner }",
                    ),
                    symbol(
                        "src/lib.rs::Inner",
                        "Inner",
                        SymbolKind::Struct,
                        "struct Inner { x: i32 }",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo", "src/lib.rs", "foo"),
                    node("src/lib.rs::Wrapper", "src/lib.rs", "Wrapper"),
                    node("src/lib.rs::Inner", "src/lib.rs", "Inner"),
                ],
                edges: vec![
                    Edge {
                        from: "src/lib.rs::foo".to_string(),
                        to: "src/lib.rs::Wrapper".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::Wrapper".to_string(),
                        to: "src/lib.rs::Inner".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs)
  - struct Wrapper (src/lib.rs) — uses: Inner

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Wrapper (src/lib.rs)

```
struct Wrapper { inner: Inner }
```

### struct Inner (src/lib.rs)

```
struct Inner { x: i32 }
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_fold_non_function_child_when_its_only_children_are_cycle_edges() {
        // `Node` is a non-function type whose only outgoing edge is a
        // cycle edge back to itself — `children_by_node` still records an
        // entry for it, so it is *not* foldable (folding requires no
        // outgoing edges at all) and must render as its own nested line
        // with the cycle warning still visible beneath it.
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                    symbol(
                        "src/lib.rs::Node",
                        "Node",
                        SymbolKind::Struct,
                        "struct Node { next: Option<Box<Node>> }",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("src/lib.rs::foo", "src/lib.rs", "foo"),
                    node("src/lib.rs::Node", "src/lib.rs", "Node"),
                ],
                edges: vec![
                    Edge {
                        from: "src/lib.rs::foo".to_string(),
                        to: "src/lib.rs::Node".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "src/lib.rs::Node".to_string(),
                        to: "src/lib.rs::Node".to_string(),
                        is_cycle: true,
                    },
                ],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

2 changed symbols in 1 file

- fn foo (src/lib.rs)
  - struct Node (src/lib.rs)
    - ⚠️ struct Node (src/lib.rs) — dependency cycle, see above

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Node (src/lib.rs)

```
struct Node { next: Option<Box<Node>> }
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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
            tests: vec![],
            hotspots: vec![],
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
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

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
    fn should_omit_hotspots_section_when_hotspots_is_empty() {
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
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_hotspots_section_between_change_graph_and_definitions_when_hotspots_is_non_empty()
     {
        // `UpsertItemsRequest` (a struct) is referenced by two changed
        // functions — the label reuses tree_label's `{kind} {name}
        // ({path})` form, so the line reads
        // "struct UpsertItemsRequest (store/items.go) — used by 2: ..."
        // exactly as the ADR spec requires, and used_by names are joined
        // in the order `compute_hotspots` already sorted them in (not
        // re-sorted here).
        let report = Report {
            files: vec![FileReport {
                path: "store/items.go".to_string(),
                symbols: vec![
                    symbol(
                        "store/items.go::HandleFoo",
                        "HandleFoo",
                        SymbolKind::Function,
                        "func HandleFoo(req UpsertItemsRequest) error",
                    ),
                    symbol(
                        "store/items.go::HandleBar",
                        "HandleBar",
                        SymbolKind::Function,
                        "func HandleBar(req UpsertItemsRequest) error",
                    ),
                    symbol(
                        "store/items.go::UpsertItemsRequest",
                        "UpsertItemsRequest",
                        SymbolKind::Struct,
                        "type UpsertItemsRequest struct { Items []Item }",
                    ),
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![
                    node("store/items.go::HandleFoo", "store/items.go", "HandleFoo"),
                    node("store/items.go::HandleBar", "store/items.go", "HandleBar"),
                    node(
                        "store/items.go::UpsertItemsRequest",
                        "store/items.go",
                        "UpsertItemsRequest",
                    ),
                ],
                edges: vec![
                    Edge {
                        from: "store/items.go::HandleFoo".to_string(),
                        to: "store/items.go::UpsertItemsRequest".to_string(),
                        is_cycle: false,
                    },
                    Edge {
                        from: "store/items.go::HandleBar".to_string(),
                        to: "store/items.go::UpsertItemsRequest".to_string(),
                        is_cycle: false,
                    },
                ],
                roots: vec![
                    "store/items.go::HandleFoo".to_string(),
                    "store/items.go::HandleBar".to_string(),
                ],
            },
            tests: vec![],
            hotspots: vec![Hotspot {
                id: "store/items.go::UpsertItemsRequest".to_string(),
                path: "store/items.go".to_string(),
                name: "UpsertItemsRequest".to_string(),
                used_by: vec!["HandleBar".to_string(), "HandleFoo".to_string()],
            }],
        };

        let expected = "\
## Change graph

3 changed symbols in 1 file

- fn HandleFoo (store/items.go) — uses: UpsertItemsRequest
- fn HandleBar (store/items.go) — uses: UpsertItemsRequest

## Hotspots

- struct UpsertItemsRequest (store/items.go) — used by 2: HandleBar, HandleFoo

## Definitions

### fn HandleFoo (store/items.go)

```
func HandleFoo(req UpsertItemsRequest) error
```

### struct UpsertItemsRequest (store/items.go)

```
type UpsertItemsRequest struct { Items []Item }
```

### fn HandleBar (store/items.go)

```
func HandleBar(req UpsertItemsRequest) error
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_hotspot_line_for_symbol_with_no_matching_definition() {
        // Same defensive rationale as the "NOTE" block below: `hotspots`
        // could in principle reference a node id with no corresponding
        // `ExtractedSymbol` in `files` (the node is present in `graph`, so
        // the empty-output guard does not short-circuit, but `files` itself
        // has no matching symbol — mirroring the other lookup-miss tests'
        // setup). Unlike "Change graph"/"Definitions" (which use
        // `SymbolLookup`, keyed by symbol id, to find the container/
        // signature and skip the line entirely on a miss), the "Hotspots"
        // line still renders on a lookup miss, falling back to a bare
        // `{name} ({path})` label with no kind prefix, rather than being
        // dropped outright.
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
                edges: vec![],
                roots: vec!["src/lib.rs::ghost".to_string()],
            },
            tests: vec![],
            hotspots: vec![Hotspot {
                id: "src/lib.rs::ghost".to_string(),
                path: "src/lib.rs".to_string(),
                name: "ghost".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            }],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file


## Hotspots

- ghost (src/lib.rs) — used by 2: a, b

## Definitions

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // NOTE: these three tests hand-build a `Report` whose `graph` refers to
    // node ids that have no corresponding `ExtractedSymbol` in `files` — an
    // inconsistency `pipeline::analyze_diff` never actually produces (the
    // graph is always built from, and ids stamped onto, the very same
    // `files` list), but exercised here defensively so the lookup-miss
    // fallback branches (`SymbolLookup::get` returning `None`) have direct
    // coverage rather than being unreachable-in-practice dead code.

    #[test]
    fn should_skip_definitions_entry_when_visit_order_id_has_no_matching_symbol() {
        // `dfs_pre_order`'s `visit_order` is derived from `graph.nodes`, not
        // `files`, so a node with no matching `ExtractedSymbol` reaches the
        // `let Some(..) = lookup.get(id) else { continue }` branch in the
        // "Definitions" loop; the malformed root must simply be skipped
        // rather than panicking or emitting a broken heading.
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
                edges: vec![],
                roots: vec!["src/lib.rs::ghost".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file


## Definitions

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_nothing_for_root_when_root_id_has_no_matching_symbol() {
        // Same lookup miss as above, but hit inside `render_tree_node`'s
        // own `let Some(..) = lookup.get(id) else { return Ok(()) }` guard
        // (the "Change graph" tree-line branch) rather than the
        // "Definitions" loop.
        let report = Report {
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
                edges: vec![],
                roots: vec!["src/lib.rs::ghost".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file


## Definitions

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        // Both malformed-root branches (the "Change graph" line and the
        // "Definitions" entry) are exercised by the same minimal report;
        // asserted together since there is no simpler input that isolates
        // only one of the two lookups.
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_omit_cycle_warning_line_when_cycle_target_id_has_no_matching_symbol() {
        // A cycle edge whose `to` id has no matching symbol hits
        // `render_tree_node`'s inner `let Some(..) = lookup.get(child_id)
        // else { continue }` guard (the cycle-warning branch specifically,
        // as opposed to the two tests above which exercise the
        // non-cycle-edge lookups) — the warning line is simply omitted
        // rather than rendering a broken label.
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
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::ghost".to_string(),
                    is_cycle: true,
                }],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
        };

        let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

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
            tests: vec![],
            hotspots: vec![],
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
  },
  \"tests\": [],
  \"hotspots\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }
}
