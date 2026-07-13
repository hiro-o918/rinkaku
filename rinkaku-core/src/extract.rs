//! Tree-sitter based signature extraction.
//!
//! Given a source file's text and the line ranges that changed in a diff
//! (see [`crate::diff::LineRange`]), finds the definitions that contain
//! those changed lines and slices out their signatures — the API surface,
//! without the implementation body.

use crate::diff::LineRange;
use crate::language::LanguageSupport;
use serde::Serialize;
use std::collections::HashMap;
use tree_sitter::StreamingIterator;

/// The kind of symbol a definition node represents, expressed in
/// language-neutral terms so callers don't need to match on
/// language-specific tree-sitter node kinds.
///
/// No `Impl` variant: impl/class/interface bodies are never reported as
/// symbols in their own right when one of their nested members was itself
/// touched (see the filtering in `extract_changed_symbols`) — they only
/// contribute `container` names to the members nested inside them.
///
/// Methods (Go receiver methods, Python/TypeScript class methods, Rust
/// impl/trait methods, TypeScript arrow functions bound to a
/// `const`/`let`/`var`) are all reported as `Function`, matching the
/// precedent already set by the Rust support: `container` is what
/// distinguishes "a method of X" from a free function, so a separate
/// `Method` variant would duplicate information already carried by
/// `container` without adding any.
/// Variants are named for the language-neutral concept they represent, not
/// for a specific language's keyword (e.g. `Class` covers both Python
/// `class` and TypeScript `class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Class,
    Interface,
    TypeAlias,
}

/// A changed symbol's contract impact (ADR 0014), classified by comparing
/// its comment-stripped, normalized signature against the base side's:
///
/// - [`Classification::Added`]: no matching symbol on the base side at all
///   (a brand-new definition).
/// - [`Classification::SignatureChanged`]: a matching base-side symbol
///   exists, but its signature text differs — the API surface itself
///   changed, not just the implementation.
/// - [`Classification::BodyOnly`]: a matching base-side symbol exists with
///   an identical signature — only the body changed.
///
/// `None` (rather than a fourth "unknown" variant) is used when base-side
/// content wasn't available to compare against at all (e.g. plain stdin
/// input with no resolvable base commit) — see
/// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter. Modeling
/// "unknown" as the field's absence, rather than as a variant of this enum,
/// keeps every variant here meaning "we know, and this is what we found"; a
/// caller checking `symbol.classification.is_none()` reads unambiguously as
/// "classification wasn't attempted" rather than "found nothing interesting".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Added,
    SignatureChanged,
    BodyOnly,
}

/// A definition whose signature was extracted because one of its lines
/// (declaration or body) fell inside a changed range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractedSymbol {
    /// Stable identifier matching this symbol's [`crate::graph::Node::id`]
    /// once graph-building has run, so JSON consumers can correlate a
    /// symbol with the graph's `nodes`/`edges`/`roots` without recomputing
    /// the `{path}::{name}` scheme themselves. Empty until
    /// [`crate::graph::build_graph`] populates it (mirrors `dependencies`
    /// and `omitted_dependency_matches`, both populated post-extraction by
    /// a later pipeline stage rather than by `build_symbol`).
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    /// Declaration text without its body, whitespace-normalized. Doc
    /// comments and attributes are not included.
    pub signature: String,
    /// Full definition range (new-side, 1-based inclusive) — body included,
    /// since this describes where the change lives, not the signature's
    /// own extent.
    pub range: LineRange,
    /// The enclosing impl/trait/class block's descriptive name, or a Go
    /// method's receiver type name, if the definition belongs to one (e.g.
    /// `Some("impl Foo")`, `Some("class Point")`, `Some("Repo")`).
    pub container: Option<String>,
    /// Names this definition references (called functions, referenced
    /// types), as captured by [`LanguageSupport::reference_query`].
    /// Deduplicated but otherwise unresolved — an intermediate pipeline
    /// artifact, not part of rinkaku's output shape, so it is excluded
    /// from serialization. [`crate::deps::resolve_dependencies`] resolves
    /// these against a repo-wide definition index to populate
    /// `dependencies`.
    #[serde(skip)]
    pub referenced_names: Vec<String>,
    /// This symbol's 1-hop dependencies: `referenced_names` that resolved
    /// to a definition elsewhere in the repo (ADR 0003), excluding the
    /// symbol's own definition and any symbol already reported in the same
    /// diff (see `crate::deps::resolve_dependencies`). Empty when
    /// dependency resolution was skipped (`--deps 0`) or found nothing.
    ///
    /// Capped at 3 matches per referenced name, ranked by path proximity to
    /// this symbol's own file (see `deps::resolve_dependencies`'s doc
    /// comment) — matches beyond the cap are counted, not dropped, in
    /// `omitted_dependency_matches`.
    pub dependencies: Vec<crate::deps::ResolvedSymbol>,
    /// Count of same-name candidate definitions that resolved but were cut
    /// by the top-3-per-name cap on `dependencies` (ADR 0003's name-only
    /// resolution can otherwise return many same-named matches for a common
    /// identifier). Zero when every match fit under the cap, dependency
    /// resolution was skipped (`--deps 0`), or nothing resolved.
    ///
    /// Serialized as `omitted_matches` (shorter, output-facing name) even
    /// though the Rust field name spells out "dependency" for clarity at
    /// the call site.
    #[serde(rename = "omitted_matches")]
    pub omitted_dependency_matches: usize,
    /// Whether this definition is test code by its AST context (ADR 0009),
    /// e.g. Rust's `#[cfg(test)]` modules and `#[test]`/`#[rstest]`/
    /// `#[tokio::test]`-attributed functions — see
    /// [`crate::language::LanguageSupport::is_test_definition`]. `false`
    /// for every language whose test convention is fully captured by file
    /// path alone (`is_test_definition`'s default), which is the common
    /// case; path-based detection happens at the file level in
    /// `pipeline.rs`, not here, since it does not depend on any individual
    /// node. An intermediate pipeline artifact, not part of rinkaku's
    /// output shape (test symbols are filtered out of `files` before a
    /// `Report` is built, see `pipeline::analyze_diff`), so excluded from
    /// serialization like `referenced_names`.
    #[serde(skip)]
    pub is_test: bool,
    /// This symbol's contract impact (ADR 0014), or `None` when no base-side
    /// content was available to classify against (see
    /// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter).
    /// Populated by [`crate::pipeline::classify_symbols`], a pipeline stage
    /// that runs after extraction — `None` here at construction time, same
    /// as `dependencies`/`id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// The base-side symbol's comment-stripped, normalized signature, only
    /// when [`Classification::SignatureChanged`] — lets renderers show a
    /// before/after diff of the signature text itself. `None` for every
    /// other classification (including `None` classification) since there
    /// is nothing meaningful to show otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_signature: Option<String>,
}

/// Extracts the signatures of definitions that contain at least one
/// changed line. A changed line that isn't inside any definition (e.g. a
/// top-level statement) is not surfaced — v1 only reports symbol-level
/// changes.
pub fn extract_changed_symbols(
    source: &str,
    lang: &dyn LanguageSupport,
    changed_ranges: &[LineRange],
) -> Vec<ExtractedSymbol> {
    if changed_ranges.is_empty() {
        return Vec::new();
    }

    with_definition_nodes(source, lang, |all_nodes, source_bytes, reference_query| {
        let touched_nodes: Vec<tree_sitter::Node> = all_nodes
            .iter()
            .copied()
            .filter(|node| overlaps_any(node_to_line_range(*node), changed_ranges))
            .collect();

        touched_nodes
            .iter()
            .filter(|node| {
                // Prefer the narrowest enclosing definition: a touched node
                // that itself contains another touched node (e.g. an
                // `impl_item`/`class_definition` containing a touched method,
                // or a Python function containing a touched nested function)
                // is suppressed as a symbol in its own right — otherwise a
                // single changed line would surface both the inner definition
                // and every definition enclosing it. Go's `method_declaration`
                // is exempt implicitly: it is never nested inside its receiver
                // struct's node (see `find_container`'s doc comment), so this
                // situation cannot arise for Go structs.
                !touched_nodes
                    .iter()
                    .any(|other| other != *node && is_descendant_of(*other, **node))
            })
            .filter_map(|node| build_symbol(*node, source_bytes, reference_query, lang))
            .collect()
    })
}

/// Extracts every definition in `source`, regardless of whether it
/// changed. Used by [`crate::deps::TagsResolver`] to build a repo-wide
/// name-to-signature index: dependency resolution needs to look up
/// definitions in files that were not part of the diff at all, so it
/// cannot reuse `extract_changed_symbols`, which only ever reports
/// definitions overlapping a given set of changed ranges.
///
/// Unlike `extract_changed_symbols`, nested definitions are not
/// suppressed in favor of their narrowest enclosing one — an index needs
/// every definition, and a nested definition's own `container` (set by
/// `build_symbol`/`find_container`) already records its relationship to
/// its enclosing block, so there is nothing to suppress.
pub fn extract_all_symbols(source: &str, lang: &dyn LanguageSupport) -> Vec<ExtractedSymbol> {
    with_definition_nodes(source, lang, |all_nodes, source_bytes, reference_query| {
        all_nodes
            .iter()
            .filter_map(|node| build_symbol(*node, source_bytes, reference_query, lang))
            .collect()
    })
}

/// A symbol present on the base side of a diff but absent (by name and
/// container) from the head side — ADR 0014's `removed` classification,
/// reported separately from `ExtractedSymbol` since a removed symbol has no
/// head-side signature, range, or dependencies to speak of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    /// The base-side symbol's comment-stripped, normalized signature —
    /// the same text that would have been `previous_signature` on the head
    /// symbol had one still existed to attach it to.
    pub signature: String,
}

/// Classifies every symbol in `head_symbols` by contract impact (ADR 0014),
/// setting its `classification`/`previous_signature` in place, and returns
/// the base-side symbols this file had that no longer exist on the head
/// side at all (`removed`).
///
/// Matches head and base symbols within the same file by `(name,
/// container)` — the same identity `graph::collect_nodes` uses for a
/// symbol's stable id, one file at a time rather than by any cross-file
/// index, since ADR 0014 only classifies a *changed* file's own symbols
/// against that same file's base content.
///
/// - A head symbol with no base-side match at all → [`Classification::Added`].
/// - A head symbol with a base-side match whose comment-stripped,
///   normalized signature differs → [`Classification::SignatureChanged`],
///   with `previous_signature` set to the base signature.
/// - A head symbol with a base-side match whose signature is identical →
///   [`Classification::BodyOnly`].
/// - A base symbol with no head-side match, whose base-side range overlaps
///   `old_changed_ranges` (the diff's old-side hunk ranges for this file) →
///   returned as a [`RemovedSymbol`]. A base-only symbol *outside* every
///   changed range is not reported: nothing in the diff actually touched
///   it, so it is unrelated to this change (e.g. a symbol that merely moved
///   later in the file because of an unrelated edit above it) — restricting
///   to overlapping ranges is what keeps this from flooding output on a
///   diff that only touches a small part of a large file.
///
/// Pure: takes both sides' already-extracted symbol lists and matches them
/// in memory, no IO. `lang` is not needed here — `head_symbols` and
/// `base_symbols` are both already the output of `extract_changed_symbols`/
/// `extract_all_symbols`, whose signatures are already comment-stripped and
/// normalized (ADR 0014's first change) — so signature comparison is a
/// plain string comparison, not a second parse.
pub fn classify_symbols(
    head_symbols: &mut [ExtractedSymbol],
    base_symbols: &[ExtractedSymbol],
    old_changed_ranges: &[LineRange],
    path: &str,
) -> Vec<RemovedSymbol> {
    let base_by_identity: HashMap<(&str, Option<&str>), &ExtractedSymbol> = base_symbols
        .iter()
        .map(|s| ((s.name.as_str(), s.container.as_deref()), s))
        .collect();

    let mut matched_base_identities: std::collections::HashSet<(&str, Option<&str>)> =
        std::collections::HashSet::new();

    for symbol in head_symbols.iter_mut() {
        let identity = (symbol.name.as_str(), symbol.container.as_deref());
        match base_by_identity.get(&identity) {
            None => {
                symbol.classification = Some(Classification::Added);
            }
            Some(base_symbol) => {
                matched_base_identities.insert(identity);
                if base_symbol.signature == symbol.signature {
                    symbol.classification = Some(Classification::BodyOnly);
                } else {
                    symbol.classification = Some(Classification::SignatureChanged);
                    symbol.previous_signature = Some(base_symbol.signature.clone());
                }
            }
        }
    }

    base_symbols
        .iter()
        .filter(|base_symbol| {
            let identity = (base_symbol.name.as_str(), base_symbol.container.as_deref());
            !matched_base_identities.contains(&identity)
        })
        .filter(|base_symbol| overlaps_any(base_symbol.range, old_changed_ranges))
        .map(|base_symbol| RemovedSymbol {
            name: base_symbol.name.clone(),
            kind: base_symbol.kind,
            path: path.to_string(),
            signature: base_symbol.signature.clone(),
        })
        .collect()
}

/// Parses `source`, runs `lang`'s `definition_query` to find every
/// `@definition` node, and hands the resulting nodes (plus the source
/// bytes they borrow from, and a compiled `reference_query`) to `f`. Node
/// values borrow from the parsed tree, so this scoped-callback shape —
/// rather than returning `Vec<Node>` directly — keeps the tree alive
/// exactly as long as needed without leaking it or threading a `Tree`
/// value out through every caller. Shared by `extract_changed_symbols`
/// and `extract_all_symbols`, which differ only in how they filter/use
/// the node list.
///
/// `reference_query` is compiled once here (file granularity) rather than
/// once per definition node: `Query::new` takes ~1ms, and a repo-wide
/// index (`deps::TagsResolver::new`) calls into this path once per file
/// but `build_symbol` used to be called once per *definition*, so
/// compiling inside `build_symbol` multiplied that cost by the file's
/// definition count — measured as several seconds of pure recompilation
/// overhead on a mid-sized repo (see the `--deps` performance note in
/// `deps.rs`).
fn with_definition_nodes<T>(
    source: &str,
    lang: &dyn LanguageSupport,
    f: impl FnOnce(&[tree_sitter::Node], &[u8], &tree_sitter::Query) -> T,
) -> T {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar())
        .expect("LanguageSupport grammar must be loadable by tree-sitter");
    let tree = parser
        .parse(source, None)
        .expect("parsing a source string always produces a tree");

    let query = tree_sitter::Query::new(&lang.grammar(), lang.definition_query())
        .expect("LanguageSupport definition query must be valid");
    let definition_capture_index = query
        .capture_index_for_name("definition")
        .expect("definition query must have a @definition capture");
    let reference_query = tree_sitter::Query::new(&lang.grammar(), lang.reference_query())
        .expect("LanguageSupport reference query must be valid");

    let mut cursor = tree_sitter::QueryCursor::new();
    let source_bytes = source.as_bytes();
    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);

    let mut nodes = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if capture.index == definition_capture_index {
                nodes.push(capture.node);
            }
        }
    }
    f(&nodes, source_bytes, &reference_query)
}

/// Whether `node` is strictly nested inside `ancestor` in the syntax tree.
fn is_descendant_of(node: tree_sitter::Node, ancestor: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent == ancestor {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Converts a tree-sitter node's byte-oriented row span into a 1-based
/// inclusive [`LineRange`], matching the convention `diff::parse_unified_diff`
/// uses for new-side line numbers.
fn node_to_line_range(node: tree_sitter::Node) -> LineRange {
    LineRange {
        start: node.start_position().row + 1,
        end: node.end_position().row + 1,
    }
}

/// Whether `range` shares at least one line with any range in `others`.
fn overlaps_any(range: LineRange, others: &[LineRange]) -> bool {
    others
        .iter()
        .any(|other| range.start <= other.end && other.start <= range.end)
}

/// Builds an [`ExtractedSymbol`] from a captured definition node, or
/// `None` if the node kind isn't one this module knows how to report
/// (defensive default for query/grammar drift, not expected in practice
/// given `definition_query` only captures known kinds).
fn build_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    reference_query: &tree_sitter::Query,
    lang: &dyn LanguageSupport,
) -> Option<ExtractedSymbol> {
    let kind = symbol_kind(node)?;
    let name = definition_name(node, source)?;
    let signature = slice_signature(node, source);
    let container = find_container(node, source);
    let referenced_names = collect_referenced_names(node, source, reference_query);
    let is_test = lang.is_test_definition(node, source);

    Some(ExtractedSymbol {
        // Populated later by `graph::build_graph`, once node IDs are
        // assigned across the whole diff (see the field's doc comment).
        id: String::new(),
        name,
        kind,
        signature,
        range: node_to_line_range(node),
        container,
        referenced_names,
        // Populated later by `deps::resolve_dependencies`, once the full
        // set of a file's extracted symbols is known (needed to exclude
        // diff-internal symbols from the resolved dependency list).
        dependencies: Vec::new(),
        omitted_dependency_matches: 0,
        is_test,
        // Populated later by `pipeline::classify_symbols`, which needs the
        // base-side content this function has no access to.
        classification: None,
        previous_signature: None,
    })
}

/// Runs `reference_query` (already compiled by `with_definition_nodes`,
/// once per file rather than once per definition) over the subtree rooted
/// at `node`, returning the deduplicated names it captures (called
/// function/method names, referenced type names). Sorted for determinism
/// — tree-sitter's match order is not a meaningful signal here, and
/// downstream consumers (`deps.rs`, rendering) benefit from a stable
/// order.
///
/// Reads every capture whose name starts with `reference.` (see the doc
/// comment on [`LanguageSupport::reference_query`]) rather than a single
/// named capture, since each language's query alternation captures a
/// different sub-node depending on which branch matched (the callee
/// identifier for a call, the identifier itself for a type reference).
///
/// `_` and single-character identifiers are dropped before insertion
/// (`is_noise_name`): they are near-universal across unrelated files
/// (Python/Go's conventional throwaway `_`, one-letter loop/receiver
/// variables reused as call targets like `x()`), so a name-only resolver
/// (ADR 0003) matches them against dozens of unrelated definitions instead
/// of the one actually referenced — pure noise in the "Depends on" output
/// rather than a useful, if imprecise, match.
fn collect_referenced_names(
    node: tree_sitter::Node,
    source: &[u8],
    reference_query: &tree_sitter::Query,
) -> Vec<String> {
    let reference_capture_indices: std::collections::HashSet<u32> = reference_query
        .capture_names()
        .iter()
        .enumerate()
        .filter(|(_, name)| name.starts_with("reference."))
        .map(|(index, _)| index as u32)
        .collect();

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(reference_query, node, source);

    let mut names = std::collections::BTreeSet::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if !reference_capture_indices.contains(&capture.index) {
                continue;
            }
            if let Ok(text) = capture.node.utf8_text(source)
                && !is_noise_name(text)
            {
                names.insert(text.to_string());
            }
        }
    }

    names.into_iter().collect()
}

/// Whether `name` is too generic to be worth resolving: the bare `_`
/// placeholder, or any single-character identifier. Both appear constantly
/// across unrelated definitions in most codebases, so under v1's name-only
/// resolution (ADR 0003) they produce many spurious matches rather than
/// useful ones — see `collect_referenced_names`'s doc comment.
fn is_noise_name(name: &str) -> bool {
    name.chars().count() <= 1
}

/// Maps a captured definition node to a language-neutral [`SymbolKind`].
/// Node kind strings are unique across the grammars this module supports
/// (Rust, Go, Python, TypeScript/TSX), so a single flat match is sufficient
/// without needing to know which `LanguageSupport` a node came from.
///
/// Takes the node rather than just its kind string because Go's
/// `type_spec` needs to inspect its `type` field to tell a struct from an
/// interface — the definition query captures `type_spec` for both (see
/// `language/go.rs`), so the node kind alone is ambiguous for Go.
fn symbol_kind(node: tree_sitter::Node) -> Option<SymbolKind> {
    match node.kind() {
        // Rust.
        "function_item" | "function_signature_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        // Go.
        "type_spec" => match node.child_by_field_name("type")?.kind() {
            "struct_type" => Some(SymbolKind::Struct),
            "interface_type" => Some(SymbolKind::Interface),
            _ => None,
        },
        "function_declaration" => Some(SymbolKind::Function),
        "method_declaration" => Some(SymbolKind::Function),
        // Python.
        "class_definition" => Some(SymbolKind::Class),
        "function_definition" => Some(SymbolKind::Function),
        // TypeScript.
        "interface_declaration" => Some(SymbolKind::Interface),
        "type_alias_declaration" => Some(SymbolKind::TypeAlias),
        "class_declaration" | "abstract_class_declaration" => Some(SymbolKind::Class),
        "method_definition" | "abstract_method_signature" => Some(SymbolKind::Function),
        "enum_declaration" => Some(SymbolKind::Enum),
        // `variable_declarator` is captured only for `const f = () => {}`
        // style arrow-function bindings (see the TypeScript definition
        // query); other declarators are never captured.
        "variable_declarator" => Some(SymbolKind::Function),
        _ => None,
    }
}

/// Extracts a definition's declared name.
///
/// Most kinds expose their name through a `name` field
/// (`type_identifier`/`identifier`/`field_identifier`/...), which is
/// uniform across all grammars this module supports. `type_spec` (Go) is
/// the only kind that needs special handling: it is technically named via
/// its own `name` field too, so the generic path already covers it — kept
/// as a fallthrough rather than a special case.
fn definition_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Slices a definition's signature: the declaration text with implementation
/// detail and comment nodes removed, whitespace normalized to single spaces.
///
/// - `function_item`, `function_declaration` (Go/TS), `method_declaration`
///   (Go), `function_definition` (Python), `method_definition` (TS),
///   `variable_declarator` (TS arrow function): body stripped, only the
///   declaration up to (not including) the body is kept.
/// - `struct_item`, `enum_item`, `trait_item`, `type_spec` (Go),
///   `interface_declaration`, `type_alias_declaration`, `enum_declaration`
///   (TS), `abstract_method_signature` (TS): no separate "body" in the
///   implementation sense — their fields/variants/method signatures *are*
///   the API surface — so the whole node text is kept.
/// - `class_definition` (Python), `class_declaration` /
///   `abstract_class_declaration` (TS): the whole class text is kept
///   (field/method signatures are the API surface, same as
///   struct/interface), but nested method *bodies* — including a class
///   field whose value is an arrow function, e.g. `area = (): number => {
///   ... }` — are stripped so a class reads as a list of member signatures
///   rather than full implementations. A per-method signature listing
///   (rather than "whole class minus method bodies") would be more precise
///   but adds real complexity — e.g. reconciling which subset of members to
///   show when only one changed — that v1 defers; see the module-level
///   rationale in `language/python.rs` and `language/typescript.rs`.
///
/// Comment nodes (`line_comment`/`block_comment` in Rust, `comment` in
/// Go/Python/TypeScript — see [`is_comment_node`]) inside the kept range are
/// stripped in every case (ADR 0014): otherwise a comment-only edit inside a
/// struct/interface/class body would change the reported signature string,
/// making a `body_only`/`signature_changed` classification based on
/// signature-string equality fire incorrectly. This is a pre-1.0 output
/// change sanctioned by the ADR — some previously-reported signature strings
/// that contained inline comments now omit them.
fn slice_signature(node: tree_sitter::Node, source: &[u8]) -> String {
    if matches!(
        node.kind(),
        "class_definition" | "class_declaration" | "abstract_class_declaration"
    ) {
        let mut removed_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        collect_method_body_ranges(node, &mut removed_ranges);
        collect_comment_ranges(node, &removed_ranges.clone(), &mut removed_ranges);
        return normalize_whitespace(&text_with_ranges_removed(node, source, removed_ranges));
    }

    // `variable_declarator`'s body (a TS arrow function's `{ ... }`) is
    // nested one level deeper, under its `value` field, rather than being a
    // direct `body` field of the captured node itself.
    let body = if node.kind() == "variable_declarator" {
        node.child_by_field_name("value")
            .and_then(|value| value.child_by_field_name("body"))
    } else if matches!(
        node.kind(),
        "function_item"
            | "function_declaration"
            | "method_declaration"
            | "function_definition"
            | "method_definition"
    ) {
        node.child_by_field_name("body")
    } else {
        None
    };

    let text_end = body
        .map(|body| body.start_byte())
        .unwrap_or(node.end_byte());
    let mut comment_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    collect_comment_ranges(node, &[], &mut comment_ranges);
    // Comments at/after `text_end` fall inside the body, which is dropped
    // wholesale below anyway — only ones inside the kept declaration prefix
    // need to be individually removed.
    comment_ranges.retain(|range| range.start < text_end);

    let mut removed_ranges = comment_ranges;
    if let Some(body) = body {
        removed_ranges.push(body.start_byte()..node.end_byte());
    }

    let raw = text_with_ranges_removed(node, source, removed_ranges);
    normalize_whitespace(&raw)
}

/// Removes every byte range in `ranges` from `node`'s own text (`node`'s
/// full span, not just the declaration prefix — callers that only want a
/// prefix pre-truncate `ranges` to stop at that boundary), returning the
/// remainder as a `String`. Ranges are sorted and removed front-to-back,
/// advancing a `cursor` past each removed range in turn, so earlier
/// removals naturally narrow what later ones can still remove; if a range
/// starts before the current `cursor` (overlapping ranges, defensively not
/// expected in practice) *that one range's removal* is skipped — its own
/// iteration does nothing and `cursor` is left wherever the previous
/// iteration advanced it to — rather than the whole function panicking on
/// an invalid slice.
fn text_with_ranges_removed(
    node: tree_sitter::Node,
    source: &[u8],
    mut ranges: Vec<std::ops::Range<usize>>,
) -> String {
    ranges.sort_by_key(|r| r.start);

    let mut result = Vec::with_capacity(source.len());
    let mut cursor = node.start_byte();
    for range in &ranges {
        if range.start < cursor {
            continue; // Defensive: overlapping ranges should not occur.
        }
        result.extend_from_slice(&source[cursor..range.start.min(node.end_byte())]);
        cursor = range.end.max(cursor);
    }
    result.extend_from_slice(&source[cursor.min(node.end_byte())..node.end_byte()]);
    String::from_utf8(result).unwrap_or_default()
}

/// Recursively collects the byte ranges of every nested method body inside
/// a class node (`function_definition`/`method_definition`, or a TS class
/// field whose value is an arrow function, e.g. `area = (): number => {
/// ... }`), without descending into a method's own body (a nested function
/// *inside* a method body is implementation detail, not a member
/// signature).
///
/// `public_field_definition`'s body is nested one level deeper, under its
/// `value` field's own `body`, rather than being a direct `body` field of
/// the field definition itself — same shape as `variable_declarator` in
/// `slice_signature`.
fn collect_method_body_ranges(node: tree_sitter::Node, ranges: &mut Vec<std::ops::Range<usize>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let is_method = matches!(child.kind(), "function_definition" | "method_definition");
        if is_method && let Some(body) = child.child_by_field_name("body") {
            ranges.push(body.start_byte()..child.end_byte());
            continue; // Don't descend into the stripped body.
        }
        if child.kind() == "public_field_definition"
            && let Some(value) = child.child_by_field_name("value")
            && value.kind() == "arrow_function"
            && let Some(body) = value.child_by_field_name("body")
        {
            ranges.push(body.start_byte()..child.end_byte());
            continue; // Don't descend into the stripped body.
        }
        collect_method_body_ranges(child, ranges);
    }
}

/// Recursively collects the byte ranges of every comment node
/// ([`is_comment_node`]) inside `node`, skipping any range already covered
/// by `already_removed` (e.g. a method body `collect_method_body_ranges`
/// already sliced out) so a comment nested inside an already-removed range
/// isn't redundantly appended a second time — `text_with_ranges_removed`
/// would still handle an overlapping range correctly (it clamps against
/// `cursor`), but skipping it here keeps `ranges` a non-overlapping set,
/// matching that function's stated "not expected in practice" assumption.
fn collect_comment_ranges(
    node: tree_sitter::Node,
    already_removed: &[std::ops::Range<usize>],
    ranges: &mut Vec<std::ops::Range<usize>>,
) {
    if is_comment_node(node) {
        ranges.push(node.start_byte()..node.end_byte());
        return; // A comment node has no children worth descending into.
    }
    if already_removed
        .iter()
        .any(|r| r.start <= node.start_byte() && node.end_byte() <= r.end)
    {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comment_ranges(child, already_removed, ranges);
    }
}

/// Whether `node` is a tree-sitter comment node under any of the four
/// grammars this module supports: Rust splits line/block comments into two
/// distinct kinds (`line_comment`, `block_comment`); Go, Python, and
/// TypeScript each use a single `comment` kind for both forms (verified
/// against each grammar directly — Go's grammar has no such split and
/// Python/TypeScript comments are line-oriented `#`/`//`/`/* */` all
/// captured under the same node kind).
fn is_comment_node(node: tree_sitter::Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment" | "comment")
}

/// Collapses runs of whitespace (including newlines/indentation from the
/// original source) into single spaces, and trims the result.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Walks up from `node` to find an enclosing container (Rust
/// `impl_item`/`trait_item`, Go method receiver type, Python/TypeScript
/// `class_definition`/`class_declaration`), returning a descriptive
/// container name (e.g. `"impl Foo"`, `"trait Bar"`, `"Repo"`, `"class
/// Point"`). Returns `None` for top-level definitions.
///
/// Go is handled differently from the rest: a `method_declaration` is never
/// nested inside its receiver type's node (see `is_container_only_node`),
/// so its container is read directly off its own `receiver` field rather
/// than by walking ancestors.
fn find_container(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    if node.kind() == "method_declaration" {
        return go_receiver_type_name(node, source);
    }

    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "impl_item" => {
                let type_name = candidate
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source).ok())?;
                return Some(format!("impl {type_name}"));
            }
            "trait_item" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("trait {name}"));
            }
            "class_definition" | "class_declaration" | "abstract_class_declaration" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("class {name}"));
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

/// Extracts the receiver type name from a Go `method_declaration`'s
/// `receiver` field (a `parameter_list` containing one
/// `parameter_declaration`), stripping the leading `*` for pointer
/// receivers so `func (r *Repo) Save(...)` and `func (r Repo) Save(...)`
/// both report container `"Repo"`.
fn go_receiver_type_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    let param = receiver
        .named_children(&mut cursor)
        .find(|c| c.kind() == "parameter_declaration")?;
    let type_node = param.child_by_field_name("type")?;
    let type_text = type_node.utf8_text(source).ok()?;
    Some(type_text.trim_start_matches('*').to_string())
}

#[cfg(test)]
#[path = "extract_tests/mod.rs"]
mod tests;
