//! Derived, read-only note-location markers (ADR 0048's Rendering
//! boundary decision): a side table the tree pane and diff pane consult to
//! badge/mark a row or line that already has a review note attached,
//! mirroring `blast_radius_selection`/`diff_pane_content`'s own
//! cache-on-change precedent in `crate::lib::run_app` — [`build_note_markers`]
//! must never be called from inside `crate::ui::draw`/`row_view`/the diff
//! pane's line-rendering functions, only from `run_app`, gated by
//! [`should_recompute_note_markers`].

use crate::review::Note;
use std::collections::HashMap;

/// Per-path/per-symbol/per-line note counts and ranges, derived once from
/// [`crate::review::ReviewState::notes`] on a change-gated schedule
/// (`crate::lib::run_app`'s `NoteMarkers` cache, alongside
/// `blast_radius_selection`/`diff_pane_content`) — never recomputed inside
/// the draw path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteMarkers {
    /// Note count per symbol id — consulted by a symbol tree row.
    pub symbol_counts: HashMap<String, usize>,
    /// Note count per file path — consulted by a `File` tree row. Includes
    /// every note under that path regardless of whether it carries a
    /// `symbol_id` (v1 only composes symbol-anchored notes, but this stays
    /// keyed by path rather than requiring a symbol so a future
    /// file-level note is covered by construction rather than needing a
    /// second field).
    pub file_counts: HashMap<String, usize>,
    /// Every note's new-side line range, keyed by path — consulted by the
    /// diff pane to mark individual lines. A note with no `range` (v1:
    /// non-symbol locations only, out of scope per `crate::review`'s
    /// module doc comment) contributes no entry.
    pub line_ranges: HashMap<String, Vec<(usize, usize)>>,
}

/// Builds a [`NoteMarkers`] from `notes` — a plain walk over the
/// accumulated review notes, O(notes), unbounded in the number of notes a
/// long review session accrues (this function's own doc comment on why it
/// must be change-gated, not called per frame).
pub fn build_note_markers(notes: &[Note]) -> NoteMarkers {
    let mut markers = NoteMarkers::default();
    for note in notes {
        *markers
            .file_counts
            .entry(note.location.path.clone())
            .or_insert(0) += 1;
        if let Some(symbol_id) = &note.location.symbol_id {
            *markers.symbol_counts.entry(symbol_id.clone()).or_insert(0) += 1;
        }
        if let Some(range) = note.location.range {
            markers
                .line_ranges
                .entry(note.location.path.clone())
                .or_default()
                .push(range);
        }
    }
    markers
}

/// Whether line `line` (1-based, new-side) in file `path` falls inside any
/// note's range — the diff pane's own per-line marker lookup.
pub fn line_has_note(markers: &NoteMarkers, path: &str, line: usize) -> bool {
    markers.line_ranges.get(path).is_some_and(|ranges| {
        ranges
            .iter()
            .any(|&(start, end)| start <= line && line <= end)
    })
}

#[cfg(test)]
#[path = "note_markers_tests.rs"]
mod tests;
