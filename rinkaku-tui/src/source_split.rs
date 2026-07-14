//! Reconstructs a file's old-side text and pairs it against the new side
//! into split (side-by-side) rows for the source view's diff overlay (ADR
//! 0049), reusing [`crate::split_pairing::pair_hunk_lines`]'s changed-run
//! alignment rather than a second algorithm — see that ADR's Decision 1-2
//! for why old-side reconstruction is a pure reverse-application over
//! already-parsed [`FileHunks`] rather than a second IO read.

use crate::diff_view::{DiffLine, DiffLineKind, FileHunks};
use crate::split_pairing::{SplitRow, pair_hunk_lines};

#[cfg(test)]
#[path = "source_split/tests.rs"]
mod tests;

/// One row of the source view's split (side-by-side) diff overlay (ADR
/// 0049): the old-side and new-side line, either of which can be `None` —
/// a filler cell with nothing on that side, the same convention
/// [`crate::split_pairing::SplitRow`] uses for the diff pane's own split
/// view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSplitRow {
    pub left: Option<SourceSplitLine>,
    pub right: Option<SourceSplitLine>,
    pub kind: SourceSplitRowKind,
}

/// One side's line number and content for a [`SourceSplitRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSplitLine {
    pub line_number: usize,
    pub content: String,
}

/// What kind of row a [`SourceSplitRow`] is, for the renderer to color it:
/// `Unchanged` mirrors the same line on both sides (whether outside any
/// hunk, or a hunk's own `Context` line), `Changed` came from a hunk's
/// removed/added run (via [`pair_hunk_lines`]), and `Filler` is a blank
/// alignment row with nothing on either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSplitRowKind {
    Unchanged,
    Changed,
    Filler,
}

/// Reconstructs the old-side full-file text from `new_lines` (the source
/// view's already-read new-side content) and `file_hunks` (ADR 0049
/// decision 1), by walking the file once and, for each hunk, replaying its
/// `Hunk::lines` in reverse: `Removed`/`Context` lines become old-side
/// text, `Added` lines are dropped. Unchanged stretches between hunks (and
/// before the first / after the last) copy `new_lines` through unchanged.
///
/// Returns `None` when a hunk's `new_range` is missing, its declared start
/// falls outside `new_lines`, or a `Context` line's recorded text doesn't
/// match `new_lines` at the position this walk computes — the same drift
/// [`crate::source_diff::overlay_source_lines`] already detects for the
/// unified overlay (ADR 0046 decision 5), reused here rather than a second
/// check: reconstructing an old side from a diff that doesn't line up with
/// the file on disk would silently produce wrong content, worse than no
/// split view at all.
pub fn reconstruct_old_lines(new_lines: &[String], file_hunks: &FileHunks) -> Option<Vec<String>> {
    let mut old_lines = Vec::with_capacity(new_lines.len());
    let mut new_cursor = 0; // 0-based index into `new_lines` consumed so far.

    for hunk in &file_hunks.hunks {
        let (range_start, _) = hunk.new_range?;
        if range_start == 0 || range_start - 1 < new_cursor || range_start - 1 > new_lines.len() {
            return None;
        }
        old_lines.extend(new_lines[new_cursor..range_start - 1].iter().cloned());

        let mut new_position = range_start;
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Context => {
                    if new_lines.get(new_position - 1) != Some(&line.content) {
                        return None;
                    }
                    old_lines.push(line.content.clone());
                    new_position += 1;
                }
                DiffLineKind::Added => {
                    new_position += 1;
                }
                DiffLineKind::Removed => {
                    old_lines.push(line.content.clone());
                }
            }
        }
        new_cursor = new_position - 1;
    }

    if new_cursor > new_lines.len() {
        return None;
    }
    old_lines.extend(new_lines[new_cursor..].iter().cloned());
    Some(old_lines)
}

/// Builds one [`SourceSplitRow`] per line of `new_lines`, pairing it
/// against `reconstruct_old_lines`'s reconstructed old side (ADR 0049
/// decisions 2-3). Returns `None` when reconstruction itself returns
/// `None` (drift, or an unreadable hunk) — the caller falls back to the
/// unified overlay in that case (ADR 0049 decision 6).
///
/// The gap before each hunk (and after the last one) is a pure unchanged
/// stretch, copied onto both sides with old/new line numbers advanced in
/// lockstep (the two are equal text by construction there). Each hunk
/// itself — `Context` lines included — is handed to [`pair_hunk_lines`]
/// as a whole: a `Context` line comes back as its own mirrored row (the
/// same "one row per input line" shape [`pair_hunk_lines`] already gives
/// every line, matching this function's own `Unchanged` row for a
/// same-numbered gap line, just already paired); a changed run's rows
/// come back `Changed`/`Filler` per that function's own alignment.
pub fn split_source_rows(
    new_lines: &[String],
    file_hunks: &FileHunks,
) -> Option<Vec<SourceSplitRow>> {
    let old_lines = reconstruct_old_lines(new_lines, file_hunks)?;

    let mut rows = Vec::with_capacity(new_lines.len());
    let mut old_cursor = 0; // 0-based index into `old_lines`, next unconsumed line.
    let mut new_cursor = 0; // 0-based index into `new_lines`, next unconsumed line.

    for hunk in &file_hunks.hunks {
        // `reconstruct_old_lines` already validated every hunk's
        // `new_range` and `Context` lines against `new_lines`, so this
        // walk cannot underflow or go out of bounds.
        let (range_start, _) = hunk.new_range?;
        let gap_len = (range_start - 1).saturating_sub(new_cursor);
        rows.extend(unchanged_rows(
            &old_lines, new_lines, old_cursor, new_cursor, gap_len,
        ));
        old_cursor += gap_len;
        new_cursor += gap_len;

        for split_row in pair_hunk_lines(&hunk.lines) {
            let left = split_row.left.as_ref().map(|line| {
                let source_line = SourceSplitLine {
                    line_number: old_cursor + 1,
                    content: line.content.clone(),
                };
                old_cursor += 1;
                source_line
            });
            let right = split_row.right.as_ref().map(|line| {
                let source_line = SourceSplitLine {
                    line_number: new_cursor + 1,
                    content: line.content.clone(),
                };
                new_cursor += 1;
                source_line
            });
            let kind = if left.is_none() && right.is_none() {
                SourceSplitRowKind::Filler
            } else if source_split_row_is_context(&split_row, &hunk.lines) {
                SourceSplitRowKind::Unchanged
            } else {
                SourceSplitRowKind::Changed
            };
            rows.push(SourceSplitRow { left, right, kind });
        }
    }

    let trailing_len = new_lines.len().saturating_sub(new_cursor);
    rows.extend(unchanged_rows(
        &old_lines,
        new_lines,
        old_cursor,
        new_cursor,
        trailing_len,
    ));

    Some(rows)
}

/// Whether `split_row` is [`pair_hunk_lines`]'s mirrored-`Context`-line
/// row shape (`left_index == right_index`, pointing at a `Context` line in
/// `hunk_lines`) rather than a changed-run pairing — the only place
/// `pair_hunk_lines` ever sets both indices to the same value ([`pair_hunk_lines`]'s
/// own `Context` arm).
fn source_split_row_is_context(split_row: &SplitRow, hunk_lines: &[DiffLine]) -> bool {
    match (split_row.left_index, split_row.right_index) {
        (Some(left_index), Some(right_index)) if left_index == right_index => hunk_lines
            .get(left_index)
            .is_some_and(|line| line.kind == DiffLineKind::Context),
        _ => false,
    }
}

/// `count` [`SourceSplitRowKind::Unchanged`] rows mirroring
/// `old_lines[old_start..old_start + count]` against
/// `new_lines[new_start..new_start + count]` — the two slices are equal by
/// construction (an unchanged stretch is copied verbatim by
/// [`reconstruct_old_lines`]), so 1-based line numbers on each side are
/// `old_start`/`new_start` advanced in lockstep.
fn unchanged_rows(
    old_lines: &[String],
    new_lines: &[String],
    old_start: usize,
    new_start: usize,
    count: usize,
) -> Vec<SourceSplitRow> {
    (0..count)
        .map(|offset| SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: old_start + offset + 1,
                content: old_lines[old_start + offset].clone(),
            }),
            right: Some(SourceSplitLine {
                line_number: new_start + offset + 1,
                content: new_lines[new_start + offset].clone(),
            }),
            kind: SourceSplitRowKind::Unchanged,
        })
        .collect()
}
