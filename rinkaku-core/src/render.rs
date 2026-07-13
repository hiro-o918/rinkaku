//! Rendering the extraction pipeline's results into an output format.
//!
//! [`Report`] is the pipeline-wide result shape produced by either
//! [`crate::pipeline::analyze_diff`] (a diff, [`ReportOrigin::Diff`]) or
//! [`crate::pipeline::analyze_repo`] (a whole-repo outline with no diff
//! involved, [`ReportOrigin::RepoOutline`] — ADR 0017): per-file extracted
//! symbols plus the files that were skipped (unsupported language, binary,
//! or deleted; `analyze_repo` never populates this), plus the
//! [`crate::graph::SymbolGraph`] built over those symbols (ADR 0008). This
//! module turns a `Report` into either Markdown (the default, meant for
//! humans and LLMs) or JSON (`serde`-derived, for machine consumption).
//!
//! Markdown renders in this order: a "Change graph" tree for a diff, or
//! "Repository graph" for a whole-repo outline (names only, rooted at the
//! graph's auto-detected entry points) giving the reader a call-hierarchy
//! reading order, with an optional "Hotspots" sub-section (ADR 0013) right
//! after it; "Definitions" — the full signature of every symbol, in the
//! same tree order, each shown exactly once (ADR 0008's decision to avoid
//! duplicating a symbol reachable from multiple roots); "Removed symbols" —
//! base-side symbols with no head-side counterpart at all (ADR 0014,
//! diff-only: `report.removed` is always empty for a whole-repo outline),
//! omitted when empty; "Tests" — a per-file count of changed test symbols
//! excluded from the graph/definitions above by default (ADR 0009); "Other
//! changed files" — files with no changed-symbol-level content (e.g. pure
//! renames); and "Skipped files". A whole-repo outline's wording drops every
//! "changed" qualifier (`report.origin` picks the noun — see
//! `change_graph_summary`), since nothing changed in that mode.
//!
//! ADR 0014 also marks each "Change graph"/"Hotspots"/"Definitions" line
//! with its contract-impact classification (`— new` / `— signature
//! changed`; `body_only` and not-attempted classifications render
//! unmarked), and a `signature_changed` symbol's "Definitions" entry shows
//! a ` ```diff ` block (base signature as `-`, head signature as `+`)
//! instead of the plain fenced signature every other classification gets.
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

use crate::extract::{Classification, ExtractedSymbol, RemovedSymbol, SymbolKind};
use crate::graph::{Hotspot, Node, NodeId, SymbolGraph};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use thiserror::Error;

/// The result of running the extraction pipeline over a whole diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Which pipeline entry point produced this report (ADR 0017):
    /// [`ReportOrigin::Diff`] (the default — `analyze_diff`, every existing
    /// input mode) or [`ReportOrigin::RepoOutline`] (`analyze_repo`, the
    /// whole-repo default with no diff involved at all). Rendering reads
    /// this to pick change-oriented wording ("changed symbols") vs.
    /// outline-oriented wording ("symbols") for the same underlying data
    /// shape — see `render_markdown`'s "## Change graph"/"## Repository
    /// graph" split.
    ///
    /// `#[serde(default, skip_serializing_if = ...)]` keeps every existing
    /// `analyze_diff`-produced JSON report byte-for-byte unchanged: the
    /// field is omitted entirely when it's the default `Diff`, and only
    /// appears (as `"origin": "repo-outline"`) for the new whole-repo mode,
    /// which has no prior JSON shape to stay compatible with.
    #[serde(default, skip_serializing_if = "ReportOrigin::is_diff")]
    pub origin: ReportOrigin,
    pub files: Vec<FileReport>,
    pub skipped: Vec<SkippedFile>,
    /// The dependency graph over `files`' symbols (ADR 0008): edges and
    /// entry points used to render "Change graph" in Markdown, exposed here
    /// too so JSON consumers get the same structure without recomputing it.
    pub graph: SymbolGraph,
    /// Per-file counts of changed test symbols excluded from `files`
    /// under `--exclude-tests` (ADR 0009's mechanism; ADR 0025 flipped
    /// the default so this is now opt-in). Empty in the default run
    /// (test symbols stay in `files` like any other symbol) and only
    /// populated when the CLI passes `--exclude-tests`. Source order (the
    /// order files were first encountered in the diff), same as `files`.
    pub tests: Vec<TestFileSummary>,
    /// Fan-in hotspots (ADR 0013): changed symbols referenced by two or more
    /// other changed symbols, sorted by fan-in descending. Derived from
    /// `graph` via [`crate::graph::compute_hotspots`] and kept as its own
    /// `Report` field (rather than recomputed at render time) so JSON
    /// consumers get it without recomputing the aggregation themselves,
    /// matching how `graph` itself is already exposed alongside `files`.
    pub hotspots: Vec<Hotspot>,
    /// Symbols present on the base side of a diff but absent from the head
    /// side entirely (ADR 0014's `removed` classification) — reported
    /// separately from `files` since a removed symbol has no head-side
    /// signature/range/dependencies of its own. Always empty when no base
    /// content was available to classify against (see
    /// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter),
    /// same as every symbol's `classification` staying `None` in that case.
    pub removed: Vec<crate::extract::RemovedSymbol>,
}

/// Which pipeline entry point produced a [`Report`] (ADR 0017). `Default`
/// is `Diff` — every pre-ADR-0017 caller builds a `Report` via
/// `analyze_diff`, so defaulting to it is what keeps those `Report { ... }`
/// literals (and the JSON they serialize to) unchanged without having to
/// touch every one of them to spell out `origin` explicitly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportOrigin {
    #[default]
    Diff,
    RepoOutline,
}

impl ReportOrigin {
    /// Predicate form of `matches!(self, ReportOrigin::Diff)`, for
    /// `#[serde(skip_serializing_if = ...)]`, which needs a `fn(&T) -> bool`
    /// path rather than an inline expression.
    fn is_diff(&self) -> bool {
        matches!(self, ReportOrigin::Diff)
    }
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
    /// A human-oriented call/dependency graph rendered as a mermaid
    /// `flowchart` document (ADR 0021) — opt-in, aimed at GitHub's native
    /// mermaid rendering (PR comments/descriptions), not the default
    /// Markdown output ADR 0013/0015 keep machine-facing.
    Mermaid,
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
        OutputFormat::Mermaid => Ok(render_mermaid(report)),
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
        && report.removed.is_empty()
    {
        return Ok(String::new());
    }

    let lookup = SymbolLookup::build(&report.files);
    let children = children_by_node(&report.graph);
    let visit_order = dfs_pre_order(&report.graph, &children);

    let mut out = String::new();

    if !report.graph.nodes.is_empty() {
        let heading = match report.origin {
            ReportOrigin::Diff => "## Change graph",
            ReportOrigin::RepoOutline => "## Repository graph",
        };
        writeln!(out, "{heading}")?;
        writeln!(out)?;
        writeln!(
            out,
            "{}",
            change_graph_summary(&report.graph.nodes, report.origin)
        )?;
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

    if !report.removed.is_empty() {
        writeln!(out, "## Removed symbols")?;
        writeln!(out)?;
        // `removed_symbol_label` omits `container` (see its own doc
        // comment: `RemovedSymbol` carries no stable id to disambiguate
        // with), so two distinct removed symbols that share name+kind but
        // differ only by container — e.g. the same method name removed
        // from two different impls/classes in one file — would otherwise
        // render as identical duplicate lines. Deduplicated here (first
        // occurrence kept, `report.removed`'s own order otherwise
        // preserved) rather than by changing the label itself, since the
        // label's job is display, not identity.
        let mut printed_lines: HashSet<String> = HashSet::new();
        for removed in &report.removed {
            let line = removed_symbol_label(removed);
            if printed_lines.insert(line.clone()) {
                writeln!(out, "- {line}")?;
            }
        }
        writeln!(out)?;
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

/// Builds the one-line summary shown under the "## Change graph"/
/// "## Repository graph" heading, e.g. `16 changed symbols in 3 files —
/// most in store/items.go (11)` for a diff, or `16 symbols in 3 files —
/// most in store/items.go (11)` for a whole-repo outline (ADR 0017: nothing
/// changed in that mode, so the "changed" qualifier would misdescribe it —
/// see `origin`) (ADR 0012 decision 3). Computed from `nodes` alone (not
/// `edges`/`roots`), so it stays meaningful even if graph-building changes
/// independently.
///
/// The `— most in ...` suffix is dropped when every node lives in the same
/// file: naming "the file with the most nodes" is redundant when there is
/// only one file to begin with. Ties for "most" go to whichever path's node
/// appears first in `nodes` (stable, diff-derived order), matching the
/// tie-break `render_markdown` already relies on elsewhere (e.g. root
/// order) rather than an arbitrary path-string sort.
///
/// Callers must not call this with an empty `nodes` — `render_markdown`
/// only emits the "Change graph"/"Repository graph" section (and this
/// summary) when `graph.nodes` is non-empty, matching pre-ADR-0012 behavior
/// for an empty graph.
fn change_graph_summary(nodes: &[Node], origin: ReportOrigin) -> String {
    let total = nodes.len();
    let symbol_noun = match (total, origin) {
        (1, ReportOrigin::Diff) => "changed symbol",
        (_, ReportOrigin::Diff) => "changed symbols",
        (1, ReportOrigin::RepoOutline) => "symbol",
        (_, ReportOrigin::RepoOutline) => "symbols",
    };

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
        return format!("{total} {symbol_noun} in {file_count} {file_noun}");
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
        "{total} {symbol_noun} in {file_count} {file_noun} — most in {hotspot_path} ({hotspot_count})"
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
    let label = labeled_with_marker(path, symbol);

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
/// label as the tree (with ADR 0014's contract-impact marker, same as
/// "Change graph"/"Hotspots"), the signature block, and its unchanged 1-hop
/// `dependencies` under "Depends on:".
///
/// The signature block is a plain fence for every classification except
/// [`Classification::SignatureChanged`] (or classification not attempted),
/// which instead gets a ` ```diff ` block showing the base signature as a
/// `-` line and the head signature as a `+` line — the same before/after
/// shape a reviewer already reads diffs in, applied to just the signature
/// rather than the whole file. The container comment line, when present, is
/// unchanged either way (not part of the diff, since it never differs
/// between base and head — `find_container`'s output depends only on
/// enclosing-block structure, not on the signature text being compared).
fn render_definition(
    out: &mut String,
    path: &str,
    symbol: &ExtractedSymbol,
) -> Result<(), RenderError> {
    writeln!(out, "### {}", labeled_with_marker(path, symbol))?;
    writeln!(out)?;

    let container_line = symbol.container.as_deref().map(|c| format!("// {c}"));
    match (symbol.classification, &symbol.previous_signature) {
        (Some(Classification::SignatureChanged), Some(previous_signature)) => {
            let fence = fence_for_diff(
                container_line.as_deref(),
                previous_signature,
                &symbol.signature,
            );
            writeln!(out, "{fence}diff")?;
            if let Some(container_line) = &container_line {
                writeln!(out, "{container_line}")?;
            }
            writeln!(out, "-{previous_signature}")?;
            writeln!(out, "+{}", symbol.signature)?;
            writeln!(out, "{fence}")?;
        }
        _ => {
            let fence = fence_for(container_line.as_deref(), &symbol.signature);
            writeln!(out, "{fence}")?;
            if let Some(container_line) = &container_line {
                writeln!(out, "{container_line}")?;
            }
            writeln!(out, "{}", symbol.signature)?;
            writeln!(out, "{fence}")?;
        }
    }
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

/// [`tree_label`] with ADR 0014's `— <marker>` contract-impact annotation
/// appended (`— new` / `— signature changed`), or the bare label unchanged
/// when [`classification_marker`] has nothing to say for this symbol
/// (`body_only` or classification not attempted). Shared by
/// `render_tree_node`'s tree rows and [`hotspot_label`]'s "Hotspots" lines
/// so the marker reads identically in both sections, same as `tree_label`
/// itself already does.
fn labeled_with_marker(path: &str, symbol: &ExtractedSymbol) -> String {
    let label = tree_label(path, symbol);
    match classification_marker(symbol.classification) {
        Some(marker) => format!("{label} — {marker}"),
        None => label,
    }
}

/// Builds the "Hotspots" line label for a [`Hotspot`], reusing
/// [`labeled_with_marker`] via `lookup` so a hotspot's label (including its
/// ADR 0014 marker) is identical to how the same symbol is labeled in
/// "Change graph"/"Definitions" (ADR 0013's requirement that labels stay
/// consistent across sections) — including the `:{start_line}`
/// disambiguation suffix when applicable.
///
/// Falls back to a bare `{name} ({path})` (no kind prefix, no marker) when
/// `lookup` has no matching `ExtractedSymbol` for `hotspot.id` — defensive,
/// since `pipeline::analyze_diff` always builds `hotspots` from the same
/// `graph` whose node ids match `files`' stamped symbol ids (same invariant
/// `render_tree_node`'s own lookup-miss guards rely on), so this branch is
/// not expected to trigger in practice.
fn hotspot_label(hotspot: &Hotspot, lookup: &SymbolLookup) -> String {
    match lookup.get(&hotspot.id) {
        Some((path, symbol)) => labeled_with_marker(path, symbol),
        None => format!("{} ({})", hotspot.name, hotspot.path),
    }
}

/// Builds the "Removed symbols" line label for a [`RemovedSymbol`]:
/// `{prefix} {name} ({path})`, the same form [`tree_label`] uses, minus the
/// `:{start_line}` disambiguation suffix — `RemovedSymbol` carries no
/// stable id the way `ExtractedSymbol` does (a removed symbol was never
/// stamped into the graph, see `graph::collect_nodes`), so there is no
/// signal to disambiguate two same-named removed symbols by, the same way
/// there would be nothing to key a `(see above)` reference off of either.
fn removed_symbol_label(removed: &RemovedSymbol) -> String {
    format!(
        "{} {} ({})",
        symbol_kind_prefix(removed.kind),
        removed.name,
        removed.path
    )
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

/// The `— <marker>` annotation text for a symbol's contract impact (ADR
/// 0014), reusing the existing `— ` idiom already used for `— uses:`/`—
/// used by`/`— dependency cycle` annotations elsewhere in this module.
/// `None` for [`Classification::BodyOnly`] (nothing contract-relevant to
/// call out) and for `None` classification (never attempted — see
/// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter): both
/// render unmarked rather than guessing.
fn classification_marker(classification: Option<Classification>) -> Option<&'static str> {
    match classification {
        Some(Classification::Added) => Some("new"),
        Some(Classification::SignatureChanged) => Some("signature changed"),
        Some(Classification::BodyOnly) | None => None,
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

/// [`fence_for`]'s sibling for a `signature_changed` symbol's ` ```diff `
/// block (ADR 0014): widens against both the base and head signature text,
/// not just the head signature `fence_for` alone considers, since the
/// fenced block's content is now both lines.
fn fence_for_diff(
    container_line: Option<&str>,
    previous_signature: &str,
    signature: &str,
) -> String {
    let longest_run = [container_line.unwrap_or(""), previous_signature, signature]
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

/// The short label shown for a [`SkipReason`] — `"unsupported language"`,
/// `"binary"`, `"deleted"`, `"generated"`. `pub` (rather than private to
/// this module) so other renderers of the same [`Report`] data — currently
/// `rinkaku-tui`'s entry-tree view — can show the identical wording instead
/// of maintaining a second copy of this match that could drift from
/// Markdown's.
pub fn skip_reason_label(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::UnsupportedLanguage => "unsupported language",
        SkipReason::Binary => "binary",
        SkipReason::Deleted => "deleted",
        SkipReason::Generated => "generated",
    }
}

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
/// Infallible (`Result` in [`render`]'s signature is uniform across
/// formats, not because this can fail): unlike `render_markdown`'s
/// `std::fmt::Write` calls, building a `String` via `push_str`/`write!`
/// into an owned buffer that is only ever handed back to the caller cannot
/// error the way a fallible `io::Write` sink could.
fn render_mermaid(report: &Report) -> String {
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

    #[test]
    fn should_render_empty_markdown_when_report_has_no_files_and_no_skips() {
        let report = Report {
            origin: ReportOrigin::Diff,
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
  \"hotspots\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    // ADR 0017: `origin` must stay invisible in JSON for every existing
    // `analyze_diff`-produced report (see
    // `should_keep_generated_entry_in_json_output` above, whose expected
    // JSON has no `"origin"` key at all) — this is the flip side, pinning
    // that a whole-repo outline's `RepoOutline` origin *does* serialize, as
    // `"origin": "repo-outline"`, so JSON consumers can tell the two modes
    // apart.
    #[test]
    fn should_serialize_origin_field_when_report_is_a_repo_outline() {
        let report = Report {
            origin: ReportOrigin::RepoOutline,
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
        };

        let expected = "\
{
  \"origin\": \"repo-outline\",
  \"files\": [],
  \"skipped\": [],
  \"graph\": {
    \"nodes\": [],
    \"edges\": [],
    \"roots\": []
  },
  \"tests\": [],
  \"hotspots\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_change_graph_and_definitions_when_report_has_one_symbol() {
        let report = Report {
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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

    // ADR 0017: a whole-repo outline has no diff, so "Change graph"/
    // "changed symbols" would misdescribe it — this pins the alternate
    // heading and noun `ReportOrigin::RepoOutline` selects, using the same
    // multi-file/hotspot shape as
    // `should_render_summary_with_hotspot_when_report_has_multiple_symbols_and_files`
    // so the two tests differ only in `origin` and its wording.
    #[test]
    fn should_render_repository_graph_heading_and_drop_changed_wording_when_origin_is_repo_outline()
    {
        let report = Report {
            origin: ReportOrigin::RepoOutline,
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
            removed: vec![],
        };

        let expected = "\
## Repository graph

5 symbols in 2 files — most in store/items.go (3)


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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
                edges: vec![],
                roots: vec!["src/lib.rs::ghost".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
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
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
                edges: vec![],
                roots: vec!["src/lib.rs::ghost".to_string()],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
            origin: ReportOrigin::Diff,
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
            removed: vec![],
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
  \"hotspots\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    mod classification_rendering_tests {
        use super::*;
        use crate::extract::{Classification, RemovedSymbol};
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[test]
        fn should_append_new_marker_to_tree_and_definition_when_symbol_is_added() {
            let mut foo = symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()");
            foo.classification = Some(Classification::Added);
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![foo],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![],
            };

            let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — new

## Definitions

### fn foo (src/lib.rs) — new

```
fn foo()
```

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_render_diff_block_and_marker_when_symbol_is_signature_changed() {
            let mut foo = symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo(a: i32, b: i32) -> i32",
            );
            foo.classification = Some(Classification::SignatureChanged);
            foo.previous_signature = Some("fn foo(a: i32) -> i32".to_string());
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![foo],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![],
            };

            let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — signature changed

## Definitions

### fn foo (src/lib.rs) — signature changed

```diff
-fn foo(a: i32) -> i32
+fn foo(a: i32, b: i32) -> i32
```

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        // A signature-changed symbol with a container gets the container
        // comment line rendered unchanged above the diff lines — it is not
        // itself part of the base/head comparison (see `render_definition`'s
        // doc comment).
        #[test]
        fn should_render_container_comment_above_diff_lines_when_signature_changed_symbol_has_container()
         {
            let mut bar = symbol(
                "src/lib.rs::bar",
                "bar",
                SymbolKind::Function,
                "fn bar(&self, extra: i32) -> i32",
            );
            bar.container = Some("impl Foo".to_string());
            bar.classification = Some(Classification::SignatureChanged);
            bar.previous_signature = Some("fn bar(&self) -> i32".to_string());
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![bar],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::bar".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![],
            };

            let expected = "\
## Change graph

1 changed symbol in 1 file

- fn bar (src/lib.rs) — signature changed

## Definitions

### fn bar (src/lib.rs) — signature changed

```diff
// impl Foo
-fn bar(&self) -> i32
+fn bar(&self, extra: i32) -> i32
```

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        // `body_only` and unattempted (`None`) classification both render
        // completely unmarked — no `— <marker>` suffix anywhere, and a
        // plain (non-diff) fenced signature block.
        #[rstest]
        #[case::should_render_unmarked_when_classification_is_body_only(Some(
            Classification::BodyOnly
        ))]
        #[case::should_render_unmarked_when_classification_is_none(None)]
        fn should_render_unmarked_tree_and_definition(
            #[case] classification: Option<Classification>,
        ) {
            let mut foo = symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()");
            foo.classification = classification;
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![foo],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![],
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
        fn should_append_marker_to_hotspot_line_before_used_by() {
            let mut shared = symbol(
                "src/lib.rs::shared",
                "shared",
                SymbolKind::Function,
                "fn shared()",
            );
            shared.classification = Some(Classification::SignatureChanged);
            shared.previous_signature = Some("fn shared(a: i32)".to_string());
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![shared],
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
                    used_by: vec!["caller_one".to_string(), "caller_two".to_string()],
                }],
                removed: vec![],
            };

            let markdown =
                render(&report, OutputFormat::Markdown).expect("markdown render succeeds");
            // NOTE: partial assert (searching for one line) rather than a
            // fully qualified comparison of the whole render — this test's
            // concern is solely the "Hotspots" line's marker placement, and
            // the "Change graph"/"Definitions" sections above it are
            // already covered by other tests in this module (e.g.
            // `should_render_diff_block_and_marker_when_symbol_is_signature_changed`).
            let hotspots_line = markdown
                .lines()
                .find(|line| line.contains("used by"))
                .expect("hotspots section must contain the shared symbol's line");

            assert_eq!(
                "- fn shared (src/lib.rs) — signature changed — used by 2: caller_one, caller_two",
                hotspots_line
            );
        }

        #[test]
        fn should_render_removed_symbols_section_between_definitions_and_tests() {
            let report = Report {
                origin: ReportOrigin::Diff,
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
                tests: vec![TestFileSummary {
                    path: "src/lib.rs".to_string(),
                    symbol_count: 1,
                }],
                hotspots: vec![],
                removed: vec![RemovedSymbol {
                    name: "old_helper".to_string(),
                    kind: SymbolKind::Function,
                    path: "src/lib.rs".to_string(),
                    signature: "fn old_helper()".to_string(),
                }],
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

## Removed symbols

- fn old_helper (src/lib.rs)

## Tests

- src/lib.rs: 1 changed test symbol

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        // A diff whose only changed-symbol-level content is a removal (no
        // graph nodes at all, e.g. a whole function deleted with nothing
        // added back — see `pipeline::tests::classification_wiring_tests`'s
        // "hunk only removes lines" case) must still render "## Removed
        // symbols" on its own — the empty-output guard at the top of
        // `render_markdown` must not treat an empty `graph.nodes` as "there
        // is nothing to say" when `removed` is non-empty.
        #[test]
        fn should_render_removed_symbols_section_alone_when_graph_is_empty() {
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![],
                    edges: vec![],
                    roots: vec![],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![RemovedSymbol {
                    name: "old_helper".to_string(),
                    kind: SymbolKind::Function,
                    path: "src/lib.rs".to_string(),
                    signature: "fn old_helper()".to_string(),
                }],
            };

            let expected = "\
## Removed symbols

- fn old_helper (src/lib.rs)

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        // Regression test: `removed_symbol_label` deliberately omits
        // `container` (see its own doc comment), so two distinct removed
        // symbols sharing name+kind but differing only by container (e.g.
        // the same method name removed from two different impls in one
        // file) render identical lines. Without deduplication this would
        // print the same line twice; the section must show it once, in
        // first-occurrence order.
        #[test]
        fn should_deduplicate_identical_removed_symbol_lines() {
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![],
                    edges: vec![],
                    roots: vec![],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![
                    RemovedSymbol {
                        name: "save".to_string(),
                        kind: SymbolKind::Function,
                        path: "src/lib.rs".to_string(),
                        signature: "fn save(&self)".to_string(),
                    },
                    RemovedSymbol {
                        name: "other".to_string(),
                        kind: SymbolKind::Function,
                        path: "src/lib.rs".to_string(),
                        signature: "fn other(&self)".to_string(),
                    },
                    // Same name+kind+path as the first entry, but a
                    // different container/signature — the label is
                    // identical to the first line even though this is a
                    // genuinely distinct removed symbol (different impl).
                    RemovedSymbol {
                        name: "save".to_string(),
                        kind: SymbolKind::Function,
                        path: "src/lib.rs".to_string(),
                        signature: "fn save(&self, id: &str)".to_string(),
                    },
                ],
            };

            let expected = "\
## Removed symbols

- fn save (src/lib.rs)
- fn other (src/lib.rs)

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_omit_removed_symbols_section_when_removed_is_empty() {
            let report = Report {
                origin: ReportOrigin::Diff,
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
                removed: vec![],
            };

            let markdown =
                render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert!(!markdown.contains("## Removed symbols"));
        }

        // A diff isn't valid Markdown to embed unfenced — ensure the fence
        // still widens against a backtick run appearing in either the base
        // or head signature text, not just the head signature the way
        // `fence_for` alone would check.
        #[test]
        fn should_widen_fence_when_previous_signature_contains_a_backtick_run() {
            let mut foo = symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo() -> i32",
            );
            foo.classification = Some(Classification::SignatureChanged);
            // Three consecutive backticks in the *base* signature only —
            // proves the fence widens against `previous_signature`, not
            // just the head `signature` the way plain `fence_for` would.
            foo.previous_signature =
                Some("fn foo() { let s = \"```rust\\nfn f() {}\\n```\"; }".to_string());
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![foo],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![],
            };

            let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs) — signature changed

## Definitions

### fn foo (src/lib.rs) — signature changed

````diff
-fn foo() { let s = \"```rust\\nfn f() {}\\n```\"; }
+fn foo() -> i32
````

"
            .to_string();
            let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_serialize_classification_and_previous_signature_and_removed_in_json() {
            let mut foo = symbol(
                "src/lib.rs::foo",
                "foo",
                SymbolKind::Function,
                "fn foo(a: i32, b: i32) -> i32",
            );
            foo.classification = Some(Classification::SignatureChanged);
            foo.previous_signature = Some("fn foo(a: i32) -> i32".to_string());
            let report = Report {
                origin: ReportOrigin::Diff,
                files: vec![FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![foo],
                }],
                skipped: vec![],
                graph: SymbolGraph {
                    nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                    edges: vec![],
                    roots: vec!["src/lib.rs::foo".to_string()],
                },
                tests: vec![],
                hotspots: vec![],
                removed: vec![RemovedSymbol {
                    name: "old_helper".to_string(),
                    kind: SymbolKind::Function,
                    path: "src/lib.rs".to_string(),
                    signature: "fn old_helper()".to_string(),
                }],
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
          \"signature\": \"fn foo(a: i32, b: i32) -> i32\",
          \"range\": {
            \"start\": 1,
            \"end\": 1
          },
          \"container\": null,
          \"dependencies\": [],
          \"omitted_matches\": 0,
          \"classification\": \"signature_changed\",
          \"previous_signature\": \"fn foo(a: i32) -> i32\"
        }
      ]
    }
  ],
  \"skipped\": [],
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
  \"hotspots\": [],
  \"removed\": [
    {
      \"name\": \"old_helper\",
      \"kind\": \"Function\",
      \"path\": \"src/lib.rs\",
      \"signature\": \"fn old_helper()\"
    }
  ]
}"
            .to_string();
            let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

            assert_eq!(expected, actual);
        }
    }
}

#[cfg(test)]
mod mermaid_tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::{Classification, SymbolKind};
    use crate::graph::{Edge, Hotspot, Node, SymbolGraph};
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
