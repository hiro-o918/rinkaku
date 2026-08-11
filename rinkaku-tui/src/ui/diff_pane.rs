//! Diff right-pane (TUI iteration 2, [`crate::app::RightPane::Diff`]; ADR
//! 0072 reshapes its content): the selected file's raw `git diff` hunks, in
//! original order, unchanged regardless of whether a file row or a symbol
//! row within it is selected — a symbol selection only changes where the
//! pane auto-scrolls to.

use super::scroll::{
    Body, render_scrollable_pane, truncate_line_to_width, truncate_to_width_keeping_tail,
};
use super::style::{expand_tabs_text, pane_border_style, styled_content_spans};
use crate::app::{App, DiffTarget, DiffViewMode, Focus};
use crate::diff_shape::AttributedHunk;
use crate::diff_view::{DiffLine, DiffLineKind};
use crate::highlight::{self, HighlightedFile, TokenSpan};
use crate::row_view::{BadgeContext, push_badge_spans};
use crate::tree::{Badges, NodeKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::render::Report;

/// The Diff pane's own area width below which split (side-by-side)
/// rendering falls back to unified regardless of [`DiffViewMode`] (ADR 0044
/// decision 7): 100 columns leaves roughly 49 usable columns per side after
/// the border and the 1-column gutter — narrower than that, a real code
/// line wraps onto several visual rows on each side and the two columns no
/// longer stay visually aligned, defeating split view's own purpose.
pub(crate) const MIN_SPLIT_VIEW_WIDTH: u16 = 100;

/// Background tint for an `Added`/`Removed` line (ADR 0018 decision: diff
/// signal moves to the background so a token's own color can carry the
/// foreground). 256-color indexed dark green/red rather than the named
/// `Color::Green`/`Color::Red` used for the `+`/`-` marker itself — a
/// named-color *background* at full saturation would fight with the
/// foreground token colors for attention, whereas these dim indexed tones
/// (in the xterm 256 palette's grayscale-adjacent dark green/red range)
/// stay legible as "this line changed" without competing with the text.
pub(crate) const ADDED_BG: Color = Color::Indexed(22);
pub(crate) const REMOVED_BG: Color = Color::Indexed(52);

/// The Diff pane's base title (before [`super::scroll::scroll_indicator`]'s
/// suffix is appended) — always the plain `" Diff "` every other pane's
/// title uses. Naming the current row now lives in
/// [`diff_pane_header_lines`] instead (a 2-line in-pane header, not the
/// title), since a symbol/file name plus its full path routinely overflows
/// the width the title bar had to work with.
pub(crate) const DIFF_PANE_TITLE: &str = " Diff ";

/// Builds the Diff pane's pinned in-pane header (above the scrollable hunk
/// content via `render_scrollable_pane`'s `header_lines` parameter, so the
/// row's identity and its badge summary stay visible no matter how far the
/// reviewer has scrolled into the hunks):
///
/// - Line 1 (bold): `"<symbol name> · <path>"` for a symbol row, or the
///   bare `path` for a file/skipped-file row (`selection_name` is `None`
///   there). Truncated from the *head* when it overflows `width` — the
///   symbol name or a path's basename is the informative tail.
/// - Line 2 (badges via [`push_badge_spans`]): the exact same badge set
///   the left tree row renders for this row — reused verbatim so the two
///   views can't drift, and the diff pane inherits every future badge
///   change for free. Rendered under [`BadgeContext::File`]: a `Dir` row
///   never reaches this header (no diff to show), and a symbol row's
///   badges ([`crate::tree::symbol_badges`]) contribute the same fields
///   `BadgeContext::File` reads. Omitted when the row's badges are all
///   zero.
/// - Line 3 (dim, only when `ranges` is non-empty): `"range: 23-73, ..."`
///   — the distinct new-side line spans. `ranges` must arrive already
///   sorted+deduped ([`crate::diff_shape::changed_line_ranges`]). On
///   overflow the range list itself is head-truncated (the *later* line
///   numbers are usually what the reviewer scrolled to see); the
///   `"range: "` label stays fixed so the line's meaning survives.
pub(crate) fn diff_pane_header_lines(
    selection_name: Option<&str>,
    path: &str,
    badges: &Badges,
    ranges: &[(usize, usize)],
    width: usize,
) -> Vec<Line<'static>> {
    let identification = match selection_name {
        Some(name) => format!("{name} · {path}"),
        None => path.to_string(),
    };
    let mut lines = vec![Line::styled(
        truncate_to_width_keeping_tail(&identification, width),
        Style::default().add_modifier(Modifier::BOLD),
    )];

    let mut badge_spans: Vec<Span<'static>> = Vec::new();
    push_badge_spans(&mut badge_spans, badges, BadgeContext::File);
    if !badge_spans.is_empty() {
        lines.push(truncate_line_to_width(&Line::from(badge_spans), width));
    }

    if !ranges.is_empty() {
        let prefix = "range: ";
        // Drop the whole line rather than push a `range: ` prefix with
        // nothing after it — a truncated tail is meaningful, an empty tail
        // is not, and letting the prefix render alone would overflow
        // `width` (ratatui clips silently, so the overflow reads as
        // "range: " being the whole message).
        if width > prefix.chars().count() {
            let range_list = ranges
                .iter()
                .map(|(start, end)| {
                    if start == end {
                        start.to_string()
                    } else {
                        format!("{start}-{end}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let range_budget = width - prefix.chars().count();
            let truncated_range_list = truncate_to_width_keeping_tail(&range_list, range_budget);
            let range_line = Line::from(vec![
                Span::raw(prefix),
                Span::styled(
                    truncated_range_list,
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]);
            lines.push(range_line);
        }
    }

    lines
}

/// Draws the diff pane (TUI iteration 2, [`crate::app::RightPane::Diff`];
/// ADR 0072 reshapes its content): the selected file's raw hunks, in
/// original order — `diff_content` is already shaped by
/// `crate::diff_shape::build_diff_pane_content`, computed once per handled
/// key by `crate::run_app` (this function must not call it itself, mirroring
/// `App::selected_blast_radius_view`'s own "must not call from `ui::draw`"
/// constraint and the reason it exists — see that method's doc comment).
/// A directory row, or a row with nothing to show (no hunks found, e.g. a
/// mismatch between `report` and the diff), falls back to a placeholder
/// message rather than an empty pane; `App::selected_diff_target` is called
/// here (not cached) purely to pick which of the two placeholder messages
/// applies — it is an O(rows) lookup, not the O(diff size) hunk-walk
/// `diff_content` itself avoids recomputing.
///
/// Returns the clamped scroll offset actually applied, or `None` when the
/// placeholder path was taken — mirrors `draw_detail_pane`'s own return
/// value for the identical reason (`render_scrollable_pane`'s doc comment).
pub(crate) fn draw_diff_pane(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_content: &crate::diff_shape::DiffPaneContent,
    diff_highlights: &[HighlightedFile],
    annotation_markers: &crate::annotation_markers::AnnotationMarkers,
    area: Rect,
) -> Option<usize> {
    use crate::diff_shape::DiffPaneContent;

    let focused = app.focus() == Focus::Right;
    let target = app.selected_diff_target(report);
    let path: &str = match &target {
        Some(DiffTarget::File { path }) => path.as_str(),
        None => "",
    };

    let hunks: &[AttributedHunk] = match diff_content {
        DiffPaneContent::Empty => {
            let message = match &target {
                None => "(select a symbol or file row to see its diff)".to_string(),
                Some(_) => format!("(no diff hunks found for {path})"),
            };
            let block = Block::bordered()
                .title(DIFF_PANE_TITLE)
                .border_style(pane_border_style(focused));
            let paragraph = Paragraph::new(message)
                .block(block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
            return None;
        }
        DiffPaneContent::File(hunks) => hunks,
    };

    let highlighted_file = highlight::highlighted_file(diff_highlights, path);

    // ADR 0044 decision 7: split view falls back to unified below
    // `MIN_SPLIT_VIEW_WIDTH`, regardless of `diff_view_mode` — a pane this
    // narrow cannot show two aligned columns without each one wrapping to a
    // different visual-row count, defeating the alignment split view exists
    // for. The toggle itself is untouched (`app.diff_view_mode()` keeps its
    // real value), so widening the terminal immediately shows split without
    // needing `v` pressed again.
    let split_requested = app.diff_view_mode() == DiffViewMode::Split;
    let split_fits = area.width >= MIN_SPLIT_VIEW_WIDTH;
    let render_split = split_requested && split_fits;

    let unified_lines = if render_split {
        Vec::new()
    } else {
        diff_pane_lines(hunks, highlighted_file, annotation_markers, path)
    };
    let split_rows = if render_split {
        diff_pane_split_rows(hunks, highlighted_file, annotation_markers, path)
    } else {
        (Vec::new(), Vec::new())
    };

    let ranges = crate::diff_shape::changed_line_ranges(
        &hunks
            .iter()
            .map(|attributed| &attributed.hunk)
            .collect::<Vec<_>>(),
    );
    let header_width = area.width.saturating_sub(2) as usize;

    // `selected_diff_header_name` returns the path itself on a file row
    // (its own doc comment), so it cannot be passed to
    // `diff_pane_header_lines` unconditionally — pairing it with `path`
    // there would print the path twice.
    let header_name = app.selected_diff_header_name();
    let selection_name = if selected_row_is_symbol(app) {
        header_name
    } else {
        None
    };
    let selected_badges = selected_row_badges(app);
    let mut header_lines = diff_pane_header_lines(
        selection_name,
        path,
        &selected_badges,
        &ranges,
        header_width,
    );
    // ADR 0044 decision 7: the toggle stays flipped even when the pane is
    // too narrow to honor it — this note is the only visible sign why `v`
    // didn't change anything, rather than a silent no-op.
    if split_requested && !split_fits {
        header_lines.push(Line::styled(
            "(split view needs a wider pane)",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    let body = if render_split {
        Body::Split(&split_rows.0, &split_rows.1)
    } else {
        Body::Single(&unified_lines)
    };

    Some(render_scrollable_pane(
        frame,
        DIFF_PANE_TITLE,
        &header_lines,
        body,
        app.right_pane_scroll(),
        area,
        focused,
    ))
}

/// Whether the row currently under the cursor is a present (non-removed)
/// symbol row — the same row-kind branch [`App::selected_diff_header_name`]
/// uses to decide whether its returned name is a symbol name (pair with
/// `path`) or a file path (already the whole identification, render bare).
fn selected_row_is_symbol(app: &App) -> bool {
    let rows = app.nav().rows(app.tree());
    let Some(row) = rows.get(app.nav().cursor()) else {
        return false;
    };
    matches!(&row.node.kind, NodeKind::Symbol(symbol_ref) if !symbol_ref.removed)
}

/// The `Badges` on the row currently under the cursor — read from the
/// same `nav.rows(tree)` lookup every other selection accessor already
/// uses, so the diff pane's line 2 renders exactly what the tree row
/// renders (single source of truth for what a row's badges say).
///
/// Returns [`Badges::default`] (all-zero) when there is no row, or when
/// the cursor is on a `Dir`/`Section`/`TestGroup` — those never reach the
/// header path at all in practice (empty pane placeholder instead), but
/// returning an empty badge set here is the honest fallback.
fn selected_row_badges(app: &App) -> Badges {
    let rows = app.nav().rows(app.tree());
    let Some(row) = rows.get(app.nav().cursor()) else {
        return Badges::default();
    };
    match &row.node.kind {
        NodeKind::File | NodeKind::Symbol(_) => row.node.badges,
        NodeKind::Dir | NodeKind::Section(_) | NodeKind::TestGroup { .. } => Badges::default(),
    }
}

/// Formats `hunks` into styled lines (ADR 0072: original `git diff` order,
/// no per-symbol section headers): hunk headers stay dim, `+`/`-` marker
/// glyphs keep their existing bold green/red foreground, and each line's
/// own code tokens are colored by [`highlight::lookup_hunk_highlight_by_index`]
/// when available (ADR 0018) — falling back to [`plain_diff_line`]
/// (green/red foreground plus the same `ADDED_BG`/`REMOVED_BG` tint,
/// unstyled for context) when a hunk has no highlight (unknown extension,
/// parse/query failure, or `highlighted_file` itself being `None`) so
/// highlighting can never make a diff harder to read than before.
pub(crate) fn diff_pane_lines(
    hunks: &[AttributedHunk],
    highlighted_file: Option<&HighlightedFile>,
    annotation_markers: &crate::annotation_markers::AnnotationMarkers,
    path: &str,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (hunk_index, attributed) in hunks.iter().enumerate() {
        if hunk_index > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            attributed.hunk.header.clone(),
            Style::default().fg(Color::DarkGray),
        ));

        let hunk_highlight =
            highlight::lookup_hunk_highlight_by_index(highlighted_file, attributed.source_index);
        let new_side_lines = new_side_line_numbers(&attributed.hunk);

        for (line_index, line) in attributed.hunk.lines.iter().enumerate() {
            // `hunk_highlight` is `Option<&[LineHighlight]>`, and
            // `LineHighlight` is itself `Option<Vec<TokenSpan>>`
            // (per-line fallback within an otherwise-highlighted hunk) —
            // `flatten` collapses "no highlight data at all for this
            // hunk" and "this specific line had no highlight" into the
            // same `None` `diff_line` already treats as its fallback
            // signal.
            let token_spans = hunk_highlight
                .and_then(|lines| lines.get(line_index).cloned())
                .flatten();
            let has_annotation = new_side_lines[line_index].is_some_and(|line_no| {
                crate::annotation_markers::line_has_annotation(annotation_markers, path, line_no)
            });
            lines.push(prefix_annotation_marker(
                diff_line(line, token_spans),
                has_annotation,
            ));
        }
    }
    lines
}

/// This hunk's own new-side line number for each of `hunk.lines`, `None`
/// for a pure-`Removed` line (which has no new-side position of its own —
/// [`crate::diff_view::hunk_intersects`]'s own doc comment on the same
/// "a removed line is a position, not a range" distinction). Starts
/// counting from `hunk.new_range`'s own start (already the hunk body's
/// *actual* new-side extent, not the header's possibly-inaccurate claim —
/// `Hunk::new_range`'s own doc comment), incrementing once per `Added`/
/// `Context` line, mirroring how a unified diff's new-side numbering works.
///
/// `None` for every line when `hunk.new_range` itself is `None` (an
/// unreadable header, or new-side start `0` — [`crate::diff_view::Hunk::new_range`]'s
/// doc comment) — there is no starting point to count from.
fn new_side_line_numbers(hunk: &crate::diff_view::Hunk) -> Vec<Option<usize>> {
    let Some((start, _)) = hunk.new_range else {
        return vec![None; hunk.lines.len()];
    };
    let mut next_line = start;
    hunk.lines
        .iter()
        .map(|line| match line.kind {
            DiffLineKind::Removed => None,
            DiffLineKind::Added | DiffLineKind::Context => {
                let current = next_line;
                next_line += 1;
                Some(current)
            }
        })
        .collect()
}

/// Prepends a 1-character annotation-marker column (ADR 0048) to `line`: a
/// cyan `*` when `has_annotation`, a space otherwise — every diff-pane row
/// gets this column regardless, so the marker's presence/absence never
/// shifts the rest of the line's own columns out of alignment with its
/// neighbors.
fn prefix_annotation_marker(line: Line<'static>, has_annotation: bool) -> Line<'static> {
    let marker = if has_annotation {
        Span::styled("*", Style::default().fg(Color::Cyan))
    } else {
        Span::raw(" ")
    };
    let mut spans = vec![marker];
    spans.extend(line.spans);
    // `Line::from(spans)` alone would drop `line.style` — the base style
    // `Line::styled` (e.g. `plain_diff_line`'s single-span Added/Removed
    // lines) applies at the *line* level, patched onto each span at render
    // time (`Line::styled_graphemes`), not copied onto the spans
    // themselves — so this rebuilt `Line` must carry the same base style
    // forward, or every span here silently loses its foreground/background.
    Line::from(spans).style(line.style)
}

/// Split-view (ADR 0044) counterpart of [`diff_pane_lines`]: the same
/// hunk-header scaffold, but each hunk's body is paired via
/// [`crate::diff_shape::pair_hunk_lines`] into old-side/new-side columns
/// instead of one interleaved column. Returns `(left, right)`, each the
/// same length — [`crate::diff_shape::SplitRow`]'s own invariant (one row
/// per source [`DiffLine`]) means every hunk contributes the same row
/// count here as it does to [`diff_pane_lines`].
pub(crate) fn diff_pane_split_rows(
    hunks: &[AttributedHunk],
    highlighted_file: Option<&HighlightedFile>,
    annotation_markers: &crate::annotation_markers::AnnotationMarkers,
    path: &str,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (hunk_index, attributed) in hunks.iter().enumerate() {
        if hunk_index > 0 {
            left.push(Line::raw(""));
            right.push(Line::raw(""));
        }
        let header = Line::styled(
            attributed.hunk.header.clone(),
            Style::default().fg(Color::DarkGray),
        );
        left.push(header.clone());
        right.push(header);

        let hunk_highlight =
            highlight::lookup_hunk_highlight_by_index(highlighted_file, attributed.source_index);
        let split_rows = crate::diff_shape::pair_hunk_lines(&attributed.hunk.lines);
        let new_side_lines = new_side_line_numbers(&attributed.hunk);

        for split_row in &split_rows {
            left.push(split_side_line(
                split_row.left.as_ref(),
                split_row.left_index,
                hunk_highlight,
                None,
            ));
            let right_has_annotation = split_row
                .right_index
                .and_then(|index| new_side_lines.get(index).copied().flatten())
                .is_some_and(|line_no| {
                    crate::annotation_markers::line_has_annotation(
                        annotation_markers,
                        path,
                        line_no,
                    )
                });
            right.push(split_side_line(
                split_row.right.as_ref(),
                split_row.right_index,
                hunk_highlight,
                Some(right_has_annotation),
            ));
        }
    }
    (left, right)
}

/// One [`SplitRow`](crate::diff_shape::SplitRow) side's rendered [`Line`] —
/// a blank filler line for a `None` cell, or `diff_line`'s usual rendering
/// looked up by `index` (that side's position in the hunk's original
/// interleaved `lines`, [`crate::diff_shape::SplitRow::left_index`]/
/// `right_index`'s own doc comment).
///
/// `has_annotation` (ADR 0048) is `Some(bool)` on the new-side (right) call, and
/// `None` on the old-side (left) call — split view only marks the new
/// side, matching [`crate::review::AnnotationLocation`]'s own new-side-only
/// anchoring; a `Some` value prefixes the 1-column marker, `None` prefixes
/// nothing at all (the old side keeps its pre-ADR-0048 column layout
/// unchanged).
fn split_side_line(
    line: Option<&DiffLine>,
    index: Option<usize>,
    hunk_highlight: Option<&[Option<Vec<TokenSpan>>]>,
    has_annotation: Option<bool>,
) -> Line<'static> {
    let rendered = match (line, index) {
        (Some(line), Some(index)) => {
            let token_spans = hunk_highlight
                .and_then(|lines| lines.get(index).cloned())
                .flatten();
            diff_line(line, token_spans)
        }
        _ => Line::raw(""),
    };
    match has_annotation {
        Some(has_annotation) => prefix_annotation_marker(rendered, has_annotation),
        None => rendered,
    }
}

/// Builds one display line for a hunk body line, coloring its code tokens
/// per `token_spans` (`None` when highlighting is unavailable for this
/// line — falls back to [`plain_diff_line`], which now also carries the
/// `ADDED_BG`/`REMOVED_BG` tint so a highlighted and an unhighlighted hunk
/// read as the same "this line changed" signal). The `+`/`-` marker glyph
/// itself is always pushed as its own bold-colored span, kept outside of
/// `line.content`'s token coloring so it is never masked by a token span
/// that happens to start at byte 0.
pub(crate) fn diff_line(line: &DiffLine, token_spans: Option<Vec<TokenSpan>>) -> Line<'static> {
    match &token_spans {
        Some(spans) => {
            let bg = match line.kind {
                DiffLineKind::Added => Some(ADDED_BG),
                DiffLineKind::Removed => Some(REMOVED_BG),
                DiffLineKind::Context => None,
            };
            let mut result_spans = vec![marker_span(line.kind, bg)];
            result_spans.extend(styled_content_spans(&line.content, spans, bg));
            Line::from(result_spans)
        }
        None => plain_diff_line(line),
    }
}

/// The fallback line style for a line highlighting could not cover
/// (unknown extension, parse/query failure, or no highlighted file at
/// all): the same `+`/`-`/green/red foreground as the highlighted path,
/// plus the same `ADDED_BG`/`REMOVED_BG` tint — a context line stays
/// unstyled since it carries no diff signal either way.
pub(crate) fn plain_diff_line(line: &DiffLine) -> Line<'static> {
    let content = expand_tabs_text(&line.content);
    match line.kind {
        DiffLineKind::Added => Line::styled(
            format!("+{content}"),
            added_removed_style(DiffLineKind::Added),
        ),
        DiffLineKind::Removed => Line::styled(
            format!("-{content}"),
            added_removed_style(DiffLineKind::Removed),
        ),
        DiffLineKind::Context => Line::raw(format!(" {content}")),
    }
}

/// The fg/bg pair for an `Added`/`Removed` line (ADR 0018: diff signal lives
/// in the background, not just the foreground), used by [`plain_diff_line`].
///
/// Panics on `DiffLineKind::Context`: no caller here ever has a
/// context-kind line to style.
fn added_removed_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Added => Style::default().fg(Color::Green).bg(ADDED_BG),
        DiffLineKind::Removed => Style::default().fg(Color::Red).bg(REMOVED_BG),
        DiffLineKind::Context => {
            unreachable!("plain diff lines are never Context-kind")
        }
    }
}

/// The leading `+`/`-`/` ` marker glyph as its own span: bold green/red for
/// `+`/`-` (unchanged from the pane's original style) with the line's
/// background tint applied so the marker doesn't visually break from the
/// rest of a tinted line; a plain space (no bg) for a context line.
pub(crate) fn marker_span(kind: DiffLineKind, bg: Option<Color>) -> Span<'static> {
    let (glyph, fg) = match kind {
        DiffLineKind::Added => ("+", Some(Color::Green)),
        DiffLineKind::Removed => ("-", Some(Color::Red)),
        DiffLineKind::Context => (" ", None),
    };
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    if let Some(fg) = fg {
        style = style.fg(fg);
    }
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    Span::styled(glyph, style)
}

#[cfg(test)]
#[path = "diff_pane_tests/mod.rs"]
mod tests;
