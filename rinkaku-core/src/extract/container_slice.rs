//! Narrows a reported container's signature down to its header plus only
//! the member lines a diff actually touched (ADR 0071).
//!
//! A container node (Python/TypeScript `class`) is reported as a symbol
//! in its own right when a changed line falls directly in the class's
//! own body — not inside any nested member — and `slice_signature`
//! otherwise keeps every member's *signature* even when only one member
//! (or a plain field) actually changed. This module computes the extra
//! byte ranges to strip so untouched members disappear from the
//! rendered signature entirely, while `slice_signature`'s existing
//! method-body/comment stripping still applies to whatever remains.

use super::{LineRange, is_descendant_of, overlaps_any};
use crate::extract::definition_span::DefinitionNode;

/// The byte ranges of every member definition inside `container` that
/// does *not* overlap `changed_ranges` — i.e. every member whose entire
/// span should be dropped from `container`'s signature, header and all.
///
/// A member is any node in `all_definition_nodes` that is strictly
/// nested inside `container` — the same language-neutral "descendant of
/// a captured `@definition` node" notion the narrowest-enclosing-
/// definition rule already uses — so this applies identically to a
/// Python class method, a TypeScript class method, or a class nested
/// inside another class, with no per-language node-kind matching.
///
/// Each member's own widened span (ADR 0073: decorator/attribute
/// inclusive) is further widened to its whole source line
/// ([`widen_to_whole_line`]) before being returned: a captured definition
/// node's span does not include a trailing terminator some grammars place
/// outside it (e.g. TypeScript's `abstract_method_signature` excludes its
/// own trailing `;`), so removing only the bare node span would leave a
/// stray `;` and blank indentation behind. Starting from the member's
/// decorator/attribute rather than its bare node also means an untouched
/// member's decorator is dropped along with the rest of it, not left
/// behind as an orphaned line.
///
/// The widened ranges are then merged ([`merge_adjacent_ranges`]) before
/// being returned: when two or more untouched members share the same
/// physical line (e.g. `abstract area(): number; abstract perimeter():
/// number;`), each member's own node span stops right before the next
/// member's text starts, so widening the first member's range forward
/// only reaches as far as its own `;` — it cannot "see" the second
/// member's trailing `;` to widen into, since that byte range is already
/// claimed by the second member's own (separately widened) range. Merging
/// closes that gap instead of leaving each member to widen in isolation.
pub(super) fn untouched_member_ranges<'a>(
    container: tree_sitter::Node<'a>,
    all_definition_nodes: &[DefinitionNode<'a>],
    changed_ranges: &[LineRange],
    source: &[u8],
) -> Vec<std::ops::Range<usize>> {
    let widened: Vec<std::ops::Range<usize>> = all_definition_nodes
        .iter()
        .filter(|definition| is_descendant_of(definition.node, container))
        .filter(|definition| !overlaps_any(definition.line_range(), changed_ranges))
        .map(|definition| {
            widen_to_whole_line(
                definition.span_start_byte()..definition.node.end_byte(),
                source,
            )
        })
        .collect();
    merge_adjacent_ranges(widened, source)
}

/// Sorts `ranges` by start position and merges any two that overlap or are
/// separated only by bytes [`widen_to_whole_line`] would itself have
/// widened across (horizontal whitespace and/or a single `;`) — i.e. the
/// gap between them contains nothing worth keeping. This is what lets two
/// untouched members sharing one physical line be removed as a single
/// span instead of each stopping short at the other's boundary.
fn merge_adjacent_ranges(
    mut ranges: Vec<std::ops::Range<usize>>,
    source: &[u8],
) -> Vec<std::ops::Range<usize>> {
    ranges.sort_by_key(|r| r.start);

    let mut merged: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if is_only_gap_filler(&source[last.end.min(range.start)..range.start]) => {
                last.end = last.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// Whether `gap` contains only bytes that are safe to fold into a merged
/// removal: horizontal whitespace and/or a single leading `;`. Mirrors
/// what [`widen_to_whole_line`] itself would strip, so merging two ranges
/// across such a gap never removes anything [`widen_to_whole_line`]
/// wouldn't already have removed had the gap not been claimed by another
/// range first.
fn is_only_gap_filler(gap: &[u8]) -> bool {
    let after_semicolon = if gap.first() == Some(&b';') {
        &gap[1..]
    } else {
        gap
    };
    after_semicolon.iter().all(|&b| b == b' ' || b == b'\t')
}

/// Extends `range` to cover its whole source line: backward past leading
/// horizontal whitespace to (not including) the previous newline, when
/// that whitespace is the only thing between the previous newline and
/// `range`'s start; forward past a single trailing `;` and any horizontal
/// whitespace up to (not including) the next newline, when that is the
/// only thing between `range`'s end and the next newline. Either side is
/// widened independently — a member that shares its line with other kept
/// content on one side only still gets the other side trimmed.
fn widen_to_whole_line(range: std::ops::Range<usize>, source: &[u8]) -> std::ops::Range<usize> {
    let line_start = source[..range.start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let start = if source[line_start..range.start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
    {
        line_start
    } else {
        range.start
    };

    let mut after = range.end;
    if source.get(after) == Some(&b';') {
        after += 1;
    }
    let line_end = source[after..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |offset| after + offset);
    let end = if source[after..line_end]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
    {
        line_end
    } else {
        range.end
    };

    start..end
}
