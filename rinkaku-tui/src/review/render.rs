//! Per-sink rendering (ADR 0048's "destination-specific formatting is each
//! sink's own responsibility" decision): pure functions turning the same
//! audience-neutral `Vec<Annotation>` into sink A's human-addressed review
//! comments ([`render_review_comments`]) or sink B's AI-addressed Markdown
//! packet ([`render_agent_packet`]).
//!
//! ADR 0067 widened the annotation target set beyond present symbols, so
//! not every annotation resolves an `anchor`/`range` GitHub's review API can
//! post an inline comment against (a `File`/`Dir`/`RemovedSymbol` target
//! never does). [`partition_for_export`] splits sink A's input on exactly
//! that: anchored annotations still become [`RenderedComment`]s via
//! [`render_review_comments`]; unanchored ones are collected into sink A's
//! "Additional notes" body section via [`render_additional_notes`] instead
//! of the unsound `(1, 1)` fallback this module used to fall back to.

use super::{Annotation, AnnotationTarget, RenderedComment};

/// Splits `annotations` into the anchored subset (has a real `anchor` or
/// `range` to post an inline comment against) and the unanchored subset
/// (ADR 0067) — `crate::review_flow::perform_export` renders the former via
/// [`render_review_comments`] and the latter via [`render_additional_notes`],
/// composing both into one pending-review submission.
pub fn partition_for_export(annotations: &[Annotation]) -> (Vec<&Annotation>, Vec<&Annotation>) {
    annotations
        .iter()
        .partition(|annotation| has_anchor(&annotation.location))
}

fn has_anchor(location: &super::AnnotationLocation) -> bool {
    location.anchor.or(location.range).is_some()
}

/// Renders `annotations` into sink A's [`RenderedComment`]s (ADR 0048): one
/// per annotation, in order. `line` is the annotation's anchor end (the
/// last line of the first hunk-intersecting contiguous run — see
/// [`super::AnnotationLocation::anchor`]), falling back to the symbol's own
/// range end. Every `annotation` is expected to already have one or the
/// other — [`partition_for_export`] is what keeps an unanchored annotation
/// from ever reaching this function (ADR 0067 removed the old `(1, 1)`
/// fallback this module used when neither resolved). `start_line` is set
/// only when the anchor spans more than one line — GitHub's multi-line
/// comment API distinguishes a single-line comment (`start_line` omitted)
/// from a range comment.
pub fn render_review_comments(annotations: &[&Annotation]) -> Vec<RenderedComment> {
    annotations
        .iter()
        .filter_map(|annotation| {
            let (start, end) = annotation.location.anchor.or(annotation.location.range)?;
            Some(RenderedComment {
                path: annotation.location.path.clone(),
                line: end,
                start_line: (start != end).then_some(start),
                body: annotation.body.clone(),
            })
        })
        .collect()
}

/// Renders sink A's "Additional notes" body section (ADR 0067) for the
/// unanchored half of [`partition_for_export`]'s split. Returns an empty
/// string for an empty slice so `crate::review_flow::perform_export` can
/// unconditionally append this to [`super::ExportRequest::GithubReview`]'s
/// fixed summary without an extra "any unanchored annotations?" branch of
/// its own. Reuses [`annotation_heading`] — the same location text sink B
/// renders, just addressed to a human reviewer reading the review body
/// instead of an AI agent reading a packet.
pub fn render_additional_notes(annotations: &[&Annotation]) -> String {
    if annotations.is_empty() {
        return String::new();
    }
    let mut section = String::from("\n\n## Additional notes\n");
    for annotation in annotations {
        section.push_str(&format!(
            "- `{}`: {}\n",
            annotation_heading(annotation),
            annotation.body.lines().next().unwrap_or("")
        ));
    }
    section
}

/// Renders `annotations` into sink B's AI-addressed Markdown packet (ADR
/// 0048): a request line followed by one section per annotation — its
/// location heading, the originating symbol's signature (when the
/// annotation carries one) as a fenced code block, then the annotation's
/// own body verbatim.
pub fn render_agent_packet(annotations: &[Annotation]) -> String {
    let mut packet =
        String::from("# Review annotations\n\nAddress each of the following review annotations.\n");
    for annotation in annotations {
        packet.push('\n');
        packet.push_str(&format!("## {}\n", annotation_heading(annotation)));
        if let Some(signature) = &annotation.signature {
            packet.push_str("```\n");
            packet.push_str(signature);
            packet.push('\n');
            packet.push_str("```\n");
        }
        packet.push_str(&annotation.body);
        packet.push('\n');
    }
    packet
}

/// The `## {path}:{start}-{end} {symbol_name}` heading for one annotation
/// in [`render_agent_packet`]'s output (also reused by
/// [`render_additional_notes`]'s bullets) — extracted so the "which range,
/// which name" formatting logic is unit-testable independent of the
/// surrounding packet assembly. A [`AnnotationTarget::Dir`] location always
/// carries an empty `range`/`symbol_name` (ADR 0067), so it is special-cased
/// to a trailing `/` rather than falling through to the bare-path case a
/// `File`/`RemovedSymbol` location with no resolvable range would also hit —
/// otherwise a directory heading would be textually indistinguishable from
/// a same-path file heading. A [`AnnotationTarget::RemovedSymbol`] heading
/// gets a trailing `(removed)` (ADR 0067 Decision 3) so it stays
/// distinguishable from an unanchored present-`Symbol` heading, which
/// degrades to the same `{path} {symbol_name}` text otherwise.
fn annotation_heading(annotation: &Annotation) -> String {
    let location = &annotation.location;
    if matches!(location.target, AnnotationTarget::Dir) {
        return format!("{}/", location.path);
    }
    let range = location.anchor.or(location.range).map(|(start, end)| {
        if start == end {
            format!("{start}")
        } else {
            format!("{start}-{end}")
        }
    });
    let heading = match (range, &location.symbol_name) {
        (Some(range), Some(name)) => format!("{}:{range} {name}", location.path),
        (Some(range), None) => format!("{}:{range}", location.path),
        (None, Some(name)) => format!("{} {name}", location.path),
        (None, None) => location.path.clone(),
    };
    if matches!(location.target, AnnotationTarget::RemovedSymbol) {
        format!("{heading} (removed)")
    } else {
        heading
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
