//! Splits one [`Hunk`] shared by multiple symbol ranges into per-symbol
//! sub-hunks (ADR 0053), split out from `crate::diff_shape` (CLAUDE.md's
//! file-size discipline: an independent responsibility — line-level hunk
//! splitting — distinct from that module's section-building and
//! unified-view line counting), following the `crate::split_pairing`
//! precedent for the same kind of extraction.
//!
//! ADR 0029 attributed a hunk intersecting several symbol ranges to every
//! one of them by cloning the whole hunk into each owning section — simple,
//! but it meant a reviewer walking adjacent symbols saw the same hunk body
//! repeated once per section. ADR 0053 replaces that whole-hunk duplication
//! with splitting: each symbol's section gets only the lines that are
//! actually "its own".

use crate::diff_view::{DiffLine, DiffLineKind, Hunk};
use rinkaku_core::diff::LineRange;

/// One symbol's slice of a split hunk: a subset of the original hunk's
/// `lines`, with its own recomputed `@@` header and new-side range (ADR
/// 0053 decision "every sub-hunk gets an exactly recomputed header").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubHunk {
    pub header: String,
    pub new_range: Option<(usize, usize)>,
    pub lines: Vec<DiffLine>,
    /// This sub-hunk's start index within the original hunk's `lines` —
    /// threaded through so a caller holding the original hunk's
    /// `source_index` (`crate::diff_shape::AttributedHunk`) can still look
    /// up per-line highlight data, which stays keyed by position in the
    /// *original* hunk (`crate::highlight::highlight_hunk`'s own doc
    /// comment on why it is computed once per original hunk, not per
    /// sub-hunk).
    pub origin_offset: usize,
}

/// Splits `hunk` into per-symbol [`SubHunk`]s against `symbols` (each
/// entry's own index into the caller's symbol list, paired with its
/// [`LineRange`], mirroring `crate::diff_shape::build_file_content`'s
/// existing `symbols.iter().enumerate()` shape). Returns one
/// `(Option<usize>, SubHunk)` per maximal contiguous run of lines sharing
/// the same owner set — `Some(index)` for a symbol-owned run, `None` for a
/// run that intersects no symbol at all (module-level bucket, ADR 0053
/// decision). A line whose new-file position falls inside more than one
/// symbol's range (only possible with pathologically overlapping symbol
/// ranges — real extractors don't produce these, but ADR 0029's own
/// "attribute to every intersecting symbol" contract still applies) appears
/// in more than one returned entry, mirroring ADR 0029's pre-split behavior
/// for that line rather than picking one owner arbitrarily.
///
/// When `hunk` intersects fewer than two symbol ranges, returns exactly one
/// entry wrapping `hunk` unchanged (`origin_offset: 0`) — splitting only
/// has a purpose once a hunk is actually shared, and single-owner hunks
/// (the common case) skip the line-by-line scan entirely.
pub fn split_hunk(hunk: &Hunk, symbols: &[(usize, LineRange)]) -> Vec<(Option<usize>, SubHunk)> {
    let owners = line_owners(hunk, symbols);
    let all_owners: std::collections::HashSet<usize> = owners.iter().flatten().copied().collect();
    let every_line_unowned = owners.iter().all(Vec::is_empty);

    // Nothing to split when at most one distinct symbol touches this hunk
    // at all: either every line is unowned (module-level), or every owned
    // line shares the same single symbol.
    if all_owners.len() <= 1 {
        let owner = if every_line_unowned {
            None
        } else {
            all_owners.into_iter().next()
        };
        return vec![(
            owner,
            SubHunk {
                header: hunk.header.clone(),
                new_range: hunk.new_range,
                lines: hunk.lines.clone(),
                origin_offset: 0,
            },
        )];
    }

    build_sub_hunks(hunk, &owners)
}

/// Resolves the set of owners for each line in `hunk.lines` (same length,
/// same order): every symbol index whose range contains that line's
/// new-file position, empty when none does.
///
/// Added/Context lines carry their own new-file line number (the same
/// derivation `crate::ui::diff_pane::new_side_line_numbers` uses) and are
/// resolved directly against `symbols`. A Removed line has no new-file line
/// number of its own — deletion is a *position*, not a range
/// (`crate::diff_view::Hunk::new_range`'s own zero-width convention) — so it
/// inherits the owner set of the current run, resolved in a second pass
/// once every Added/Context line's owners are known: a leading run of
/// Removed lines (before any Added/Context line has appeared) takes the
/// owner set of the *next* resolved line, and every other Removed line
/// takes the owner set of the *previous* resolved line.
fn line_owners(hunk: &Hunk, symbols: &[(usize, LineRange)]) -> Vec<Vec<usize>> {
    let mut owners: Vec<Option<Vec<usize>>> = vec![None; hunk.lines.len()];
    let mut next_new_line = hunk.new_range.map(|(start, _)| start);

    for (index, line) in hunk.lines.iter().enumerate() {
        if line.kind == DiffLineKind::Removed {
            continue;
        }
        let Some(new_line) = next_new_line else {
            continue;
        };
        let line_owners: Vec<usize> = symbols
            .iter()
            .filter(|(_, range)| range.start <= new_line && new_line <= range.end)
            .map(|(symbol_index, _)| *symbol_index)
            .collect();
        owners[index] = Some(line_owners);
        next_new_line = Some(new_line + 1);
    }

    // Retroactive attribution for Removed runs (function doc comment):
    // forward-fill from the previous resolved line, then back-fill any
    // still-unresolved leading run from the next resolved line.
    let mut last_owner: Option<Vec<usize>> = None;
    for slot in owners.iter_mut() {
        match slot {
            Some(owner) => last_owner = Some(owner.clone()),
            None => *slot = last_owner.clone(),
        }
    }
    let mut next_owner: Option<Vec<usize>> = None;
    for slot in owners.iter_mut().rev() {
        match slot {
            Some(owner) => next_owner = Some(owner.clone()),
            None => *slot = next_owner.clone(),
        }
    }

    owners
        .into_iter()
        .map(|owner| owner.unwrap_or_default())
        .collect()
}

/// Groups `hunk.lines` into maximal contiguous runs, one per distinct
/// symbol (plus one for the module-level "no symbol" bucket), turning each
/// run into its own [`SubHunk`] with an exactly recomputed header
/// (function's own doc comment on [`split_hunk`]). A symbol whose owned
/// lines are all non-contiguous (interleaved with another symbol's lines)
/// yields more than one run, and thus more than one `SubHunk`, for that
/// same symbol — this is the exact shape splitting is meant to produce:
/// `crate::diff_shape::build_file_content` pushes every returned entry onto
/// that symbol's section in order, so its section still reads as one
/// contiguous diff top-to-bottom even though this function emitted several
/// pieces.
fn build_sub_hunks(hunk: &Hunk, owners: &[Vec<usize>]) -> Vec<(Option<usize>, SubHunk)> {
    // Every distinct owner that appears anywhere, each contributing its own
    // independent run-scan over the whole line range — this is what lets a
    // pathologically overlapping pair of symbol ranges (Context, ADR 0029)
    // both claim the same line without one silently winning over the other.
    let mut distinct_owners: Vec<Option<usize>> = Vec::new();
    for line_owners in owners {
        if line_owners.is_empty() && !distinct_owners.contains(&None) {
            distinct_owners.push(None);
        }
        for &owner in line_owners {
            if !distinct_owners.contains(&Some(owner)) {
                distinct_owners.push(Some(owner));
            }
        }
    }

    let mut result: Vec<(usize, Option<usize>, SubHunk)> = Vec::new();
    for owner in distinct_owners {
        let owns = |index: usize| -> bool {
            match owner {
                Some(symbol_index) => owners[index].contains(&symbol_index),
                None => owners[index].is_empty(),
            }
        };

        let mut next_new_line = hunk.new_range.map(|(start, _)| start).unwrap_or(1);
        let old_start = old_start_for_hunk(hunk).unwrap_or(0);
        let mut old_line_offset = 0usize;

        let mut index = 0;
        while index < hunk.lines.len() {
            if !owns(index) {
                if hunk.lines[index].kind != DiffLineKind::Removed {
                    next_new_line += 1;
                }
                if hunk.lines[index].kind != DiffLineKind::Added {
                    old_line_offset += 1;
                }
                index += 1;
                continue;
            }

            let start = index;
            let mut end = index + 1;
            while end < hunk.lines.len() && owns(end) {
                end += 1;
            }

            let run = &hunk.lines[start..end];
            let run_new_line_count = run
                .iter()
                .filter(|line| line.kind != DiffLineKind::Removed)
                .count();
            let run_old_line_count = run
                .iter()
                .filter(|line| line.kind != DiffLineKind::Added)
                .count();

            let new_range = if run_new_line_count == 0 {
                // A pure-deletion run: same zero-width convention
                // `crate::diff_view::Hunk::new_range` already uses for a
                // pure-deletion hunk — the position content used to occupy,
                // not a `start <= end` span.
                (next_new_line > 0).then_some((next_new_line, next_new_line - 1))
            } else {
                Some((next_new_line, next_new_line + run_new_line_count - 1))
            };
            let header =
                format_sub_hunk_header(old_start + old_line_offset, run_old_line_count, new_range);

            result.push((
                start,
                owner,
                SubHunk {
                    header,
                    new_range,
                    lines: run.to_vec(),
                    origin_offset: start,
                },
            ));

            next_new_line += run_new_line_count;
            old_line_offset += run_old_line_count;
            index = end;
        }
    }

    // Restore document order across owners: `distinct_owners`' own order
    // (first-appearance) does not match line order once a later-appearing
    // owner's first run actually starts earlier than an earlier-appearing
    // owner's later run — sorting by each entry's own start index is the
    // one ordering `crate::diff_shape::build_file_content` actually needs
    // (it pushes each entry onto its owning section, order within a
    // section coming from this sort, not from `distinct_owners`).
    result.sort_by_key(|(start, _, _)| *start);
    result
        .into_iter()
        .map(|(_, owner, sub_hunk)| (owner, sub_hunk))
        .collect()
}

/// Parses the original hunk header's old-side start (`@@ -a,b +c,d @@`'s
/// `a`), the base every sub-hunk's own old-side start is computed relative
/// to (ADR 0053 decision: "old-side start is the original hunk's old-side
/// start plus lines consumed by preceding sub-hunks"). `None` when the
/// header doesn't match the expected shape — mirrors
/// `crate::diff_view::parse_new_side_header`'s own defensive-parse
/// contract, since a hunk whose header this module cannot read at all
/// cannot have a meaningful old-side start recomputed for its sub-hunks
/// either.
fn old_start_for_hunk(hunk: &Hunk) -> Option<usize> {
    let body = hunk.header.strip_prefix("@@ ")?.split(" @@").next()?;
    let old_part = body.split(' ').next()?;
    let old_part = old_part.strip_prefix('-')?;
    let start_str = old_part.split(',').next()?;
    start_str.parse().ok()
}

/// Formats a sub-hunk's own `@@ -old_start,old_count +new_start,new_count @@`
/// header from its resolved old-side start/count and new-side range.
fn format_sub_hunk_header(
    old_start: usize,
    old_count: usize,
    new_range: Option<(usize, usize)>,
) -> String {
    let (new_start, new_count) = match new_range {
        Some((start, end)) if start <= end => (start, end - start + 1),
        Some((start, _)) => (start, 0),
        None => (0, 0),
    };
    format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@")
}

#[cfg(test)]
#[path = "hunk_split_tests/mod.rs"]
mod tests;
