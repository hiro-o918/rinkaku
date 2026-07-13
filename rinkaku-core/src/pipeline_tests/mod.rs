//! Test suite for [`crate::pipeline`], split by responsibility per ADR 0028
//! so the production file stays under the file-size warn threshold.
//!
//! Topic modules:
//!
//! - [`analyze_diff`] — top-level `analyze_diff` behavior: empty input,
//!   per-file skip cases (deleted / binary / unsupported-language / pure
//!   rename), diff-parse and read-file error paths, multi-file mixed
//!   outcomes, Go interface/receiver nesting end-to-end, resolver
//!   invocation contract (`Some`/`None`), and fan-in wiring
//!   (ADR 0013, named per ADR 0034, end-to-end).
//! - [`is_generated_content`] — ADR 0011: the `is_generated_content`
//!   marker-detection helper's positive and negative cases (rstest).
//! - [`test_symbol_exclusion`] — ADR 0009: `partition_test_symbols`'s
//!   behavior via `analyze_diff` — per-file and whole-file test-symbol
//!   exclusion, `include_tests` gating, pure-rename retention.
//! - [`generated_exclusion`] — ADR 0010 & 0011: attribute-based
//!   (`generated_paths`) and content-based (marker detection) generated-file
//!   skipping via `analyze_diff`, including the deleted-wins-over-generated
//!   ordering and the `include_generated` opt-out.
//! - [`classification_wiring`] — ADR 0014: `classify_against_base`
//!   end-to-end via `analyze_diff` — `SignatureChanged`/`Added`/`removed`
//!   population, `read_base_file: None` "not attempted" contract,
//!   rename base-path routing, and the "never call `read_base_file` for
//!   an `Added` file" contract.
//! - [`collect_referenced_names`] — the `collect_referenced_names`
//!   helper's reference-name gathering, empty-input, deleted-file skip,
//!   and malformed-diff error path.
//! - [`analyze_repo`] — ADR 0017: whole-repo `analyze_repo` — empty
//!   input, extracting every symbol, `classification: None` invariant,
//!   unsupported-language / read-fail / generated-path / generated-content
//!   / test-path skips, and fan-in aggregation.
//! - [`file_size_warnings`] — ADR 0028 integration: `analyze_diff` and
//!   `analyze_repo` both thread `(path, line_count)` pairs through to
//!   `Report::file_size_warnings`, and skipped files never appear there.
//! - [`parallel_determinism`] — ADR 0029 regression: `analyze_repo`'s
//!   rayon-driven per-file loop must produce byte-identical, source-order
//!   `Report`s across repeated calls.

use std::collections::HashMap;

mod analyze_diff;
mod analyze_repo;
mod classification_wiring;
mod collect_referenced_names;
mod file_size_warnings;
mod generated_exclusion;
mod is_generated_content;
mod parallel_determinism;
mod test_symbol_exclusion;

/// Builds a `read_file` port backed by an in-memory map, so tests never
/// touch the real filesystem.
pub(super) fn fake_reader(
    files: HashMap<&'static str, &'static str>,
) -> impl Fn(&str) -> std::io::Result<String> {
    move |path: &str| {
        files
            .get(path)
            .map(|s| s.to_string())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path.to_string()))
    }
}

/// An empty `SymbolGraph`, for tests where no changed symbols exist.
pub(super) fn empty_graph() -> crate::graph::SymbolGraph {
    crate::graph::SymbolGraph {
        nodes: vec![],
        edges: vec![],
        roots: vec![],
    }
}
