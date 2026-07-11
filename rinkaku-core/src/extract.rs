//! Tree-sitter based signature extraction.
//!
//! Given a source file's text and the line ranges that changed in a diff
//! (see [`crate::diff::LineRange`]), finds the definitions that contain
//! those changed lines and slices out their signatures — the API surface,
//! without the implementation body.

use crate::diff::LineRange;
use crate::language::LanguageSupport;
use serde::Serialize;
use tree_sitter::StreamingIterator;

/// The kind of symbol a definition node represents, expressed in
/// language-neutral terms so callers don't need to match on
/// language-specific tree-sitter node kinds.
///
/// No `Impl` variant: impl blocks are never reported as symbols in their
/// own right (see the filtering in `extract_changed_symbols`) — they only
/// contribute `container` names to the members nested inside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
}

/// A definition whose signature was extracted because one of its lines
/// (declaration or body) fell inside a changed range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Declaration text without its body, whitespace-normalized. Doc
    /// comments and attributes are not included.
    pub signature: String,
    /// Full definition range (new-side, 1-based inclusive) — body included,
    /// since this describes where the change lives, not the signature's
    /// own extent.
    pub range: LineRange,
    /// The enclosing impl/trait block's descriptive name, if the
    /// definition is nested inside one (e.g. `Some("impl Foo")`).
    pub container: Option<String>,
}

/// Extracts the signatures of definitions that contain at least one
/// changed line. A changed line that isn't inside any definition (e.g. a
/// top-level statement) is not surfaced — v1 only reports symbol-level
/// changes.
pub fn extract_changed_symbols(
    source: &str,
    lang: &dyn LanguageSupport,
    changed_ranges: &[LineRange],
) -> Vec<ExtractedSymbol> {
    if changed_ranges.is_empty() {
        return Vec::new();
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar())
        .expect("LanguageSupport grammar must be loadable by tree-sitter");
    let tree = parser
        .parse(source, None)
        .expect("parsing a source string always produces a tree");

    let query = tree_sitter::Query::new(&lang.grammar(), lang.definition_query())
        .expect("LanguageSupport definition query must be valid");
    let definition_capture_index = query
        .capture_index_for_name("definition")
        .expect("definition query must have a @definition capture");

    let mut cursor = tree_sitter::QueryCursor::new();
    let source_bytes = source.as_bytes();
    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);

    let mut touched_nodes = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if capture.index != definition_capture_index {
                continue;
            }
            let node = capture.node;
            let node_range = node_to_line_range(node);
            if overlaps_any(node_range, changed_ranges) {
                touched_nodes.push(node);
            }
        }
    }

    touched_nodes
        .iter()
        .filter(|node| {
            // Impl/trait blocks are captured so `find_container` can name
            // nested members, but are only reported as symbols in their
            // own right when none of their nested definitions were
            // themselves touched — otherwise a changed method line would
            // surface both the method and its enclosing block.
            !matches!(node.kind(), "impl_item" | "trait_item")
                || !touched_nodes
                    .iter()
                    .any(|other| other != *node && is_descendant_of(*other, **node))
        })
        .filter_map(|node| build_symbol(*node, source_bytes))
        .collect()
}

/// Whether `node` is strictly nested inside `ancestor` in the syntax tree.
fn is_descendant_of(node: tree_sitter::Node, ancestor: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent == ancestor {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Converts a tree-sitter node's byte-oriented row span into a 1-based
/// inclusive [`LineRange`], matching the convention `diff::parse_unified_diff`
/// uses for new-side line numbers.
fn node_to_line_range(node: tree_sitter::Node) -> LineRange {
    LineRange {
        start: node.start_position().row + 1,
        end: node.end_position().row + 1,
    }
}

/// Whether `range` shares at least one line with any range in `others`.
fn overlaps_any(range: LineRange, others: &[LineRange]) -> bool {
    others
        .iter()
        .any(|other| range.start <= other.end && other.start <= range.end)
}

/// Builds an [`ExtractedSymbol`] from a captured definition node, or
/// `None` if the node kind isn't one this module knows how to report
/// (defensive default for query/grammar drift, not expected in practice
/// given `definition_query` only captures known kinds).
fn build_symbol(node: tree_sitter::Node, source: &[u8]) -> Option<ExtractedSymbol> {
    let kind = symbol_kind(node.kind())?;
    let name = definition_name(node, source)?;
    let signature = slice_signature(node, source);
    let container = find_container(node, source);

    Some(ExtractedSymbol {
        name,
        kind,
        signature,
        range: node_to_line_range(node),
        container,
    })
}

fn symbol_kind(node_kind: &str) -> Option<SymbolKind> {
    match node_kind {
        "function_item" | "function_signature_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        _ => None,
    }
}

/// Extracts a definition's declared name from its `name`/`type_identifier`
/// field, as exposed by tree-sitter-rust's grammar for all definition
/// kinds this module handles.
fn definition_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Slices a definition's signature: the declaration text with its body
/// removed and internal whitespace normalized to single spaces.
///
/// Struct/enum/trait definitions have no separate "body" in the
/// implementation sense — their fields/variants/method signatures *are*
/// the API surface — so the whole node text is kept for those kinds.
/// Only `function_item` has a `block` body to strip.
fn slice_signature(node: tree_sitter::Node, source: &[u8]) -> String {
    let text_range = match node.child_by_field_name("body") {
        Some(body) if node.kind() == "function_item" => node.start_byte()..body.start_byte(),
        _ => node.start_byte()..node.end_byte(),
    };
    let raw = std::str::from_utf8(&source[text_range]).unwrap_or("");
    normalize_whitespace(raw)
}

/// Collapses runs of whitespace (including newlines/indentation from the
/// original source) into single spaces, and trims the result.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Walks up from `node` to find an enclosing `impl_item`/`trait_item`,
/// returning a descriptive container name (e.g. `"impl Foo"`,
/// `"trait Bar"`). Returns `None` for top-level definitions.
fn find_container(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "impl_item" => {
                let type_name = candidate
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source).ok())?;
                return Some(format!("impl {type_name}"));
            }
            "trait_item" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("trait {name}"));
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::rust::RustSupport;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_return_empty_vec_when_changed_ranges_is_empty() {
        let source = "fn foo() {}\n";
        let lang = RustSupport;

        let expected: Vec<ExtractedSymbol> = Vec::new();
        let actual = extract_changed_symbols(source, &lang, &[]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_function_signature_when_body_line_changed() {
        let source = "\
fn foo(a: i32) -> i32 {
    let b = a + 1;
    b
}
";
        let lang = RustSupport;
        // Line 2 (`let b = a + 1;`) is inside the body only.
        let changed_ranges = vec![LineRange { start: 2, end: 2 }];

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 4 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_function_signature_when_signature_line_changed() {
        let source = "\
fn foo(a: i32, c: i32) -> i32 {
    a + c
}
";
        let lang = RustSupport;
        // Line 1 is the signature line itself.
        let changed_ranges = vec![LineRange { start: 1, end: 1 }];

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32, c: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_full_struct_signature_when_field_changed() {
        let source = "\
struct Point {
    x: i32,
    y: i32,
}
";
        let lang = RustSupport;
        // Line 3 (`y: i32,`) is a field, not a separate body.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected = vec![ExtractedSymbol {
            name: "Point".to_string(),
            kind: SymbolKind::Struct,
            signature: "struct Point { x: i32, y: i32, }".to_string(),
            range: LineRange { start: 1, end: 4 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_container_when_method_inside_impl_block_changed() {
        let source = "\
struct Foo;

impl Foo {
    fn bar(&self) -> i32 {
        42
    }
}
";
        let lang = RustSupport;
        // Line 5 (`42`) is inside `bar`'s body.
        let changed_ranges = vec![LineRange { start: 5, end: 5 }];

        let expected = vec![ExtractedSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::Function,
            signature: "fn bar(&self) -> i32".to_string(),
            range: LineRange { start: 4, end: 6 },
            container: Some("impl Foo".to_string()),
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_container_when_method_signature_inside_impl_block_changed() {
        let source = "\
struct Foo;

impl Foo {
    fn bar(&self, extra: i32) -> i32 {
        extra
    }
}
";
        let lang = RustSupport;
        // Line 4 is the method's own signature line, not its body.
        let changed_ranges = vec![LineRange { start: 4, end: 4 }];

        let expected = vec![ExtractedSymbol {
            name: "bar".to_string(),
            kind: SymbolKind::Function,
            signature: "fn bar(&self, extra: i32) -> i32".to_string(),
            range: LineRange { start: 4, end: 6 },
            container: Some("impl Foo".to_string()),
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_full_enum_signature_when_variant_changed() {
        let source = "\
enum Color {
    Red,
    Green,
    Blue,
}
";
        let lang = RustSupport;
        // Line 3 (`Green,`) is one variant among several.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected = vec![ExtractedSymbol {
            name: "Color".to_string(),
            kind: SymbolKind::Enum,
            signature: "enum Color { Red, Green, Blue, }".to_string(),
            range: LineRange { start: 1, end: 5 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_method_signature_with_trait_container_when_method_declaration_changed() {
        let source = "\
trait Greeter {
    fn greet(&self) -> String;
}
";
        let lang = RustSupport;
        let changed_ranges = vec![LineRange { start: 2, end: 2 }];

        // The changed line is fully inside `fn greet(...)`'s own range, so
        // that method signature is reported (with its trait as container)
        // rather than the whole trait body — same "narrowest enclosing
        // definition" rule used for impl methods.
        let expected = vec![ExtractedSymbol {
            name: "greet".to_string(),
            kind: SymbolKind::Function,
            signature: "fn greet(&self) -> String;".to_string(),
            range: LineRange { start: 2, end: 2 },
            container: Some("trait Greeter".to_string()),
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_trait_signature_when_no_method_line_specifically_changed() {
        let source = "\
trait Greeter {
    fn greet(&self) -> String;
}
";
        let lang = RustSupport;
        // Line 1 (`trait Greeter {`) belongs to the trait node but not to
        // any single method signature inside it.
        let changed_ranges = vec![LineRange { start: 1, end: 1 }];

        let expected = vec![ExtractedSymbol {
            name: "Greeter".to_string(),
            kind: SymbolKind::Trait,
            signature: "trait Greeter { fn greet(&self) -> String; }".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
        let source = "\
fn foo() {}

const X: i32 = 1;
";
        let lang = RustSupport;
        // Line 3 is a top-level const item, not covered by definition_query.
        let changed_ranges = vec![LineRange { start: 3, end: 3 }];

        let expected: Vec<ExtractedSymbol> = Vec::new();
        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_extract_only_the_touched_function_when_two_functions_exist(
        vec![LineRange { start: 2, end: 2 }],
        vec![ExtractedSymbol {
            name: "first".to_string(),
            kind: SymbolKind::Function,
            signature: "fn first()".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
        }],
    )]
    fn extract_changed_symbols_selective_cases(
        #[case] changed_ranges: Vec<LineRange>,
        #[case] expected: Vec<ExtractedSymbol>,
    ) {
        let source = "\
fn first() {
    1
}

fn second() {
    2
}
";
        let lang = RustSupport;

        let actual = extract_changed_symbols(source, &lang, &changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_rust_file() {
        use crate::diff::parse_unified_diff;
        use crate::language::language_for_path;

        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo(a: i32) -> i32 {
-    a
+    a + 1
 }
";
        let source = "\
fn foo(a: i32) -> i32 {
    a + 1
}
";
        let changed_file = parse_unified_diff(diff)
            .expect("diff should parse")
            .into_iter()
            .next()
            .expect("diff should contain one changed file");
        let lang = language_for_path(&changed_file.path).expect("*.rs should resolve to Rust");

        let expected = vec![ExtractedSymbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "fn foo(a: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
        }];
        let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_skip_file_end_to_end_when_extension_is_unsupported() {
        use crate::language::language_for_path;

        // Registry lookup, not extraction: an unsupported extension means
        // the pipeline never reaches `extract_changed_symbols` for this
        // file — there is no `LanguageSupport` to pass it.
        let actual = language_for_path("src/notes.txt");

        assert!(actual.is_none());
    }
}
