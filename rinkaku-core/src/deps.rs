//! 1-hop dependency resolution (ADR 0003).
//!
//! [`Resolver`] is the port through which the pipeline resolves a symbol's
//! referenced names (see [`crate::extract::ExtractedSymbol::referenced_names`])
//! into the definitions they point to, if any exist in the repository.
//! [`TagsResolver`] is the v1 implementation: an approximate, syntactic
//! resolver built on the same tree-sitter definition queries used for
//! extraction, with no type information. LSP-backed resolvers (pyright,
//! gopls, rust-analyzer, ...) are a future, opt-in `Resolver` impl that can
//! be plugged in without reshaping the pipeline.
//!
//! Performance: `TagsResolver::new` indexes every file `main.rs` passes
//! it (all of `git ls-files`, not just the diff), each parsed once via
//! [`crate::extract::extract_all_symbols`] — this used to dominate
//! `--deps 1`'s wall-clock time because query compilation (`Query::new`)
//! ran once per *definition* rather than once per *file* (fixed; see
//! `extract::with_definition_nodes`'s doc comment). With that fixed, the
//! remaining `--deps 1` overhead in `--base` mode is mostly the
//! `git show`/`git ls-files` subprocess cost of reading every indexed
//! file's content, one `git` invocation per file — measured as
//! significantly larger than the file-parsing cost itself on a
//! ~100-file repository; not addressed here, since it is `main.rs`'s
//! file-reading strategy rather than this module's indexing logic.

use crate::extract::extract_all_symbols;
use crate::language::LanguageSupport;
use serde::Serialize;
use std::collections::HashMap;

/// A definition found by a [`Resolver`] for a referenced name. Reported
/// verbatim in [`crate::extract::ExtractedSymbol::dependencies`], so it is
/// part of rinkaku's output shape (unlike `referenced_names`) and derives
/// `Serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedSymbol {
    pub signature: String,
    /// Path of the file the definition lives in, as provided to the
    /// resolver's file source (e.g. `TagsResolver::new`'s `files`).
    pub path: String,
}

/// Resolves a referenced name (a called function, a referenced type, ...)
/// to the definition(s) it points to in the repository, if any.
///
/// Returns every matching definition rather than a single one: v1's
/// [`TagsResolver`] matches by name alone, with no type information to
/// disambiguate overloads or same-named symbols in different
/// modules/packages, so more than one match is a normal, expected outcome
/// rather than an error condition. Callers decide how to present multiple
/// matches (e.g. list them all under "Depends on").
pub trait Resolver {
    fn resolve(&self, name: &str) -> Vec<ResolvedSymbol>;
}

/// v1 [`Resolver`]: builds a name-to-definition index by parsing every
/// supported file handed to it via [`TagsResolver::new`] with the same
/// tree-sitter `definition_query` used for extraction, then resolves by
/// exact name match.
///
/// Approximate by construction (ADR 0003): no type information means a
/// name match cannot distinguish overloads, shadowed names, or same-named
/// symbols in unrelated modules — all definitions sharing a name are
/// returned, not just the "right" one.
pub struct TagsResolver {
    index: HashMap<String, Vec<ResolvedSymbol>>,
}

impl TagsResolver {
    /// Builds the resolver's index eagerly from `files`: `(path, content)`
    /// pairs for every file the resolver should be able to resolve
    /// definitions from. Files are provided rather than discovered here so
    /// this module stays pure (no filesystem/`git` access) — `main.rs`
    /// supplies the real file list via `git ls-files`, tests supply an
    /// in-memory list.
    ///
    /// Files with no registered [`LanguageSupport`] for their extension
    /// are silently skipped, matching the pipeline's handling of
    /// unsupported files elsewhere (`pipeline::analyze_diff`).
    pub fn new(
        files: impl IntoIterator<Item = (String, String)>,
        language_for_path: impl Fn(&str) -> Option<&'static dyn LanguageSupport>,
    ) -> Self {
        let mut index: HashMap<String, Vec<ResolvedSymbol>> = HashMap::new();

        for (path, content) in files {
            let Some(lang) = language_for_path(&path) else {
                continue;
            };
            for symbol in extract_all_symbols(&content, lang) {
                index.entry(symbol.name).or_default().push(ResolvedSymbol {
                    signature: symbol.signature,
                    path: path.clone(),
                });
            }
        }

        Self { index }
    }
}

impl Resolver for TagsResolver {
    fn resolve(&self, name: &str) -> Vec<ResolvedSymbol> {
        self.index.get(name).cloned().unwrap_or_default()
    }
}

/// Populates every symbol's `dependencies` by resolving its
/// `referenced_names` through `resolver`, across every file in the
/// report — a symbol in one changed file may reference a symbol changed
/// in another, so exclusion is computed over the whole diff, not
/// per-file.
///
/// Two kinds of matches are deliberately excluded from the resulting
/// `dependencies`, both to avoid redundant noise rather than because they
/// are wrong:
/// - **Self-references**: a symbol's own declared name often appears in
///   its `referenced_names` (e.g. a struct's name is syntactically a type
///   reference inside its own definition — see the doc comment on
///   `LanguageSupport::reference_query`). Resolving it would just point
///   the symbol back at itself.
/// - **Diff-internal symbols**: if a resolved dependency matches another
///   symbol already reported in this same diff, it is already shown in
///   full elsewhere in the report; repeating it under "dependencies" adds
///   noise without adding information.
///
/// Matching for both exclusions is keyed on `(name, path)`, not name
/// alone: a `referenced_names` entry only carries a bare name, but each
/// candidate it resolves to (`ResolvedSymbol`) carries its own `path`, so
/// exclusion is checked per resolved candidate rather than by filtering
/// `referenced_names` up front. Name-only matching would wrongly drop a
/// dependency whenever the diff happens to also touch an unrelated,
/// same-named symbol in a *different* file (e.g. a changed `a.rs::helper`
/// coinciding with the actual dependency target `b.rs::helper`) — see
/// ADR 0003 for why resolution itself stays name-based (no type info),
/// but exclusion does not need to inherit that imprecision.
pub fn resolve_dependencies(
    files: Vec<crate::render::FileReport>,
    resolver: &dyn Resolver,
) -> Vec<crate::render::FileReport> {
    let diff_symbols: std::collections::HashSet<(String, String)> = files
        .iter()
        .flat_map(|file| {
            file.symbols
                .iter()
                .map(move |symbol| (symbol.name.clone(), file.path.clone()))
        })
        .collect();

    files
        .into_iter()
        .map(|file| {
            let file_path = file.path.clone();
            crate::render::FileReport {
                path: file.path,
                symbols: file
                    .symbols
                    .into_iter()
                    .map(|mut symbol| {
                        let own_key = (symbol.name.clone(), file_path.clone());
                        symbol.dependencies = symbol
                            .referenced_names
                            .iter()
                            .flat_map(|name| {
                                resolver
                                    .resolve(name)
                                    .into_iter()
                                    .map(move |resolved| (name.clone(), resolved))
                            })
                            .filter(|(name, resolved)| {
                                let key = (name.clone(), resolved.path.clone());
                                key != own_key && !diff_symbols.contains(&key)
                            })
                            .map(|(_, resolved)| resolved)
                            .collect();
                        symbol
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::go::GoSupport;
    use crate::language::rust::RustSupport;
    use pretty_assertions::assert_eq;

    /// Test-only `language_for_path`: routes `.rs` to Rust and `.go` to
    /// Go, mirroring `language::language_for_path` without depending on
    /// the full registry (keeps these tests independent of which
    /// languages are registered there).
    fn lang_for_path(path: &str) -> Option<&'static dyn LanguageSupport> {
        if path.ends_with(".rs") {
            Some(&RustSupport)
        } else if path.ends_with(".go") {
            Some(&GoSupport)
        } else {
            None
        }
    }

    #[test]
    fn should_resolve_function_call_when_callee_is_defined_in_repo() {
        let files = [(
            "src/lib.rs".to_string(),
            "fn helper(x: i32) -> i32 {\n    x\n}\n".to_string(),
        )];
        let resolver = TagsResolver::new(files, lang_for_path);

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
        let resolver = TagsResolver::new(files, lang_for_path);

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
        let resolver = TagsResolver::new(files, lang_for_path);

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
        let resolver = TagsResolver::new(files, lang_for_path);

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

    #[test]
    fn should_skip_file_with_unsupported_language_when_building_index() {
        let files = [(
            "src/notes.txt".to_string(),
            "helper is defined here".to_string(),
        )];
        let resolver = TagsResolver::new(files, lang_for_path);

        let expected: Vec<ResolvedSymbol> = Vec::new();
        let actual = resolver.resolve("helper");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_index_definitions_across_multiple_languages() {
        let files = [
            (
                "src/lib.rs".to_string(),
                "fn helper() -> i32 {\n    1\n}\n".to_string(),
            ),
            (
                "repo.go".to_string(),
                "package main\n\nfunc greet() string {\n\treturn \"hi\"\n}\n".to_string(),
            ),
        ];
        let resolver = TagsResolver::new(files, lang_for_path);

        let expected = vec![ResolvedSymbol {
            signature: "func greet() string".to_string(),
            path: "repo.go".to_string(),
        }];
        let actual = resolver.resolve("greet");

        assert_eq!(expected, actual);
    }

    mod resolve_dependencies_tests {
        use super::*;
        use crate::diff::LineRange;
        use crate::extract::{ExtractedSymbol, SymbolKind};
        use crate::render::FileReport;
        use pretty_assertions::assert_eq;

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
                name: name.to_string(),
                kind: SymbolKind::Function,
                signature: format!("fn {name}()"),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
                dependencies: vec![],
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
    }
}
