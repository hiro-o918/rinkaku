//! Tests for [`super::render_markdown`], split by which section /
//! behavior each file pins:
//!
//! - [`empty_and_ordering`] — the empty-report short-circuit, and the
//!   ordering of the "Tests" / "Other changed files" / "Skipped files"
//!   sections relative to each other.
//! - [`change_graph_summary`] — the one-line summary under "## Change
//!   graph" / "## Repository graph" and the origin-driven wording.
//! - [`change_graph_tree`] — tree rendering: nesting, DFS order,
//!   `(see above)`, cycle warnings, and `— uses:` folding.
//! - [`definition_body`] — one symbol's "### ..." entry: container
//!   comment, `Depends on:` list, and the fence-widening rules.
//! - [`sections_skipped_fan_in_filesize`] — the "Skipped files",
//!   "High fan-in symbols", and "File sizes" sections plus the
//!   ADR 0028 JSON shape.
//! - [`lookup_miss_defenses`] — defensive branches for a `graph` node
//!   with no matching `ExtractedSymbol` in `files`.
//! - [`classification_and_removed`] — ADR 0014 classification markers,
//!   the diff-signature block, and the "Removed symbols" section.
//! - [`untested_changes`] — ADR 0059's "## Untested changes" section:
//!   omit-when-empty and its placement between "High fan-in symbols"
//!   and "File sizes".

use super::*;
use crate::diff::LineRange;
use crate::extract::SymbolKind;
use crate::graph::Node;

mod change_graph_summary;
mod change_graph_tree;
mod classification_and_removed;
mod definition_body;
mod empty_and_ordering;
mod lookup_miss_defenses;
mod sections_skipped_fan_in_filesize;
mod untested_changes;

/// Builds an `ExtractedSymbol` for rendering tests, with `id` set (the
/// graph-building pipeline stage this module assumes already ran) and
/// every other field defaulted to something inert unless overridden via
/// struct-update syntax at the call site.
pub(super) fn symbol(id: &str, name: &str, kind: SymbolKind, signature: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind,
        signature: signature.to_string(),
        range: LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

pub(super) fn node(id: &str, path: &str, name: &str) -> Node {
    Node {
        id: id.to_string(),
        path: path.to_string(),
        name: name.to_string(),
        is_test: false,
    }
}
