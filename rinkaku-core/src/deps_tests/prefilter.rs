use super::*;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

#[test]
fn should_index_definitions_from_file_containing_a_referenced_name() {
    let files = [(
        "src/lib.rs".to_string(),
        "fn helper(x: i32) -> i32 {\n    x\n}\n".to_string(),
    )];
    let reference_names: HashSet<String> = ["helper".to_string()].into_iter().collect();

    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &reference_names,
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
fn should_skip_indexing_file_whose_content_contains_no_referenced_name() {
    // "src/other.rs" defines `unrelated`, but nothing in
    // `reference_names` appears anywhere in its content, so it is
    // never parsed and its definitions never make it into the
    // index — this is the whole point of the prefilter: skip
    // parsing files that cannot possibly satisfy any reference.
    let files = [(
        "src/other.rs".to_string(),
        "fn unrelated() -> i32 {\n    1\n}\n".to_string(),
    )];
    let reference_names: HashSet<String> = ["helper".to_string()].into_iter().collect();

    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &reference_names,
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected: Vec<ResolvedSymbol> = Vec::new();
    let actual = resolver.resolve("unrelated");

    assert_eq!(expected, actual);
}

#[test]
fn should_still_index_file_when_referenced_name_appears_incidentally_in_content() {
    // The prefilter is a coarse substring match, not a symbol-aware
    // one: a file is indexed whenever a referenced name appears
    // anywhere in its raw content (e.g. inside another
    // definition's body, not just as the definition's own name).
    // This deliberately never drops a file that could plausibly
    // define something reachable — recall is never sacrificed, see
    // the module-level doc comment on why substring matching is
    // safe here.
    let files = [(
        "src/lib.rs".to_string(),
        "fn wrapper() -> i32 {\n    helper()\n}\n\nfn helper() -> i32 {\n    1\n}\n".to_string(),
    )];
    let reference_names: HashSet<String> = ["helper".to_string()].into_iter().collect();

    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &reference_names,
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected = vec![ResolvedSymbol {
        signature: "fn helper() -> i32".to_string(),
        path: "src/lib.rs".to_string(),
    }];
    let actual = resolver.resolve("helper");

    assert_eq!(expected, actual);
}

#[test]
fn should_index_nothing_when_reference_names_is_empty() {
    let files = [(
        "src/lib.rs".to_string(),
        "fn helper() -> i32 {\n    1\n}\n".to_string(),
    )];
    let reference_names: HashSet<String> = HashSet::new();

    let resolver = TagsResolver::new(
        files,
        lang_for_path,
        &reference_names,
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected: Vec<ResolvedSymbol> = Vec::new();
    let actual = resolver.resolve("helper");

    assert_eq!(expected, actual);
}
