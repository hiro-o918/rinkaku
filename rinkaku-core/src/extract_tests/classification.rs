//! Tests pinning [`super::classify_symbols`] (ADR 0014): matches head
//! and base symbols within one file by `(name, container)` identity to
//! set each head symbol's `classification`
//! ([`super::Classification::Added`] /
//! [`super::Classification::SignatureChanged`] /
//! [`super::Classification::BodyOnly`]) and returns any base-only
//! symbols whose base-side range overlaps `old_changed_ranges` as
//! [`super::RemovedSymbol`]s.

use super::*;
use pretty_assertions::assert_eq;

/// Builds an `ExtractedSymbol` for classification tests: `id`,
/// `dependencies`, `omitted_dependency_matches`, `referenced_names`
/// stay at their inert defaults since matching/classification never
/// reads them — only `name`/`kind`/`signature`/`range`/`container`
/// matter here.
fn symbol(
    name: &str,
    container: Option<&str>,
    signature: &str,
    range: LineRange,
) -> ExtractedSymbol {
    ExtractedSymbol {
        id: String::new(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: signature.to_string(),
        range,
        container: container.map(str::to_string),
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

#[test]
fn should_classify_as_added_when_no_base_side_match_exists() {
    let mut head = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];
    let base: Vec<ExtractedSymbol> = vec![];

    let removed = classify_symbols(&mut head, &base, &[], "src/lib.rs");

    let mut expected = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];
    expected[0].classification = Some(Classification::Added);
    let expected_removed: Vec<RemovedSymbol> = Vec::new();

    assert_eq!(expected, head);
    assert_eq!(expected_removed, removed);
}

#[test]
fn should_classify_as_signature_changed_when_base_signature_differs() {
    let mut head = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32, b: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];
    let base = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];

    let removed = classify_symbols(&mut head, &base, &[], "src/lib.rs");

    let mut expected = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32, b: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];
    expected[0].classification = Some(Classification::SignatureChanged);
    expected[0].previous_signature = Some("fn foo(a: i32) -> i32".to_string());
    let expected_removed: Vec<RemovedSymbol> = Vec::new();

    assert_eq!(expected, head);
    assert_eq!(expected_removed, removed);
}

#[test]
fn should_classify_as_body_only_when_base_signature_is_identical() {
    let mut head = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 4 },
    )];
    let base = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 3 },
    )];

    let removed = classify_symbols(&mut head, &base, &[], "src/lib.rs");

    let mut expected = vec![symbol(
        "foo",
        None,
        "fn foo(a: i32) -> i32",
        LineRange { start: 1, end: 4 },
    )];
    expected[0].classification = Some(Classification::BodyOnly);
    let expected_removed: Vec<RemovedSymbol> = Vec::new();

    assert_eq!(expected, head);
    assert_eq!(expected_removed, removed);
}

// Matching is by (name, container), not name alone: a base-side
// method of a different container must not be treated as this
// head symbol's base counterpart, even though the bare name
// matches.
#[test]
fn should_classify_as_added_when_base_match_has_different_container() {
    let mut head = vec![symbol(
        "save",
        Some("impl Foo"),
        "fn save(&self)",
        LineRange { start: 1, end: 3 },
    )];
    let base = vec![symbol(
        "save",
        Some("impl Bar"),
        "fn save(&self)",
        LineRange { start: 1, end: 3 },
    )];

    let removed = classify_symbols(&mut head, &base, &[], "src/lib.rs");

    let mut expected = vec![symbol(
        "save",
        Some("impl Foo"),
        "fn save(&self)",
        LineRange { start: 1, end: 3 },
    )];
    expected[0].classification = Some(Classification::Added);
    // The base's "save" (impl Bar) never matched any head symbol,
    // and its range does overlap `old_changed_ranges` in this case
    // — but this test passes an empty range set, so nothing
    // qualifies as removed either. See the dedicated removed-symbol
    // tests below for that path.
    let expected_removed: Vec<RemovedSymbol> = Vec::new();

    assert_eq!(expected, head);
    assert_eq!(expected_removed, removed);
}

#[test]
fn should_report_removed_when_base_only_symbol_overlaps_old_changed_ranges() {
    let mut head: Vec<ExtractedSymbol> = vec![];
    let base = vec![symbol(
        "deprecated_helper",
        None,
        "fn deprecated_helper()",
        LineRange { start: 5, end: 7 },
    )];
    let old_changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let removed = classify_symbols(&mut head, &base, &old_changed_ranges, "src/lib.rs");

    let expected_head: Vec<ExtractedSymbol> = vec![];
    let expected_removed = vec![RemovedSymbol {
        name: "deprecated_helper".to_string(),
        kind: SymbolKind::Function,
        path: "src/lib.rs".to_string(),
        signature: "fn deprecated_helper()".to_string(),
    }];

    assert_eq!(expected_head, head);
    assert_eq!(expected_removed, removed);
}

#[test]
fn should_not_report_removed_when_base_only_symbol_does_not_overlap_old_changed_ranges() {
    let mut head: Vec<ExtractedSymbol> = vec![];
    let base = vec![symbol(
        "unrelated_helper",
        None,
        "fn unrelated_helper()",
        LineRange { start: 50, end: 52 },
    )];
    // The diff touched line 6 only, nowhere near this symbol's
    // base-side range — an edit elsewhere in the file must not
    // make every other base-only symbol show up as "removed".
    let old_changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let removed = classify_symbols(&mut head, &base, &old_changed_ranges, "src/lib.rs");

    let expected_removed: Vec<RemovedSymbol> = Vec::new();
    assert_eq!(expected_removed, removed);
}

// Regression: matching must key on (name, container), not name
// alone. Two base-side symbols share the bare name "helper" but
// have different containers; only one has a head-side match. If
// matching were name-only, the matched "impl Foo" head symbol
// could wrongly be treated as also covering "impl Bar"'s base
// symbol, silently dropping it instead of reporting it removed.
#[test]
fn should_report_removed_when_a_second_base_symbol_of_same_name_has_no_head_match() {
    // Base has two distinct "helper" symbols distinguished by
    // container; head only kept the "impl Foo" one.
    let mut head = vec![symbol(
        "helper",
        Some("impl Foo"),
        "fn helper(&self)",
        LineRange { start: 1, end: 3 },
    )];
    let base = vec![
        symbol(
            "helper",
            Some("impl Foo"),
            "fn helper(&self)",
            LineRange { start: 1, end: 3 },
        ),
        symbol(
            "helper",
            Some("impl Bar"),
            "fn helper(&self)",
            LineRange { start: 10, end: 12 },
        ),
    ];
    let old_changed_ranges = vec![LineRange { start: 11, end: 11 }];

    let removed = classify_symbols(&mut head, &base, &old_changed_ranges, "src/lib.rs");

    let mut expected_head = vec![symbol(
        "helper",
        Some("impl Foo"),
        "fn helper(&self)",
        LineRange { start: 1, end: 3 },
    )];
    expected_head[0].classification = Some(Classification::BodyOnly);
    let expected_removed = vec![RemovedSymbol {
        name: "helper".to_string(),
        kind: SymbolKind::Function,
        path: "src/lib.rs".to_string(),
        signature: "fn helper(&self)".to_string(),
    }];

    assert_eq!(expected_head, head);
    assert_eq!(expected_removed, removed);
}
