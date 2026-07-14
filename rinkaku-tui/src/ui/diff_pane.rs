//! Diff right-pane (TUI iteration 2, [`crate::app::RightPane::Diff`]; ADR
//! 0020 reshapes its content): the raw unified-diff hunks touching the row
//! under the cursor, either clipped to a symbol row's own line range or
//! grouped into per-symbol sections for a file row.

use super::scroll::{
    Body, render_scrollable_pane, truncate_line_to_width, truncate_to_width_keeping_tail,
};
use super::style::{pane_border_style, styled_content_spans};
use crate::app::{App, DiffTarget, DiffViewMode, Focus};
use crate::diff_shape::DiffSection;
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
///   sorted+deduped ([`crate::diff_shape::changed_line_ranges`]) so a
///   file selection whose hunks ADR 0029 clones across multiple owning
///   symbols still produces one entry per distinct span, not one per
///   section that owns it. On overflow the range list itself is
///   head-truncated (the *later* line numbers are usually what the
///   reviewer scrolled to see); the `"range: "` label stays fixed so
///   the line's meaning survives.
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

/// Draws the diff pane (TUI iteration 2, [`crate::app::RightPane::Diff`]; ADR 0020
/// reshapes its content): the raw unified-diff hunks touching the row under
/// the cursor, clipped to a symbol's own line range for a symbol row, or
/// grouped into per-symbol sections (plus a trailing "(module level)"
/// section) for a file row — `diff_content` is already shaped by
/// `crate::diff_shape::build_diff_pane_content`, computed once per handled
/// key by `crate::run_app` (this function must not call it itself, mirroring
/// `App::selected_blast_radius_view`'s own "must not call from `ui::draw`"
/// constraint and the reason it exists — see that method's doc comment).
/// A directory row, or a row with nothing to show (no hunks found, e.g. a
/// mismatch between `report` and the diff), falls back to a placeholder
/// message rather than an empty pane; `App::selected_diff_target` is called
/// here (not cached) purely to pick which of the two placeholder messages
/// applies — it is an O(rows) lookup, not the O(diff size) hunk-walk
/// `diff_content` itself avoids recomputing. `diff_highlights` is looked up
/// by `source_index` rather than pointer identity now that hunks are cloned
/// into shaped sections (`crate::diff_shape::AttributedHunk`'s own doc
/// comment).
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
    area: Rect,
) -> Option<usize> {
    use crate::diff_shape::DiffPaneContent;

    let focused = app.focus() == Focus::Right;
    let target = app.selected_diff_target(report);
    let path: &str = match &target {
        Some(DiffTarget::File { path }) => path.as_str(),
        None => "",
    };

    let sections: Vec<&crate::diff_shape::DiffSection> = match diff_content {
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
        DiffPaneContent::File(sections) => sections.iter().collect(),
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

    // ADR 0027: `DiffPaneContent` no longer has a symbol-clip variant, so
    // the diff pane always renders with section headers on. `diff_pane_lines`'s/
    // `diff_pane_split_rows`'s `show_section_headers` parameter is now always
    // `true` at this call site, kept as a parameter to leave that layout knob
    // visible in one place rather than hard-coding it inside either function.
    let unified_lines = if render_split {
        Vec::new()
    } else {
        diff_pane_lines(&sections, true, highlighted_file)
    };
    let split_rows = if render_split {
        diff_pane_split_rows(&sections, true, highlighted_file)
    } else {
        (Vec::new(), Vec::new())
    };

    // A symbol row's ranges scope to that symbol's own section only; a
    // file row (no `selected_diff_focus`) scopes to every section — mirrors
    // `App::selected_diff_target`'s own file-vs-symbol row scoping.
    let focus = app.selected_diff_focus(report);
    let range_sections: Vec<&crate::diff_shape::DiffSection> = match &focus {
        Some(focus) => sections
            .iter()
            .filter(|section| section.symbol_id.as_deref() == Some(focus.symbol_id.as_str()))
            .copied()
            .collect(),
        None => sections.clone(),
    };
    let ranges = crate::diff_shape::changed_line_ranges(&range_sections);
    let header_width = area.width.saturating_sub(2) as usize;

    // `selected_diff_header_name` is the single source for what line 1
    // names: the symbol's own name on a symbol row (paired with `path`
    // below to form `"<name> · <path>"`), or the file row's path
    // (rendered bare, `selection_name = None`). Feeding both row kinds
    // through the accessor — rather than only the symbol arm — keeps its
    // file-row branch on the rendered path, not dead. Row `badges` come
    // straight off the same `nav.rows(tree)` entry every other lookup
    // already reads, so line 2 renders exactly what the tree row does
    // (no drift).
    let header_name = app.selected_diff_header_name();
    let (selection_name, header_path) = if focus.is_some() {
        (header_name, path)
    } else {
        (None, header_name.unwrap_or(path))
    };
    let selected_badges = selected_row_badges(app);
    let mut header_lines = diff_pane_header_lines(
        selection_name,
        header_path,
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

/// Formats every [`DiffSection`] in `sections` into styled lines (ADR
/// 0020): a section header (a symbol's own signature, styled bold, or the
/// fixed "(module level)" label) only when `show_section_headers` is set —
/// a single-section symbol selection has nothing to disambiguate a header
/// would add value to, so it is omitted there and the pane opens straight
/// on the (optional) contract header/hunks, matching this pane's pre-ADR-
/// 0020 layout for a symbol row. A file selection (multiple sections, or
/// one section that still benefits from being named) always shows headers.
/// Each section's own `contract_header` (when present) renders as a 2-line
/// red/green old/new pair before that section's hunks — the outline-before-
/// implementation disclosure order ADR 0020 asks for.
///
/// Within each section, hunk headers stay dim, `+`/`-` marker glyphs keep
/// their existing bold green/red foreground, and each line's own code
/// tokens are colored by [`highlight::lookup_hunk_highlight_by_index`] when
/// available (ADR 0018/0020) — falling back to the plain green/red/
/// unstyled line style this pane always had when a hunk has no highlight
/// (unknown extension, parse/query failure, or `highlighted_file` itself
/// being `None`) so highlighting can never make a diff harder to read than
/// before.
pub(crate) fn diff_pane_lines(
    sections: &[&DiffSection],
    show_section_headers: bool,
    highlighted_file: Option<&HighlightedFile>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            lines.push(Line::raw(""));
        }
        if show_section_headers {
            lines.push(Line::styled(
                section.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(contract) = &section.contract_header {
            lines.push(Line::styled(
                format!("- {}", contract.previous_signature),
                Style::default().fg(Color::Red),
            ));
            lines.push(Line::styled(
                format!("+ {}", contract.signature),
                Style::default().fg(Color::Green),
            ));
        }

        for (hunk_index, attributed) in section.hunks.iter().enumerate() {
            if hunk_index > 0 || show_section_headers || section.contract_header.is_some() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(
                attributed.hunk.header.clone(),
                Style::default().fg(Color::DarkGray),
            ));

            let hunk_highlight = highlight::lookup_hunk_highlight_by_index(
                highlighted_file,
                attributed.source_index,
            );

            for (line_index, line) in attributed.hunk.lines.iter().enumerate() {
                // `hunk_highlight` is `Option<&[LineHighlight]>`, and
                // `LineHighlight` is itself `Option<Vec<TokenSpan>>`
                // (per-line fallback within an otherwise-highlighted hunk)
                // — `flatten` collapses "no highlight data at all for this
                // hunk" and "this specific line had no highlight" into the
                // same `None` `diff_line` already treats as its fallback
                // signal.
                let token_spans = hunk_highlight
                    .and_then(|lines| lines.get(line_index).cloned())
                    .flatten();
                lines.push(diff_line(line, token_spans));
            }
        }
    }
    lines
}

/// Split-view (ADR 0044) counterpart of [`diff_pane_lines`]: the same
/// section/contract-header/hunk-header scaffold (a scaffold line renders
/// identically on both sides, so `left`/`right` share it), but each hunk's
/// body is paired via [`crate::diff_shape::pair_hunk_lines`] into old-side/
/// new-side columns instead of one interleaved column. Returns `(left,
/// right)`, each the same length — [`crate::diff_shape::SplitRow`]'s own
/// invariant (one row per source [`DiffLine`]) means every hunk contributes
/// the same row count here as it does to [`diff_pane_lines`], so this
/// function's total line count always matches `diff_pane_lines`'s for the
/// same `sections`/`show_section_headers` — required for `walk_sections`'
/// shared line-counting (ADR 0044 decision 4) to stay correct regardless of
/// which of the two this pane actually renders.
pub(crate) fn diff_pane_split_rows(
    sections: &[&DiffSection],
    show_section_headers: bool,
    highlighted_file: Option<&HighlightedFile>,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            left.push(Line::raw(""));
            right.push(Line::raw(""));
        }
        if show_section_headers {
            let title = Line::styled(
                section.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            );
            left.push(title.clone());
            right.push(title);
        }
        if let Some(contract) = &section.contract_header {
            left.push(Line::styled(
                format!("- {}", contract.previous_signature),
                Style::default().fg(Color::Red),
            ));
            right.push(Line::raw(""));
            left.push(Line::raw(""));
            right.push(Line::styled(
                format!("+ {}", contract.signature),
                Style::default().fg(Color::Green),
            ));
        }

        for (hunk_index, attributed) in section.hunks.iter().enumerate() {
            if hunk_index > 0 || show_section_headers || section.contract_header.is_some() {
                left.push(Line::raw(""));
                right.push(Line::raw(""));
            }
            let header = Line::styled(
                attributed.hunk.header.clone(),
                Style::default().fg(Color::DarkGray),
            );
            left.push(header.clone());
            right.push(header);

            let hunk_highlight = highlight::lookup_hunk_highlight_by_index(
                highlighted_file,
                attributed.source_index,
            );
            let split_rows = crate::diff_shape::pair_hunk_lines(&attributed.hunk.lines);

            for split_row in &split_rows {
                left.push(split_side_line(
                    split_row.left.as_ref(),
                    split_row.left_index,
                    hunk_highlight,
                ));
                right.push(split_side_line(
                    split_row.right.as_ref(),
                    split_row.right_index,
                    hunk_highlight,
                ));
            }
        }
    }
    (left, right)
}

/// One [`SplitRow`](crate::diff_shape::SplitRow) side's rendered [`Line`] —
/// a blank filler line for a `None` cell, or `diff_line`'s usual rendering
/// looked up by `index` (that side's position in the hunk's *original*
/// interleaved `lines`, [`crate::diff_shape::SplitRow::left_index`]/
/// `right_index`'s own doc comment on why this must be the original index,
/// not the split row's own position).
fn split_side_line(
    line: Option<&DiffLine>,
    index: Option<usize>,
    hunk_highlight: Option<&[Option<Vec<TokenSpan>>]>,
) -> Line<'static> {
    match (line, index) {
        (Some(line), Some(index)) => {
            let token_spans = hunk_highlight
                .and_then(|lines| lines.get(index).cloned())
                .flatten();
            diff_line(line, token_spans)
        }
        _ => Line::raw(""),
    }
}

/// Builds one display line for a hunk body line, coloring its code tokens
/// per `token_spans` (`None` when highlighting is unavailable for this
/// line — falls back to the pane's original plain style). The `+`/`-`
/// marker glyph itself is always pushed as its own bold-colored span, kept
/// outside of `line.content`'s token coloring so it is never masked by a
/// token span that happens to start at byte 0.
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

/// The pane's original (pre-ADR-0018) plain green/red/unstyled line style —
/// the fallback for a line highlighting could not cover.
pub(crate) fn plain_diff_line(line: &DiffLine) -> Line<'static> {
    match line.kind {
        DiffLineKind::Added => Line::styled(
            format!("+{}", line.content),
            Style::default().fg(Color::Green),
        ),
        DiffLineKind::Removed => Line::styled(
            format!("-{}", line.content),
            Style::default().fg(Color::Red),
        ),
        DiffLineKind::Context => Line::raw(format!(" {}", line.content)),
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
