//! Overlays a file's diff hunks onto its full-file source view (ADR 0046),
//! reusing [`crate::diff_view`]'s already-parsed [`Hunk`]s rather than
//! re-diffing or growing the diff pane's own hunk-relative coordinate
//! space (ADR 0046's Context section explains why the diff pane itself was
//! rejected as the home for this).

use crate::diff_view::{DiffLineKind, FileHunks, Hunk};

#[cfg(test)]
#[path = "source_diff/tests.rs"]
mod tests;

/// One diff-only line an overlay adds beyond what [`crate::source::SourceView`]
/// already renders, either a new-side line already present in the source
/// (`Added`) or a line that no longer exists there (`Removed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayLineKind {
    Added,
    Removed,
}

/// One [`Hunk`] line's position in the overlaid file, in the same 1-based
/// new-side numbering [`crate::source::SourceView`] uses for its own lines.
///
/// `Removed` carries the new-side line number it sits *before* — the same
/// "deletion is a position, not a line" convention
/// [`crate::diff_view::hunk_intersects`] already uses for a pure-deletion
/// hunk's `new_range` (this module's own doc comment), reused here rather
/// than inventing a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkOverlayLine {
    pub position: usize,
    pub kind: OverlayLineKind,
    pub content: String,
}

/// Walks `hunk.lines`, computing each `Added`/`Removed` line's position in
/// the overlaid file. `Context` lines are dropped from the result — the
/// source view's own lines already render them, so the overlay only needs
/// to add information for lines the diff actually changed.
///
/// Returns an empty `Vec` when `hunk.new_range` is `None` (an unreadable
/// header, or a deletion at new-side line 0 — [`Hunk::new_range`]'s own doc
/// comment): neither case has a position to seed the walk from.
pub fn hunk_overlay_lines(hunk: &Hunk) -> Vec<HunkOverlayLine> {
    let Some((range_start, _)) = hunk.new_range else {
        return Vec::new();
    };

    let mut position = range_start;
    let mut result = Vec::new();
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Added => {
                result.push(HunkOverlayLine {
                    position,
                    kind: OverlayLineKind::Added,
                    content: line.content.clone(),
                });
                position += 1;
            }
            DiffLineKind::Context => {
                position += 1;
            }
            DiffLineKind::Removed => {
                result.push(HunkOverlayLine {
                    position,
                    kind: OverlayLineKind::Removed,
                    content: line.content.clone(),
                });
            }
        }
    }
    result
}

/// One display row of the overlaid source view: either an unmodified source
/// line (`line_number` is that line's 1-based number in
/// [`crate::source::SourceView::lines`]), a source line the diff added, or a
/// diff-removed line inserted between two source lines (no `line_number` of
/// its own — [`HunkOverlayLine`]'s own doc comment on why a deletion has a
/// position but not a line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayRow {
    Unchanged { line_number: usize, content: String },
    Added { line_number: usize, content: String },
    Removed { content: String },
}

/// Composites every hunk in `file_hunks` onto `source_lines` (1-based
/// numbering, matching [`crate::source::SourceView::lines`]'s own
/// convention), producing one [`OverlayRow`] per output row: one entry per
/// source line (tagged `Added` when a hunk claims that new-side line
/// number, `Unchanged` otherwise) plus one extra `Removed` entry
/// immediately before the added/unchanged row a deletion's position points
/// at.
///
/// Returns `None` when a hunk's `Context` lines don't match `source_lines`
/// at the position the hunk claims — the file on disk has drifted from the
/// diff (ADR 0046 decision 5) — since composing with a wrong position
/// mapping would place colored lines in the wrong place, worse than no
/// overlay at all.
pub fn overlay_source_lines(
    source_lines: &[String],
    file_hunks: &FileHunks,
) -> Option<Vec<OverlayRow>> {
    for hunk in &file_hunks.hunks {
        if !hunk_matches_source(hunk, source_lines) {
            return None;
        }
    }

    let mut removed_before: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    let mut added_at: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for hunk in &file_hunks.hunks {
        for overlay_line in hunk_overlay_lines(hunk) {
            match overlay_line.kind {
                OverlayLineKind::Added => {
                    added_at.insert(overlay_line.position, overlay_line.content);
                }
                OverlayLineKind::Removed => {
                    removed_before
                        .entry(overlay_line.position)
                        .or_default()
                        .push(overlay_line.content);
                }
            }
        }
    }

    let mut rows = Vec::new();
    for (index, content) in source_lines.iter().enumerate() {
        let line_number = index + 1;
        if let Some(removed) = removed_before.remove(&line_number) {
            rows.extend(
                removed
                    .into_iter()
                    .map(|content| OverlayRow::Removed { content }),
            );
        }
        rows.push(if added_at.contains_key(&line_number) {
            OverlayRow::Added {
                line_number,
                content: content.clone(),
            }
        } else {
            OverlayRow::Unchanged {
                line_number,
                content: content.clone(),
            }
        });
    }
    // A deletion positioned at (source_lines.len() + 1) — the end of the
    // file — has no `Unchanged`/`Added` row above to anchor before; drain
    // whatever remains in source line order for a deterministic tail.
    let mut trailing_positions: Vec<usize> = removed_before.keys().copied().collect();
    trailing_positions.sort_unstable();
    for position in trailing_positions {
        if let Some(removed) = removed_before.remove(&position) {
            rows.extend(
                removed
                    .into_iter()
                    .map(|content| OverlayRow::Removed { content }),
            );
        }
    }

    Some(rows)
}

/// Slices `rows` (an [`overlay_source_lines`] result) down to the rows
/// belonging to the 1-based inclusive source line range
/// `[start_line, end_line]` — the source view's already-clamped scroll
/// window (`crate::ui::source_screen::clamped_window`, which operates on
/// raw source line indices, unaffected by how many extra `Removed` rows an
/// overlay inserts).
///
/// A `Removed` row has no `line_number` of its own (`OverlayRow`'s own doc
/// comment); its *anchor* is the `Unchanged`/`Added` row immediately
/// following it in `rows`' own order (or, for a run of `Removed` rows at
/// the very end of `rows` — a deletion at end of file, `overlay_source_lines`'s
/// own trailing-position handling — the source's last line, since that run
/// renders only once the reviewer has scrolled to see the file's end). A
/// `Removed` row is included exactly when its anchor line number falls in
/// `[start_line, end_line]`.
pub fn rows_in_source_range(
    rows: &[OverlayRow],
    start_line: usize,
    end_line: usize,
) -> &[OverlayRow] {
    let anchors = row_anchor_line_numbers(rows);

    let first = anchors.iter().position(|anchor| *anchor >= start_line);
    let Some(first) = first else {
        return &[];
    };
    let last = anchors[first..]
        .iter()
        .position(|anchor| *anchor > end_line)
        .map(|offset| first + offset)
        .unwrap_or(rows.len());

    &rows[first..last]
}

/// One entry per `rows`, the 1-based source line number that row is
/// anchored to for range-membership purposes ([`rows_in_source_range`]'s
/// own doc comment on what "anchor" means for a `Removed` row).
fn row_anchor_line_numbers(rows: &[OverlayRow]) -> Vec<usize> {
    let last_line_number =
        rows.iter()
            .filter_map(|row| match row {
                OverlayRow::Unchanged { line_number, .. }
                | OverlayRow::Added { line_number, .. } => Some(*line_number),
                OverlayRow::Removed { .. } => None,
            })
            .next_back()
            .unwrap_or(0);

    let mut anchors = vec![0; rows.len()];
    for (index, row) in rows.iter().enumerate().rev() {
        anchors[index] = match row {
            OverlayRow::Unchanged { line_number, .. } | OverlayRow::Added { line_number, .. } => {
                *line_number
            }
            OverlayRow::Removed { .. } => {
                anchors.get(index + 1).copied().unwrap_or(last_line_number)
            }
        };
    }
    anchors
}

/// Whether every `Context` line in `hunk` matches `source_lines` at the
/// position [`hunk_overlay_lines`]'s own walk would place it — the drift
/// check backing [`overlay_source_lines`]'s `None` return (ADR 0046
/// decision 5).
fn hunk_matches_source(hunk: &Hunk, source_lines: &[String]) -> bool {
    let Some((range_start, _)) = hunk.new_range else {
        // No position to check against; `hunk_overlay_lines` already
        // returns nothing for this hunk, so it cannot introduce a drifted
        // row either.
        return true;
    };

    let mut position = range_start;
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Context => {
                let Some(actual) = source_lines.get(position - 1) else {
                    return false;
                };
                if actual != &line.content {
                    return false;
                }
                position += 1;
            }
            DiffLineKind::Added => position += 1,
            DiffLineKind::Removed => {}
        }
    }
    true
}
