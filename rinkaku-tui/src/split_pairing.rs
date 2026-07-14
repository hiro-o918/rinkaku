//! Split (side-by-side) diff view row pairing (ADR 0044 decisions 3-4,
//! amended), split out from `crate::diff_shape` (CLAUDE.md's file-size
//! discipline: an independent responsibility — pairing removed/added lines
//! into rendered rows — distinct from that module's section-building and
//! unified-view line counting) once this amendment's alignment algorithm
//! grew the pairing logic past a single function. Re-exported from
//! `crate::diff_shape` so every external call site keeps using
//! `crate::diff_shape::{SplitRow, pair_hunk_lines}` unchanged.

use crate::diff_view::{DiffLine, DiffLineKind};

/// One rendered row of a split (side-by-side) diff view (ADR 0044): the
/// old-side and new-side [`DiffLine`] shown on that row, either of which
/// can be `None` — a filler cell with nothing on that side. `left_index`/
/// `right_index` is that side's line's position in the hunk's original
/// `lines` slice (`None` alongside a `None` line) — `crate::ui::diff_pane`
/// needs this to look up [`crate::highlight::lookup_hunk_highlight_by_index`]'s
/// per-line highlight table, which is indexed by that original interleaved
/// position, not by `SplitRow` position (the two diverge as soon as any run
/// merges two source lines onto one row).
///
/// [`pair_hunk_lines`] always returns one `SplitRow` per input
/// [`DiffLine`], never fewer, so a hunk's split-mode row count matches its
/// unified-mode row count exactly (ADR 0044 decision 4) — this is what lets
/// `crate::diff_shape`'s `walk_sections`/`hunk_start_lines`/
/// `section_start_line_for_symbol`/`symbol_id_for_scroll_line` stay
/// unchanged regardless of [`crate::app::DiffViewMode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub left: Option<DiffLine>,
    pub left_index: Option<usize>,
    pub right: Option<DiffLine>,
    pub right_index: Option<usize>,
}

/// Pairs a hunk's interleaved `lines` into old-side/new-side [`SplitRow`]s
/// for the split diff view (ADR 0044 decision 3, amended). A `Context` line
/// mirrors onto both sides. A maximal run of consecutive `Removed` lines
/// immediately followed by a maximal run of consecutive `Added` lines is
/// aligned by [`align_run`] — see its own doc comment for the pairing rule.
/// A `Removed` run with no following `Added` run (or vice versa) pairs every
/// line against `None` on the other side.
///
/// Always returns exactly `lines.len()` rows (ADR 0044 decision 4): when a
/// pairing merges two source lines onto one rendered row, a trailing
/// `SplitRow { left: None, right: None }` filler row is appended for each
/// merge, so the total row count never drops below the unified view's own
/// line count — see [`SplitRow`]'s own doc comment for why this invariant
/// matters.
pub fn pair_hunk_lines(lines: &[DiffLine]) -> Vec<SplitRow> {
    let mut rows = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        match lines[i].kind {
            DiffLineKind::Context => {
                rows.push(SplitRow {
                    left: Some(lines[i].clone()),
                    left_index: Some(i),
                    right: Some(lines[i].clone()),
                    right_index: Some(i),
                });
                i += 1;
            }
            DiffLineKind::Removed => {
                let removed_start = i;
                let mut removed_end = i;
                while removed_end < lines.len() && lines[removed_end].kind == DiffLineKind::Removed
                {
                    removed_end += 1;
                }
                let added_start = removed_end;
                let mut added_end = removed_start.max(added_start);
                while added_end < lines.len() && lines[added_end].kind == DiffLineKind::Added {
                    added_end += 1;
                }

                let removed_run = &lines[removed_start..removed_end];
                let added_run = &lines[added_start..added_end];
                rows.extend(align_run(
                    removed_run,
                    removed_start,
                    added_run,
                    added_start,
                ));

                i = added_end;
            }
            DiffLineKind::Added => {
                // An `Added` run with no preceding `Removed` run (a pure
                // insertion) — the `Removed` arm above already consumes any
                // `Added` run that immediately follows a `Removed` run, so
                // reaching this arm means there was none.
                rows.push(SplitRow {
                    left: None,
                    left_index: None,
                    right: Some(lines[i].clone()),
                    right_index: Some(i),
                });
                i += 1;
            }
        }
    }
    rows
}

/// A run pair longer than this on either side skips [`align_run`]'s O(n*m)
/// similarity DP and falls back straight to [`positional_pairing`] — the
/// DP cost is quadratic in run length, and a replace of this size is rare
/// enough in practice that avoiding the cost matters more than the alignment
/// quality gain for it.
const SIMILARITY_ALIGNMENT_MAX_RUN_LEN: usize = 200;

/// The minimum per-line similarity score (from [`line_similarity`], a
/// `0.0..=1.0` normalized token overlap) for two lines to be considered the
/// "same" line during alignment. Below this, [`align_run`] treats the pair
/// as unrelated rather than forcing a low-confidence match.
const SIMILARITY_THRESHOLD: f64 = 0.5;

/// Aligns one removed/added run pair into [`SplitRow`]s (ADR 0044 amendment
/// to decision 3). Positional pairing (row `i` gets the run's `i`-th
/// removed/added line) reads well when a replace changes the *content* of
/// each line in place, but reads badly when the two runs are shifted
/// relative to each other — e.g. lines inserted ahead of an otherwise
/// unchanged line pushes that line's row to line up against unrelated
/// content instead of its near-identical counterpart (the motivating case
/// for this change: a leading doc comment inserted ahead of an unchanged
/// signature).
///
/// Instead, this finds the order-preserving matching between `removed_run`
/// and `added_run` that maximizes total [`line_similarity`], keeping only
/// matches at or above [`SIMILARITY_THRESHOLD`] (a Needleman-Wunsch-style
/// alignment DP, gap cost 0 since an unmatched line just becomes its own
/// `None`-paired row rather than being penalized). An unmatched line on
/// either side becomes its own row paired with `None`; a run longer than
/// [`SIMILARITY_ALIGNMENT_MAX_RUN_LEN`] skips the DP and falls back to
/// [`positional_pairing`], and so does a run pair with zero matches above
/// threshold — the latter reproduces the pre-amendment behavior exactly
/// rather than emitting every line as unmatched, since positional pairing is
/// already the best available guess once similarity has no signal to offer.
///
/// Preserves [`pair_hunk_lines`]'s total-row invariant: one filler row per
/// matched pair, mirroring [`positional_pairing`]'s own filler-row count for
/// the fully-positional case.
fn align_run(
    removed_run: &[DiffLine],
    removed_start: usize,
    added_run: &[DiffLine],
    added_start: usize,
) -> Vec<SplitRow> {
    if removed_run.len() > SIMILARITY_ALIGNMENT_MAX_RUN_LEN
        || added_run.len() > SIMILARITY_ALIGNMENT_MAX_RUN_LEN
    {
        return positional_pairing(removed_run, removed_start, added_run, added_start);
    }

    let pairs = best_alignment(removed_run, added_run);
    if pairs.is_empty() {
        return positional_pairing(removed_run, removed_start, added_run, added_start);
    }

    let mut rows = Vec::with_capacity(removed_run.len() + added_run.len());
    let mut removed_cursor = 0;
    let mut added_cursor = 0;
    for (removed_offset, added_offset) in &pairs {
        while removed_cursor < *removed_offset {
            rows.push(SplitRow {
                left: Some(removed_run[removed_cursor].clone()),
                left_index: Some(removed_start + removed_cursor),
                right: None,
                right_index: None,
            });
            removed_cursor += 1;
        }
        while added_cursor < *added_offset {
            rows.push(SplitRow {
                left: None,
                left_index: None,
                right: Some(added_run[added_cursor].clone()),
                right_index: Some(added_start + added_cursor),
            });
            added_cursor += 1;
        }
        rows.push(SplitRow {
            left: Some(removed_run[removed_cursor].clone()),
            left_index: Some(removed_start + removed_cursor),
            right: Some(added_run[added_cursor].clone()),
            right_index: Some(added_start + added_cursor),
        });
        removed_cursor += 1;
        added_cursor += 1;
    }
    while removed_cursor < removed_run.len() {
        rows.push(SplitRow {
            left: Some(removed_run[removed_cursor].clone()),
            left_index: Some(removed_start + removed_cursor),
            right: None,
            right_index: None,
        });
        removed_cursor += 1;
    }
    while added_cursor < added_run.len() {
        rows.push(SplitRow {
            left: None,
            left_index: None,
            right: Some(added_run[added_cursor].clone()),
            right_index: Some(added_start + added_cursor),
        });
        added_cursor += 1;
    }

    // One filler row per matched pair, so the total row count stays at
    // `removed_run.len() + added_run.len()` regardless of how many lines
    // matched — the same invariant `positional_pairing` maintains for its
    // own (fully-positional) matching.
    for _ in 0..pairs.len() {
        rows.push(SplitRow {
            left: None,
            left_index: None,
            right: None,
            right_index: None,
        });
    }
    rows
}

/// The order-preserving, maximum-total-similarity matching between
/// `removed_run` and `added_run`, keeping only pairs at or above
/// [`SIMILARITY_THRESHOLD`]. Returns `(removed_offset, added_offset)` pairs
/// in increasing order on both sides — a standard Needleman-Wunsch
/// alignment DP with zero gap cost, where the "substitution score" is
/// [`line_similarity`] when it clears the threshold and `f64::MIN` (never
/// worth taking) otherwise.
fn best_alignment(removed_run: &[DiffLine], added_run: &[DiffLine]) -> Vec<(usize, usize)> {
    let rows = removed_run.len();
    let cols = added_run.len();
    let mut score = vec![vec![0.0_f64; cols + 1]; rows + 1];
    for r in 1..=rows {
        for c in 1..=cols {
            let similarity =
                line_similarity(&removed_run[r - 1].content, &added_run[c - 1].content);
            let match_score = if similarity >= SIMILARITY_THRESHOLD {
                score[r - 1][c - 1] + similarity
            } else {
                f64::MIN
            };
            score[r][c] = match_score.max(score[r - 1][c]).max(score[r][c - 1]);
        }
    }

    let mut pairs = Vec::new();
    let mut r = rows;
    let mut c = cols;
    while r > 0 && c > 0 {
        if score[r][c] == score[r - 1][c] {
            r -= 1;
        } else if score[r][c] == score[r][c - 1] {
            c -= 1;
        } else {
            pairs.push((r - 1, c - 1));
            r -= 1;
            c -= 1;
        }
    }
    pairs.reverse();
    pairs
}

/// Normalized token-overlap similarity between two lines, in `0.0..=1.0` —
/// the Jaccard index of their whitespace-split token sets. Token-based
/// (rather than character-based) so reindentation or a single changed
/// argument does not swamp the score the way a character diff would, and
/// order-insensitive so a reordered clause still scores high — both traits
/// suit comparing source lines, whose meaningful unit is the token, not the
/// character.
fn line_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;

    let tokens_a: HashSet<&str> = a.split_whitespace().collect();
    let tokens_b: HashSet<&str> = b.split_whitespace().collect();
    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// The pre-amendment pairing rule (ADR 0044 decision 3): row `i` of the run
/// gets the run's `i`-th removed/added line; when the two runs differ in
/// length, the longer run's excess lines pair against `None` on the shorter
/// side, one row per excess line, followed by one filler row per paired
/// row (decision 4). Used directly by [`align_run`] as its fallback when
/// similarity alignment has no signal to offer or the run is too large to
/// align cheaply.
fn positional_pairing(
    removed_run: &[DiffLine],
    removed_start: usize,
    added_run: &[DiffLine],
    added_start: usize,
) -> Vec<SplitRow> {
    let mut rows = Vec::with_capacity(removed_run.len() + added_run.len());
    let paired = removed_run.len().max(added_run.len());
    for offset in 0..paired {
        rows.push(SplitRow {
            left: removed_run.get(offset).cloned(),
            left_index: (offset < removed_run.len()).then_some(removed_start + offset),
            right: added_run.get(offset).cloned(),
            right_index: (offset < added_run.len()).then_some(added_start + offset),
        });
    }
    // Decision 4's filler rows: the run consumed `removed_run.len() +
    // added_run.len()` source lines but only emitted `paired` rows so far —
    // pad back up to the source count with blank filler rows.
    let consumed = removed_run.len() + added_run.len();
    for _ in paired..consumed {
        rows.push(SplitRow {
            left: None,
            left_index: None,
            right: None,
            right_index: None,
        });
    }
    rows
}

#[cfg(test)]
#[path = "split_pairing_tests/mod.rs"]
mod tests;
