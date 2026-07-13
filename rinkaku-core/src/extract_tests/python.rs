//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Python sources: function
//! and class signatures with nested method bodies stripped, decorator
//! and nested-function edge cases (decorators do not extend a
//! definition's range up to the decorator line), comment stripping
//! inside class signatures, and the Python end-to-end path via
//! `parse_unified_diff` + `language_for_path`.

use super::*;
use crate::language::python::PythonSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_extract_function_signature_when_body_line_changed() {
    let source = "\
def foo(a):
    b = a + 1
    return b
";
    let lang = PythonSupport;
    // Line 2 (`b = a + 1`) is inside the body only.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "def foo(a):".to_string(),
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
def foo(a, c):
    return a + c
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "def foo(a, c):".to_string(),
        range: LineRange { start: 1, end: 2 },
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
fn should_extract_only_the_inner_function_when_nested_function_body_changed() {
    let source = "\
def top_level(a, b):
    def inner(c):
        return c + 1
    return inner(a) + b
";
    let lang = PythonSupport;
    // Line 3 (`return c + 1`) is inside `inner`'s body only.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    // A nested function is reported like any other function, with
    // no container: its nearest ancestor definition is another
    // `function_definition`, not a class, so `find_container`
    // walks past it and finds nothing (see extract.rs doc comment
    // on `find_container`).
    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "inner".to_string(),
        kind: SymbolKind::Function,
        signature: "def inner(c):".to_string(),
        range: LineRange { start: 2, end: 3 },
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
fn should_not_detect_change_when_only_decorator_line_changed() {
    let source = "\
@decorator_v2
def decorated(a):
    return a
";
    let lang = PythonSupport;
    // Line 1 is the decorator, outside `function_definition`'s own
    // row range (see the doc comment on `DEFINITION_QUERY` in
    // language/python.rs) — a deliberate v1 simplification.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_decorated_function_signature_when_body_changed() {
    let source = "\
@decorator
def decorated(a):
    return a
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "decorated".to_string(),
        kind: SymbolKind::Function,
        signature: "def decorated(a):".to_string(),
        range: LineRange { start: 2, end: 3 },
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
class Point:
    x: int
    y: int

    def __init__(self, x, y):
        self.x = x
        self.y = y
";
    let lang = PythonSupport;
    // Line 3 (`y: int`) is a class-level field annotation, not
    // inside any method.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Class,
        signature: "class Point: x: int y: int def __init__(self, x, y):".to_string(),
        range: LineRange { start: 1, end: 7 },
        container: None,
        // "int" is the shared field-annotation type of both `x`
        // and `y`, deduplicated to a single entry.
        referenced_names: vec!["int".to_string()],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

// ADR 0014: a `#` comment inside the class body, outside any method,
// must be stripped from the reported signature just like a method
// body is.
#[test]
fn should_strip_comment_from_class_signature() {
    let source = "\
class Point:
    # a comment
    x: int
    y: int

    def __init__(self, x, y):
        self.x = x
        self.y = y
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Class,
        signature: "class Point: x: int y: int def __init__(self, x, y):".to_string(),
        range: LineRange { start: 1, end: 8 },
        container: None,
        referenced_names: vec!["int".to_string()],
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
class Point:
    def __init__(self, x):
        self.x = x
";
    let lang = PythonSupport;
    // Line 3 (`self.x = x`) is inside `__init__`'s body.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "__init__".to_string(),
        kind: SymbolKind::Function,
        signature: "def __init__(self, x):".to_string(),
        range: LineRange { start: 2, end: 3 },
        container: Some("class Point".to_string()),
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
class Point:
    def __init__(self, x):
        self.x = x
";
    let lang = PythonSupport;
    // Line 2 is the method's own signature line.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "__init__".to_string(),
        kind: SymbolKind::Function,
        signature: "def __init__(self, x):".to_string(),
        range: LineRange { start: 2, end: 3 },
        container: Some("class Point".to_string()),
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
class Point:
    def __init__(self, x):
        self.x = x

    def label(self):
        return str(self.x)
";
    let lang = PythonSupport;
    // Line 6 is inside `label`'s body only.
    let changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "label".to_string(),
        kind: SymbolKind::Function,
        signature: "def label(self):".to_string(),
        range: LineRange { start: 5, end: 6 },
        container: Some("class Point".to_string()),
        // `str(self.x)` is a call to the bare identifier `str`
        // (Python has no distinct built-in-type node kind, so
        // `str` is captured the same as any user-defined callable
        // — see REFERENCE_QUERY's doc comment in
        // language/python.rs).
        referenced_names: vec!["str".to_string()],
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
def foo():
    pass

X = 1
";
    let lang = PythonSupport;
    // Line 4 is a top-level assignment, not covered by
    // definition_query.
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_python_file() {
    use crate::diff::parse_unified_diff;
    use crate::language::language_for_path;

    let diff = "\
diff --git a/point.py b/point.py
index e69de29..4b825dc 100644
--- a/point.py
+++ b/point.py
@@ -2,2 +2,2 @@
     def __init__(self, x):
-        self.x = 0
+        self.x = x
";
    let source = "\
class Point:
    def __init__(self, x):
        self.x = x
";
    let changed_file = parse_unified_diff(diff)
        .expect("diff should parse")
        .into_iter()
        .next()
        .expect("diff should contain one changed file");
    let lang = language_for_path(&changed_file.path).expect("*.py should resolve to Python");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "__init__".to_string(),
        kind: SymbolKind::Function,
        signature: "def __init__(self, x):".to_string(),
        range: LineRange { start: 2, end: 3 },
        container: Some("class Point".to_string()),
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
