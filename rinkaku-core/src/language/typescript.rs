//! TypeScript `LanguageSupport` implementation.
//!
//! `.ts` and `.tsx` are both TypeScript in the language-neutral sense but
//! parse under different tree-sitter grammars (`LANGUAGE_TYPESCRIPT` vs.
//! `LANGUAGE_TSX`, the latter adding JSX syntax) — mixing them up would
//! either misparse a `.ts` file's `<Type>value` cast syntax as JSX or fail
//! to parse a `.tsx` file's JSX elements. Both node kinds and field names
//! used by `DEFINITION_QUERY` are identical between the two grammars, so a
//! single query is shared by two `LanguageSupport` impls, one per grammar.

use super::LanguageSupport;

/// Tree-sitter query capturing the definition node kinds whose signatures
/// rinkaku extracts: functions (top-level and class methods),
/// arrow functions bound to `const`/`let`/`var`, classes (including
/// `abstract class`), abstract method signatures, interfaces, type
/// aliases, and enums.
///
/// `variable_declarator` is captured whenever its `value` is an
/// `arrow_function` (`const f = () => {}` / `let f = () => {}` /
/// `var f = () => {}`), so a plain `const x = 5;` binding is correctly
/// excluded while `let`/`var` arrow-function bindings are captured the same
/// as `const` ones. Only the declarator is captured, not the enclosing
/// `lexical_declaration`/`variable_declaration` — a multi-binding statement
/// (`const a = 1, b = () => {};`) is handled per declarator, matching how
/// Go's `type_spec` is captured independently of its `type_declaration`
/// parent.
///
/// `abstract_class_declaration` is a distinct node kind from
/// `class_declaration` in this grammar (an `abstract class Foo { ... }`),
/// so it needs its own capture; `abstract_method_signature` (a
/// body-less `abstract area(): number;` member inside such a class) is
/// likewise distinct from `method_definition`.
const DEFINITION_QUERY: &str = "\
[
  (function_declaration) @definition.function
  (method_definition) @definition.function
  (abstract_method_signature) @definition.function
  (variable_declarator value: (arrow_function)) @definition.function
  (class_declaration) @definition.class
  (abstract_class_declaration) @definition.class
  (interface_declaration) @definition.interface
  (type_alias_declaration) @definition.type_alias
  (enum_declaration) @definition.enum
] @definition";

pub struct TypeScriptSupport;

impl LanguageSupport for TypeScriptSupport {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }
}

/// TSX (`.tsx`): TypeScript with JSX syntax. Reports the same `"typescript"`
/// name as [`TypeScriptSupport`] since it is the same language for
/// rinkaku's output purposes — only the grammar (and thus what parses
/// without error) differs.
pub struct TsxSupport;

impl LanguageSupport for TsxSupport {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_typescript_as_name() {
        let support = TypeScriptSupport;

        assert_eq!("typescript", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = TypeScriptSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("typescript grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("function main(): void {}\n", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn should_report_typescript_as_name_for_tsx() {
        let support = TsxSupport;

        assert_eq!("typescript", support.name());
    }

    #[test]
    fn should_produce_a_tsx_grammar_that_parses_jsx_without_errors() {
        let support = TsxSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("tsx grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("const Component = () => <div>Hi</div>;\n", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn should_compile_definition_query_against_the_typescript_grammar() {
        let support = TypeScriptSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the TypeScript grammar");
    }

    #[test]
    fn should_compile_definition_query_against_the_tsx_grammar() {
        let support = TsxSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the TSX grammar");
    }
}
