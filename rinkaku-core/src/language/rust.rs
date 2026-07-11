//! Rust `LanguageSupport` implementation.

use super::LanguageSupport;

/// Tree-sitter query capturing the definition node kinds whose signatures
/// rinkaku extracts: free functions, trait method signatures (no body),
/// structs, enums, traits, and impl blocks (captured so `extract.rs` can
/// walk up to them for `container` naming).
const DEFINITION_QUERY: &str = "\
[
  (function_item) @definition.function
  (function_signature_item) @definition.function
  (struct_item) @definition.struct
  (enum_item) @definition.enum
  (trait_item) @definition.trait
  (impl_item) @definition.impl
] @definition";

pub struct RustSupport;

impl LanguageSupport for RustSupport {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_rust_as_name() {
        let support = RustSupport;

        assert_eq!("rust", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("rust grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("fn main() {}", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }
}
