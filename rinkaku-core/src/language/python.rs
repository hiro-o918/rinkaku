//! Python `LanguageSupport` implementation.

use super::LanguageSupport;

/// Tree-sitter query capturing the definition node kinds whose signatures
/// rinkaku extracts: functions (top-level, nested, and class methods alike
/// — `extract.rs`'s `find_container` distinguishes a method from a free
/// function by walking up to an enclosing `class_definition`, and a nested
/// function naturally gets no container since its nearest such ancestor is
/// another `function_definition`, not a class) and classes.
///
/// `function_definition`/`class_definition` are captured directly rather
/// than their enclosing `decorated_definition` wrapper. This means a
/// decorator-only line change (with the `def`/`class` line itself
/// untouched) is not detected as touching the definition — a deliberate
/// v1 simplification consistent with "only symbol-level changes are
/// surfaced" (see `extract_changed_symbols`'s module doc); decorators are
/// also never included in the extracted signature text for the same
/// reason a Rust definition's attributes aren't.
const DEFINITION_QUERY: &str = "\
[
  (function_definition) @definition.function
  (class_definition) @definition.class
] @definition";

pub struct PythonSupport;

impl LanguageSupport for PythonSupport {
    fn name(&self) -> &'static str {
        "python"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_python_as_name() {
        let support = PythonSupport;

        assert_eq!("python", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = PythonSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("python grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("def main():\n    pass\n", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }
}
