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

    /// Rust's `tests/` directory convention: integration test crates
    /// (`tests/*.rs`, each compiled as its own binary by `cargo test`).
    /// Unit tests (`#[cfg(test)] mod tests { ... }`) live colocated with
    /// production code instead, so they are not caught by path alone — see
    /// `is_test_definition`.
    fn is_test_path(&self, path: &str) -> bool {
        path.split('/').any(|segment| segment == "tests")
    }

    /// Whether `node` (a captured `@definition` node) is a Rust unit test:
    /// nested inside a `#[cfg(test)]`-attributed `mod`, or itself
    /// `#[test]`/`#[rstest]`/`#[tokio::test]`-attributed. Checked by
    /// walking ancestors rather than only the immediate parent, since a
    /// test helper function nested a few `impl`/`mod` levels inside a
    /// `#[cfg(test)] mod tests { ... }` block is still test code.
    fn is_test_definition(&self, node: tree_sitter::Node, source: &[u8]) -> bool {
        if has_test_attribute(node, source) {
            return true;
        }
        let mut current = node.parent();
        while let Some(candidate) = current {
            if candidate.kind() == "mod_item" && has_cfg_test_attribute(candidate, source) {
                return true;
            }
            current = candidate.parent();
        }
        false
    }
}

/// Whether `node` is immediately preceded by a `#[test]`, `#[rstest]`, or
/// `#[tokio::test]` attribute — a test function marker. Tree-sitter's Rust
/// grammar attaches an `attribute_item` as a preceding *sibling* of the
/// node it annotates, not a child, so this walks backward through
/// `prev_sibling` (skipping over other attributes/doc comments stacked on
/// the same item) rather than inspecting `node`'s own children.
///
/// Matches any attribute whose name (the bare `identifier`, e.g. `test` in
/// `#[test]`, or the `scoped_identifier`'s `name` field, e.g. `test` in
/// `#[tokio::test]`) is `"test"`, plus the literal `rstest` attribute
/// (which has no further `#[rstest::...]`-scoped form in common usage) —
/// covers the attribute macros named in ADR 0009 without needing to
/// enumerate every async test runner's exact macro path.
fn has_test_attribute(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(candidate) = sibling {
        if candidate.kind() == "attribute_item" && is_test_attribute_item(candidate, source) {
            return true;
        }
        if candidate.kind() != "attribute_item" {
            break;
        }
        sibling = candidate.prev_sibling();
    }
    false
}

/// Whether an `attribute_item` node's inner `attribute` name is `test` or
/// `rstest` — see `has_test_attribute`'s doc comment for which forms this
/// covers.
///
/// `attribute`'s grammar has no `name` field for its leading identifier
/// (only `arguments`/`value` are named fields, per
/// `tree-sitter-rust`'s `node-types.json`); the identifier or
/// `scoped_identifier` is its first named child instead.
fn is_test_attribute_item(attribute_item: tree_sitter::Node, source: &[u8]) -> bool {
    let Some(attribute) = attribute_item
        .named_child(0)
        .filter(|n| n.kind() == "attribute")
    else {
        return false;
    };
    let Some(name_node) = attribute.named_child(0) else {
        return false;
    };
    let name_text = match name_node.kind() {
        "scoped_identifier" => name_node.child_by_field_name("name"),
        _ => Some(name_node),
    }
    .and_then(|n| n.utf8_text(source).ok());
    matches!(name_text, Some("test") | Some("rstest"))
}

/// Whether a `mod_item` node is immediately preceded by a `#[cfg(test)]`
/// attribute — same preceding-sibling structure as `has_test_attribute`,
/// but checking for the `cfg(test)` argument shape instead of a bare test
/// marker name.
fn has_cfg_test_attribute(mod_item: tree_sitter::Node, source: &[u8]) -> bool {
    let mut sibling = mod_item.prev_sibling();
    while let Some(candidate) = sibling {
        if candidate.kind() == "attribute_item" && is_cfg_test_attribute_item(candidate, source) {
            return true;
        }
        if candidate.kind() != "attribute_item" {
            break;
        }
        sibling = candidate.prev_sibling();
    }
    false
}

/// Whether an `attribute_item` is `#[cfg(test)]` specifically (not just any
/// `#[cfg(...)]`): its inner `attribute` must be named `cfg` with a
/// `token_tree` argument whose text is exactly `(test)`. See
/// `is_test_attribute_item`'s doc comment for why the name is read as a
/// plain child rather than a `name` field.
fn is_cfg_test_attribute_item(attribute_item: tree_sitter::Node, source: &[u8]) -> bool {
    let Some(attribute) = attribute_item
        .named_child(0)
        .filter(|n| n.kind() == "attribute")
    else {
        return false;
    };
    let is_cfg = attribute
        .named_child(0)
        .and_then(|n| n.utf8_text(source).ok())
        == Some("cfg");
    if !is_cfg {
        return false;
    }
    attribute
        .child_by_field_name("arguments")
        .and_then(|n| n.utf8_text(source).ok())
        == Some("(test)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tree_sitter::StreamingIterator;

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

    #[rstest]
    #[case::should_return_true_when_path_has_tests_directory_segment("tests/integration.rs", true)]
    #[case::should_return_true_when_tests_directory_is_nested("crate/tests/it.rs", true)]
    #[case::should_return_false_when_path_is_ordinary_module("src/lib.rs", false)]
    #[case::should_return_false_when_filename_merely_contains_test_substring(
        "src/contest.rs",
        false
    )]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = RustSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }

    /// Parses `source` and returns the first node captured by
    /// `DEFINITION_QUERY` whose own text contains `needle` — a small
    /// end-to-end harness for `is_test_definition` tests, which need a real
    /// tree-sitter node (attribute siblings, `mod_item` ancestors) rather
    /// than the flat `ExtractedSymbol` shape `extract.rs` produces.
    fn find_definition_node<'a>(
        tree: &'a tree_sitter::Tree,
        source: &'a str,
        needle: &str,
    ) -> tree_sitter::Node<'a> {
        let support = RustSupport;
        let query = tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the Rust grammar");
        let definition_capture_index = query
            .capture_index_for_name("definition")
            .expect("definition query must have a @definition capture");
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                if capture.index == definition_capture_index
                    && capture
                        .node
                        .utf8_text(source.as_bytes())
                        .is_ok_and(|text| text.contains(needle))
                {
                    return capture.node;
                }
            }
        }
        panic!("no definition node containing {needle:?} found in parsed source");
    }

    #[test]
    fn should_return_true_when_function_is_nested_inside_cfg_test_mod() {
        let source = "\
#[cfg(test)]
mod tests {
    fn helper() {}
}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn helper");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(actual);
    }

    #[test]
    fn should_return_true_when_function_has_test_attribute() {
        let source = "\
#[test]
fn should_add_two_numbers() {}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn should_add_two_numbers");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(actual);
    }

    #[test]
    fn should_return_true_when_function_has_rstest_attribute() {
        let source = "\
#[rstest]
fn should_do_something(#[case] input: i32) {}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn should_do_something");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(actual);
    }

    #[test]
    fn should_return_true_when_function_has_tokio_test_attribute() {
        let source = "\
#[tokio::test]
async fn should_fetch_data() {}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn should_fetch_data");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(actual);
    }

    #[test]
    fn should_return_false_when_function_has_no_test_marker() {
        let source = "\
fn helper() -> i32 {
    42
}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn helper");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(!actual);
    }

    #[test]
    fn should_return_false_when_mod_has_unrelated_cfg_attribute() {
        let source = "\
#[cfg(feature = \"extra\")]
mod extra {
    fn helper() {}
}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn helper");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(!actual);
    }

    #[test]
    fn should_return_true_when_function_is_deeply_nested_inside_cfg_test_mod() {
        // A helper function nested inside an inner `mod` that is itself
        // inside `#[cfg(test)] mod tests` is still test code — the walk
        // must not stop at the immediate parent.
        let source = "\
#[cfg(test)]
mod tests {
    mod nested {
        fn helper() {}
    }
}
";
        let support = RustSupport;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&support.grammar()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = find_definition_node(&tree, source, "fn helper");

        let actual = support.is_test_definition(node, source.as_bytes());

        assert!(actual);
    }
}
