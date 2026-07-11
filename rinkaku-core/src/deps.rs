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

use crate::extract::extract_all_symbols;
use crate::language::LanguageSupport;
use std::collections::HashMap;

/// A definition found by a [`Resolver`] for a referenced name.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}
