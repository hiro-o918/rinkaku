//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on TypeScript / TSX sources:
//! functions, interfaces, type aliases, enums, arrow-function const
//! bindings via `variable_declarator`, class field arrow-function body
//! stripping, abstract classes / methods, comment stripping in kept
//! signature text, and the TypeScript / TSX end-to-end paths via
//! `parse_unified_diff` + `language_for_path`.

use super::*;
use crate::language::typescript::TypeScriptSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_extract_function_signature_when_body_line_changed() {
    let source = "\
function foo(a: number): number {
    return a + 1;
}
";
    let lang = TypeScriptSupport;
    // Line 2 (`return a + 1;`) is inside the body only.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "function foo(a: number): number".to_string(),
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
fn should_extract_function_signature_when_signature_line_changed() {
    let source = "\
function foo(a: number, c: number): number {
    return a + c;
}
";
    let lang = TypeScriptSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "function foo(a: number, c: number): number".to_string(),
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
fn should_extract_arrow_function_signature_when_const_bound_body_changed() {
    let source = "\
const arrow = (a: number): number => {
    return a + 1;
};
";
    let lang = TypeScriptSupport;
    // Line 2 is inside the arrow function's body only.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "arrow".to_string(),
        kind: SymbolKind::Function,
        signature: "arrow = (a: number): number =>".to_string(),
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
fn should_not_report_plain_const_binding_as_a_symbol() {
    let source = "\
const notArrow = 5;

function useIt(): number {
    return notArrow;
}
";
    let lang = TypeScriptSupport;
    // Line 1 is a plain (non-arrow-function) const binding.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_full_interface_signature_when_method_signature_changed() {
    let source = "\
interface Shape {
    area(): number;
    perimeter(): number;
}
";
    let lang = TypeScriptSupport;
    // Line 3 (`perimeter(): number;`) is one member among several.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Shape".to_string(),
        kind: SymbolKind::Interface,
        signature: "interface Shape { area(): number; perimeter(): number; }".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        // The interface's own name is a `type_identifier` (self-
        // reference, filtered later by deps.rs); `area`/`perimeter`
        // are its method signature names (ADR 0012 decision 2);
        // `number` is TypeScript's built-in `predefined_type`, a
        // distinct node kind the reference query does not capture.
        referenced_names: vec![
            "Shape".to_string(),
            "area".to_string(),
            "perimeter".to_string(),
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
fn should_not_include_property_signature_names_in_interface_referenced_names() {
    let source = "\
interface Repo {
    id: string;
    save(item: string): void;
}
";
    let lang = TypeScriptSupport;
    // Line 2 (`id: string;`) is a plain data field, not a method
    // signature; touching it (rather than the `save` line) still
    // reports the whole interface since neither member line is
    // itself the interface's own declaration line, but keeps this
    // test focused on the `referenced_names` distinction between a
    // property and a method signature.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Repo".to_string(),
        kind: SymbolKind::Interface,
        signature: "interface Repo { id: string; save(item: string): void; }".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        // "id" (a `property_signature` name) is deliberately
        // excluded; only "save" (a `method_signature` name) is
        // included alongside the interface's own name.
        referenced_names: vec!["Repo".to_string(), "save".to_string()],
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
fn should_extract_full_type_alias_signature_when_member_changed() {
    let source = "\
type Point = {
    x: number;
    y: number;
};
";
    let lang = TypeScriptSupport;
    // Line 3 (`y: number;`) is one member of the object type.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::TypeAlias,
        signature: "type Point = { x: number; y: number; };".to_string(),
        range: LineRange { start: 1, end: 4 },
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

#[test]
fn should_extract_full_enum_signature_when_member_changed() {
    let source = "\
enum Color {
    Red,
    Green,
    Blue,
}
";
    let lang = TypeScriptSupport;
    // Line 3 (`Green,`) is one variant among several.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Color".to_string(),
        kind: SymbolKind::Enum,
        signature: "enum Color { Red, Green, Blue, }".to_string(),
        range: LineRange { start: 1, end: 5 },
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
fn should_extract_class_signature_with_method_bodies_stripped_when_field_changed() {
    let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
    let lang = TypeScriptSupport;
    // Line 2 (`radius: number;`) is a class-level field, not
    // inside any method.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Circle".to_string(),
        kind: SymbolKind::Class,
        signature: "class Circle { radius: number; area(): number }".to_string(),
        range: LineRange { start: 1, end: 7 },
        container: None,
        referenced_names: vec!["Circle".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

// ADR 0014: both `//` and `/* */` comments in this grammar parse
// under the same `comment` node kind (unlike Rust's split), and
// both must be stripped from a class signature.
#[test]
fn should_strip_line_and_block_comments_from_class_signature() {
    let source = "\
class Circle {
    // a line comment
    radius: number; /* a block comment */

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
    let lang = TypeScriptSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Circle".to_string(),
        kind: SymbolKind::Class,
        signature: "class Circle { radius: number; area(): number }".to_string(),
        range: LineRange { start: 1, end: 8 },
        container: None,
        referenced_names: vec!["Circle".to_string()],
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
fn should_set_container_to_class_name_when_method_body_changed() {
    let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
    let lang = TypeScriptSupport;
    // Line 5 is inside `area`'s body.
    let changed_ranges = vec![LineRange { start: 5, end: 5 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "area".to_string(),
        kind: SymbolKind::Function,
        signature: "area(): number".to_string(),
        range: LineRange { start: 4, end: 6 },
        container: Some("class Circle".to_string()),
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
fn should_set_container_to_class_name_when_method_signature_changed() {
    let source = "\
class Circle {
    radius: number;

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
    let lang = TypeScriptSupport;
    // Line 4 is the method's own signature line.
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "area".to_string(),
        kind: SymbolKind::Function,
        signature: "area(): number".to_string(),
        range: LineRange { start: 4, end: 6 },
        container: Some("class Circle".to_string()),
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
fn should_extract_only_the_touched_method_when_class_has_two_methods() {
    let source = "\
class Circle {
    constructor(radius: number) {
        this.radius = radius;
    }

    area(): number {
        return 3.14 * this.radius * this.radius;
    }
}
";
    let lang = TypeScriptSupport;
    // Line 7 is inside `area`'s body only.
    let changed_ranges = vec![LineRange { start: 7, end: 7 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "area".to_string(),
        kind: SymbolKind::Function,
        signature: "area(): number".to_string(),
        range: LineRange { start: 6, end: 8 },
        container: Some("class Circle".to_string()),
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
fn should_return_empty_vec_when_changed_line_is_outside_any_definition() {
    let source = "\
function foo(): void {}

const X: number = 1;
";
    let lang = TypeScriptSupport;
    // Line 3 is a top-level, non-arrow-function const binding, not
    // covered by definition_query.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_typescript_file() {
    use crate::diff::parse_unified_diff;
    use crate::language::language_for_path;

    let diff = "\
diff --git a/shape.ts b/shape.ts
index e69de29..4b825dc 100644
--- a/shape.ts
+++ b/shape.ts
@@ -1,3 +1,3 @@
 function foo(a: number): number {
-    return a;
+    return a + 1;
 }
";
    let source = "\
function foo(a: number): number {
    return a + 1;
}
";
    let changed_file = parse_unified_diff(diff)
        .expect("diff should parse")
        .into_iter()
        .next()
        .expect("diff should contain one changed file");
    let lang = language_for_path(&changed_file.path).expect("*.ts should resolve to TypeScript");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "function foo(a: number): number".to_string(),
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
fn should_extract_abstract_method_signature_with_class_container_when_abstract_method_line_changed()
{
    let source = "\
abstract class Shape {
    abstract area(): number;
    abstract perimeter(): number;
}
";
    let lang = TypeScriptSupport;
    // Line 3 (`abstract perimeter(): number;`) is fully inside that
    // method's own node range, so — same "narrowest enclosing
    // definition" rule as Rust trait methods — the method itself is
    // reported (with its class as container) rather than the whole
    // class body.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "perimeter".to_string(),
        kind: SymbolKind::Function,
        signature: "abstract perimeter(): number".to_string(),
        range: LineRange { start: 3, end: 3 },
        container: Some("class Shape".to_string()),
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
fn should_extract_abstract_class_signature_when_no_member_line_specifically_changed() {
    let source = "\
abstract class Shape {
    abstract area(): number;
    abstract perimeter(): number;
}
";
    let lang = TypeScriptSupport;
    // Line 1 (`abstract class Shape {`) belongs to the class node
    // but not to any single member signature inside it.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Shape".to_string(),
        kind: SymbolKind::Class,
        signature:
            "abstract class Shape { abstract area(): number; abstract perimeter(): number; }"
                .to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec!["Shape".to_string()],
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
fn should_strip_arrow_function_body_when_class_field_arrow_function_signature_changed() {
    let source = "\
class Circle {
    radius: number;

    area = (): number => {
        return 3.14 * this.radius * this.radius;
    };
}
";
    let lang = TypeScriptSupport;
    // Line 2 (`radius: number;`) is a class-level field, not
    // inside the arrow function body — the extracted class
    // signature must still have the arrow function's body
    // stripped, matching how `method_definition` bodies are
    // stripped.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Circle".to_string(),
        kind: SymbolKind::Class,
        signature: "class Circle { radius: number; area = (): number => ; }".to_string(),
        range: LineRange { start: 1, end: 7 },
        container: None,
        // The reference query runs over the full node (including
        // the arrow function's body, which is only stripped from
        // the rendered *signature* text, not from the tree
        // `collect_referenced_names` walks) but `this.radius` is
        // a member expression, not a bare identifier, so it is
        // not captured; only the class's own self-reference is.
        referenced_names: vec!["Circle".to_string()],
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
fn should_resolve_tsx_extension_end_to_end_and_extract_arrow_component() {
    use crate::language::language_for_path;

    let source = "\
const Component = () => {
    return 1;
};
";
    // Line 2 is inside the arrow function's body only.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];
    let lang = language_for_path("src/Component.tsx").expect("*.tsx should resolve to TSX");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Component".to_string(),
        kind: SymbolKind::Function,
        signature: "Component = () =>".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, lang, &changed_ranges);

    assert_eq!(expected, actual);
}
