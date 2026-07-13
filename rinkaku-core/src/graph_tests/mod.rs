//! Tests for `crate::graph`, split from the source file (ADR 0028) and
//! grouped by which pub function each block pins:
//!
//! - `build_graph` — `build_graph`'s structural coverage: empty, single
//!   node, edge, self-reference exclusion, and (path, name) id
//!   disambiguation with `@line` suffixes
//! - `roots_and_cycles` — `find_roots` (via SCC condensation) and
//!   `mark_cycle_edges`, including multi-cycle and shared-descendant
//!   non-cycle cases
//! - `stamp_ids` — `stamp_ids`'s pairing of `ExtractedSymbol.id` back
//!   onto the source `FileReport`s, including the disambiguated case
//! - `compute_fan_ins` — fan-in threshold, referrer dedup, cycle-edge
//!   contribution, and the descending / tie-break ordering
//! - `pivot` — `pivot_graph` / `pivot_roots` / `path_under_prefix`:
//!   subset root election, cycle across pivot, and empty-prefix identity

use super::*;
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};

mod build_graph;
mod compute_fan_ins;
mod pivot;
mod roots_and_cycles;
mod stamp_ids;

/// Builds an `ExtractedSymbol` with a given `name`/`referenced_names`,
/// filling every other field with a fixed placeholder — these tests
/// only care about the graph-building fields.
pub(super) fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
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
