use super::*;
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::render::FileReport;
use pretty_assertions::assert_eq;
use rstest::rstest;

/// A fake `Resolver` backed by an in-memory map, for tests that
/// exercise `resolve_dependencies`'s exclusion logic in isolation
/// from `TagsResolver`'s indexing behavior (already covered by the
/// tests above).
struct FakeResolver {
    matches: HashMap<&'static str, Vec<ResolvedSymbol>>,
}

impl Resolver for FakeResolver {
    fn resolve(&self, name: &str) -> Vec<ResolvedSymbol> {
        self.matches.get(name).cloned().unwrap_or_default()
    }
}

fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
    ExtractedSymbol {
        id: String::new(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range: LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

/// Builds a `ResolvedSymbol` candidate at `path`, with a signature
/// derived from the path so mismatched ordering is easy to spot in
/// a failing assertion.
fn candidate(path: &str) -> ResolvedSymbol {
    ResolvedSymbol {
        signature: format!("fn helper() // {path}"),
        path: path.to_string(),
    }
}

#[test]
fn should_populate_dependencies_when_referenced_name_resolves() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["helper"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "helper",
            vec![ResolvedSymbol {
                signature: "fn helper()".to_string(),
                path: "src/util.rs".to_string(),
            }],
        )]),
    };

    let expected = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![ExtractedSymbol {
            dependencies: vec![ResolvedSymbol {
                signature: "fn helper()".to_string(),
                path: "src/util.rs".to_string(),
            }],
            ..symbol("foo", vec!["helper"])
        }],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_self_reference_from_dependencies() {
    // "Point" resolves to a real definition (itself), but a
    // symbol referencing its own name (see the doc comment on
    // `LanguageSupport::reference_query`) must not list itself as
    // a dependency.
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("Point", vec!["Point"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "Point",
            vec![ResolvedSymbol {
                signature: "struct Point".to_string(),
                path: "src/lib.rs".to_string(),
            }],
        )]),
    };

    let expected = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("Point", vec!["Point"])],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_dependency_already_reported_elsewhere_in_the_diff() {
    // "helper" is itself a changed symbol reported in this diff
    // (a different file than "foo"), so it must not be repeated
    // under "foo"'s dependencies even though it resolves.
    let files = vec![
        FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("foo", vec!["helper"])],
        },
        FileReport {
            path: "src/util.rs".to_string(),
            symbols: vec![symbol("helper", vec![])],
        },
    ];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "helper",
            vec![ResolvedSymbol {
                signature: "fn helper()".to_string(),
                path: "src/util.rs".to_string(),
            }],
        )]),
    };

    let expected = files.clone();
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_keep_dependency_when_a_same_named_symbol_exists_elsewhere_in_the_diff() {
    // "helper" is changed in this diff, but at "src/a.rs::helper" —
    // a different file from the actual dependency target,
    // "src/b.rs::helper". Excluding by name alone would wrongly
    // drop this dependency just because a same-named, unrelated
    // symbol happens to also be part of the diff; exclusion must
    // be keyed on (name, path), not name alone.
    let files = vec![
        FileReport {
            path: "src/a.rs".to_string(),
            symbols: vec![symbol("foo", vec!["helper"]), symbol("helper", vec![])],
        },
        FileReport {
            path: "src/b.rs".to_string(),
            symbols: vec![],
        },
    ];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "helper",
            vec![ResolvedSymbol {
                signature: "fn helper()".to_string(),
                path: "src/b.rs".to_string(),
            }],
        )]),
    };

    let expected = vec![
        FileReport {
            path: "src/a.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    dependencies: vec![ResolvedSymbol {
                        signature: "fn helper()".to_string(),
                        path: "src/b.rs".to_string(),
                    }],
                    ..symbol("foo", vec!["helper"])
                },
                symbol("helper", vec![]),
            ],
        },
        FileReport {
            path: "src/b.rs".to_string(),
            symbols: vec![],
        },
    ];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_leave_dependencies_empty_when_referenced_name_does_not_resolve() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["i32"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::new(),
    };

    let expected = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["i32"])],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_rank_same_file_candidate_above_other_candidates() {
    // Four same-named candidates at increasing path distance from
    // the referencing symbol's own file ("src/pkg/a.rs"): itself,
    // same directory, a shared grandparent, and a wholly unrelated
    // top-level path. Fed to the resolver in the *reverse* of
    // proximity order so this test cannot pass by accident of
    // input ordering.
    let files = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![symbol("foo", vec!["helper"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "helper",
            vec![
                candidate("other/unrelated.rs"),
                candidate("src/other_pkg/c.rs"),
                candidate("src/pkg/b.rs"),
                candidate("src/pkg/a.rs"),
            ],
        )]),
    };

    let expected = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![ExtractedSymbol {
            dependencies: vec![
                candidate("src/pkg/a.rs"),
                candidate("src/pkg/b.rs"),
                candidate("src/other_pkg/c.rs"),
            ],
            omitted_dependency_matches: 1,
            ..symbol("foo", vec!["helper"])
        }],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_keep_all_matches_when_at_or_under_the_cap() {
    let files = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![symbol("foo", vec!["helper"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([(
            "helper",
            vec![candidate("src/pkg/b.rs"), candidate("src/pkg/c.rs")],
        )]),
    };

    let expected = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![ExtractedSymbol {
            dependencies: vec![candidate("src/pkg/b.rs"), candidate("src/pkg/c.rs")],
            omitted_dependency_matches: 0,
            ..symbol("foo", vec!["helper"])
        }],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

/// Boundary coverage around [`MAX_MATCHES_PER_NAME`] (3): 2 matches
/// (under the cap), exactly 3 (at the cap), and 4 (one over).
/// `should_keep_all_matches_when_at_or_under_the_cap` and
/// `should_rank_same_file_candidate_above_other_candidates` above
/// already cover 2-and-under and over-the-cap cases respectively
/// (the latter also exercising proximity ranking), but neither pins
/// down the exact-cap boundary (3 candidates, 0 omitted) — this
/// table adds that case alongside the other two so the boundary is
/// asserted explicitly rather than only implied.

#[rstest]
#[case::should_keep_all_and_omit_none_when_two_candidates_are_under_the_cap(
    vec![candidate("src/pkg/b.rs"), candidate("src/pkg/c.rs")],
    vec![candidate("src/pkg/b.rs"), candidate("src/pkg/c.rs")],
    0,
)]
#[case::should_keep_all_and_omit_none_when_three_candidates_exactly_meet_the_cap(
    vec![
        candidate("src/pkg/b.rs"),
        candidate("src/pkg/c.rs"),
        candidate("src/pkg/d.rs"),
    ],
    vec![
        candidate("src/pkg/b.rs"),
        candidate("src/pkg/c.rs"),
        candidate("src/pkg/d.rs"),
    ],
    0,
)]
#[case::should_truncate_to_cap_and_omit_one_when_four_candidates_exceed_the_cap(
    vec![
        candidate("src/pkg/b.rs"),
        candidate("src/pkg/c.rs"),
        candidate("src/pkg/d.rs"),
        candidate("src/pkg/e.rs"),
    ],
    vec![
        candidate("src/pkg/b.rs"),
        candidate("src/pkg/c.rs"),
        candidate("src/pkg/d.rs"),
    ],
    1,
)]
fn resolve_dependencies_cap_boundary_cases(
    #[case] resolved_candidates: Vec<ResolvedSymbol>,
    #[case] expected_dependencies: Vec<ResolvedSymbol>,
    #[case] expected_omitted: usize,
) {
    // All candidates live in the same directory as the referencing
    // symbol ("src/pkg/a.rs"), so they share proximity rank and are
    // kept in the resolver's original (here: already-closest-first)
    // order — this test is about the cap boundary, not ranking
    // order, which `should_rank_same_file_candidate_above_other_candidates`
    // already covers.
    let files = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![symbol("foo", vec!["helper"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([("helper", resolved_candidates)]),
    };

    let expected = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![ExtractedSymbol {
            dependencies: expected_dependencies,
            omitted_dependency_matches: expected_omitted,
            ..symbol("foo", vec!["helper"])
        }],
    }];
    let actual = resolve_dependencies(files, &resolver);

    assert_eq!(expected, actual);
}

#[test]
fn should_accumulate_omitted_matches_across_multiple_referenced_names() {
    // Two different referenced names each overflow the per-name cap
    // by one match; the symbol-level omitted count is their sum,
    // not just the last name processed.
    let files = vec![FileReport {
        path: "src/pkg/a.rs".to_string(),
        symbols: vec![symbol("foo", vec!["helper", "other"])],
    }];
    let resolver = FakeResolver {
        matches: HashMap::from([
            (
                "helper",
                vec![
                    candidate("src/pkg/b.rs"),
                    candidate("src/pkg/c.rs"),
                    candidate("src/pkg/d.rs"),
                    candidate("src/pkg/e.rs"),
                ],
            ),
            (
                "other",
                vec![
                    ResolvedSymbol {
                        signature: "fn other() // src/pkg/f.rs".to_string(),
                        path: "src/pkg/f.rs".to_string(),
                    },
                    ResolvedSymbol {
                        signature: "fn other() // src/pkg/g.rs".to_string(),
                        path: "src/pkg/g.rs".to_string(),
                    },
                    ResolvedSymbol {
                        signature: "fn other() // src/pkg/h.rs".to_string(),
                        path: "src/pkg/h.rs".to_string(),
                    },
                    ResolvedSymbol {
                        signature: "fn other() // src/pkg/i.rs".to_string(),
                        path: "src/pkg/i.rs".to_string(),
                    },
                ],
            ),
        ]),
    };

    let actual = resolve_dependencies(files, &resolver);

    // NOTE: partial assertion — only `omitted_dependency_matches`
    // and the dependency count are checked, not the full ranked
    // list, because all four candidates per name sit at the same
    // proximity rank ("same directory") and their relative order
    // among equally-ranked candidates is not a contract this test
    // needs to pin down; the per-name cap-and-count behavior is.
    let deps_symbol = &actual[0].symbols[0];
    assert_eq!(6, deps_symbol.dependencies.len());
    assert_eq!(2, deps_symbol.omitted_dependency_matches);
}
