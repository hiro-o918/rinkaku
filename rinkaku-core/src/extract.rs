//! Tree-sitter based signature extraction.
//!
//! Given a source file's text and the line ranges that changed in a diff
//! (see [`crate::diff::LineRange`]), finds the definitions that contain
//! those changed lines and slices out their signatures — the API surface,
//! without the implementation body.

use crate::diff::LineRange;
use crate::language::LanguageSupport;
use serde::Serialize;
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

/// A definition whose signature was extracted because one of its lines
/// (declaration or body) fell inside a changed range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractedSymbol {
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
            .filter_map(|node| build_symbol(*node, source_bytes, reference_query))
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
            .filter_map(|node| build_symbol(*node, source_bytes, reference_query))
            .collect()
    })
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
) -> Option<ExtractedSymbol> {
    let kind = symbol_kind(node)?;
    let name = definition_name(node, source)?;
    let signature = slice_signature(node, source);
    let container = find_container(node, source);
    let referenced_names = collect_referenced_names(node, source, reference_query);

    Some(ExtractedSymbol {
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
/// detail removed and internal whitespace normalized to single spaces.
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
fn slice_signature(node: tree_sitter::Node, source: &[u8]) -> String {
    if matches!(
        node.kind(),
        "class_definition" | "class_declaration" | "abstract_class_declaration"
    ) {
        return normalize_whitespace(&class_signature_text(node, source));
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

    let text_range = body
        .map(|body| node.start_byte()..body.start_byte())
        .unwrap_or(node.start_byte()..node.end_byte());
    let raw = std::str::from_utf8(&source[text_range]).unwrap_or("");
    normalize_whitespace(raw)
}

/// Builds a class/class-declaration's signature text: the full node text
/// with every nested method body's byte range removed, leaving member
/// signatures (fields, method declarations) intact. Byte ranges are
/// collected first and removed back-to-front so earlier removals don't
/// shift the offsets of ones still pending.
fn class_signature_text(node: tree_sitter::Node, source: &[u8]) -> String {
    let mut body_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    collect_method_body_ranges(node, &mut body_ranges);
    body_ranges.sort_by_key(|r| r.start);

    let mut result = Vec::with_capacity(source.len());
    let mut cursor = node.start_byte();
    for range in &body_ranges {
        if range.start < cursor {
            continue; // Defensive: overlapping ranges should not occur.
        }
        result.extend_from_slice(&source[cursor..range.start]);
        cursor = range.end;
    }
    result.extend_from_slice(&source[cursor..node.end_byte()]);
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
mod tests {
    use super::*;
    use crate::language::rust::RustSupport;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_return_empty_vec_when_changed_ranges_is_empty() {
        let source = "fn foo() {}\n";
        let lang = RustSupport;

        let expected: Vec<ExtractedSymbol> = Vec::new();
        let actual = extract_changed_symbols(source, &lang, &[]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_every_definition_regardless_of_changed_ranges() {
        let source = "\
fn helper(x: i32) -> i32 {
    x
}

struct Point {
    x: i32,
}
";
        let lang = RustSupport;

        let expected = vec![
            ExtractedSymbol {
                name: "helper".to_string(),
                kind: SymbolKind::Function,
                signature: "fn helper(x: i32) -> i32".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            },
            ExtractedSymbol {
                name: "Point".to_string(),
                kind: SymbolKind::Struct,
                signature: "struct Point { x: i32, }".to_string(),
                range: LineRange { start: 5, end: 7 },
                container: None,
                referenced_names: vec!["Point".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            },
        ];
        let actual = extract_all_symbols(source, &lang);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_exclude_underscore_and_single_char_identifiers_from_referenced_names() {
        let source = "\
fn foo() -> i32 {
    let _ = bar();
    let a = 1;
    x(a)
}
";
        let lang = RustSupport;
        let changed_ranges = vec![LineRange { start: 2, end: 2 }];

        // `bar` and `x` are real call targets (length > 1, not `_`), kept.
        // A bare `_` is never captured as a `call_expression` callee by
        // Rust's grammar, so this test instead exercises the general
        // filter shared by every language's `collect_referenced_names`
        // call site, which must drop both `_` and any single-character
        // identifier (e.g. Python/TS's common but never-informative `x`,
        // `_` local names) as noise unlikely to resolve to a meaningful,
        // uniquely named definition.
        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo() -> i32".to_string(),
            range: LineRange { start: 1, end: 5 },
            container: None,
            referenced_names: vec!["bar".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_vec_when_source_has_no_definitions() {
        let source = "const X: i32 = 1;\n";
        let lang = RustSupport;

        let expected: Vec<ExtractedSymbol> = Vec::new();
        let actual = extract_all_symbols(source, &lang);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_function_signature_when_body_line_changed() {
        let source = "\
fn foo(a: i32) -> i32 {
    let b = a + 1;
    b
}
";
        let lang = RustSupport;
        // Line 2 (`let b = a + 1;`) is inside the body only.
        let changed_ranges = vec![LineRange { start: 2, end: 2 }];

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 4 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_function_signature_when_signature_line_changed() {
        let source = "\
fn foo(a: i32, c: i32) -> i32 {
    a + c
}
";
        let lang = RustSupport;
        // Line 1 is the signature line itself.
        let changed_ranges = vec![LineRange { start: 1, end: 1 }];

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32, c: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_full_struct_signature_when_field_changed() {
        let source = "\
struct Point {
    x: i32,
    y: i32,
}
";
        let lang = RustSupport;
        // Line 3 (`y: i32,`) is a field, not a separate body.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected = vec![ExtractedSymbol {
            name: "Point".to_string(),
            kind: SymbolKind::Struct,
            signature: "struct Point { x: i32, y: i32, }".to_string(),
            range: LineRange { start: 1, end: 4 },
            container: None,
            // The struct's own name appears as a `type_identifier` too
            // (it is the definition's declared name), so it is captured
            // as a reference the same as any other type mention. `deps.rs`
            // filters self-references before resolving.
            referenced_names: vec!["Point".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_container_when_method_inside_impl_block_changed() {
        let source = "\
struct Foo;

impl Foo {
    fn bar(&self) -> i32 {
        42
    }
}
";
        let lang = RustSupport;
        // Line 5 (`42`) is inside `bar`'s body.
        let changed_ranges = vec![LineRange { start: 5, end: 5 }];

        let expected = vec![ExtractedSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::Function,
            signature: "fn bar(&self) -> i32".to_string(),
            range: LineRange { start: 4, end: 6 },
            container: Some("impl Foo".to_string()),
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_container_when_method_signature_inside_impl_block_changed() {
        let source = "\
struct Foo;

impl Foo {
    fn bar(&self, extra: i32) -> i32 {
        extra
    }
}
";
        let lang = RustSupport;
        // Line 4 is the method's own signature line, not its body.
        let changed_ranges = vec![LineRange { start: 4, end: 4 }];

        let expected = vec![ExtractedSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::Function,
            signature: "fn bar(&self, extra: i32) -> i32".to_string(),
            range: LineRange { start: 4, end: 6 },
            container: Some("impl Foo".to_string()),
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_full_enum_signature_when_variant_changed() {
        let source = "\
enum Color {
    Red,
    Green,
    Blue,
}
";
        let lang = RustSupport;
        // Line 3 (`Green,`) is one variant among several.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected = vec![ExtractedSymbol {
            name: "Color".to_string(),
            kind: SymbolKind::Enum,
            signature: "enum Color { Red, Green, Blue, }".to_string(),
            range: LineRange { start: 1, end: 5 },
            container: None,
            // Same self-reference note as the struct case above: the
            // enum's own name is a `type_identifier`.
            referenced_names: vec!["Color".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_method_signature_with_trait_container_when_method_declaration_changed() {
        let source = "\
trait Greeter {
    fn greet(&self) -> String;
}
";
        let lang = RustSupport;
        let changed_ranges = vec![LineRange { start: 2, end: 2 }];

        // The changed line is fully inside `fn greet(...)`'s own range, so
        // that method signature is reported (with its trait as container)
        // rather than the whole trait body — same "narrowest enclosing
        // definition" rule used for impl methods.
        let expected = vec![ExtractedSymbol {
            name: "greet".to_string(),
            kind: SymbolKind::Function,
            signature: "fn greet(&self) -> String;".to_string(),
            range: LineRange { start: 2, end: 2 },
            container: Some("trait Greeter".to_string()),
            referenced_names: vec!["String".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_trait_signature_when_no_method_line_specifically_changed() {
        let source = "\
trait Greeter {
    fn greet(&self) -> String;
}
";
        let lang = RustSupport;
        // Line 1 (`trait Greeter {`) belongs to the trait node but not to
        // any single method signature inside it.
        let changed_ranges = vec![LineRange { start: 1, end: 1 }];

        let expected = vec![ExtractedSymbol {
            name: "Greeter".to_string(),
            kind: SymbolKind::Trait,
            signature: "trait Greeter { fn greet(&self) -> String; }".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            // The trait's own name plus the referenced `String` return
            // type of its method signature.
            referenced_names: vec!["Greeter".to_string(), "String".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
        let source = "\
fn foo() {}

const X: i32 = 1;
";
        let lang = RustSupport;
        // Line 3 is a top-level const item, not covered by definition_query.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected: Vec<ExtractedSymbol> = Vec::new();
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_extract_only_the_touched_function_when_two_functions_exist(
        vec![LineRange { start: 2, end: 2 }],
        vec![ExtractedSymbol {
            name: "first".to_string(),
            kind: SymbolKind::Function,
            signature: "fn first()".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }],
    )]
    fn extract_changed_symbols_selective_cases(
        #[case] changed_ranges: Vec<LineRange>,
        #[case] expected: Vec<ExtractedSymbol>,
    ) {
        let source = "\
fn first() {
    1
}

fn second() {
    2
}
";
        let lang = RustSupport;

        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_rust_file() {
        use crate::diff::parse_unified_diff;
        use crate::language::language_for_path;

        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
        let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
        let changed_file = parse_unified_diff(diff)
            .expect("diff should parse")
            .into_iter()
            .next()
            .expect("diff should contain one changed file");
        let lang = language_for_path(&changed_file.path).expect("*.rs should resolve to Rust");

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
        }];
        let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_file_end_to_end_when_extension_is_unsupported() {
        use crate::language::language_for_path;

        // Registry lookup, not extraction: an unsupported extension means
        // the pipeline never reaches `extract_changed_symbols` for this
        // file — there is no `LanguageSupport` to pass it.
        let actual = language_for_path("src/notes.txt");

        assert!(actual.is_none());
    }

    mod go {
        use super::*;
        use crate::language::go::GoSupport;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_extract_function_signature_when_body_line_changed() {
            let source = "\
package main

func foo(a int) int {
	b := a + 1
	return b
}
";
            let lang = GoSupport;
            // Line 4 (`b := a + 1`) is inside the body only.
            let changed_ranges = vec![LineRange { start: 4, end: 4 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "func foo(a int) int".to_string(),
                range: LineRange { start: 3, end: 6 },
                container: None,
                // Go has no distinct node kind for built-in types: `int`
                // parses as `type_identifier`, same as a user-defined
                // type, and is captured the same way (see the doc comment
                // on `REFERENCE_QUERY` in language/go.rs).
                referenced_names: vec!["int".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_function_signature_when_signature_line_changed() {
            let source = "\
package main

func foo(a int, c int) int {
	return a + c
}
";
            let lang = GoSupport;
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "func foo(a int, c int) int".to_string(),
                range: LineRange { start: 3, end: 5 },
                container: None,
                referenced_names: vec!["int".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_full_struct_signature_when_field_changed() {
            let source = "\
package main

type Repo struct {
	Name string
	Size int
}
";
            let lang = GoSupport;
            // Line 5 (`Size int`) is a field, not a separate body.
            let changed_ranges = vec![LineRange { start: 5, end: 5 }];

            let expected = vec![ExtractedSymbol {
                name: "Repo".to_string(),
                kind: SymbolKind::Struct,
                signature: "Repo struct { Name string Size int }".to_string(),
                range: LineRange { start: 3, end: 6 },
                container: None,
                // "Repo" is the struct's own name (self-reference,
                // filtered later by deps.rs); "string"/"int" are field
                // types, built-in but syntactically indistinguishable
                // from user types in Go (see REFERENCE_QUERY's doc
                // comment).
                referenced_names: vec!["Repo".to_string(), "int".to_string(), "string".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_full_interface_signature_when_method_elem_changed() {
            let source = "\
package main

type Fetcher interface {
	Fetch(id string) (string, error)
}
";
            let lang = GoSupport;
            let changed_ranges = vec![LineRange { start: 4, end: 4 }];

            let expected = vec![ExtractedSymbol {
                name: "Fetcher".to_string(),
                kind: SymbolKind::Interface,
                signature: "Fetcher interface { Fetch(id string) (string, error) }".to_string(),
                range: LineRange { start: 3, end: 5 },
                container: None,
                referenced_names: vec![
                    "Fetcher".to_string(),
                    "error".to_string(),
                    "string".to_string(),
                ],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_not_report_type_alias_as_a_symbol() {
            let source = "\
package main

type Alias = string

func useAlias(a Alias) Alias {
	return a
}
";
            let lang = GoSupport;
            // Line 3 is the plain type alias declaration.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_receiver_type_when_pointer_receiver_method_body_changed() {
            let source = "\
package main

type Repo struct {
	Name string
}

func (r *Repo) Save(id string) error {
	return nil
}
";
            let lang = GoSupport;
            // Line 8 (`return nil`) is inside `Save`'s body.
            let changed_ranges = vec![LineRange { start: 8, end: 8 }];

            let expected = vec![ExtractedSymbol {
                name: "Save".to_string(),
                kind: SymbolKind::Function,
                signature: "func (r *Repo) Save(id string) error".to_string(),
                range: LineRange { start: 7, end: 9 },
                container: Some("Repo".to_string()),
                // "Repo" comes from the pointer receiver's type
                // (`*Repo`); the `*` prefix is not part of the
                // `type_identifier` node, so the reference query captures
                // the bare type name.
                referenced_names: vec![
                    "Repo".to_string(),
                    "error".to_string(),
                    "string".to_string(),
                ],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_receiver_type_when_value_receiver_method_signature_changed() {
            let source = "\
package main

type Repo struct {
	Name string
}

func (r Repo) Label() string {
	return r.Name
}
";
            let lang = GoSupport;
            // Line 7 is the method's own signature line.
            let changed_ranges = vec![LineRange { start: 7, end: 7 }];

            let expected = vec![ExtractedSymbol {
                name: "Label".to_string(),
                kind: SymbolKind::Function,
                signature: "func (r Repo) Label() string".to_string(),
                range: LineRange { start: 7, end: 9 },
                container: Some("Repo".to_string()),
                referenced_names: vec!["Repo".to_string(), "string".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
            let source = "\
package main

func foo() {}

var x = 1
";
            let lang = GoSupport;
            // Line 5 is a top-level var declaration, not covered by
            // definition_query.
            let changed_ranges = vec![LineRange { start: 5, end: 5 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_go_file() {
            use crate::diff::parse_unified_diff;
            use crate::language::language_for_path;

            let diff = "\
diff --git a/repo.go b/repo.go
index e69de29..4b825dc 100644
--- a/repo.go
+++ b/repo.go
@@ -6,3 +6,3 @@
 func (r *Repo) Save(id string) error {
-	return errors.New(\"not implemented\")
+	return nil
 }
";
            let source = "\
package main

type Repo struct {
	Name string
}

func (r *Repo) Save(id string) error {
	return nil
}
";
            let changed_file = parse_unified_diff(diff)
                .expect("diff should parse")
                .into_iter()
                .next()
                .expect("diff should contain one changed file");
            let lang = language_for_path(&changed_file.path).expect("*.go should resolve to Go");

            let expected = vec![ExtractedSymbol {
                name: "Save".to_string(),
                kind: SymbolKind::Function,
                signature: "func (r *Repo) Save(id string) error".to_string(),
                range: LineRange { start: 7, end: 9 },
                container: Some("Repo".to_string()),
                referenced_names: vec![
                    "Repo".to_string(),
                    "error".to_string(),
                    "string".to_string(),
                ],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

            assert_eq!(expected, actual);
        }
    }

    mod python {
        use super::*;
        use crate::language::python::PythonSupport;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_extract_function_signature_when_body_line_changed() {
            let source = "\
def foo(a):
    b = a + 1
    return b
";
            let lang = PythonSupport;
            // Line 2 (`b = a + 1`) is inside the body only.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "def foo(a):".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_function_signature_when_signature_line_changed() {
            let source = "\
def foo(a, c):
    return a + c
";
            let lang = PythonSupport;
            let changed_ranges = vec![LineRange { start: 1, end: 1 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "def foo(a, c):".to_string(),
                range: LineRange { start: 1, end: 2 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_only_the_inner_function_when_nested_function_body_changed() {
            let source = "\
def top_level(a, b):
    def inner(c):
        return c + 1
    return inner(a) + b
";
            let lang = PythonSupport;
            // Line 3 (`return c + 1`) is inside `inner`'s body only.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            // A nested function is reported like any other function, with
            // no container: its nearest ancestor definition is another
            // `function_definition`, not a class, so `find_container`
            // walks past it and finds nothing (see extract.rs doc comment
            // on `find_container`).
            let expected = vec![ExtractedSymbol {
                name: "inner".to_string(),
                kind: SymbolKind::Function,
                signature: "def inner(c):".to_string(),
                range: LineRange { start: 2, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_not_detect_change_when_only_decorator_line_changed() {
            let source = "\
@decorator_v2
def decorated(a):
    return a
";
            let lang = PythonSupport;
            // Line 1 is the decorator, outside `function_definition`'s own
            // row range (see the doc comment on `DEFINITION_QUERY` in
            // language/python.rs) — a deliberate v1 simplification.
            let changed_ranges = vec![LineRange { start: 1, end: 1 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_decorated_function_signature_when_body_changed() {
            let source = "\
@decorator
def decorated(a):
    return a
";
            let lang = PythonSupport;
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "decorated".to_string(),
                kind: SymbolKind::Function,
                signature: "def decorated(a):".to_string(),
                range: LineRange { start: 2, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_class_signature_with_method_bodies_stripped_when_field_changed() {
            let source = "\
class Point:
    x: int
    y: int

    def __init__(self, x, y):
        self.x = x
        self.y = y
";
            let lang = PythonSupport;
            // Line 3 (`y: int`) is a class-level field annotation, not
            // inside any method.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "Point".to_string(),
                kind: SymbolKind::Class,
                signature: "class Point: x: int y: int def __init__(self, x, y):".to_string(),
                range: LineRange { start: 1, end: 7 },
                container: None,
                // "int" is the shared field-annotation type of both `x`
                // and `y`, deduplicated to a single entry.
                referenced_names: vec!["int".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_class_name_when_method_body_changed() {
            let source = "\
class Point:
    def __init__(self, x):
        self.x = x
";
            let lang = PythonSupport;
            // Line 3 (`self.x = x`) is inside `__init__`'s body.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "__init__".to_string(),
                kind: SymbolKind::Function,
                signature: "def __init__(self, x):".to_string(),
                range: LineRange { start: 2, end: 3 },
                container: Some("class Point".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_class_name_when_method_signature_changed() {
            let source = "\
class Point:
    def __init__(self, x):
        self.x = x
";
            let lang = PythonSupport;
            // Line 2 is the method's own signature line.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "__init__".to_string(),
                kind: SymbolKind::Function,
                signature: "def __init__(self, x):".to_string(),
                range: LineRange { start: 2, end: 3 },
                container: Some("class Point".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_only_the_touched_method_when_class_has_two_methods() {
            let source = "\
class Point:
    def __init__(self, x):
        self.x = x

    def label(self):
        return str(self.x)
";
            let lang = PythonSupport;
            // Line 6 is inside `label`'s body only.
            let changed_ranges = vec![LineRange { start: 6, end: 6 }];

            let expected = vec![ExtractedSymbol {
                name: "label".to_string(),
                kind: SymbolKind::Function,
                signature: "def label(self):".to_string(),
                range: LineRange { start: 5, end: 6 },
                container: Some("class Point".to_string()),
                // `str(self.x)` is a call to the bare identifier `str`
                // (Python has no distinct built-in-type node kind, so
                // `str` is captured the same as any user-defined callable
                // — see REFERENCE_QUERY's doc comment in
                // language/python.rs).
                referenced_names: vec!["str".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
            let source = "\
def foo():
    pass

X = 1
";
            let lang = PythonSupport;
            // Line 4 is a top-level assignment, not covered by
            // definition_query.
            let changed_ranges = vec![LineRange { start: 4, end: 4 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_python_file() {
            use crate::diff::parse_unified_diff;
            use crate::language::language_for_path;

            let diff = "\
diff --git a/point.py b/point.py
index e69de29..4b825dc 100644
--- a/point.py
+++ b/point.py
@@ -2,2 +2,2 @@
     def __init__(self, x):
-        self.x = 0
+        self.x = x
";
            let source = "\
class Point:
    def __init__(self, x):
        self.x = x
";
            let changed_file = parse_unified_diff(diff)
                .expect("diff should parse")
                .into_iter()
                .next()
                .expect("diff should contain one changed file");
            let lang =
                language_for_path(&changed_file.path).expect("*.py should resolve to Python");

            let expected = vec![ExtractedSymbol {
                name: "__init__".to_string(),
                kind: SymbolKind::Function,
                signature: "def __init__(self, x):".to_string(),
                range: LineRange { start: 2, end: 3 },
                container: Some("class Point".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

            assert_eq!(expected, actual);
        }
    }

    mod typescript {
        use super::*;
        use crate::language::typescript::TypeScriptSupport;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_extract_function_signature_when_body_line_changed() {
            let source = "\
function foo(a: number): number {
    return a + 1;
}
";
            let lang = TypeScriptSupport;
            // Line 2 (`return a + 1;`) is inside the body only.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "function foo(a: number): number".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_function_signature_when_signature_line_changed() {
            let source = "\
function foo(a: number, c: number): number {
    return a + c;
}
";
            let lang = TypeScriptSupport;
            let changed_ranges = vec![LineRange { start: 1, end: 1 }];

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "function foo(a: number, c: number): number".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_arrow_function_signature_when_const_bound_body_changed() {
            let source = "\
const arrow = (a: number): number => {
    return a + 1;
};
";
            let lang = TypeScriptSupport;
            // Line 2 is inside the arrow function's body only.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "arrow".to_string(),
                kind: SymbolKind::Function,
                signature: "arrow = (a: number): number =>".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_not_report_plain_const_binding_as_a_symbol() {
            let source = "\
const notArrow = 5;

function useIt(): number {
    return notArrow;
}
";
            let lang = TypeScriptSupport;
            // Line 1 is a plain (non-arrow-function) const binding.
            let changed_ranges = vec![LineRange { start: 1, end: 1 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_full_interface_signature_when_method_signature_changed() {
            let source = "\
interface Shape {
    area(): number;
    perimeter(): number;
}
";
            let lang = TypeScriptSupport;
            // Line 3 (`perimeter(): number;`) is one member among several.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "Shape".to_string(),
                kind: SymbolKind::Interface,
                signature: "interface Shape { area(): number; perimeter(): number; }".to_string(),
                range: LineRange { start: 1, end: 4 },
                container: None,
                // The interface's own name is a `type_identifier` (self-
                // reference, filtered later by deps.rs); `number` is
                // TypeScript's built-in `predefined_type`, a distinct
                // node kind the reference query does not capture.
                referenced_names: vec!["Shape".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_full_type_alias_signature_when_member_changed() {
            let source = "\
type Point = {
    x: number;
    y: number;
};
";
            let lang = TypeScriptSupport;
            // Line 3 (`y: number;`) is one member of the object type.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "Point".to_string(),
                kind: SymbolKind::TypeAlias,
                signature: "type Point = { x: number; y: number; };".to_string(),
                range: LineRange { start: 1, end: 4 },
                container: None,
                referenced_names: vec!["Point".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_full_enum_signature_when_member_changed() {
            let source = "\
enum Color {
    Red,
    Green,
    Blue,
}
";
            let lang = TypeScriptSupport;
            // Line 3 (`Green,`) is one variant among several.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "Color".to_string(),
                kind: SymbolKind::Enum,
                signature: "enum Color { Red, Green, Blue, }".to_string(),
                range: LineRange { start: 1, end: 5 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_class_signature_with_method_bodies_stripped_when_field_changed() {
            let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
            let lang = TypeScriptSupport;
            // Line 2 (`radius: number;`) is a class-level field, not
            // inside any method.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "Circle".to_string(),
                kind: SymbolKind::Class,
                signature: "class Circle { radius: number; area(): number }".to_string(),
                range: LineRange { start: 1, end: 7 },
                container: None,
                referenced_names: vec!["Circle".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_class_name_when_method_body_changed() {
            let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
            let lang = TypeScriptSupport;
            // Line 5 is inside `area`'s body.
            let changed_ranges = vec![LineRange { start: 5, end: 5 }];

            let expected = vec![ExtractedSymbol {
                name: "area".to_string(),
                kind: SymbolKind::Function,
                signature: "area(): number".to_string(),
                range: LineRange { start: 4, end: 6 },
                container: Some("class Circle".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_set_container_to_class_name_when_method_signature_changed() {
            let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
            let lang = TypeScriptSupport;
            // Line 4 is the method's own signature line.
            let changed_ranges = vec![LineRange { start: 4, end: 4 }];

            let expected = vec![ExtractedSymbol {
                name: "area".to_string(),
                kind: SymbolKind::Function,
                signature: "area(): number".to_string(),
                range: LineRange { start: 4, end: 6 },
                container: Some("class Circle".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_only_the_touched_method_when_class_has_two_methods() {
            let source = "\
class Circle {
    constructor(radius: number) {
        this.radius = radius;
    }

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
            let lang = TypeScriptSupport;
            // Line 7 is inside `area`'s body only.
            let changed_ranges = vec![LineRange { start: 7, end: 7 }];

            let expected = vec![ExtractedSymbol {
                name: "area".to_string(),
                kind: SymbolKind::Function,
                signature: "area(): number".to_string(),
                range: LineRange { start: 6, end: 8 },
                container: Some("class Circle".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
            let source = "\
function foo(): void {}

const X: number = 1;
";
            let lang = TypeScriptSupport;
            // Line 3 is a top-level, non-arrow-function const binding, not
            // covered by definition_query.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected: Vec<ExtractedSymbol> = Vec::new();
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_typescript_file() {
            use crate::diff::parse_unified_diff;
            use crate::language::language_for_path;

            let diff = "\
diff --git a/shape.ts b/shape.ts
index e69de29..4b825dc 100644
--- a/shape.ts
+++ b/shape.ts
@@ -1,3 +1,3 @@
 function foo(a: number): number {
-    return a;
+    return a + 1;
 }
";
            let source = "\
function foo(a: number): number {
    return a + 1;
}
";
            let changed_file = parse_unified_diff(diff)
                .expect("diff should parse")
                .into_iter()
                .next()
                .expect("diff should contain one changed file");
            let lang =
                language_for_path(&changed_file.path).expect("*.ts should resolve to TypeScript");

            let expected = vec![ExtractedSymbol {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                signature: "function foo(a: number): number".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_abstract_method_signature_with_class_container_when_abstract_method_line_changed()
         {
            let source = "\
abstract class Shape {
    abstract area(): number;
    abstract perimeter(): number;
}
";
            let lang = TypeScriptSupport;
            // Line 3 (`abstract perimeter(): number;`) is fully inside that
            // method's own node range, so — same "narrowest enclosing
            // definition" rule as Rust trait methods — the method itself is
            // reported (with its class as container) rather than the whole
            // class body.
            let changed_ranges = vec![LineRange { start: 3, end: 3 }];

            let expected = vec![ExtractedSymbol {
                name: "perimeter".to_string(),
                kind: SymbolKind::Function,
                signature: "abstract perimeter(): number".to_string(),
                range: LineRange { start: 3, end: 3 },
                container: Some("class Shape".to_string()),
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_extract_abstract_class_signature_when_no_member_line_specifically_changed() {
            let source = "\
abstract class Shape {
    abstract area(): number;
    abstract perimeter(): number;
}
";
            let lang = TypeScriptSupport;
            // Line 1 (`abstract class Shape {`) belongs to the class node
            // but not to any single member signature inside it.
            let changed_ranges = vec![LineRange { start: 1, end: 1 }];

            let expected = vec![ExtractedSymbol {
                name: "Shape".to_string(),
                kind: SymbolKind::Class,
                signature:
                    "abstract class Shape { abstract area(): number; abstract perimeter(): number; }"
                        .to_string(),
                range: LineRange { start: 1, end: 4 },
                container: None,
                referenced_names: vec!["Shape".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_strip_arrow_function_body_when_class_field_arrow_function_signature_changed() {
            let source = "\
class Circle {
    radius: number;

    area = (): number => {
        return 3.14 * this.radius * this.radius;
    };
}
";
            let lang = TypeScriptSupport;
            // Line 2 (`radius: number;`) is a class-level field, not
            // inside the arrow function body — the extracted class
            // signature must still have the arrow function's body
            // stripped, matching how `method_definition` bodies are
            // stripped.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];

            let expected = vec![ExtractedSymbol {
                name: "Circle".to_string(),
                kind: SymbolKind::Class,
                signature: "class Circle { radius: number; area = (): number => ; }".to_string(),
                range: LineRange { start: 1, end: 7 },
                container: None,
                // The reference query runs over the full node (including
                // the arrow function's body, which is only stripped from
                // the rendered *signature* text, not from the tree
                // `collect_referenced_names` walks) but `this.radius` is
                // a member expression, not a bare identifier, so it is
                // not captured; only the class's own self-reference is.
                referenced_names: vec!["Circle".to_string()],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, &lang, &changed_ranges);

            assert_eq!(expected, actual);
        }

        #[test]
        fn should_resolve_tsx_extension_end_to_end_and_extract_arrow_component() {
            use crate::language::language_for_path;

            let source = "\
const Component = () => {
    return 1;
};
";
            // Line 2 is inside the arrow function's body only.
            let changed_ranges = vec![LineRange { start: 2, end: 2 }];
            let lang = language_for_path("src/Component.tsx").expect("*.tsx should resolve to TSX");

            let expected = vec![ExtractedSymbol {
                name: "Component".to_string(),
                kind: SymbolKind::Function,
                signature: "Component = () =>".to_string(),
                range: LineRange { start: 1, end: 3 },
                container: None,
                referenced_names: vec![],
                dependencies: vec![],
                omitted_dependency_matches: 0,
            }];
            let actual = extract_changed_symbols(source, lang, &changed_ranges);

            assert_eq!(expected, actual);
        }
    }
}
