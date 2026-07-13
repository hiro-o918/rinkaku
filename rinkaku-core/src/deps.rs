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
//! it (all of `git ls-files`, not just the diff). Two costs used to
//! dominate `--deps 1`'s wall-clock time:
//! - Query compilation (`Query::new`) ran once per *definition* rather
//!   than once per *file* (fixed; see `extract::with_definition_nodes`'s
//!   doc comment).
//! - Every indexed file was parsed
//!   ([`crate::extract::extract_all_symbols`]) even though most files in
//!   a real repository define nothing any changed symbol actually
//!   references. `TagsResolver::new`'s `reference_names` parameter fixes
//!   this: files are prefiltered by a substring search
//!   (`aho-corasick`, run once over all reference names instead of once
//!   per name) before parsing, skipping the ones that cannot contain a
//!   match at all — see `should_parse_file` for why this cannot miss a
//!   real match (no recall loss).
//!
//! Remaining `--deps 1` overhead in `--base` mode is mostly the
//! `git show`/`git ls-files` subprocess cost of *reading* every indexed
//! file's content (one `git` invocation per file), which the prefilter
//! above does not reduce — it only skips parsing, not reading, since
//! whether a file's content matches can only be known after reading it.
//! Not addressed here, since it is `main.rs`'s file-reading strategy
//! rather than this module's indexing logic.
//!
//! Measured effect (see the PR description for the full numbers,
//! `git archive`-extracted files so `git show` cost is excluded): on a
//! same-language repository with all-generic-noise filtered
//! `reference_names` (no `Vec`/`Option`/`String`/... — see below),
//! ~88% fewer files were parsed and indexing was ~8x faster. But when
//! `reference_names` includes common standard-library-style names (as a
//! typical Rust diff's referenced names often do — `Vec`, `Option`,
//! `Some`, `Ok`, `String`, ...), the prefilter's effect shrinks sharply:
//! one real-world diff still had 93% of files pass the prefilter, since
//! those names appear in nearly every file. `should_parse_file` is a
//! substring match over raw content, not scoped to actual definition
//! sites, so it cannot narrow this further without risking false
//! negatives (see its own doc comment) — accepted as a known limitation
//! rather than solved here.

use crate::extract::extract_all_symbols;
use crate::language::LanguageSupport;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

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
    /// `reference_names` is the full set of names any changed symbol in
    /// the diff actually references (gathered by `main.rs` before calling
    /// this). A file is only parsed if its content contains at least one
    /// of these names as a substring — see `should_parse_file`'s doc
    /// comment for why this prefilter cannot cause a real definition to
    /// be missed. Passing an empty set (no diff, or `--deps 0`'s caller
    /// never reaching this path) indexes nothing, which is correct: no
    /// name is referenced, so no definition needs to be found.
    ///
    /// `include_tests` mirrors `pipeline::analyze_diff`'s flag of the same
    /// name (ADR 0009's mechanism; ADR 0025 flipped the CLI-facing default
    /// to include tests and renamed the flag to `--exclude-tests`),
    /// extended to this repo-wide index: `false` (the CLI's
    /// `--exclude-tests`) excludes test symbols the same two ways
    /// `analyze_diff` does — a whole file `language.is_test_path`
    /// considers a test file is skipped entirely, and within every other
    /// file, only symbols [`crate::extract::extract_all_symbols`] marked
    /// `ExtractedSymbol::is_test` (AST context, e.g. Rust's `#[cfg(test)]`)
    /// are dropped from indexing. Without this, a changed production
    /// symbol's `referenced_names` could resolve to a same-named test
    /// helper/fixture elsewhere in the repo — a name match a reviewer
    /// would almost always read as coincidental noise in "Depends on:",
    /// not a real dependency, since production code should not actually
    /// depend on test-only definitions (see ADR 0009's Consequences).
    /// `true` (the CLI's new default) indexes every symbol, matching
    /// `analyze_diff`'s own `include_tests: true` behavior.
    ///
    /// `generated_paths` and `include_generated` extend the same exclusion
    /// principle to generated files (ADR 0010/0011's Consequences): a
    /// changed production symbol can just as easily reference a type
    /// defined in a generated file (e.g. an ORM's model struct, dragging
    /// in every column/tag as "Depends on:" noise) as a test helper, and
    /// for the same reason — the reference is a coincidental name match a
    /// reviewer never asked to see, not a meaningful dependency signal.
    /// `generated_paths` is the caller-resolved `.gitattributes` set (ADR
    /// 0010, e.g. `main.rs`'s `git check-attr`, run once over every
    /// indexed path rather than the diff's changed paths — see
    /// `main.rs`'s `check_generated_paths_batch`), checked per-file the
    /// same way `is_test_path` is; on top of that, every file that reaches
    /// parsing is also checked with [`is_generated_content`] (ADR 0011),
    /// same as `analyze_diff`. `include_generated` (`false` = CLI default)
    /// gates both checks together, mirroring `--include-generated`'s
    /// effect on `analyze_diff`.
    ///
    /// Files with no registered [`LanguageSupport`] for their extension
    /// are silently skipped, matching the pipeline's handling of
    /// unsupported files elsewhere (`pipeline::analyze_diff`).
    pub fn new(
        files: impl IntoIterator<Item = (String, String)>,
        language_for_path: impl Fn(&str) -> Option<&'static dyn LanguageSupport>,
        reference_names: &HashSet<String>,
        include_tests: bool,
        generated_paths: &HashSet<String>,
        include_generated: bool,
    ) -> Self {
        let mut index: HashMap<String, Vec<ResolvedSymbol>> = HashMap::new();
        // `AhoCorasick::new` only errors on pathological inputs this call
        // site cannot produce: an empty pattern set is handled gracefully
        // (matches nothing, not an error), and the automaton construction
        // itself only fails on internal overflow at pattern counts/lengths
        // far beyond what a diff's `reference_names` (identifier-sized
        // strings, at most a few hundred per run) could realistically
        // reach. `.expect()` here documents "this is not expected to fail
        // in practice" rather than a genuinely handled error path — there
        // is no meaningful fallback if it somehow did (the resolver simply
        // could not be built).
        let matcher = aho_corasick::AhoCorasick::new(reference_names)
            .expect("reference_names must build a valid AhoCorasick matcher");

        for (path, content) in files {
            let Some(lang) = language_for_path(&path) else {
                continue;
            };
            if !include_tests && lang.is_test_path(&path) {
                continue;
            }
            if !include_generated && generated_paths.contains(&path) {
                continue;
            }
            if !include_generated && is_generated_content(&content) {
                continue;
            }
            if !should_parse_file(&matcher, &content) {
                continue;
            }
            for symbol in extract_all_symbols(&content, lang) {
                if !include_tests && symbol.is_test {
                    continue;
                }
                index.entry(symbol.name).or_default().push(ResolvedSymbol {
                    signature: symbol.signature,
                    path: path.clone(),
                });
            }
        }

        Self { index }
    }
}

/// Whether `content` could plausibly define something a changed symbol
/// references, based on a single `aho-corasick` pass over all reference
/// names at once (rather than one `str::contains` scan per name).
///
/// This is a coarse substring test, not a symbol-aware one: it does not
/// verify the match is an actual identifier (vs., say, a substring inside
/// a comment or string literal) or that it is the file's *definition* of
/// that name rather than some unrelated mention. That imprecision is
/// deliberately accepted — the goal is only to decide whether parsing
/// `content` is worth attempting, and `extract_all_symbols` (the real,
/// syntax-aware definition finder) still runs afterward and is the only
/// thing that actually populates the index. Skipping a file here can
/// therefore never cause `resolve()` to miss a real definition, since any
/// file containing the definition's own name as text necessarily passes
/// this filter (a definition's name always appears literally in its own
/// declaration) — the prefilter can only save work, not recall.
fn should_parse_file(matcher: &aho_corasick::AhoCorasick, content: &str) -> bool {
    matcher.is_match(content)
}

/// Number of leading lines checked by [`is_generated_content`] — mirrors
/// GitHub linguist's own "near the top of the file" scope for its
/// content-based generated-file heuristics (ADR 0011).
const GENERATED_MARKER_SCAN_LINES: usize = 5;

/// Whether `content`'s first [`GENERATED_MARKER_SCAN_LINES`] lines carry a
/// linguist-compatible generated-file marker (ADR 0011): a `@generated`
/// marker (Facebook-style, matched as a plain substring — deliberately not
/// narrowed further per the ADR's "don't overthink context around
/// `@generated`" decision), or a single line containing both `Code
/// generated` and `DO NOT EDIT` (Go tooling/protobuf's
/// `// Code generated by <tool>. DO NOT EDIT.` convention and its `#`-
/// commented equivalents — matched by substring rather than anchoring to a
/// specific comment syntax, since the comment marker itself varies by
/// language). Case-sensitive, matching linguist's own casing for these
/// exact markers.
///
/// A pure text check with no knowledge of `LanguageSupport`/comment syntax
/// by design (ADR 0011's rejected alternative: porting linguist's full
/// rule set) — deliberately a small, easily-audited subset rather than a
/// comprehensive port.
///
/// `pub(crate)` rather than private: shared by `TagsResolver::new` (this
/// module, to exclude generated files from the repo-wide dependency index —
/// ADR 0010/0011's Consequences on dependency resolution) and
/// `pipeline::analyze_diff` (to exclude them from the diff's own changed
/// symbols). Lives here rather than in `pipeline.rs` since `pipeline.rs`
/// already imports from this module (`Resolver`/`resolve_dependencies`);
/// the reverse import would be a cycle.
pub(crate) fn is_generated_content(content: &str) -> bool {
    content
        .lines()
        .take(GENERATED_MARKER_SCAN_LINES)
        .any(|line| line.contains("@generated") || is_code_generated_do_not_edit_line(line))
}

/// Whether `line` contains both `Code generated` and `DO NOT EDIT` —
/// linguist's `^// Code generated .* DO NOT EDIT\.$` pattern, relaxed to a
/// same-line substring match on both phrases (see
/// [`is_generated_content`]'s doc comment for why the comment prefix and
/// trailing-period anchor are not checked).
fn is_code_generated_do_not_edit_line(line: &str) -> bool {
    line.contains("Code generated") && line.contains("DO NOT EDIT")
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
///
/// Also caps same-name candidates at [`MAX_MATCHES_PER_NAME`] per
/// referenced name, ranked by [`path_proximity_rank`] so the kept matches
/// are the ones most likely relevant to the referencing symbol; the excess
/// count is reported via `ExtractedSymbol::omitted_dependency_matches`
/// rather than silently dropped.
///
/// Ranking uses `Vec::sort_by_key`, which is stable: candidates that tie on
/// `path_proximity_rank` (e.g. several same-directory matches) keep their
/// relative order from `resolver.resolve(name)`. For [`TagsResolver`] that
/// order is insertion order into its index, which follows the order of the
/// `files` iterator `TagsResolver::new` was built from — in practice
/// `main.rs`'s `git ls-files` output, i.e. lexicographic path order. This
/// tie-break is therefore an incidental consequence of `git ls-files`'s
/// ordering, not a deliberate ranking signal; a different `Resolver`
/// implementation or file source could change which of several
/// equally-close candidates survives the cap.
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
                        let mut dependencies = Vec::new();
                        let mut omitted = 0usize;

                        for name in &symbol.referenced_names {
                            let mut candidates: Vec<ResolvedSymbol> = resolver
                                .resolve(name)
                                .into_iter()
                                .filter(|resolved| {
                                    let key = (name.clone(), resolved.path.clone());
                                    key != own_key && !diff_symbols.contains(&key)
                                })
                                .collect();

                            // Rank before truncating: the cap must keep the
                            // closest matches, not an arbitrary prefix of
                            // whatever order the resolver happened to
                            // return them in (see
                            // `rank_by_path_proximity`'s doc comment).
                            candidates.sort_by_key(|resolved| {
                                path_proximity_rank(&file_path, &resolved.path)
                            });

                            if candidates.len() > MAX_MATCHES_PER_NAME {
                                omitted += candidates.len() - MAX_MATCHES_PER_NAME;
                                candidates.truncate(MAX_MATCHES_PER_NAME);
                            }
                            dependencies.extend(candidates);
                        }

                        symbol.dependencies = dependencies;
                        symbol.omitted_dependency_matches = omitted;
                        symbol
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Maximum number of same-name candidate definitions kept per referenced
/// name. Beyond this, name-only resolution (ADR 0003) tends to surface
/// many equally-plausible-looking matches for common identifiers (e.g. a
/// `Config` struct defined in several unrelated packages) that add noise
/// rather than signal; 3 keeps the "Depends on" list skimmable while still
/// showing more than one candidate when genuinely ambiguous.
const MAX_MATCHES_PER_NAME: usize = 3;

/// Ranks how close `candidate_path` is to `referencing_path`, lower being
/// closer. Used to keep the most locally relevant matches when a
/// name-only resolver (ADR 0003) returns several same-named candidates,
/// since v1 has no type information to pick the syntactically "correct"
/// one — proximity in the repository's directory tree is used as a proxy
/// for "more likely to be the intended target", the same heuristic an
/// editor's "go to definition" fallback (or a human skimming candidates)
/// would reach for first.
///
/// Ranks, from closest to farthest:
/// 1. Same file as the referencing symbol.
/// 2. Same directory (immediate parent) as the referencing symbol.
/// 3. Shares a path prefix with the referencing symbol — ranked by *shared
///    prefix depth*, deeper (more path components in common) first, so a
///    common grandparent directory ranks closer than a common
///    great-grandparent.
/// 4. No shared directory prefix at all (other than the repository root).
///
/// Edge case: two files that both live directly at the repository root
/// (e.g. `"a.rs"` and `"b.rs"`, no `/` in the path) both have an empty
/// `path_dir_components` result and therefore rank as "same directory"
/// (rank 2), not "no shared prefix" (rank 4) — there is no directory
/// component to distinguish them by. This is a natural consequence of
/// treating the repository root as a directory like any other, not a
/// special case handled separately.
fn path_proximity_rank(
    referencing_path: &str,
    candidate_path: &str,
) -> (u8, std::cmp::Reverse<usize>) {
    if candidate_path == referencing_path {
        return (0, std::cmp::Reverse(usize::MAX));
    }

    let referencing_dir: Vec<&str> = path_dir_components(referencing_path);
    let candidate_dir: Vec<&str> = path_dir_components(candidate_path);

    if referencing_dir == candidate_dir {
        return (1, std::cmp::Reverse(usize::MAX));
    }

    let shared_depth = referencing_dir
        .iter()
        .zip(candidate_dir.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if shared_depth > 0 {
        (2, std::cmp::Reverse(shared_depth))
    } else {
        (3, std::cmp::Reverse(0))
    }
}

/// Splits a `/`-separated repository-relative path into its directory
/// components, dropping the file name itself — e.g. `"src/pkg/a.rs"` →
/// `["src", "pkg"]`. Paths are always `/`-separated regardless of host OS:
/// they come from `git`, which normalizes separators, not from
/// `std::path` traversal of the local filesystem.
fn path_dir_components(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

#[cfg(test)]
#[path = "deps_tests/mod.rs"]
mod tests;
