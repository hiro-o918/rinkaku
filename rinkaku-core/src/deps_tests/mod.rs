//! Tests for `crate::deps`, split from the source file (ADR 0028) and
//! grouped by which layer of the resolver each block pins:
//!
//! - `tags_resolver_indexing` — `TagsResolver::resolve`'s happy paths:
//!   function/type resolution, missing name, and multi-definition
//!   collection
//! - `tags_resolver_exclusions` — the `include_tests` / `include_generated`
//!   / unsupported-language filters on `TagsResolver::new`'s index
//!   (test-path exclusion, AST-detected test-def exclusion, generated
//!   marker exclusion, multi-language passthrough)
//! - `prefilter` — `should_parse_file`'s aho-corasick substring prefilter
//!   (index/skip/incidental-hit/empty-names cases), previously the
//!   `mod prefilter_tests` nested module
//! - `resolve_dependencies` — `resolve_dependencies` cross-file behavior:
//!   self-reference exclusion, diff-collision exclusion, proximity
//!   ranking, per-name cap boundary (via `rstest`), and cross-name
//!   omitted-count accumulation

use super::*;
use crate::language::go::GoSupport;
use crate::language::rust::RustSupport;

mod prefilter;
mod resolve_dependencies;
mod tags_resolver_exclusions;
mod tags_resolver_indexing;

/// Test-only `language_for_path`: routes `.rs` to Rust and `.go` to
/// Go, mirroring `language::language_for_path` without depending on
/// the full registry (keeps these tests independent of which
/// languages are registered there).
pub(super) fn lang_for_path(path: &str) -> Option<&'static dyn LanguageSupport> {
    if path.ends_with(".rs") {
        Some(&RustSupport)
    } else if path.ends_with(".go") {
        Some(&GoSupport)
    } else {
        None
    }
}

/// Builds a `reference_names` set from string literals, for tests that
/// only care about exercising `TagsResolver`'s indexing/resolution
/// behavior rather than the prefilter itself (see `mod prefilter`
/// for that). Every name the test resolves against must be included so
/// the prefilter never spuriously excludes the file under test.
pub(super) fn names(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}
