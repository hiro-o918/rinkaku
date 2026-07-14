//! Source drill-down screen (ADR 0026 for the reviewer-driven scroll,
//! ADR 0018/0020 for the shared "token foreground + line-level background
//! tint" composition with the diff pane, ADR 0046 for the diff overlay
//! composited on top of that, ADR 0049 for the split (side-by-side)
//! rendering of that overlay).

use super::diff_pane::{ADDED_BG, MIN_SPLIT_VIEW_WIDTH, REMOVED_BG, marker_span};
use super::scroll::{Body, render_scrollable_pane};
use super::style::{gap_span, pane_border_style, styled_content_spans};
use crate::app::DiffViewMode;
use crate::diff_view::{DiffLineKind, FileHunks};
use crate::source::{HighlightedSourceView, SourceView};
use crate::source_diff::{OverlayRow, overlay_source_lines, rows_in_source_range};
use crate::source_split::{SourceSplitRow, SourceSplitRowKind, split_source_rows};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

#[cfg(test)]
#[path = "source_screen_tests/mod.rs"]
mod tests;

/// Background tint for a source-screen line inside the drilled-into symbol's
/// own range — reused as the uniform `bg` for [`styled_content_spans`] the
/// same way the diff pane's `ADDED_BG`/`REMOVED_BG` are (this module's own
/// precedent, ADR 0018), so a token's foreground color from
/// [`super::style::palette_style`] is never lost underneath the symbol-range highlight;
/// the two compose instead of one replacing the other.
pub(crate) const SOURCE_HIGHLIGHT_BG: Color = Color::DarkGray;

/// Draws the source drill-down for `symbol_id`, given `source_content` —
/// `crate::run_app`'s once-per-`s`-press [`Result`] from
/// [`crate::source::load_highlighted_symbol_source`] (a file read plus a
/// full tree-sitter parse). This function itself performs no IO and no
/// highlighting: unlike an earlier version of this screen, which re-read the
/// file from disk on every frame (cheap for a plain read, but a
/// highlighting pass added on top of that would re-parse on every ~100ms
/// idle poll tick too — the exact per-frame-recompute bug ADR 0018 already
/// had to fix once for the diff pane, `crate::run_app`'s own doc comment on
/// `diff_highlights`), this screen now only re-renders the already-computed
/// `source_content`.
///
/// `scroll_top` (ADR 0026) is the reviewer's requested 0-based first-visible
/// line — an unclamped value stored in [`crate::app::Screen::Source::scroll_top`] that
/// this function clamps against the file's actual line count and the pane's
/// rendered height at draw time (`scroll_top.min(max_start)`). The initial
/// value, set by `crate::run_app` when the `s` key opens this screen, is
/// [`crate::source::visible_window`]'s centered start so the first frame
/// still shows the symbol's definition centered in the viewport; subsequent
/// motion keys move `scroll_top` away from that starting position.
/// [`crate::app::InputKey::ScrollToBottom`]'s `usize::MAX` sentinel folds cleanly through
/// this same clamp — no per-variant special case needed here.
///
/// `source_content` is `None` only when `crate::run_app` has not yet reached
/// the point of computing it (defensive — `draw`'s own `Screen::Source` arm
/// is the only caller, and it always has a symbol id in hand by then); drawn
/// as a bare bordered box with no body in that case, rather than panicking.
///
/// `diff_hunks` is `crate::run_app`'s once-per-session
/// `diff_view::parse_diff_hunks` output (ADR 0046): when the drilled-into
/// symbol's file has an entry there, its hunks are composited onto the
/// source view as an always-on added/removed overlay
/// (`crate::source_diff::overlay_source_lines`), unless the file has
/// drifted from the diff on disk, in which case the pane falls back to its
/// plain rendering with a one-line note in the title (ADR 0046 decision 5).
///
/// `diff_view_mode` (ADR 0049) is the same `App::diff_view_mode` the diff
/// pane reads: `Split` renders the overlay side-by-side (old left, new
/// right) via [`split_source_rows`], falling back to this function's
/// unified rendering — with its own title note — whenever `area`'s width
/// is below [`MIN_SPLIT_VIEW_WIDTH`] (the diff pane's own narrow-terminal
/// threshold, ADR 0044 decision 7, reused rather than a second constant)
/// or `split_source_rows` itself returns `None` (the same drift that
/// disables the unified overlay disables reconstructing an old side to
/// split against, ADR 0049 decision 6).
pub(crate) fn draw_source_screen(
    frame: &mut Frame,
    symbol_id: &str,
    scroll_top: usize,
    source_content: Option<&Result<HighlightedSourceView, String>>,
    diff_hunks: &[FileHunks],
    diff_view_mode: DiffViewMode,
    area: Rect,
) {
    let highlighted = match source_content {
        Some(Ok(highlighted)) => highlighted,
        Some(Err(message)) => {
            let block = Block::bordered()
                .title(format!(" Source: {symbol_id} "))
                .border_style(pane_border_style(true));
            // `.wrap(Wrap { trim: false })`: the error message (full path +
            // io error + the "not present in the working tree" hint added
            // alongside repo-root resolution) routinely exceeds one line at
            // ordinary pane widths. Without wrapping, `Paragraph` silently
            // truncates instead of overflowing, cutting the hint off
            // exactly where it explains the failure. `trim: false` keeps
            // the message's own leading whitespace (there isn't any here,
            // but matches this pane's other `Paragraph` usages that don't
            // opt into trimming either).
            let paragraph = Paragraph::new(message.as_str())
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
            return;
        }
        None => {
            let block = Block::bordered()
                .title(format!(" Source: {symbol_id} "))
                .border_style(pane_border_style(true));
            frame.render_widget(Paragraph::new("").block(block), area);
            return;
        }
    };
    let source = &highlighted.view;

    let file_hunks = crate::diff_view::file_hunks(diff_hunks, &source.path);
    let overlay = file_hunks.and_then(|file_hunks| overlay_source_lines(&source.lines, file_hunks));

    let split_requested = diff_view_mode == DiffViewMode::Split;
    let split_fits = area.width >= MIN_SPLIT_VIEW_WIDTH;
    let split_rows = if split_requested && split_fits {
        file_hunks.and_then(|file_hunks| split_source_rows(&source.lines, file_hunks))
    } else {
        None
    };

    let mut title = if file_hunks.is_some() && overlay.is_none() {
        format!(
            " Source: {symbol_id} (diff overlay unavailable — file on disk doesn't match the diff) "
        )
    } else {
        format!(" Source: {symbol_id} ")
    };
    if split_requested && !split_fits {
        title = format!("{} (split view needs a wider pane) ", title.trim_end());
    }
    // Always drawn as focused: this screen replaces the whole entry view
    // (tree + right pane) while open, so there is no sibling pane it needs
    // to be visually distinguished from (`render_scrollable_pane`'s own doc
    // comment makes the same call for the `?` help overlay).

    match split_rows {
        Some(split_rows) => {
            let (left, right) = source_split_lines(&highlighted.token_highlights, &split_rows);
            render_scrollable_pane(
                frame,
                &title,
                &[],
                Body::Split(&left, &right),
                scroll_top,
                area,
                true,
            );
        }
        None => {
            let block = Block::bordered()
                .title(title)
                .border_style(pane_border_style(true));

            let viewport_height = area.height.saturating_sub(2) as usize; // borders
            let (start, end) = clamped_window(source.lines.len(), scroll_top, viewport_height);

            let lines = source_lines(
                source,
                &highlighted.token_highlights,
                overlay.as_deref(),
                start,
                end,
            );
            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}

/// Clamps a reviewer-requested `scroll_top` (ADR 0026) against a file of
/// `total_lines` shown in a viewport `viewport_height` rows tall, returning
/// the 0-based half-open `[start, end)` slice to render. Handles the two
/// edge cases the source screen actually hits:
///
/// - `total_lines < viewport_height` — the whole file fits, so `start = 0`
///   regardless of the requested `scroll_top`.
/// - `scroll_top` past the end of the file (either a real overshoot from
///   repeated `j`, or [`crate::app::InputKey::ScrollToBottom`]'s `usize::MAX`
///   sentinel itself) — folded down to `total_lines - viewport_height` so
///   the last line still lands at the bottom of the pane rather than
///   scrolling off it.
pub(crate) fn clamped_window(
    total_lines: usize,
    scroll_top: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if total_lines == 0 || viewport_height == 0 {
        return (0, 0);
    }
    let max_start = total_lines.saturating_sub(viewport_height);
    let start = scroll_top.min(max_start);
    let end = (start + viewport_height).min(total_lines);
    (start, end)
}

/// Renders `source.lines[start..end]` (1-based line range `[start+1, end]`)
/// with a `{line_number:>5} | ` gutter, each line's code tokens colored per
/// `token_highlights[i]` (`None` falls back to the plain unstyled line this
/// screen always had, same contract as [`super::diff_pane::diff_line`]'s own
/// fallback) and the symbol's own highlighted range
/// (`source.highlight_start..=source.highlight_end`) composited on top as a
/// background tint — mirrors the diff pane's own "token foreground + line-
/// level background tint" composition rather than inventing a second scheme
/// for this screen.
///
/// `overlay` is `crate::source_diff::overlay_source_lines`'s result for this
/// file (ADR 0046), `None` when the file has no diff or the overlay was
/// dropped for drift (`draw_source_screen`'s own doc comment). When present,
/// `crate::source_diff::rows_in_source_range` slices it to the same
/// `[start, end)` window and each row drives its own line: an `Added` row's
/// `ADDED_BG` background wins over the symbol-range tint (ADR 0046 decision
/// 6 — the more specific signal for a reviewer who followed the symbol into
/// its file specifically to see what changed); a `Removed` row inserts an
/// extra `-`-gutter line with `REMOVED_BG`, no line number (nothing in
/// `source.lines` for it to point at) and no token highlighting (the text
/// comes from the diff, not a parsed source line).
pub(crate) fn source_lines(
    source: &SourceView,
    token_highlights: &[crate::highlight::LineHighlight],
    overlay: Option<&[OverlayRow]>,
    start: usize,
    end: usize,
) -> Vec<Line<'static>> {
    let Some(overlay) = overlay else {
        return source.lines[start..end]
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let line_index = start + offset;
                unchanged_line(source, token_highlights, line_index, text, None)
            })
            .collect();
    };

    rows_in_source_range(overlay, start + 1, end)
        .iter()
        .map(|row| match row {
            OverlayRow::Unchanged {
                line_number,
                content,
            } => unchanged_line(source, token_highlights, line_number - 1, content, None),
            OverlayRow::Added {
                line_number,
                content,
            } => unchanged_line(
                source,
                token_highlights,
                line_number - 1,
                content,
                Some(ADDED_BG),
            ),
            OverlayRow::Removed { content } => removed_line(content),
        })
        .collect()
}

/// Renders one source-file line (`text`, at 0-based `line_index`) with its
/// gutter, token highlighting, and background tint — the common rendering
/// step [`source_lines`] uses for both an unmodified line and a diff-added
/// line, which differ only in `diff_bg` (ADR 0046 decision 6: an added
/// line's tint wins over the symbol-range tint when both would apply,
/// achieved simply by `diff_bg` taking priority in the `or` below). A
/// `diff_bg` of `Some(ADDED_BG)` also swaps the gutter's usual blank space
/// for [`marker_span`]'s `+` glyph, pairing this line's gutter with
/// [`removed_line`]'s own `-` gutter for the same diff signal.
fn unchanged_line(
    source: &SourceView,
    token_highlights: &[crate::highlight::LineHighlight],
    line_index: usize,
    text: &str,
    diff_bg: Option<Color>,
) -> Line<'static> {
    let line_number = line_index + 1;
    let is_highlighted =
        line_number >= source.highlight_start && line_number <= source.highlight_end;
    let bg = diff_bg.or(is_highlighted.then_some(SOURCE_HIGHLIGHT_BG));
    let is_added = diff_bg == Some(ADDED_BG);

    let mut spans = vec![gap_span(&format!("{line_number:>5}"), bg)];
    spans.push(if is_added {
        marker_span(DiffLineKind::Added, bg)
    } else {
        gap_span(" ", bg)
    });
    spans.push(gap_span("| ", bg));
    match token_highlights.get(line_index).cloned().flatten() {
        Some(token_spans) => {
            spans.extend(styled_content_spans(text, &token_spans, bg));
        }
        None => spans.push(gap_span(text, bg)),
    }
    Line::from(spans)
}

/// Renders a diff-removed line with no source line number of its own
/// (`OverlayRow::Removed`'s own doc comment) — a `-` gutter matching the
/// width of an ordinary line's `{line_number:>5} | ` gutter,
/// [`marker_span`] for the same bold-red `-` glyph the diff pane uses,
/// [`REMOVED_BG`] applied uniformly, and no token highlighting (the text is
/// the diff's recorded old-side content, not a line
/// `crate::highlight::highlight_source_lines` ever parsed).
fn removed_line(content: &str) -> Line<'static> {
    let bg = Some(REMOVED_BG);
    Line::from(vec![
        gap_span("     ", bg),
        marker_span(DiffLineKind::Removed, bg),
        gap_span("| ", bg),
        gap_span(content, bg),
    ])
}

/// Builds the `(left, right)` column lines for the source view's split
/// (side-by-side) diff overlay (ADR 0049) from `split_rows`
/// (`crate::source_split::split_source_rows`'s output). Each
/// [`SourceSplitRow`] becomes one line on each side — a `None` cell renders
/// as a blank filler line, matching the diff pane's own split-view filler
/// convention (`crate::ui::diff_pane::split_side_line`).
fn source_split_lines(
    token_highlights: &[crate::highlight::LineHighlight],
    split_rows: &[SourceSplitRow],
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let mut left = Vec::with_capacity(split_rows.len());
    let mut right = Vec::with_capacity(split_rows.len());
    for row in split_rows {
        let (left_bg, right_bg) = match row.kind {
            SourceSplitRowKind::Unchanged => (None, None),
            SourceSplitRowKind::Changed => (Some(REMOVED_BG), Some(ADDED_BG)),
            SourceSplitRowKind::Filler => (None, None),
        };
        left.push(source_split_side_line(
            row.left.as_ref(),
            DiffLineKind::Removed,
            left_bg,
            None,
        ));
        right.push(source_split_side_line(
            row.right.as_ref(),
            DiffLineKind::Added,
            right_bg,
            (row.kind == SourceSplitRowKind::Unchanged).then_some(token_highlights),
        ));
    }
    (left, right)
}

/// One [`SourceSplitRow`] side's rendered [`Line`] — a blank line for a
/// `None` cell (a filler row, or a one-sided insertion/deletion), else a
/// `{line_number:>5} | ` gutter (matching [`unchanged_line`]'s own gutter
/// width) plus content. `token_highlights` is `Some` only for an
/// `Unchanged` row's new-side cell — the one case where the rendered text
/// is guaranteed identical to a line `crate::highlight::highlight_source_lines`
/// already parsed and its 0-based `line_number - 1` index into it is
/// valid; every other cell (a `Changed` row's old/new text, or an
/// `Unchanged` row's mirrored old-side cell, which has no highlight data
/// of its own — the unified overlay's [`removed_line`] makes the same call
/// for old-side-only text) renders as plain gap-styled text plus `bg`.
fn source_split_side_line(
    cell: Option<&crate::source_split::SourceSplitLine>,
    marker_kind: DiffLineKind,
    bg: Option<Color>,
    token_highlights: Option<&[crate::highlight::LineHighlight]>,
) -> Line<'static> {
    let Some(cell) = cell else {
        return Line::raw("");
    };

    let marker = bg.is_some();
    let mut spans = vec![gap_span(&format!("{:>5}", cell.line_number), bg)];
    spans.push(if marker {
        marker_span(marker_kind, bg)
    } else {
        gap_span(" ", bg)
    });
    spans.push(gap_span("| ", bg));
    match token_highlights.and_then(|token_highlights| {
        token_highlights
            .get(cell.line_number - 1)
            .cloned()
            .flatten()
    }) {
        Some(token_spans) => spans.extend(styled_content_spans(&cell.content, &token_spans, bg)),
        None => spans.push(gap_span(&cell.content, bg)),
    }
    Line::from(spans)
}
