//! Derived, read-only annotation-location markers (ADR 0048's Rendering
//! boundary decision): a side table the tree pane and diff pane consult to
//! badge/mark a row or line that already has an annotation attached,
//! mirroring `blast_radius_selection`/`diff_pane_content`'s own
//! cache-on-change precedent in `crate::lib::run_app` — [`build_annotation_markers`]
//! must never be called from inside `crate::ui::draw`/`row_view`/the diff
//! pane's line-rendering functions, only from `run_app`, gated by
//! [`should_recompute_annotation_markers`].

use crate::review::Annotation;
use std::collections::HashMap;

/// Per-path/per-symbol/per-line annotation counts and ranges, derived once
/// from [`crate::review::ReviewState::notes`] on a change-gated schedule
/// (`crate::lib::run_app`'s `AnnotationMarkers` cache, alongside
/// `blast_radius_selection`/`diff_pane_content`) — never recomputed inside
/// the draw path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnnotationMarkers {
    /// Annotation count per symbol id — consulted by a symbol tree row.
    pub symbol_counts: HashMap<String, usize>,
    /// Annotation count per file path — consulted by a `File` tree row.
    /// Includes every annotation under that path regardless of whether it
    /// carries a `symbol_id` (v1 only composes symbol-anchored annotations,
    /// but this stays keyed by path rather than requiring a symbol so a
    /// future file-level annotation is covered by construction rather than
    /// needing a second field).
    pub file_counts: HashMap<String, usize>,
    /// Every annotation's new-side line range, keyed by path — consulted by
    /// the diff pane to mark individual lines. An annotation with no
    /// `range` (v1: non-symbol locations only, out of scope per
    /// `crate::review`'s module doc comment) contributes no entry.
    pub line_ranges: HashMap<String, Vec<(usize, usize)>>,
}

/// Builds an [`AnnotationMarkers`] from `annotations` — a plain walk over
/// the accumulated review annotations, O(annotations), unbounded in the
/// number of annotations a long review session accrues (this function's own
/// doc comment on why it must be change-gated, not called per frame).
pub fn build_annotation_markers(annotations: &[Annotation]) -> AnnotationMarkers {
    let mut markers = AnnotationMarkers::default();
    for annotation in annotations {
        *markers
            .file_counts
            .entry(annotation.location.path.clone())
            .or_insert(0) += 1;
        if let Some(symbol_id) = &annotation.location.symbol_id {
            *markers.symbol_counts.entry(symbol_id.clone()).or_insert(0) += 1;
        }
        if let Some(range) = annotation.location.range {
            markers
                .line_ranges
                .entry(annotation.location.path.clone())
                .or_default()
                .push(range);
        }
    }
    markers
}

/// Whether line `line` (1-based, new-side) in file `path` falls inside any
/// annotation's range — the diff pane's own per-line marker lookup.
pub fn line_has_annotation(markers: &AnnotationMarkers, path: &str, line: usize) -> bool {
    markers.line_ranges.get(path).is_some_and(|ranges| {
        ranges
            .iter()
            .any(|&(start, end)| start <= line && line <= end)
    })
}

#[cfg(test)]
#[path = "annotation_markers_tests.rs"]
mod tests;
