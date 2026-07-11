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

/// Tree-sitter query capturing identifiers referenced from inside a
/// definition: called function names and referenced type names.
///
/// - `call_expression function: (identifier)` captures free function calls
///   (`helper(x)`) and tuple-struct/enum-variant constructors used as
///   calls. Method/UFCS calls (`function: (field_expression ...)` for
///   `x.bar()`, `function: (scoped_identifier ...)` for `Type::method()`)
///   are intentionally not captured: their callee is not a bare
///   identifier, and resolving them would need type information v1 does
///   not have (ADR 0003).
/// - `type_identifier` captures every named type reference (parameter
///   types, return types, struct field types, generic type arguments,
///   ...), matched anywhere in the tree rather than only in specific
///   fields — tree-sitter queries match regardless of nesting depth, so
///   this also covers `Option<Point>`'s inner `Point` without a separate
///   pattern. Rust's primitive types (`i32`, `str`, `bool`, ...) parse as
///   the distinct `primitive_type` node kind, so they are already excluded
///   by construction rather than needing an explicit exclusion list.
const REFERENCE_QUERY: &str = "\
[
  (call_expression function: (identifier) @reference.call)
  (type_identifier) @reference.type
]";

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

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
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

    #[test]
    fn should_compile_definition_query_against_its_own_grammar() {
        let support = RustSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the Rust grammar");
    }

    #[test]
    fn should_compile_reference_query_against_its_own_grammar() {
        let support = RustSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the Rust grammar");
    }
}
