//! Per-sink rendering (ADR 0048's "destination-specific formatting is each
//! sink's own responsibility" decision): pure functions turning the same
//! audience-neutral `Vec<Note>` into sink A's human-addressed review
//! comments ([`render_review_comments`]) or sink B's AI-addressed Markdown
//! packet ([`render_agent_packet`]).

use super::{Note, RenderedComment};

/// Renders `notes` into sink A's [`RenderedComment`]s (ADR 0048): one per
/// note, in order. `line` is the note's anchor end (the last line of the
/// first hunk-intersecting contiguous run — see [`super::NoteLocation::anchor`]),
/// falling back to the symbol's own range end, then to line 1, so every
/// note still produces a postable comment even when neither `anchor` nor
/// `range` resolved (a non-symbol location, out of v1's scope per the
/// module doc comment, but handled defensively rather than dropped).
/// `start_line` is set only when the anchor spans more than one line —
/// GitHub's multi-line comment API distinguishes a single-line comment
/// (`start_line` omitted) from a range comment.
pub fn render_review_comments(notes: &[Note]) -> Vec<RenderedComment> {
    notes
        .iter()
        .map(|note| {
            // The `(1, 1)` fallback is reachable only if rinkaku-core's
            // changed-range computation and this crate's own hunk parser
            // ever disagree about what counts as changed; GitHub's review
            // API may then reject the comment with a 422.
            let (start, end) = note
                .location
                .anchor
                .or(note.location.range)
                .unwrap_or((1, 1));
            RenderedComment {
                path: note.location.path.clone(),
                line: end,
                start_line: (start != end).then_some(start),
                body: note.body.clone(),
            }
        })
        .collect()
}

/// Renders `notes` into sink B's AI-addressed Markdown packet (ADR 0048):
/// a request line followed by one section per note — its location heading,
/// the originating symbol's signature (when the note carries one) as a
/// fenced code block, then the note's own body verbatim.
pub fn render_agent_packet(notes: &[Note]) -> String {
    let mut packet =
        String::from("# Review notes\n\nAddress each of the following review notes.\n");
    for note in notes {
        packet.push('\n');
        packet.push_str(&format!("## {}\n", note_heading(note)));
        if let Some(signature) = &note.signature {
            packet.push_str("```\n");
            packet.push_str(signature);
            packet.push('\n');
            packet.push_str("```\n");
        }
        packet.push_str(&note.body);
        packet.push('\n');
    }
    packet
}

/// The `## {path}:{start}-{end} {symbol_name}` heading for one note in
/// [`render_agent_packet`]'s output — extracted so the "which range, which
/// name" formatting logic is unit-testable independent of the surrounding
/// packet assembly.
fn note_heading(note: &Note) -> String {
    let location = &note.location;
    let range = location.anchor.or(location.range).map(|(start, end)| {
        if start == end {
            format!("{start}")
        } else {
            format!("{start}-{end}")
        }
    });
    match (range, &location.symbol_name) {
        (Some(range), Some(name)) => format!("{}:{range} {name}", location.path),
        (Some(range), None) => format!("{}:{range}", location.path),
        (None, Some(name)) => format!("{} {name}", location.path),
        (None, None) => location.path.clone(),
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
