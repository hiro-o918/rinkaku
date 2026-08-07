//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Go sources: function and
//! struct/interface signatures via `type_spec`, receiver-based container
//! naming for pointer- and value-receiver `method_declaration` nodes
//! (stripping the leading `*`), type alias filtering, comment stripping
//! in kept signature text, and the Go end-to-end path via
//! `parse_unified_diff` + `language_for_path`.

use super::*;
use crate::language::go::GoSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_dedent_field_after_consecutive_comments() {
    let source = "\
package main

type Foo struct {
	// one
	// two
	// three
	A string
	B string
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 7, end: 7 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Foo".to_string(),
        kind: SymbolKind::Struct,
        signature: "Foo struct {\n\n\tA string\n\tB string\n}".to_string(),
        range: LineRange { start: 3, end: 9 },
        container: None,
        referenced_names: vec!["Foo".to_string(), "string".to_string()],
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
package main

func foo(a int) int {
	b := a + 1
	return b
}
";
    let lang = GoSupport;
    // Line 4 (`b := a + 1`) is inside the body only.
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "func foo(a int) int".to_string(),
        range: LineRange { start: 3, end: 6 },
        container: None,
        // Go has no distinct node kind for built-in types: `int`
        // parses as `type_identifier`, same as a user-defined
        // type, and is captured the same way (see the doc comment
        // on `REFERENCE_QUERY` in language/go.rs).
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
fn should_extract_function_signature_when_signature_line_changed() {
    let source = "\
package main

func foo(a int, c int) int {
	return a + c
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        signature: "func foo(a int, c int) int".to_string(),
        range: LineRange { start: 3, end: 5 },
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
fn should_extract_full_struct_signature_when_field_changed() {
    let source = "\
package main

type Repo struct {
	Name string
	Size int
}
";
    let lang = GoSupport;
    // Line 5 (`Size int`) is a field, not a separate body.
    let changed_ranges = vec![LineRange { start: 5, end: 5 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Repo".to_string(),
        kind: SymbolKind::Struct,
        signature: "Repo struct {\n\tName string\n\tSize int\n}".to_string(),
        range: LineRange { start: 3, end: 6 },
        container: None,
        // "Repo" is the struct's own name (self-reference,
        // filtered later by deps.rs); "string"/"int" are field
        // types, built-in but syntactically indistinguishable
        // from user types in Go (see REFERENCE_QUERY's doc
        // comment).
        referenced_names: vec!["Repo".to_string(), "int".to_string(), "string".to_string()],
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

// ADR 0014: Go uses a single `comment` node kind for `//` comments
// (no `block_comment` split, unlike Rust).
#[test]
fn should_strip_comment_from_struct_signature() {
    let source = "\
package main

type Repo struct {
	// a comment
	Name string
	Size int
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Repo".to_string(),
        kind: SymbolKind::Struct,
        signature: "Repo struct {\n\n\tName string\n\tSize int\n}".to_string(),
        range: LineRange { start: 3, end: 7 },
        container: None,
        referenced_names: vec!["Repo".to_string(), "int".to_string(), "string".to_string()],
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
fn should_extract_full_interface_signature_when_method_elem_changed() {
    let source = "\
package main

type Fetcher interface {
	Fetch(id string) (string, error)
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Fetcher".to_string(),
        kind: SymbolKind::Interface,
        signature: "Fetcher interface {\n\tFetch(id string) (string, error)\n}".to_string(),
        range: LineRange { start: 3, end: 5 },
        container: None,
        // "Fetcher"/"error"/"string" are the interface's own name
        // and its referenced parameter/return types (bare type
        // references). "Fetch" is the interface's own method spec
        // name (ADR 0012 decision 2), captured under
        // `@reference.methodspec` (ADR 0068) since it may denote a
        // symbol nested inside a container (a receiver method).
        referenced_names: vec![
            "Fetcher".to_string(),
            "error".to_string(),
            "string".to_string(),
        ],
        referenced_method_names: vec!["Fetch".to_string()],
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
fn should_include_every_method_spec_name_in_referenced_method_names_when_interface_has_multiple_methods()
 {
    let source = "\
package main

type Repo interface {
	Save(id string) error
	Delete(id string) error
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 6 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Repo".to_string(),
        kind: SymbolKind::Interface,
        signature: "Repo interface {\n\tSave(id string) error\n\tDelete(id string) error\n}"
            .to_string(),
        range: LineRange { start: 3, end: 6 },
        container: None,
        referenced_names: vec![
            "Repo".to_string(),
            "error".to_string(),
            "string".to_string(),
        ],
        referenced_method_names: vec!["Delete".to_string(), "Save".to_string()],
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
fn should_keep_stoplisted_interface_method_name_in_referenced_method_names() {
    // Unexported (lowercase) interface method names are the case Go
    // exposes to the ADR 0064 stoplist: the stoplist is scoped to
    // receiver-call captures (issue #230), so a method spec still keeps
    // its spec-to-implementation edge even when its name collides
    // (`get`).
    let source = "\
package main

type cache interface {
	get(key string) string
}
";
    let lang = GoSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "cache".to_string(),
        kind: SymbolKind::Interface,
        signature: "cache interface {\n\tget(key string) string\n}".to_string(),
        range: LineRange { start: 3, end: 5 },
        container: None,
        referenced_names: vec!["cache".to_string(), "string".to_string()],
        referenced_method_names: vec!["get".to_string()],
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
fn should_not_report_type_alias_as_a_symbol() {
    let source = "\
package main

type Alias = string

func useAlias(a Alias) Alias {
	return a
}
";
    let lang = GoSupport;
    // Line 3 is the plain type alias declaration.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_set_container_to_receiver_type_when_pointer_receiver_method_body_changed() {
    let source = "\
package main

type Repo struct {
	Name string
}

func (r *Repo) Save(id string) error {
	return nil
}
";
    let lang = GoSupport;
    // Line 8 (`return nil`) is inside `Save`'s body.
    let changed_ranges = vec![LineRange { start: 8, end: 8 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Save".to_string(),
        kind: SymbolKind::Function,
        signature: "func (r *Repo) Save(id string) error".to_string(),
        range: LineRange { start: 7, end: 9 },
        container: Some("Repo".to_string()),
        // "Repo" comes from the pointer receiver's type
        // (`*Repo`); the `*` prefix is not part of the
        // `type_identifier` node, so the reference query captures
        // the bare type name.
        referenced_names: vec![
            "Repo".to_string(),
            "error".to_string(),
            "string".to_string(),
        ],
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
fn should_set_container_to_receiver_type_when_value_receiver_method_signature_changed() {
    let source = "\
package main

type Repo struct {
	Name string
}

func (r Repo) Label() string {
	return r.Name
}
";
    let lang = GoSupport;
    // Line 7 is the method's own signature line.
    let changed_ranges = vec![LineRange { start: 7, end: 7 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Label".to_string(),
        kind: SymbolKind::Function,
        signature: "func (r Repo) Label() string".to_string(),
        range: LineRange { start: 7, end: 9 },
        container: Some("Repo".to_string()),
        referenced_names: vec!["Repo".to_string(), "string".to_string()],
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
package main

func foo() {}

var x = 1
";
    let lang = GoSupport;
    // Line 5 is a top-level var declaration, not covered by
    // definition_query.
    let changed_ranges = vec![LineRange { start: 5, end: 5 }];

    let expected: Vec<ExtractedSymbol> = Vec::new();
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_signatures_end_to_end_from_a_parsed_diff_of_a_go_file() {
    use crate::diff::parse_unified_diff;
    use crate::language::language_for_path;

    let diff = "\
diff --git a/repo.go b/repo.go
index e69de29..4b825dc 100644
--- a/repo.go
+++ b/repo.go
@@ -6,3 +6,3 @@
 func (r *Repo) Save(id string) error {
-	return errors.New(\"not implemented\")
+	return nil
 }
";
    let source = "\
package main

type Repo struct {
	Name string
}

func (r *Repo) Save(id string) error {
	return nil
}
";
    let changed_file = parse_unified_diff(diff)
        .expect("diff should parse")
        .into_iter()
        .next()
        .expect("diff should contain one changed file");
    let lang = language_for_path(&changed_file.path).expect("*.go should resolve to Go");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Save".to_string(),
        kind: SymbolKind::Function,
        signature: "func (r *Repo) Save(id string) error".to_string(),
        range: LineRange { start: 7, end: 9 },
        container: Some("Repo".to_string()),
        referenced_names: vec![
            "Repo".to_string(),
            "error".to_string(),
            "string".to_string(),
        ],
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
