//! Go `LanguageSupport` implementation.

use super::LanguageSupport;

/// Tree-sitter query capturing the definition node kinds whose signatures
/// rinkaku extracts: free functions, receiver methods, struct types, and
/// interface types.
///
/// `type_spec` is captured with a field predicate on its `type` child
/// (`struct_type` vs. `interface_type`) rather than the whole
/// `type_declaration`, so a plain type alias (`type Alias = string`, whose
/// `type` child is neither) is correctly excluded — v1 does not report type
/// aliases for Go (unlike TypeScript, Go type aliases are rare and mostly
/// used for local renaming rather than public API surface).
///
/// Receiver methods (`method_declaration`) are captured as top-level
/// definitions, not nested inside their receiver struct's node the way
/// Rust's `impl_item` nests its methods — Go's grammar declares them as
/// siblings linked only by the receiver's type name. `extract.rs`'s
/// `find_container` reads that name directly off the `receiver` field
/// rather than by walking ancestors (see `go_receiver_type_name`).
const DEFINITION_QUERY: &str = "\
[
  (function_declaration) @definition.function
  (method_declaration) @definition.method
  (type_spec type: (struct_type)) @definition.struct
  (type_spec type: (interface_type)) @definition.interface
] @definition";

/// Tree-sitter query capturing identifiers referenced from inside a
/// definition: called function names and referenced type names.
///
/// - `call_expression function: (identifier)` captures free function calls
///   (`Helper(x)`). Method/selector calls
///   (`function: (selector_expression ...)` for `r.Save()`) are
///   intentionally not captured: their callee is not a bare identifier,
///   and resolving the receiver's type would need type information v1
///   does not have (ADR 0003).
/// - `type_identifier` captures every named type reference (parameter
///   types, return types, struct field types, ...), matched anywhere in
///   the tree rather than only in specific fields. Go's built-in types
///   (`int`, `string`, `error`, ...) are not syntactically distinct from
///   user-defined type names in this grammar — both parse as
///   `type_identifier` — so they are captured the same way and simply
///   fail to resolve later since the repo has no definition for them
///   (see the trait doc comment on `reference_query`).
const REFERENCE_QUERY: &str = "\
[
  (call_expression function: (identifier) @reference.call)
  (type_identifier) @reference.type
]";

pub struct GoSupport;

impl LanguageSupport for GoSupport {
    fn name(&self) -> &'static str {
        "go"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
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
    fn should_report_go_as_name() {
        let support = GoSupport;

        assert_eq!("go", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = GoSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("go grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("package main\n\nfunc main() {}\n", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn should_compile_definition_query_against_its_own_grammar() {
        let support = GoSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the Go grammar");
    }

    #[test]
    fn should_compile_reference_query_against_its_own_grammar() {
        let support = GoSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the Go grammar");
    }
}
