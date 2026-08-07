//! Reference-name collection: the identifiers a definition mentions,
//! gathered from the language's `reference_query` captures plus the
//! language-specific code walks (Rust macro bodies, ADR 0063; Rust
//! module-scoped calls, ADR 0064; HCL traversals, ADR 0066) that a
//! tree-sitter query cannot express. Split out of the module root along
//! this responsibility boundary when the walks pushed it over the warn
//! threshold (ADR 0028).

use tree_sitter::StreamingIterator;

/// The two reference-name sets [`collect_referenced_names`] produces,
/// split by whether the name could syntactically denote a symbol nested
/// inside a container (impl/trait/class/interface) — see
/// [`super::ExtractedSymbol::referenced_method_names`]'s doc comment (ADR
/// 0068) for why `graph::collect_edges` needs this distinction.
pub(super) struct ReferencedNames {
    /// Bare references (`@reference.call`/`@reference.type` captures, plus
    /// every non-query code walk below): cannot syntactically name a
    /// contained symbol in Python/Go/TypeScript.
    pub bare: Vec<String>,
    /// `@reference.method` and `@reference.methodspec` captures: Rust's
    /// receiver-based method calls (`@reference.method`), plus Rust's
    /// trait method names, Go's interface method-spec names, and
    /// TypeScript's interface method-signature names
    /// (`@reference.methodspec`) — all of which may legitimately denote a
    /// symbol nested inside a container. Merged into one set here since
    /// `graph::collect_edges` treats them identically (ADR 0068); the two
    /// capture names exist only so [`is_ubiquitous_method_name`]'s
    /// stoplist can apply to the former and not the latter (issue #230).
    pub method: Vec<String>,
}

/// Runs `reference_query` (already compiled by `with_definition_nodes`,
/// once per file rather than once per definition) over the subtree rooted
/// at `node`, returning the deduplicated names it captures (called
/// function/method names, referenced type names), split into
/// [`ReferencedNames::bare`]/[`ReferencedNames::method`] by capture kind.
/// Each set is sorted for determinism — tree-sitter's match order is not a
/// meaningful signal here, and downstream consumers (`deps.rs`, rendering)
/// benefit from a stable order.
///
/// Reads every capture whose name starts with `reference.` (see the doc
/// comment on [`LanguageSupport::reference_query`]) rather than a single
/// named capture, since each language's query alternation captures a
/// different sub-node depending on which branch matched (the callee
/// identifier for a call, the identifier itself for a type reference).
/// The two sets are otherwise disjoint by construction: no name is ever
/// captured under both `reference.method` and any other `reference.*`
/// prefix at the same node.
///
/// `_` and single-character identifiers are dropped before insertion
/// (`is_noise_name`): they are near-universal across unrelated files
/// (Python/Go's conventional throwaway `_`, one-letter loop/receiver
/// variables reused as call targets like `x()`), so a name-only resolver
/// (ADR 0003) matches them against dozens of unrelated definitions instead
/// of the one actually referenced — pure noise in the "Depends on" output
/// rather than a useful, if imprecise, match.
pub(super) fn collect_referenced_names(
    node: tree_sitter::Node,
    source: &[u8],
    reference_query: &tree_sitter::Query,
) -> ReferencedNames {
    let capture_names = reference_query.capture_names();

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(reference_query, node, source);

    let mut bare = std::collections::BTreeSet::new();
    let mut method = std::collections::BTreeSet::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let capture_name = capture_names[capture.index as usize];
            if !capture_name.starts_with("reference.") {
                continue;
            }
            if let Ok(text) = capture.node.utf8_text(source)
                && !is_noise_name(text)
                && !(capture_name == "reference.method" && is_ubiquitous_method_name(text))
            {
                if capture_name == "reference.method" || capture_name == "reference.methodspec" {
                    method.insert(text.to_string());
                } else {
                    bare.insert(text.to_string());
                }
            }
        }
    }

    collect_macro_body_names(node, source, &mut bare);
    collect_module_scoped_call_names(node, source, &mut bare);
    collect_hcl_traversal_names(node, source, &mut bare);

    ReferencedNames {
        bare: bare.into_iter().collect(),
        method: method.into_iter().collect(),
    }
}

/// HCL references are traversals — `(variable_expr (identifier))`
/// followed by sibling `(get_attr (identifier))` nodes — which no
/// single-capture query pattern can render in the normalized
/// `var.x`/`data.t.n` form rinkaku's name-keyed resolution needs (ADR
/// 0066). `variable_expr` is an HCL-only node kind, so the walk is
/// inert for every other grammar (same cross-grammar reasoning as
/// [`collect_macro_body_names`]).
fn collect_hcl_traversal_names(
    node: tree_sitter::Node,
    source: &[u8],
    names: &mut std::collections::BTreeSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_expr"
            && let Some(name) = hcl_traversal_name(child, source)
        {
            names.insert(name);
        }
        collect_hcl_traversal_names(child, source, names);
    }
}

/// Assembles the normalized reference for one traversal rooted at
/// `variable_expr`: the root identifier plus however many following
/// `get_attr` components the root's namespace needs — `var.`/`local.`/
/// `module.` take one, `data.` takes two (type + name), and any other
/// root is a managed-resource reference taking one (`aws_instance.web`
/// from `aws_instance.web.id`). Meta-roots (`each`, `count`, `self`,
/// `path`, `terraform`) and bare roots with no `get_attr` (locally
/// bound iteration/function names) reference nothing resolvable.
fn hcl_traversal_name(variable_expr: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let root = {
        let mut cursor = variable_expr.walk();
        variable_expr
            .children(&mut cursor)
            .find(|c| c.kind() == "identifier")?
            .utf8_text(source)
            .ok()?
            .to_string()
    };
    if matches!(
        root.as_str(),
        "each" | "count" | "self" | "path" | "terraform"
    ) {
        return None;
    }

    let parent = variable_expr.parent()?;
    let mut cursor = parent.walk();
    let attrs: Vec<String> = parent
        .children(&mut cursor)
        .skip_while(|sibling| *sibling != variable_expr)
        .skip(1)
        .take_while(|sibling| sibling.kind() == "get_attr")
        .filter_map(|get_attr| {
            let mut attr_cursor = get_attr.walk();
            get_attr
                .children(&mut attr_cursor)
                .find(|c| c.kind() == "identifier")
                .and_then(|ident| ident.utf8_text(source).ok())
                .map(str::to_string)
        })
        .collect();

    let wanted = match root.as_str() {
        "data" => 2,
        _ => 1,
    };
    if attrs.len() < wanted {
        return None;
    }
    let mut parts = vec![root];
    parts.extend(attrs.into_iter().take(wanted));
    Some(parts.join("."))
}

/// Method names so common across std traits and container/`Option`/
/// `Result`/`Iterator` idioms that a name-only match (ADR 0003) against a
/// same-named repo symbol is more likely wrong than right: one repo `fn
/// clone` drew 143 referrers, a `fn get` 138, when method captures ran
/// unfiltered (ADR 0064's measurements). Applied only to
/// `@reference.method` captures — Rust's receiver-based method calls
/// (`x.get()`), the shape those measurements were taken on. Not applied to
/// `@reference.methodspec` captures (interface/trait method declarations):
/// a spec name is a rare, deliberate declaration, not a high-fan-in call
/// site, so the stoplist's rationale does not transfer (ADR 0064
/// amendment, issue #230). The same name called as a free function
/// (`get(x)`) or defined as a symbol stays fully visible regardless.
///
/// The criterion for membership is "belongs to a std trait or ubiquitous
/// std method idiom", not "observed colliding here" — pollution appears
/// the day a same-named symbol enters a graph, so the list does not wait
/// for it.
fn is_ubiquitous_method_name(name: &str) -> bool {
    const UBIQUITOUS_METHOD_NAMES: &[&str] = &[
        "all",
        "and_then",
        "any",
        "as_bytes",
        "as_mut",
        "as_ref",
        "as_slice",
        "as_str",
        "borrow",
        "borrow_mut",
        "chain",
        "clear",
        "clone",
        "cloned",
        "cmp",
        "collect",
        "contains",
        "contains_key",
        "copied",
        "count",
        "default",
        "deref",
        "drop",
        "entry",
        "enumerate",
        "eq",
        "err",
        "expect",
        "extend",
        "filter",
        "filter_map",
        "find",
        "flat_map",
        "flatten",
        "flush",
        "fmt",
        "fold",
        "for_each",
        "from",
        "get",
        "get_mut",
        "hash",
        "insert",
        "into",
        "into_iter",
        "is_empty",
        "is_err",
        "is_none",
        "is_ok",
        "is_some",
        "iter",
        "iter_mut",
        "join",
        "keys",
        "len",
        "map",
        "map_err",
        "max",
        "min",
        "ne",
        "next",
        "ok",
        "or_else",
        "parse",
        "partial_cmp",
        "pop",
        "position",
        "push",
        "read",
        "remove",
        "replace",
        "rev",
        "skip",
        "sort",
        "sort_by",
        "sort_by_key",
        "split",
        "starts_with",
        "sum",
        "take",
        "to_owned",
        "to_string",
        "to_vec",
        "trim",
        "try_from",
        "try_into",
        "unwrap",
        "unwrap_or",
        "unwrap_or_default",
        "unwrap_or_else",
        "values",
        "write",
        "zip",
    ];
    UBIQUITOUS_METHOD_NAMES.binary_search(&name).is_ok()
}

/// Captures the called name of a module-scoped call — `render_markdown` in
/// `markdown::render_markdown(x)`, `helper` in `super::helper(1)` — which
/// `reference_query` deliberately leaves out: the same `path::name(...)`
/// shape is also a UFCS/associated call (`Format::default()`), whose name
/// must stay unresolved (every `X::new()` would otherwise edge to every
/// changed `fn new`; measured at 142 false referrers per node, ADR 0064).
/// The two are told apart by Rust's naming convention — module paths are
/// lowercase, type paths are capitalized — which a tree-sitter query
/// cannot test, hence a code walk (same cross-grammar reasoning as
/// [`collect_macro_body_names`]: `scoped_identifier` inside
/// `call_expression` with these path kinds is Rust-shaped, and the walk is
/// inert for grammars where it never matches).
fn collect_module_scoped_call_names(
    node: tree_sitter::Node,
    source: &[u8],
    names: &mut std::collections::BTreeSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "call_expression"
            && let Some(function) = child.child_by_field_name("function")
            && function.kind() == "scoped_identifier"
            && let Some(path) = function.child_by_field_name("path")
            && path_names_a_module(path, source)
            && let Some(name) = function.child_by_field_name("name")
            && let Ok(text) = name.utf8_text(source)
            && !is_noise_name(text)
        {
            names.insert(text.to_string());
        }
        collect_module_scoped_call_names(child, source, names);
    }
}

/// Whether a scoped call's `path` node names a module rather than a type:
/// the `super`/`crate` keywords, or an identifier following Rust's
/// lowercase module naming convention. A nested path (`std::fs::read`)
/// has a `scoped_identifier` here and is left uncaptured — its rightmost
/// name is almost always an external crate's item.
fn path_names_a_module(path: tree_sitter::Node, source: &[u8]) -> bool {
    match path.kind() {
        "super" | "crate" => true,
        "identifier" => path
            .utf8_text(source)
            .is_ok_and(|text| text.chars().next().is_some_and(|c| c.is_lowercase())),
        _ => false,
    }
}

/// Rust macro bodies parse as raw `token_tree` tokens, so the
/// `call_expression`/`type_identifier` patterns in `reference_query` never
/// match inside them — identifiers there are collected by this walk
/// instead (ADR 0063). `macro_invocation` is a Rust-only node kind, so
/// the walk is inert for the other grammars (same cross-grammar
/// reasoning as [`symbol_kind`]). Attribute arguments are `token_tree`s
/// under an `attribute` node, not a `macro_invocation`, and are
/// deliberately not walked.
fn collect_macro_body_names(
    node: tree_sitter::Node,
    source: &[u8],
    names: &mut std::collections::BTreeSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "macro_invocation" {
            let mut invocation_cursor = child.walk();
            for part in child.children(&mut invocation_cursor) {
                if part.kind() == "token_tree" {
                    collect_token_tree_names(part, source, names);
                }
            }
        } else {
            collect_macro_body_names(child, source, names);
        }
    }
}

/// Collects `identifier` tokens at any nesting depth of a macro body's
/// `token_tree`, skipping an identifier immediately followed by a `!`
/// token — that is a *nested* macro's name, a macro reference rather
/// than a symbol reference.
fn collect_token_tree_names(
    token_tree: tree_sitter::Node,
    source: &[u8],
    names: &mut std::collections::BTreeSet<String>,
) {
    let mut cursor = token_tree.walk();
    let children: Vec<tree_sitter::Node> = token_tree.children(&mut cursor).collect();
    for (index, child) in children.iter().enumerate() {
        match child.kind() {
            "token_tree" => collect_token_tree_names(*child, source, names),
            "identifier" => {
                let names_a_nested_macro = children
                    .get(index + 1)
                    .is_some_and(|next| next.kind() == "!");
                if !names_a_nested_macro
                    && let Ok(text) = child.utf8_text(source)
                    && !is_noise_name(text)
                {
                    names.insert(text.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Whether `name` is too generic to be worth resolving: the bare `_`
/// placeholder, or any single-character identifier. Both appear constantly
/// across unrelated definitions in most codebases, so under v1's name-only
/// resolution (ADR 0003) they produce many spurious matches rather than
/// useful ones — see `collect_referenced_names`'s doc comment.
fn is_noise_name(name: &str) -> bool {
    name.chars().count() <= 1
}
