//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Rust sources: function,
//! struct, enum, trait, and impl signatures; comment stripping in kept
//! signature text; `#[cfg(test)]` / `#[test]` test detection; the noise
//! filter shared by `collect_referenced_names`; and the Rust end-to-end
//! path via `parse_unified_diff` + `language_for_path`.

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
fn should_extract_every_definition_regardless_of_changed_ranges() {
    let source = "\
fn helper(x: i32) -> i32 {
    x
}

struct Point {
    x: i32,
}
";
    let lang = RustSupport;

    let expected = vec![
        ExtractedSymbol {
            id: String::new(),
            name: "helper".to_string(),
            kind: SymbolKind::Function,
            signature: "fn helper(x: i32) -> i32".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
        ExtractedSymbol {
            id: String::new(),
            name: "Point".to_string(),
            kind: SymbolKind::Struct,
            signature: "struct Point { x: i32, }".to_string(),
            range: LineRange { start: 5, end: 7 },
            container: None,
            referenced_names: vec!["Point".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
    ];
    let actual = extract_all_symbols(source, &lang);

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_underscore_and_single_char_identifiers_from_referenced_names() {
    let source = "\
fn foo() -> i32 {
    let _ = bar();
    let a = 1;
    x(a)
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    // `bar` and `x` are real call targets (length > 1, not `_`), kept.
    // A bare `_` is never captured as a `call_expression` callee by
    // Rust's grammar, so this test instead exercises the general
    // filter shared by every language's `collect_referenced_names`
    // call site, which must drop both `_` and any single-character
    // identifier (e.g. Python/TS's common but never-informative `x`,
    // `_` local names) as noise unlikely to resolve to a meaningful,
    // uniquely named definition.
    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "fn foo() -> i32".to_string(),
        range: LineRange { start: 1, end: 5 },
        container: None,
        referenced_names: vec!["bar".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_mark_symbol_as_test_when_nested_inside_cfg_test_mod() {
    let source = "\
#[cfg(test)]
mod tests {
    fn helper() {}
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "helper".to_string(),
        kind: SymbolKind::Function,
        signature: "fn helper()".to_string(),
        range: LineRange { start: 3, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: true,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_mark_symbol_as_test_when_function_has_test_attribute() {
    let source = "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "should_add_two_numbers".to_string(),
        kind: SymbolKind::Function,
        signature: "fn should_add_two_numbers()".to_string(),
        // Note: the `function_item` node's own range starts at the
        // `fn` line, not the `#[test]` attribute line above it — same
        // convention as Python's decorator handling (see
        // `should_not_detect_change_when_only_decorator_line_changed`).
        range: LineRange { start: 2, end: 4 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: true,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_mark_symbol_as_test_when_function_has_no_test_marker() {
    let source = "\
fn helper() -> i32 {
    42
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "helper".to_string(),
        kind: SymbolKind::Function,
        signature: "fn helper() -> i32".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_return_empty_vec_when_source_has_no_definitions() {
    let source = "const X: i32 = 1;\n";
    let lang = RustSupport;

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_all_symbols(source, &lang);

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
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "fn foo(a: i32) -> i32".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "fn foo(a: i32, c: i32) -> i32".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Struct,
        signature: "struct Point { x: i32, y: i32, }".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        // The struct's own name appears as a `type_identifier` too
        // (it is the definition's declared name), so it is captured
        // as a reference the same as any other type mention. `deps.rs`
        // filters self-references before resolving.
        referenced_names: vec!["Point".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

// ADR 0014: comment nodes inside a definition's kept signature text must
// be stripped, not just implementation bodies — otherwise a comment-only
// edit inside a struct would produce a different signature string and
// falsely register as a contract change.
#[test]
fn should_strip_line_and_block_comments_from_struct_signature() {
    let source = "\
struct Point {
    // a line comment
    x: i32, /* a block comment */
    y: i32,
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Struct,
        signature: "struct Point { x: i32, y: i32, }".to_string(),
        range: LineRange { start: 1, end: 5 },
        container: None,
        referenced_names: vec!["Point".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

// Comments inside a function's declaration prefix (before the body)
// must also be stripped — this is the part of the signature that
// actually survives into the reported `signature` string.
#[test]
fn should_strip_comment_from_function_signature_line() {
    let source = "\
fn foo(/* count */ a: i32) -> i32 {
    a
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "fn foo( a: i32) -> i32".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "bar".to_string(),
        kind: SymbolKind::Function,
        signature: "fn bar(&self) -> i32".to_string(),
        range: LineRange { start: 4, end: 6 },
        container: Some("impl Foo".to_string()),
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "bar".to_string(),
        kind: SymbolKind::Function,
        signature: "fn bar(&self, extra: i32) -> i32".to_string(),
        range: LineRange { start: 4, end: 6 },
        container: Some("impl Foo".to_string()),
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "Color".to_string(),
        kind: SymbolKind::Enum,
        signature: "enum Color { Red, Green, Blue, }".to_string(),
        range: LineRange { start: 1, end: 5 },
        container: None,
        // Same self-reference note as the struct case above: the
        // enum's own name is a `type_identifier`.
        referenced_names: vec!["Color".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "greet".to_string(),
        kind: SymbolKind::Function,
        signature: "fn greet(&self) -> String;".to_string(),
        range: LineRange { start: 2, end: 2 },
        container: Some("trait Greeter".to_string()),
        referenced_names: vec!["String".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "Greeter".to_string(),
        kind: SymbolKind::Trait,
        signature: "trait Greeter { fn greet(&self) -> String; }".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        // The trait's own name, its "greet" method name (ADR 0012
        // decision 2), and the referenced `String` return type of its
        // method signature.
        referenced_names: vec![
            "Greeter".to_string(),
            "String".to_string(),
            "greet".to_string(),
        ],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_include_both_bodiless_and_default_body_method_names_in_trait_referenced_names() {
    let source = "\
trait Repo {
    fn save(&self, id: &str);

    fn label(&self) -> String {
        String::new()
    }
}
";
    let lang = RustSupport;
    // Line 1 (`trait Repo {`) belongs to the trait node but not to
    // either method signature inside it, so the trait itself (not a
    // narrower method) is the reported symbol — same rule as
    // `should_extract_trait_signature_when_no_method_line_specifically_changed`.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Repo".to_string(),
        kind: SymbolKind::Trait,
        signature:
            "trait Repo { fn save(&self, id: &str); fn label(&self) -> String { String::new() } }"
                .to_string(),
        range: LineRange { start: 1, end: 7 },
        container: None,
        // Both the bodiless `save` signature and the default-body
        // `label` method contribute their names (ADR 0012 decision 2),
        // alongside the trait's own name and referenced types. `str`
        // is a `primitive_type` node in this grammar, not
        // `type_identifier`, so it is not captured as a reference (see
        // REFERENCE_QUERY's doc comment).
        referenced_names: vec![
            "Repo".to_string(),
            "String".to_string(),
            "label".to_string(),
            "save".to_string(),
        ],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "first".to_string(),
        kind: SymbolKind::Function,
        signature: "fn first()".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "fn foo(a: i32) -> i32".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
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
