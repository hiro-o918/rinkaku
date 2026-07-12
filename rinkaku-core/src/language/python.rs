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

/// Tree-sitter query capturing identifiers referenced from inside a
/// definition: called function/class names and referenced type
/// annotations.
///
/// - `call function: (identifier)` captures free function calls
///   (`helper(x)`) and class instantiations (`Point()` parses identically
///   to a call in this grammar, so it is covered for free). Method calls
///   (`function: (attribute ...)` for `x.bit_length()`) are intentionally
///   not captured: their callee is not a bare identifier, and resolving
///   the receiver's type would need type information v1 does not have
///   (ADR 0003).
/// - `type: (identifier)` captures named type annotations (parameter and
///   return type hints), matched anywhere in the tree. Python has no
///   distinct node kind for built-in types (`int`, `str`, ... parse as
///   plain `identifier` like any user-defined class), so they are
///   captured the same way and simply fail to resolve later since the
///   repo has no definition for them (see the trait doc comment on
///   `reference_query`).
const REFERENCE_QUERY: &str = "\
[
  (call function: (identifier) @reference.call)
  (type (identifier) @reference.type)
]";

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

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    /// pytest's own file-discovery convention (`test_*.py` / `*_test.py`,
    /// see the pytest docs' "Conventions for Python test discovery") plus a
    /// `tests/` directory anywhere in the path — the latter catches
    /// fixture/helper modules inside a test suite that don't themselves
    /// match the filename pattern (e.g. `tests/factories.py`).
    fn is_test_path(&self, path: &str) -> bool {
        is_in_tests_dir(path) || is_python_test_filename(path)
    }
}

/// Whether `path` contains a `tests/` path segment anywhere (not just as
/// the immediate parent), so `tests/unit/factories.py` is caught the same
/// as `tests/factories.py`.
fn is_in_tests_dir(path: &str) -> bool {
    path.split('/').any(|segment| segment == "tests")
}

/// Whether the file name (last `/`-separated segment) matches pytest's
/// `test_*.py` / `*_test.py` discovery convention.
fn is_python_test_filename(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    file_name.starts_with("test_") && file_name.ends_with(".py") || file_name.ends_with("_test.py")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

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

    #[test]
    fn should_compile_definition_query_against_its_own_grammar() {
        let support = PythonSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the Python grammar");
    }

    #[test]
    fn should_compile_reference_query_against_its_own_grammar() {
        let support = PythonSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the Python grammar");
    }

    #[rstest]
    #[case::should_return_true_when_filename_starts_with_test_prefix("test_repo.py", true)]
    #[case::should_return_true_when_filename_ends_with_test_suffix("repo_test.py", true)]
    #[case::should_return_true_when_path_has_tests_directory_segment(
        "src/tests/factories.py",
        true
    )]
    #[case::should_return_true_when_tests_directory_is_nested("tests/unit/factories.py", true)]
    #[case::should_return_false_when_path_is_ordinary_module("src/repo.py", false)]
    #[case::should_return_false_when_filename_merely_contains_test_substring(
        "src/contest.py",
        false
    )]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = PythonSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}
