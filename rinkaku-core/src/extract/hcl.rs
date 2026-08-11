//! HCL-specific symbol construction (ADR 0066): block-type dispatch,
//! reference-syntax naming, and per-attribute `locals` expansion.
//! Split out of the module root along this responsibility boundary
//! (ADR 0028) when the HCL arms pushed it over the warn threshold —
//! the same split `references.rs` got for the language-specific walks.

use crate::language::LanguageSupport;

/// The block-type keyword of an HCL `block` node — the text of its
/// first `identifier` child (`resource`, `variable`, `locals`, ...).
pub(super) fn hcl_block_type(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "identifier")
        .and_then(|ident| ident.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// The text inside an HCL `string_lit` label — its `template_literal`
/// descendant (labels are quoted templates in this grammar; plain
/// label strings always contain exactly one literal part).
fn hcl_string_lit_text(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| match child.kind() {
            "template_literal" => child.utf8_text(source).ok().map(|s| s.to_string()),
            "quoted_template_start" | "quoted_template_end" => None,
            _ => hcl_string_lit_text(child, source),
        })
}

/// An HCL block's symbol name in Terraform reference syntax (ADR
/// 0066): the same string other files use to reference the definition,
/// so name-keyed resolution (`TagsResolver`) and edge matching
/// (`graph::collect_edges`) work unchanged. Only `string_lit` labels
/// are read: the grammar also permits bare-identifier labels, but none
/// of the recognized Terraform block types accept them in practice, so
/// a block using one produces no symbol rather than a guessed name.
pub(super) fn hcl_block_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let block_type = hcl_block_type(node, source)?;
    let mut cursor = node.walk();
    let labels: Vec<String> = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "string_lit")
        .filter_map(|label| hcl_string_lit_text(label, source))
        .collect();
    match (block_type.as_str(), labels.as_slice()) {
        ("resource", [type_label, name]) => Some(format!("{type_label}.{name}")),
        ("data", [type_label, name]) => Some(format!("data.{type_label}.{name}")),
        ("module", [name]) => Some(format!("module.{name}")),
        ("variable", [name]) => Some(format!("var.{name}")),
        ("output", [name]) => Some(format!("output.{name}")),
        ("provider", [name]) => Some(format!("provider.{name}")),
        _ => None,
    }
}

/// One symbol per attribute of a `locals` block (ADR 0066): references
/// use `local.<attribute>` granularity, so the block itself is not a
/// symbol. Each attribute's own text is its signature and its own node
/// is the reference-collection scope, keeping fan-in per name.
pub(super) fn build_hcl_locals_symbols(
    node: tree_sitter::Node,
    source: &[u8],
    reference_query: &tree_sitter::Query,
    lang: &dyn LanguageSupport,
) -> Vec<super::ExtractedSymbol> {
    let mut block_cursor = node.walk();
    let Some(body) = node
        .children(&mut block_cursor)
        .find(|c| c.kind() == "body")
    else {
        return Vec::new();
    };
    let mut cursor = body.walk();
    body.children(&mut cursor)
        .filter(|child| child.kind() == "attribute")
        .filter_map(|attribute| {
            let mut attribute_cursor = attribute.walk();
            let name_node = attribute
                .children(&mut attribute_cursor)
                .find(|c| c.kind() == "identifier")?;
            let name = name_node.utf8_text(source).ok()?;
            let references =
                super::references::collect_referenced_names(attribute, source, reference_query);
            Some(super::ExtractedSymbol {
                id: String::new(),
                name: format!("local.{name}"),
                kind: super::SymbolKind::Block,
                signature: super::slice_signature(attribute, source, None),
                range: super::node_to_line_range(attribute),
                container: None,
                referenced_names: references.bare,
                referenced_method_names: references.method,
                dependencies: Vec::new(),
                omitted_dependency_matches: 0,
                is_test: lang.is_test_definition(attribute, source),
                classification: None,
                previous_signature: None,
            })
        })
        .collect()
}
