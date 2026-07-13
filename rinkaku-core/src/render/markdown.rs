//! Markdown rendering — the default, human/LLM-oriented output.
//!
//! Turns a [`Report`] into the multi-section Markdown document (change
//! graph, high fan-in symbols, definitions, removed symbols, tests, other
//! changed files, skipped files) that the CLI emits by default and the
//! LLM-review integration feeds directly to a model. Every function here
//! writes into a shared `String` buffer; the tests below pin the exact
//! output for each section shape.

use crate::extract::{Classification, ExtractedSymbol, RemovedSymbol, SymbolKind};
use crate::file_size::{FileSizeBand, FileSizeEntry};
use crate::graph::{FanIn, Node, NodeId, SymbolGraph};
use crate::render::RenderError;
use crate::render::report::{Report, ReportOrigin, SkipReason, SkippedFile, skip_reason_label};
use crate::render::shared::SymbolLookup;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

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
pub(super) fn render_markdown(report: &Report) -> Result<String, RenderError> {
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

        if !report.fan_ins.is_empty() {
            writeln!(out, "## High fan-in symbols")?;
            writeln!(out)?;
            for fan_in in &report.fan_ins {
                writeln!(
                    out,
                    "- {} — used by {}: {}",
                    fan_in_label(fan_in, &lookup),
                    fan_in.used_by.len(),
                    fan_in.used_by.join(", ")
                )?;
            }
            writeln!(out)?;
        }

        if !report.file_size_bands.is_empty() {
            writeln!(out, "## File sizes")?;
            writeln!(out)?;
            for entry in &report.file_size_bands {
                writeln!(out, "- {}", file_size_entry_label(entry))?;
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
/// "Change graph"/"High fan-in symbols"), the signature block, and its
/// unchanged 1-hop `dependencies` under "Depends on:".
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
/// `render_tree_node`'s tree rows and [`fan_in_label`]'s "High fan-in
/// symbols" lines so the marker reads identically in both sections, same as
/// `tree_label` itself already does.
fn labeled_with_marker(path: &str, symbol: &ExtractedSymbol) -> String {
    let label = tree_label(path, symbol);
    match classification_marker(symbol.classification) {
        Some(marker) => format!("{label} — {marker}"),
        None => label,
    }
}

/// Builds the "High fan-in symbols" line label for a [`FanIn`], reusing
/// [`labeled_with_marker`] via `lookup` so a fan-in entry's label (including
/// its ADR 0014 marker) is identical to how the same symbol is labeled in
/// "Change graph"/"Definitions" (ADR 0013's requirement that labels stay
/// consistent across sections) — including the `:{start_line}`
/// disambiguation suffix when applicable.
///
/// Falls back to a bare `{name} ({path})` (no kind prefix, no marker) when
/// `lookup` has no matching `ExtractedSymbol` for `fan_in.id` — defensive,
/// since `pipeline::analyze_diff` always builds `fan_ins` from the same
/// `graph` whose node ids match `files`' stamped symbol ids (same invariant
/// `render_tree_node`'s own lookup-miss guards rely on), so this branch is
/// not expected to trigger in practice.
fn fan_in_label(fan_in: &FanIn, lookup: &SymbolLookup) -> String {
    match lookup.get(&fan_in.id) {
        Some((path, symbol)) => labeled_with_marker(path, symbol),
        None => format!("{} ({})", fan_in.name, fan_in.path),
    }
}

/// Builds the "File sizes" line label for a [`FileSizeEntry`] (ADR 0028
/// amendment): `path (N lines)` for [`FileSizeBand::Normal`], with a
/// `, {band}` suffix for every other band.
fn file_size_entry_label(entry: &FileSizeEntry) -> String {
    match entry.band {
        FileSizeBand::Normal => format!("`{}` ({} lines)", entry.path, entry.line_count),
        FileSizeBand::Watch => format!("`{}` ({} lines, watch)", entry.path, entry.line_count),
        FileSizeBand::Warn => format!("`{}` ({} lines, warn)", entry.path, entry.line_count),
        FileSizeBand::Split => format!("`{}` ({} lines, split)", entry.path, entry.line_count),
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

#[cfg(test)]
#[path = "markdown_tests/mod.rs"]
mod tests;
