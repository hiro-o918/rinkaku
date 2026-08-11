//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Python sources: function
//! and class signatures with nested method bodies stripped, decorator
//! and nested-function edge cases (a decorator extends a definition's
//! span up to the decorator line itself — ADR 0073), comment stripping
//! inside class signatures, and the Python end-to-end path via
//! `parse_unified_diff` + `language_for_path`.

use super::*;
use crate::language::python::PythonSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_dedent_field_after_consecutive_comments() {
    let source = "\
class Foo:
    # one
    # two
    # three
    a: str
    b: str
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 5, end: 5 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Foo".to_string(),
        kind: SymbolKind::Class,
        signature: "class Foo:\n\n    a: str\n    b: str".to_string(),
        range: LineRange { start: 1, end: 6 },
        container: None,
        referenced_names: vec!["str".to_string()],
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
fn should_detect_change_when_only_decorator_line_changed() {
    let source = "\
@decorator_v2
def decorated(a):
    return a
";
    let lang = PythonSupport;
    // Line 1 is the decorator — `PythonSupport::definition_span_start`
    // widens `function_definition`'s span to include its
    // `decorated_definition` wrapper (ADR 0073), so a decorator-only
    // change is detected and the decorator is included in the reported
    // signature.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "decorated".to_string(),
        kind: SymbolKind::Function,
        signature: "@decorator_v2\ndef decorated(a):".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
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
        // The decorator is included in the signature (ADR 0073) even
        // though only the body line changed: the reported span always
        // starts at the decorator once one is present.
        signature: "@decorator\ndef decorated(a):".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
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
fn should_detect_change_when_only_class_decorator_line_changed() {
    let source = "\
@dataclass
class Point:
    x: int
    y: int
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Point".to_string(),
        kind: SymbolKind::Class,
        signature: "@dataclass\nclass Point:\n    x: int\n    y: int".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec!["int".to_string()],
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

// A decorator on a class *method* is covered by the same
// `decorated_definition`-parent check as a top-level function (ADR
// 0073) — the method itself, not the enclosing class, is reported as
// the narrowest touched definition.
#[test]
fn should_report_only_the_method_when_only_a_class_methods_decorator_line_changed() {
    let source = "\
class Widget:
    @property
    def label(self):
        return self._label
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "label".to_string(),
        kind: SymbolKind::Function,
        signature: "@property\ndef label(self):".to_string(),
        range: LineRange { start: 2, end: 4 },
        container: Some("class Widget".to_string()),
        referenced_names: vec![],
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
fn should_detect_change_when_a_decorator_is_newly_added() {
    let source = "\
@decorator
def foo(a):
    return a
";
    let lang = PythonSupport;
    // Diff-line-count semantics: an added decorator shifts the
    // definition's own diff-reported start down by one line versus the
    // undecorated base, but the changed range here is simply the new
    // decorator line itself.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "@decorator\ndef foo(a):".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
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

// NOTE: in both tests below, `__init__` is untouched by the diff and is
// dropped from the reported class signature entirely, body and
// signature line alike (ADR 0071).
#[test]
fn should_extract_class_signature_with_untouched_method_dropped_when_field_changed() {
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
        signature: "class Point:\n    x: int\n    y: int".to_string(),
        range: LineRange { start: 1, end: 7 },
        container: None,
        // "int" is the shared field-annotation type of both `x`
        // and `y`, deduplicated to a single entry.
        referenced_names: vec!["int".to_string()],
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
        signature: "class Point:\n\n    x: int\n    y: int".to_string(),
        range: LineRange { start: 1, end: 8 },
        container: None,
        referenced_names: vec!["int".to_string()],
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

// Regression for the dedent bug ADR 0060 shipped with: a nested method's
// node text starts mid-line (no leading indentation on its own first
// line) while its continuation lines still carry their absolute source
// column, so computing `min_indent` over every line (including the
// unindented first one) always floored it at 0 and left continuation
// lines at their raw source indentation instead of dedented.
#[test]
fn should_dedent_continuation_lines_when_multiline_method_signature_is_nested_in_class() {
    let source = "\
class Point:
    def __init__(
        self,
        x,
        y,
    ):
        self.x = x
";
    let lang = PythonSupport;
    // Line 7 (`self.x = x`) is inside `__init__`'s body.
    let changed_ranges = vec![LineRange { start: 7, end: 7 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "__init__".to_string(),
        kind: SymbolKind::Function,
        signature: "def __init__(\n    self,\n    x,\n    y,\n):".to_string(),
        range: LineRange { start: 2, end: 7 },
        container: Some("class Point".to_string()),
        referenced_names: vec![],
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

// ADR 0071 + ADR 0073: when the container itself is reported (a
// body-level line touched, not any member), an untouched decorated
// method is dropped from the signature decorator and all — not left
// behind as an orphaned `@property` line.
#[test]
fn should_drop_untouched_decorated_members_decorator_from_container_signature() {
    let source = "\
class Widget:
    label = \"a\"

    @property
    def untouched(self):
        return self._untouched
";
    let lang = PythonSupport;
    // Line 2 (`label = \"a\"`) is a body-level line, not inside any
    // method, so the class itself is the narrowest touched definition.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Widget".to_string(),
        kind: SymbolKind::Class,
        signature: "class Widget:\n    label = \"a\"".to_string(),
        range: LineRange { start: 1, end: 6 },
        container: None,
        referenced_names: vec![],
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

// A decorated method whose own body line is touched is reported on its
// own (narrowest-enclosing-definition rule), and its decorator is kept
// as part of that reported signature (ADR 0073) — the container is
// never reported in this case, so container-slicing does not apply.
#[test]
fn should_keep_touched_members_decorator_when_member_itself_is_reported() {
    let source = "\
class Widget:
    @property
    def touched(self):
        return self._touched
";
    let lang = PythonSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "touched".to_string(),
        kind: SymbolKind::Function,
        signature: "@property\ndef touched(self):".to_string(),
        range: LineRange { start: 2, end: 4 },
        container: Some("class Widget".to_string()),
        referenced_names: vec![],
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
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

    assert_eq!(expected, actual);
}
