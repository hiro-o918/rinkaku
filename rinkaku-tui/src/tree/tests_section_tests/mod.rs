//! Unit tests for `tree::tests_section`'s two pure helpers in isolation
//! — `crate::tree_tests::tests_section` covers the `build_tree`
//! integration behavior (routing, badges, directory nesting).

use super::*;
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};

fn symbol(id: &str, name: &str, is_test: bool) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range: LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test,
        classification: None,
        previous_signature: None,
    }
}

#[test]
fn should_treat_path_matching_is_test_path_convention_as_whole_test_file() {
    let actual = is_whole_test_file("lib_test.go", &[symbol("lib_test.go::Foo", "Foo", false)]);

    assert!(actual);
}

#[test]
fn should_treat_file_with_every_symbol_flagged_is_test_as_whole_test_file() {
    let actual = is_whole_test_file(
        "only_tests.rs",
        &[symbol("only_tests.rs::test_it", "test_it", true)],
    );

    assert!(actual);
}

#[test]
fn should_not_treat_mixed_file_as_whole_test_file() {
    let actual = is_whole_test_file(
        "mixed.rs",
        &[
            symbol("mixed.rs::real_fn", "real_fn", false),
            symbol("mixed.rs::test_it", "test_it", true),
        ],
    );

    assert!(!actual);
}

#[test]
fn should_not_treat_empty_file_as_whole_test_file() {
    let actual = is_whole_test_file("renamed.rs", &[]);

    assert!(!actual);
}

fn file_node(path: &str) -> TreeNode {
    TreeNode {
        kind: NodeKind::File,
        path: path.to_string(),
        badges: Badges::default(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

#[test]
fn should_return_none_when_section_roots_are_empty() {
    let actual = wrap_section(vec![]);

    assert_eq!(None, actual);
}

#[test]
fn should_wrap_and_sort_non_empty_section_roots_alphabetically() {
    let actual = wrap_section(vec![file_node("z_test.go"), file_node("a_test.go")]);

    let expected = Some(TreeNode {
        kind: NodeKind::Section(SectionKind::Tests),
        path: TESTS_SECTION_PATH.to_string(),
        badges: Badges::default(),
        children: vec![file_node("a_test.go"), file_node("z_test.go")],
        skip_reason: None,
        test_symbol_count: None,
    });
    assert_eq!(expected, actual);
}
