//! Tests pinning reference-name collection on Rust sources
//! (`extract/references.rs`): the shared noise filter, scoped-path
//! captures, macro-body walking (ADR 0063), and module-scoped /
//! method-call captures with the ubiquitous-name stoplist (ADR 0064).
//! Split from `rust.rs` by topic when the ADR 0064 cases pushed it
//! over the warn threshold (ADR 0028).

use super::*;
use crate::language::rust::RustSupport;
use pretty_assertions::assert_eq;
use rstest::rstest;
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
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[rstest]
#[case::should_capture_path_type_when_enum_variant_constructed_via_scoped_path(
    "\
fn build() -> Format {
    OutputFormat::Markdown
}
",
    vec!["Format".to_string(), "OutputFormat".to_string()],
)]
#[case::should_capture_path_type_but_not_method_name_for_ufcs_call(
    "\
fn build() -> Format {
    Format::default()
}
",
    vec!["Format".to_string()],
)]
#[case::should_capture_self_when_used_as_scoped_path(
    "\
fn build() -> Format {
    Self::Default
}
",
    vec!["Format".to_string(), "Self".to_string()],
)]
#[case::should_capture_only_the_outermost_module_segment_for_a_three_segment_path(
    // A three-segment scoped path parses as nested `scoped_identifier`s,
    // each layer's `path` field holding the next one out; the query only
    // matches a `scoped_identifier` whose own `path` field is a bare
    // `identifier`, true only for the innermost `mods::sub` pair.
    // `Format` sits one level too deep and is not captured — a known,
    // accepted gap, not an oversight.
    "\
fn build() -> Format {
    mods::sub::Format::default()
}
",
    vec!["Format".to_string(), "mods".to_string()],
)]
fn should_collect_scoped_identifier_path_references(
    #[case] source: &str,
    #[case] referenced_names: Vec<String>,
) {
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "build".to_string(),
        kind: SymbolKind::Function,
        signature: "fn build() -> Format".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names,
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[rstest]
#[case::should_capture_called_name_when_path_is_a_lowercase_module(
    "\
fn build() -> Format {
    markdown::render_markdown(report)
}
",
    vec![
        "Format".to_string(),
        "markdown".to_string(),
        "render_markdown".to_string(),
    ],
    vec![],
)]
#[case::should_capture_called_name_when_path_is_super(
    "\
fn build() -> Format {
    super::helper(1)
}
",
    vec!["Format".to_string(), "helper".to_string()],
    vec![],
)]
#[case::should_capture_called_name_when_path_is_crate(
    "\
fn build() -> Format {
    crate::helper(1)
}
",
    vec!["Format".to_string(), "helper".to_string()],
    vec![],
)]
#[case::should_capture_method_name_when_called_on_a_receiver(
    "\
fn build() -> Format {
    state.advance_cursor()
}
",
    vec!["Format".to_string()],
    vec!["advance_cursor".to_string()],
)]
#[case::should_skip_ubiquitous_method_name_when_called_on_a_receiver(
    "\
fn build() -> Format {
    state.clone()
}
",
    vec!["Format".to_string()],
    vec![],
)]
#[case::should_keep_ubiquitous_name_when_called_as_a_free_function(
    "\
fn build() -> Format {
    get(1)
}
",
    vec!["Format".to_string(), "get".to_string()],
    vec![],
)]
fn should_collect_scoped_and_method_call_references(
    #[case] source: &str,
    #[case] referenced_names: Vec<String>,
    #[case] referenced_method_names: Vec<String>,
) {
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "build".to_string(),
        kind: SymbolKind::Function,
        signature: "fn build() -> Format".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names,
        referenced_method_names,
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
fn should_keep_stoplisted_trait_method_name_but_still_drop_it_from_a_receiver_call() {
    // Contrasts the two `@reference.method`-family captures on the same
    // stoplisted name (`get`, issue #230): a trait method spec is a
    // declaration and keeps its edge, while a receiver call on the same
    // name stays filtered by the ADR 0064 stoplist — scoping the filter
    // to receiver-call captures only.
    let source = "\
trait Cache {
    fn get(&self) -> Format;
}
fn build() -> Format {
    state.get()
}
";
    let lang = RustSupport;
    let changed_ranges = vec![
        LineRange { start: 1, end: 1 },
        LineRange { start: 4, end: 4 },
    ];

    let expected = vec![
        ExtractedSymbol {
            id: String::new(),
            name: "Cache".to_string(),
            kind: SymbolKind::Trait,
            signature: "trait Cache {\n    fn get(&self) -> Format;\n}".to_string(),
            range: LineRange { start: 1, end: 3 },
            container: None,
            referenced_names: vec!["Cache".to_string(), "Format".to_string()],
            referenced_method_names: vec!["get".to_string()],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
        ExtractedSymbol {
            id: String::new(),
            name: "build".to_string(),
            kind: SymbolKind::Function,
            signature: "fn build() -> Format".to_string(),
            range: LineRange { start: 4, end: 6 },
            container: None,
            referenced_names: vec!["Format".to_string()],
            referenced_method_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
    ];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[rstest]
#[case::should_collect_call_and_path_identifiers_inside_macro_body(
    "\
fn build() -> Format {
    assert_eq!(inner(), Config::V1)
}
",
    vec![
        "Config".to_string(),
        "Format".to_string(),
        "V1".to_string(),
        "inner".to_string(),
    ],
)]
#[case::should_skip_nested_macro_name_inside_macro_body(
    "\
fn build() -> Format {
    assert!(matches!(value, Foo::Bar))
}
",
    vec![
        "Bar".to_string(),
        "Foo".to_string(),
        "Format".to_string(),
        "value".to_string(),
    ],
)]
#[case::should_collect_identifiers_inside_bracket_macro_body(
    "\
fn build() -> Format {
    vec![make_thing(input)]
}
",
    vec![
        "Format".to_string(),
        "input".to_string(),
        "make_thing".to_string(),
    ],
)]
#[case::should_keep_identifier_before_not_equal_operator_inside_macro_body(
    "\
fn build() -> Format {
    assert!(left != right)
}
",
    vec![
        "Format".to_string(),
        "left".to_string(),
        "right".to_string(),
    ],
)]
fn should_collect_macro_body_references(
    #[case] source: &str,
    #[case] referenced_names: Vec<String>,
) {
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "build".to_string(),
        kind: SymbolKind::Function,
        signature: "fn build() -> Format".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names,
        referenced_method_names: vec![],
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
fn should_not_collect_attribute_arguments_as_referenced_names() {
    // `#[derive(...)]`/`#[cfg(...)]` arguments are token_trees too, but
    // under an `attribute` node — the macro-body walk (ADR 0063) must not
    // turn derive names into references.
    let source = "\
#[derive(Debug, Clone)]
struct Point {
    x: i32,
}
";
    let lang = RustSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Struct,
        signature: "struct Point {\n    x: i32,\n}".to_string(),
        range: LineRange { start: 2, end: 4 },
        container: None,
        referenced_names: vec!["Point".to_string()],
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}
