//! HCL (Terraform) `LanguageSupport` implementation (ADR 0066).

use super::LanguageSupport;

/// Captures every top-level block of a config file as a definition
/// candidate. Which block types become symbols (resource, data,
/// module, variable, output, provider — and `locals`, expanded per
/// attribute) is decided in `extract.rs` by reading the block-type
/// identifier's text, since tree-sitter text predicates are not
/// evaluated by rinkaku's raw `QueryCursor` iteration. Nested blocks
/// (`tags`, `dynamic`, provisioners) live under an inner `body` and
/// are never captured.
const DEFINITION_QUERY: &str = "(config_file (body (block) @definition))";

/// Captures called function names (`cidrsubnet(...)`). HCL's built-in
/// functions have no repo definitions and simply fail to resolve — the
/// same non-resolving story as Go's built-in types. Traversal
/// references (`var.x`, `aws_instance.web.id`) are deliberately not
/// captured here: their normalized form spans several sibling nodes,
/// which `extract/references.rs`'s HCL walk assembles instead (ADR
/// 0066).
const REFERENCE_QUERY: &str = "(function_call (identifier) @reference.call)";

pub struct HclSupport;

impl LanguageSupport for HclSupport {
    fn name(&self) -> &'static str {
        "hcl"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_hcl::LANGUAGE.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    /// Terraform's native test convention: `*.tftest.hcl` files hold
    /// `run` blocks and mock providers, and are only read by
    /// `terraform test`.
    fn is_test_path(&self, path: &str) -> bool {
        path.ends_with(".tftest.hcl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_report_hcl_as_name() {
        let support = HclSupport;

        assert_eq!("hcl", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = HclSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("HCL grammar should load into a tree-sitter parser");

        let tree = parser
            .parse(
                "resource \"aws_instance\" \"web\" {\n  ami = \"abc\"\n}\n",
                None,
            )
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn should_compile_definition_query_against_its_own_grammar() {
        let support = HclSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the HCL grammar");
    }

    #[test]
    fn should_compile_reference_query_against_its_own_grammar() {
        let support = HclSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the HCL grammar");
    }

    #[rstest]
    #[case::should_return_true_when_path_ends_with_tftest_hcl("tests/plan.tftest.hcl", true)]
    #[case::should_return_false_when_path_is_tf_file("main.tf", false)]
    #[case::should_return_false_when_path_is_tofu_file("main.tofu", false)]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = HclSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}
