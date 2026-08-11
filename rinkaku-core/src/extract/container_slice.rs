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

use super::{LineRange, is_descendant_of, node_to_line_range, overlaps_any};

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
/// Each member's own node span is widened to its whole source line
/// ([`widen_to_whole_line`]) before being returned: a captured definition
/// node's span does not include a trailing terminator some grammars place
/// outside it (e.g. TypeScript's `abstract_method_signature` excludes its
/// own trailing `;`), so removing only the bare node span would leave a
/// stray `;` and blank indentation behind.
pub(super) fn untouched_member_ranges<'a>(
    container: tree_sitter::Node<'a>,
    all_definition_nodes: &[tree_sitter::Node<'a>],
    changed_ranges: &[LineRange],
    source: &[u8],
) -> Vec<std::ops::Range<usize>> {
    all_definition_nodes
        .iter()
        .filter(|node| is_descendant_of(**node, container))
        .filter(|node| !overlaps_any(node_to_line_range(**node), changed_ranges))
        .map(|node| widen_to_whole_line(node.start_byte()..node.end_byte(), source))
        .collect()
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
