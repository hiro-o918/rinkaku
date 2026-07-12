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

/// Tree-sitter query capturing identifiers referenced from inside a
/// definition: called function names, constructed class names, and
/// referenced type names.
///
/// - `call_expression function: (identifier)` captures free function
///   calls (`helper(x)`). Method calls
///   (`function: (member_expression ...)` for `x.bar()`) are
///   intentionally not captured: their callee is not a bare identifier,
///   and resolving the receiver's type would need type information v1
///   does not have (ADR 0003).
/// - `new_expression constructor: (identifier)` captures class
///   instantiation (`new Foo()`), which is syntactically distinct from a
///   call in this grammar and would otherwise be missed entirely.
/// - `type_identifier` captures every named type reference (parameter
///   types, return types, generic type arguments, ...), matched anywhere
///   in the tree — including inside `type_arguments`, so
///   `Array<Point>`'s inner `Point` is covered without a separate
///   pattern. TypeScript's built-in types (`number`, `string`, `boolean`,
///   ...) parse as the distinct `predefined_type` node kind, so they are
///   already excluded by construction rather than needing an explicit
///   exclusion list.
const REFERENCE_QUERY: &str = "\
[
  (call_expression function: (identifier) @reference.call)
  (new_expression constructor: (identifier) @reference.call)
  (type_identifier) @reference.type
]";

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

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    fn is_test_path(&self, path: &str) -> bool {
        is_test_path(path)
    }
}

/// Common test-file convention checked by both grammars: `.test.ts(x)` /
/// `.spec.ts(x)` (Jest/Vitest/Jasmine's shared discovery pattern) or a
/// `__tests__/` directory anywhere in the path (Jest's other default
/// convention, for files that don't themselves match the suffix pattern,
/// e.g. `__tests__/factories.ts`).
fn is_test_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let has_test_suffix = [".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx"]
        .iter()
        .any(|suffix| file_name.ends_with(suffix));
    has_test_suffix || path.split('/').any(|segment| segment == "__tests__")
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

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    fn is_test_path(&self, path: &str) -> bool {
        is_test_path(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

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

    #[test]
    fn should_compile_reference_query_against_the_typescript_grammar() {
        let support = TypeScriptSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the TypeScript grammar");
    }

    #[test]
    fn should_compile_reference_query_against_the_tsx_grammar() {
        let support = TsxSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the TSX grammar");
    }

    #[rstest]
    #[case::should_return_true_when_filename_has_test_ts_suffix("repo.test.ts", true)]
    #[case::should_return_true_when_filename_has_test_tsx_suffix("Repo.test.tsx", true)]
    #[case::should_return_true_when_filename_has_spec_ts_suffix("repo.spec.ts", true)]
    #[case::should_return_true_when_filename_has_spec_tsx_suffix("Repo.spec.tsx", true)]
    #[case::should_return_true_when_path_has_tests_directory_segment(
        "src/__tests__/factories.ts",
        true
    )]
    #[case::should_return_false_when_path_is_ordinary_module("src/repo.ts", false)]
    #[case::should_return_false_when_filename_merely_contains_test_substring(
        "src/contest.ts",
        false
    )]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = TypeScriptSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}
