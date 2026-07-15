//! Tests for `crate::hunk_split`, split from the source file (ADR 0028)
//! following the `crate::split_pairing_tests` precedent.

use super::*;
use crate::diff_view::{DiffLine, DiffLineKind};

mod split_hunk;

/// Builds a [`Hunk`] fixture with `new_range` derived from `new_start` and
/// the added/context line count, mirroring
/// `crate::diff_shape_tests::hunk`'s own convention (every non-`Removed`
/// entry of `kinds_and_content` counts toward the new-side span). Lets each
/// test spell out `[(DiffLineKind, &str), ...]` inline rather than building
/// `DiffLine`s by hand.
pub(super) fn hunk(header: &str, new_start: usize, lines: &[(DiffLineKind, &str)]) -> Hunk {
    let diff_lines: Vec<DiffLine> = lines
        .iter()
        .map(|(kind, content)| DiffLine {
            kind: *kind,
            content: content.to_string(),
        })
        .collect();
    let new_count = diff_lines
        .iter()
        .filter(|line| line.kind != DiffLineKind::Removed)
        .count();
    let new_range = if new_count == 0 {
        (new_start > 0).then_some((new_start, new_start - 1))
    } else {
        Some((new_start, new_start + new_count - 1))
    };
    Hunk {
        header: header.to_string(),
        new_range,
        lines: diff_lines,
    }
}

pub(super) fn sub(
    header: &str,
    new_range: Option<(usize, usize)>,
    lines: &[(DiffLineKind, &str)],
    origin_offset: usize,
) -> SubHunk {
    SubHunk {
        header: header.to_string(),
        new_range,
        lines: lines
            .iter()
            .map(|(kind, content)| DiffLine {
                kind: *kind,
                content: content.to_string(),
            })
            .collect(),
        origin_offset,
    }
}
