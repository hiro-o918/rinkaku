use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_resolve_function_call_when_callee_is_defined_in_repo() {
    let files = [(
        "src/lib.rs".to_string(),
        "fn helper(x: i32) -> i32 {\n    x\n}\n".to_string(),
    )];
    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &names(&["helper"]),
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected = vec![ResolvedSymbol {
        signature: "fn helper(x: i32) -> i32".to_string(),
        path: "src/lib.rs".to_string(),
    }];
    let actual = resolver.resolve("helper");

    assert_eq!(expected, actual);
}

#[test]
fn should_resolve_type_reference_when_type_is_defined_in_repo() {
    let files = [(
        "src/point.rs".to_string(),
        "struct Point {\n    x: i32,\n}\n".to_string(),
    )];
    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &names(&["Point"]),
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected = vec![ResolvedSymbol {
        signature: "struct Point { x: i32, }".to_string(),
        path: "src/point.rs".to_string(),
    }];
    let actual = resolver.resolve("Point");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_empty_vec_when_name_has_no_definition_in_repo() {
    let files = [(
        "src/lib.rs".to_string(),
        "fn helper(x: i32) -> i32 {\n    x\n}\n".to_string(),
    )];
    // "i32" is included in reference_names (unlike the prefilter tests)
    // specifically so this exercises "no definition found", not "file
    // excluded by the prefilter" — the file's content also contains
    // "i32" as a parameter/return type, so it would pass the prefilter
    // regardless, but being explicit keeps the test's intent clear.
    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &names(&["helper", "i32"]),
        false,
        &HashSet::new(),
        true,
        None,
    );

    // Covers both a built-in type (`i32`, never indexed since it has
    // no definition anywhere) and a name from an external
    // crate/package (equally never indexed) — v1 has no exclusion
    // list for either (see `LanguageSupport::reference_query`'s doc
    // comment); both simply fail to resolve.
    let expected: Vec<ResolvedSymbol> = Vec::new();
    let actual = resolver.resolve("i32");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_all_matches_when_name_is_defined_multiple_times() {
    let files = [
        (
            "src/a.rs".to_string(),
            "fn helper() -> i32 {\n    1\n}\n".to_string(),
        ),
        (
            "src/b.rs".to_string(),
            "fn helper() -> i32 {\n    2\n}\n".to_string(),
        ),
    ];
    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &names(&["helper"]),
        false,
        &HashSet::new(),
        true,
        None,
    );

    let mut expected = vec![
        ResolvedSymbol {
            signature: "fn helper() -> i32".to_string(),
            path: "src/a.rs".to_string(),
        },
        ResolvedSymbol {
            signature: "fn helper() -> i32".to_string(),
            path: "src/b.rs".to_string(),
        },
    ];
    let mut actual = resolver.resolve("helper");
    // NOTE: sorted before comparison. `TagsResolver::new` iterates
    // `files` in caller-provided order and the index preserves
    // insertion order per name, so this is deterministic given a
    // fixed input order already — the sort here only guards against
    // this test becoming order-dependent if that iteration order is
    // ever changed.
    expected.sort_by(|a, b| a.path.cmp(&b.path));
    actual.sort_by(|a, b| a.path.cmp(&b.path));

    assert_eq!(expected, actual);
}
