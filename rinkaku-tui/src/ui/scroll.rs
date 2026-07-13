//! Pure scroll/wrap helpers shared by the panes in this module — extracted so
//! `clamp_scroll`, `scroll_indicator`, `visible_index_window`,
//! `window_overflow_indicators`, `windowed_rows_with_indicators`,
//! `wrap_lines`/`wrap_one_line`, and `truncate_to_width` stay unit-testable
//! without a live `ratatui::backend::TestBackend`, and so
//! [`render_scrollable_pane`] itself (the one non-pure helper here) has a
//! single home shared by every pane that scrolls.

use super::style::pane_border_style;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthChar;

/// Renders `lines` into a bordered pane titled `title`, scrolled to
/// `requested_scroll` lines down and clamped to what actually fits
/// (`clamp_scroll`'s own doc comment on why clamping only happens here,
/// not in `crate::app`). When the content overflows the pane's inner
/// height, the title grows a `(first-last/total)` suffix so the reviewer
/// knows more content exists below/above without needing to scroll blind
/// (this iteration's answer to "全部が見えているように見えて実は続きがある"
/// — the same concern `crate::source`'s highlighted-window view sidesteps
/// by auto-centering instead of paginating).
///
/// Both the Detail and Diff panes share this one function rather than each
/// duplicating the clamp-then-scroll-then-indicator sequence, since the two
/// panes' only difference is which `Vec<Line>` and title they pass in.
///
/// `lines` is wrapped (via [`wrap_lines`]) to the pane's inner width
/// *before* `clamp_scroll`/`scroll_indicator` run and *before* handing it to
/// `Paragraph`, and `Paragraph::wrap` is deliberately not used here: that
/// widget's own line-wrapping happens after `Paragraph::scroll` has already
/// consumed `scroll.y` as an offset into the *unwrapped* logical lines, so
/// any logical line long enough to wrap desyncs the scroll unit from the
/// rendered unit — content past the first wrapped line of such a line
/// becomes unreachable at any scroll offset, and the overflow indicator
/// (computed from logical line count) undercounts and falsely claims
/// everything is visible. Wrapping first makes every one of
/// `clamp_scroll`/`scroll_indicator`/`Paragraph::scroll` operate on the same
/// "one rendered terminal row" unit.
///
/// Returns the actually-applied (clamped) scroll offset — dogfooding
/// finding: `App::right_pane_scroll` is deliberately an *unclamped*
/// "requested" value (its own doc comment), so repeated `j` past the
/// content's end kept incrementing that request with no visible effect,
/// and winding it back down again took as many `k` presses as it took to
/// overshoot — the scrollbar-less pane gave no feedback that this had
/// happened. `crate::run_app` feeds this return value back into `App` via
/// `App::with_right_pane_scroll` after every draw, so the *next* `k` moves
/// the visible content immediately instead of first re-tracing the
/// overshoot.
///
/// `focused` selects the pane's border style via
/// [`pane_border_style`] — dogfooding finding: every bordered pane looked
/// identical regardless of which one currently received motion keys, so a
/// reviewer had no visual way to tell which pane `j`/`k` would move. The
/// Detail/Diff/Blast-radius panes pass `app.focus() == Focus::Right`; the
/// `?` help overlay and jump popup, which are modal and always the active
/// surface while shown, pass `true` unconditionally.
pub(crate) fn render_scrollable_pane(
    frame: &mut Frame,
    title: &str,
    lines: &[Line<'static>],
    requested_scroll: usize,
    area: Rect,
    focused: bool,
) -> usize {
    // 2 columns/rows for the left/right and top/bottom border, matching
    // `draw_source_screen`'s own `saturating_sub(2)` convention for a
    // bordered pane's inner height.
    let viewport_width = area.width.saturating_sub(2) as usize;
    let viewport_height = area.height.saturating_sub(2) as usize;
    let wrapped = wrap_lines(lines, viewport_width);
    let scroll = clamp_scroll(wrapped.len(), viewport_height, requested_scroll);

    // Callers pass a title already padded with a leading/trailing space
    // (e.g. `" Detail "`, matching every other `Block` title in this
    // module) — trim the trailing one before appending the indicator so
    // the two don't produce a double space (`"Detail  (1-17/43)"`).
    let title = match scroll_indicator(wrapped.len(), viewport_height, scroll) {
        Some(indicator) => format!("{}{indicator} ", title.trim_end()),
        None => title.to_string(),
    };

    let block = Block::bordered()
        .title(title)
        .border_style(pane_border_style(focused));
    let paragraph = Paragraph::new(wrapped)
        .block(block)
        .scroll((scroll as u16, 0));
    frame.render_widget(paragraph, area);
    scroll
}

/// Wraps each of `lines` to `width` display columns, splitting a logical
/// [`Line`] into as many output lines as needed while keeping each [`Span`]'s
/// style attached to the fragment it contributed (a wrap point can fall
/// mid-span, in which case the span itself is split and both fragments keep
/// the original span's style). Width is measured with
/// [`UnicodeWidthChar::width`] (falling back to 1 column for the zero-width/
/// control-character case `width()` returns `None` for) rather than byte or
/// `char` count, so a wide (e.g. full-width CJK) character that would
/// overflow `width` on its own wraps onto the next line instead of being
/// sliced in half.
///
/// A pure, unit-testable stand-in for `ratatui::widgets::Wrap`'s own
/// char-wrapping (`trim: false` mode) — needed because this crate must know
/// the *wrapped* line count up front, before `Paragraph::scroll` ever runs
/// (see `render_scrollable_pane`'s doc comment on why `Paragraph::wrap`
/// itself cannot be used for a scrollable pane). Deliberately does not
/// attempt `ratatui::widgets::Wrap`'s word-boundary trimming behavior —
/// content here is source/diff text, not prose, so a plain character wrap
/// (breaking wherever the width limit is hit, mid-word if needed) is the
/// right fidelity, not an approximation to chase.
///
/// `width == 0` returns `lines` unchanged (nothing meaningful to wrap
/// into — an actual zero-width pane cannot render any column anyway, and
/// looping without ever advancing would otherwise be a defensive infinite-
/// loop risk).
pub(crate) fn wrap_lines(lines: &[Line<'static>], width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return lines.to_vec();
    }

    let mut output = Vec::new();
    for line in lines {
        output.extend(wrap_one_line(line, width));
    }
    output
}

/// Wraps a single logical [`Line`] into one or more output lines, per
/// [`wrap_lines`]'s doc comment. A line with no spans at all (a blank line)
/// produces exactly one empty output line, matching `ratatui::widgets::Wrap`
/// rendering a blank logical line as one blank row rather than zero rows.
pub(crate) fn wrap_one_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    let mut result_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in &line.spans {
        let style = span.style;
        let mut fragment = String::new();
        let mut fragment_width = 0usize;

        for ch in span.content.chars() {
            let char_width = ch.width().unwrap_or(1);

            if current_width + fragment_width + char_width > width {
                // Flush the fragment accumulated so far (if any) onto the
                // current output line, then start a brand-new output line —
                // the char that overflowed becomes the first char of the
                // next fragment.
                if !fragment.is_empty() {
                    current_spans.push(Span::styled(fragment.clone(), style));
                    fragment.clear();
                    fragment_width = 0;
                }
                result_lines.push(Line::from(std::mem::take(&mut current_spans)).style(line.style));
                current_width = 0;
            }

            fragment.push(ch);
            fragment_width += char_width;
        }

        if !fragment.is_empty() {
            current_spans.push(Span::styled(fragment, style));
            current_width += fragment_width;
        }
    }

    result_lines.push(Line::from(current_spans).style(line.style));
    result_lines
}

/// Truncates `text` to fit within `width` display columns, replacing the
/// tail with a single `…` (1 column) when it does not fit — unlike
/// [`wrap_lines`], which turns overflow into *more rows*, this turns
/// overflow into a shorter *single* row, for callers whose windowing math
/// (e.g. [`windowed_rows_with_indicators`]) has already committed to
/// "one logical item = one rendered row" and would desync if a row were
/// allowed to wrap (`draw_jump_popup`'s own fix for exactly that: a
/// `Paragraph::wrap`-ed candidate label taller than one row pushed later
/// candidates, including the cursor row, out of the popup's viewport with
/// no visual feedback, defeating `windowed_rows_with_indicators`'s own
/// "cursor always inside the window" contract).
///
/// Width is measured with [`UnicodeWidthChar::width`] (same fallback-to-1
/// convention as [`wrap_one_line`]) so a wide (e.g. CJK) character is never
/// sliced in half — if the last character that would fit is wide and only
/// one column of room remains, it is dropped along with the rest of the
/// tail rather than emitted half-width.
///
/// `text` already fitting within `width` (including exactly, or when
/// `width == 0`) is returned unchanged/empty respectively without adding
/// the `…` marker — nothing was actually cut off.
pub(crate) fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text_width: usize = text.chars().map(|ch| ch.width().unwrap_or(1)).sum();
    if text_width <= width {
        return text.to_string();
    }

    // Reserve 1 column for the trailing `…` marker, then greedily take
    // characters until the next one would overflow the remaining budget.
    let budget = width - 1;
    let mut result = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(1);
        if used + char_width > budget {
            break;
        }
        result.push(ch);
        used += char_width;
    }
    result.push('…');
    result
}

/// [`Line`]/[`Span`] counterpart of [`truncate_to_width`] — kept separate
/// because a tree-pane row is built from several differently-styled spans,
/// so truncating flattened text would lose which style belonged to which
/// surviving character. Stops at the first overflow and appends one `…`
/// span rather than wrapping, preserving the "one logical row stays one
/// rendered row" invariant [`windowed_rows_with_indicators`] depends on.
pub(crate) fn truncate_line_to_width(line: &Line<'static>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default().style(line.style);
    }
    let line_width: usize = line
        .spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(|ch| ch.width().unwrap_or(1))
        .sum();
    if line_width <= width {
        return line.clone();
    }

    let mut result_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut last_style = Style::default();
    // 1 column reserved for the trailing `…`.
    let budget = width.saturating_sub(1);

    'spans: for span in &line.spans {
        let style = span.style;
        let mut fragment = String::new();
        let mut fragment_width = 0usize;

        for ch in span.content.chars() {
            let char_width = ch.width().unwrap_or(1);
            if used + fragment_width + char_width > budget {
                if !fragment.is_empty() {
                    result_spans.push(Span::styled(fragment, style));
                }
                last_style = style;
                break 'spans;
            }
            fragment.push(ch);
            fragment_width += char_width;
        }

        if !fragment.is_empty() {
            result_spans.push(Span::styled(fragment, style));
            used += fragment_width;
        }
        last_style = style;
    }

    // Merge into the last span when styles match, else a single-style
    // line would fail `PartialEq` against an equivalent single-span value.
    match result_spans.last_mut() {
        Some(last) if last.style == last_style => {
            let mut content = last.content.to_string();
            content.push('…');
            last.content = content.into();
        }
        _ => result_spans.push(Span::styled("…", last_style)),
    }
    Line::from(result_spans).style(line.style)
}

/// Clamps a requested scroll offset (lines) to `[0, content_len -
/// viewport_height]` — the largest offset that still leaves the viewport
/// full of content rather than trailing off into blank space below the
/// last line. Returns 0 whenever the content already fits entirely
/// (`content_len <= viewport_height`, including `viewport_height == 0`
/// defensively), so a pane that has nothing to scroll can never report a
/// nonzero offset.
///
/// Deliberately pure and free of any `ratatui`/ `Rect` type (just the three
/// `usize`s a caller already has in hand) so it is unit-testable without a
/// `TestBackend` — `crate::app::App` intentionally does not do this
/// clamping itself (see `right_pane_scroll`'s own doc comment): only
/// `crate::ui`, at draw time, knows the pane's actual rendered height.
pub(crate) fn clamp_scroll(
    content_len: usize,
    viewport_height: usize,
    requested_scroll: usize,
) -> usize {
    let max_scroll = content_len.saturating_sub(viewport_height);
    requested_scroll.min(max_scroll)
}

/// Computes the `[start, end)` window of `total_items` rows to display in a
/// viewport `viewport_height` rows tall so that `cursor_index` is always
/// inside `[start, end)` — the tree pane's own cursor-follow scroll
/// (post-#61 review finding: `draw_tree_pane` used to hand `Nav::rows`'
/// *entire* row list to `Paragraph` unscrolled, so jumping the cursor to a
/// row outside the initial viewport — via `j`/`k` repeated past the bottom,
/// or a `gd`/`gr` jump — left the screen showing exactly the same rows as
/// before, looking like the keypress had no effect) and the jump-target
/// popup's own candidate-list scroll (same underlying gap: `draw_jump_popup`
/// used to hand every candidate to `Paragraph` unscrolled).
///
/// Mirrors `crate::source::visible_window`'s centering approach (keep the
/// point of interest mid-viewport rather than pinned to an edge, so a few
/// rows of context are visible on both sides) but for a single index rather
/// than a highlighted range, and 0-based indices/half-open `[start, end)`
/// throughout — matching this module's own row-index convention
/// (`draw_tree_pane`'s `index == cursor` check) rather than `visible_window`'s
/// 1-based line-number convention, so callers here never need to convert.
///
/// `total_items == 0` or `viewport_height == 0` returns `(0, 0)` — an empty
/// window, nothing to show either way.
pub(crate) fn visible_index_window(
    total_items: usize,
    cursor_index: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if total_items == 0 || viewport_height == 0 {
        return (0, 0);
    }

    let half = viewport_height / 2;
    let ideal_start = cursor_index.saturating_sub(half);

    // Clamp so the window never runs past the end of the list, then clamp
    // again at zero so a short list (fewer items than `viewport_height`)
    // still yields a valid, in-bounds window rather than a negative start —
    // same two-step clamp `visible_window` itself uses.
    let max_start = total_items.saturating_sub(viewport_height);
    let start = ideal_start.min(max_start);
    let end = (start + viewport_height).min(total_items);

    (start, end)
}

/// Builds a `"…N more above"`/`"…N more below"` pair of indicator lines for
/// content windowed by [`visible_index_window`] — `above`/`below` are the
/// counts of items hidden on each side (`start`/`total_items - end`
/// respectively), formatted only when nonzero so a window that already
/// shows everything (or sits at an edge) does not grow a spurious "…0 more"
/// line. Returned as `(above, below)`, each `Option<String>`, for the caller
/// to place immediately before/after the windowed content — kept as plain
/// `String`s rather than `ratatui::text::Line` so this stays unit-testable
/// without a `ratatui` type, matching [`scroll_indicator`]'s own precedent.
pub(crate) fn window_overflow_indicators(
    total_items: usize,
    window_start: usize,
    window_end: usize,
) -> (Option<String>, Option<String>) {
    let above = window_start;
    let below = total_items.saturating_sub(window_end);
    (
        (above > 0).then(|| format!("… {above} more above")),
        (below > 0).then(|| format!("… {below} more below")),
    )
}

/// Ties [`visible_index_window`] and [`window_overflow_indicators`]
/// together correctly for a caller that renders the indicator lines
/// *inside* the same fixed-height viewport as the windowed content itself
/// (`draw_tree_pane`/`draw_jump_popup`'s own layout): naively computing the
/// content window against the *full* `viewport_height` and then
/// unconditionally prepending/appending indicator lines on top overflows
/// the viewport by up to 2 rows, silently clipping the last row or two of
/// real content off the bottom of the pane (including, in the worst case,
/// the cursor row itself — the exact bug this windowing feature exists to
/// fix, reintroduced one layer up). This function reserves a row for each
/// indicator *before* sizing the content window, so the total row count
/// (indicators + content) never exceeds `viewport_height`.
///
/// Reserving is a small fixed-point search rather than a single
/// calculation: whether the "above"/"below" indicator is needed at all
/// depends on where the content window ends up, which itself depends on
/// how many rows are reserved for indicators — so this recomputes the
/// window with 0, then up to 2, reserved rows until the reservation and the
/// window it produces agree. This always converges in at most 3 iterations
/// (each iteration can only add a reservation, never remove one, and there
/// are only two indicators to add), so a small bounded loop is used rather
/// than proving a closed-form formula.
///
/// Returns `(content_start, content_end, above_indicator, below_indicator)`.
pub(crate) fn windowed_rows_with_indicators(
    total_items: usize,
    cursor_index: usize,
    viewport_height: usize,
) -> (usize, usize, Option<String>, Option<String>) {
    let mut reserved = 0;
    loop {
        let content_height = viewport_height.saturating_sub(reserved);
        let (start, end) = visible_index_window(total_items, cursor_index, content_height);
        let (above, below) = window_overflow_indicators(total_items, start, end);
        let needed = above.is_some() as usize + below.is_some() as usize;
        if needed <= reserved {
            return (start, end, above, below);
        }
        reserved = needed;
    }
}

/// Builds the `(first-last/total)` title suffix for a pane whose content
/// overflows its viewport, or `None` when everything already fits (nothing
/// to indicate). `scroll` must already be clamped (`clamp_scroll`) — this
/// function does not re-clamp, it only formats.
pub(crate) fn scroll_indicator(
    content_len: usize,
    viewport_height: usize,
    scroll: usize,
) -> Option<String> {
    if content_len <= viewport_height {
        return None;
    }
    let first_visible = scroll + 1;
    let last_visible = (scroll + viewport_height).min(content_len);
    Some(format!(" ({first_visible}-{last_visible}/{content_len})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};

    // --- clamp_scroll / scroll_indicator (pure helpers) ---

    #[test]
    fn should_return_zero_scroll_when_content_fits_entirely() {
        let actual = clamp_scroll(5, 10, 3);

        assert_eq!(0, actual);
    }

    #[test]
    fn should_clamp_requested_scroll_to_max_scroll_when_it_overshoots() {
        // 20 lines of content in a 10-row viewport: max_scroll = 10, so a
        // request of 15 clamps down to 10 (the last full page).
        let actual = clamp_scroll(20, 10, 15);

        assert_eq!(10, actual);
    }

    #[test]
    fn should_pass_through_requested_scroll_when_within_bounds() {
        let actual = clamp_scroll(20, 10, 4);

        assert_eq!(4, actual);
    }

    #[test]
    fn should_return_zero_scroll_when_viewport_height_is_zero() {
        // A degenerate (zero-height) pane can never scroll — `max_scroll`
        // saturates at `content_len` itself, but a requested scroll of 0
        // (the only value `App` ever starts at) still clamps to 0.
        let actual = clamp_scroll(20, 0, 0);

        assert_eq!(0, actual);
    }

    #[test]
    fn should_return_none_indicator_when_content_fits_entirely() {
        let actual = scroll_indicator(5, 10, 0);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_indicator_at_top_when_content_overflows_and_scroll_is_zero() {
        let actual = scroll_indicator(20, 10, 0);

        assert_eq!(Some(" (1-10/20)".to_string()), actual);
    }

    #[test]
    fn should_return_indicator_reflecting_scroll_position() {
        let actual = scroll_indicator(20, 10, 4);

        assert_eq!(Some(" (5-14/20)".to_string()), actual);
    }

    #[test]
    fn should_clamp_last_visible_to_content_len_at_max_scroll() {
        // scroll=10, viewport=10 would naively suggest last_visible=20,
        // which happens to equal content_len here anyway; this pins the
        // `.min(content_len)` clamp directly rather than relying on the
        // coincidence.
        let actual = scroll_indicator(20, 10, 10);

        assert_eq!(Some(" (11-20/20)".to_string()), actual);
    }

    // --- visible_index_window / window_overflow_indicators (pure helpers,
    // #61-review fix: cursor-follow scroll for the tree pane and jump popup)

    #[test]
    fn should_return_empty_window_when_total_items_is_zero() {
        let actual = visible_index_window(0, 0, 10);

        assert_eq!((0, 0), actual);
    }

    #[test]
    fn should_return_empty_window_when_viewport_height_is_zero() {
        let actual = visible_index_window(20, 5, 0);

        assert_eq!((0, 0), actual);
    }

    #[test]
    fn should_show_whole_list_when_it_fits_entirely_within_viewport() {
        let actual = visible_index_window(5, 2, 10);

        assert_eq!((0, 5), actual);
    }

    #[test]
    fn should_center_window_around_cursor_when_list_exceeds_viewport() {
        // 100 items, cursor at index 49, viewport 10 -> half=5,
        // ideal_start=44, max_start=90 (not clamped) -> (44, 54).
        let actual = visible_index_window(100, 49, 10);

        assert_eq!((44, 54), actual);
    }

    #[test]
    fn should_clamp_window_to_start_of_list_when_cursor_is_near_the_top() {
        let actual = visible_index_window(100, 1, 10);

        assert_eq!((0, 10), actual);
    }

    #[test]
    fn should_clamp_window_to_end_of_list_when_cursor_is_near_the_bottom() {
        let actual = visible_index_window(100, 98, 10);

        assert_eq!((90, 100), actual);
    }

    #[test]
    fn should_keep_cursor_row_inside_the_window_when_jumping_far_from_the_current_position() {
        // The exact scenario the #61-review finding describes: a cursor
        // that jumps from near the top of a long list straight to near the
        // bottom (e.g. a gd/gr jump) must land inside the returned window,
        // not leave it showing the same rows as before the jump.
        let (start, end) = visible_index_window(200, 180, 20);

        assert!(
            start <= 180 && 180 < end,
            "cursor 180 not in [{start}, {end})"
        );
    }

    #[test]
    fn should_return_no_indicators_when_window_covers_the_whole_list() {
        let actual = window_overflow_indicators(5, 0, 5);

        assert_eq!((None, None), actual);
    }

    #[test]
    fn should_return_above_indicator_only_when_window_starts_past_the_top() {
        let actual = window_overflow_indicators(100, 10, 20);

        assert_eq!(
            (
                Some("… 10 more above".to_string()),
                Some("… 80 more below".to_string())
            ),
            actual
        );
    }

    #[test]
    fn should_return_no_above_indicator_when_window_starts_at_the_top() {
        let actual = window_overflow_indicators(100, 0, 20);

        assert_eq!((None, Some("… 80 more below".to_string())), actual);
    }

    #[test]
    fn should_return_no_below_indicator_when_window_reaches_the_end() {
        let actual = window_overflow_indicators(100, 80, 100);

        assert_eq!((Some("… 80 more above".to_string()), None), actual);
    }

    // --- windowed_rows_with_indicators (pure helper) ---
    //
    // Regression coverage for the reserved-row bug found while writing
    // `should_window_candidates_around_cursor_when_popup_has_more_candidates_than_fit`:
    // naively computing the content window against the full viewport height
    // and then unconditionally prepending/appending indicator lines
    // overflows the viewport by up to 2 rows, clipping the cursor row
    // itself off the bottom in the worst case.

    #[test]
    fn should_return_whole_list_with_no_indicators_when_it_fits_the_viewport() {
        let actual = windowed_rows_with_indicators(5, 2, 10);

        assert_eq!((0, 5, None, None), actual);
    }

    #[test]
    fn should_reserve_a_row_for_the_below_indicator_when_cursor_is_near_the_top() {
        // 100 items, cursor at 0, viewport 10: without reservation the
        // content window alone would be (0, 10), needing a "below"
        // indicator — reserving 1 row for it must shrink the content
        // window to (0, 9) so the indicator line has room without pushing
        // total rows past the viewport.
        let (start, end, above, below) = windowed_rows_with_indicators(100, 0, 10);

        assert_eq!((0, 9), (start, end));
        assert_eq!(None, above);
        assert_eq!(Some("… 91 more below".to_string()), below);
        // The rendered row count (indicator + content) must never exceed
        // the viewport.
        let below_rows = below.is_some() as usize;
        assert!(end - start + below_rows <= 10);
    }

    #[test]
    fn should_reserve_a_row_for_the_above_indicator_when_cursor_is_near_the_bottom() {
        let (start, end, above, below) = windowed_rows_with_indicators(100, 99, 10);

        assert_eq!((91, 100), (start, end));
        assert_eq!(Some("… 91 more above".to_string()), above);
        assert_eq!(None, below);
        let above_rows = above.is_some() as usize;
        assert!(end - start + above_rows <= 10);
    }

    #[test]
    fn should_reserve_rows_for_both_indicators_when_cursor_is_in_the_middle() {
        let (start, end, above, below) = windowed_rows_with_indicators(100, 50, 10);

        assert!(above.is_some());
        assert!(below.is_some());
        // The cursor must still be inside the (possibly shrunk) content
        // window — the entire point of this function.
        assert!(start <= 50 && 50 < end, "cursor 50 not in [{start}, {end})");
        // Total rendered rows (2 indicators + content) must never exceed
        // the viewport.
        assert!(end - start + 2 <= 10);
    }

    #[test]
    fn should_keep_cursor_visible_after_reserving_indicator_rows_at_a_small_viewport() {
        // A tight viewport (3 rows) where reserving rows for indicators
        // could plausibly starve the content window down to nothing —
        // pins that the cursor row itself is never sacrificed.
        let (start, end, _above, below) = windowed_rows_with_indicators(50, 49, 3);

        assert!(start <= 49 && 49 < end, "cursor 49 not in [{start}, {end})");
        let below_rows = below.is_some() as usize;
        assert!(end - start + below_rows <= 3);
    }

    // --- wrap_lines (pure helper) ---

    #[test]
    fn should_return_lines_unchanged_when_width_is_zero() {
        let lines = vec![Line::raw("hello world")];

        let actual = wrap_lines(&lines, 0);

        assert_eq!(lines, actual);
    }

    #[test]
    fn should_return_one_empty_line_when_input_line_is_blank() {
        let lines = vec![Line::raw("")];

        let actual = wrap_lines(&lines, 10);

        assert_eq!(vec![Line::raw("")], actual);
    }

    #[test]
    fn should_not_wrap_when_line_fits_exactly_within_width() {
        let lines = vec![Line::raw("abcde")];

        let actual = wrap_lines(&lines, 5);

        assert_eq!(vec![Line::raw("abcde")], actual);
    }

    #[test]
    fn should_split_long_ascii_line_into_multiple_lines_at_the_width_boundary() {
        let lines = vec![Line::raw("abcdefghij")];

        let actual = wrap_lines(&lines, 4);

        assert_eq!(
            vec![Line::raw("abcd"), Line::raw("efgh"), Line::raw("ij"),],
            actual
        );
    }

    #[test]
    fn should_wrap_full_width_characters_without_splitting_a_double_width_char_across_lines() {
        // Each "あ" is 2 columns wide; a width-3 pane can fit "あ" (2) plus
        // one more column, but the second "あ" would overflow to column 4,
        // so it wraps onto the next line rather than being sliced in half.
        let lines = vec![Line::raw("ああa")];

        let actual = wrap_lines(&lines, 3);

        assert_eq!(vec![Line::raw("あ"), Line::raw("あa")], actual);
    }

    #[test]
    fn should_preserve_span_style_on_both_fragments_when_a_styled_span_is_split_by_wrapping() {
        let style = Style::default().fg(Color::Red);
        let lines = vec![Line::from(vec![Span::styled("abcdef", style)])];

        let actual = wrap_lines(&lines, 4);

        assert_eq!(
            vec![
                Line::from(vec![Span::styled("abcd", style)]),
                Line::from(vec![Span::styled("ef", style)]),
            ],
            actual
        );
    }

    #[test]
    fn should_preserve_distinct_span_styles_when_a_multi_span_line_wraps_across_span_boundaries() {
        // "ab" (unstyled) + "cdef" (red): a width-3 wrap must split after
        // "abc" (2 unstyled chars + 1 red char) and carry each fragment's
        // own style into the split, not just the first span's.
        let red = Style::default().fg(Color::Red);
        let lines = vec![Line::from(vec![Span::raw("ab"), Span::styled("cdef", red)])];

        let actual = wrap_lines(&lines, 3);

        assert_eq!(
            vec![
                Line::from(vec![Span::raw("ab"), Span::styled("c", red)]),
                Line::from(vec![Span::styled("def", red)]),
            ],
            actual
        );
    }

    #[test]
    fn should_wrap_each_logical_line_independently_when_multiple_lines_are_passed() {
        let lines = vec![Line::raw("abcdef"), Line::raw("xy")];

        let actual = wrap_lines(&lines, 4);

        assert_eq!(
            vec![Line::raw("abcd"), Line::raw("ef"), Line::raw("xy")],
            actual
        );
    }

    // --- truncate_to_width (pure helper, jump popup one-row-per-candidate fix) ---

    #[test]
    fn should_return_text_unchanged_when_it_fits_exactly_within_width() {
        let actual = truncate_to_width("abcde", 5);

        assert_eq!("abcde".to_string(), actual);
    }

    #[test]
    fn should_return_text_unchanged_when_it_is_shorter_than_width() {
        let actual = truncate_to_width("abc", 10);

        assert_eq!("abc".to_string(), actual);
    }

    #[test]
    fn should_return_empty_string_when_width_is_zero() {
        let actual = truncate_to_width("abcdef", 0);

        assert_eq!("".to_string(), actual);
    }

    #[test]
    fn should_truncate_long_ascii_text_with_trailing_marker_when_it_overflows_width() {
        // width=5 -> 4 chars of budget + 1 column for the trailing "…".
        let actual = truncate_to_width("abcdefgh", 5);

        assert_eq!("abcd…".to_string(), actual);
    }

    #[test]
    fn should_not_split_a_double_width_character_when_truncating() {
        // "あ" is 2 columns; a width-4 budget after reserving 1 column for
        // "…" leaves 3 columns, which fits "a" (1) + "あ" (2) exactly but
        // not a second "あ" (would need 5) — so the second "あ" and
        // everything after it is dropped rather than sliced in half.
        let actual = truncate_to_width("aああb", 4);

        assert_eq!("aあ…".to_string(), actual);
    }

    #[test]
    fn should_truncate_mixed_cjk_and_ascii_label_when_it_overflows_width() {
        let actual = truncate_to_width("シンボル名 (path/to/very/long/file.rs)", 10);

        assert_eq!("シンボル…".to_string(), actual);
    }

    #[test]
    fn should_return_only_the_marker_when_width_is_one() {
        // width=1 leaves a budget of 0 after reserving 1 column for "…", so
        // every character of the input is dropped and only the marker
        // itself remains — the narrowest width at which truncation still
        // produces non-empty output (width=0 is the separate empty-string
        // case covered above).
        let actual = truncate_to_width("abcdef", 1);

        assert_eq!("…".to_string(), actual);
    }

    #[test]
    fn should_return_only_the_marker_when_width_is_one_and_first_char_is_double_width() {
        // Same width=1 boundary, but the first character of the input is a
        // 2-column CJK character that would not fit in the 0-column budget
        // either — makes sure the double-width guard and the width=1
        // budget-exhaustion guard compose correctly instead of one
        // masking a bug in the other.
        let actual = truncate_to_width("あいう", 1);

        assert_eq!("…".to_string(), actual);
    }

    // --- truncate_line_to_width (styled, multi-span counterpart) ---

    #[test]
    fn should_return_line_unchanged_when_it_fits_within_width() {
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cde", Style::default())]);

        let actual = truncate_line_to_width(&line, 5);

        assert_eq!(line, actual);
    }

    #[test]
    fn should_truncate_single_span_line_with_trailing_marker_when_it_overflows_width() {
        let line = Line::raw("abcdefgh");

        let actual = truncate_line_to_width(&line, 5);

        assert_eq!(Line::raw("abcd…"), actual);
    }

    #[test]
    fn should_preserve_each_surviving_spans_own_style_when_truncating_a_multi_span_line() {
        let red = Style::default().fg(Color::Red);
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cdef", red)]);

        let actual = truncate_line_to_width(&line, 4);

        assert_eq!(
            Line::from(vec![Span::raw("ab"), Span::styled("c…", red)]),
            actual
        );
    }

    #[test]
    fn should_preserve_line_level_selected_style_when_truncating_an_overflowing_line() {
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);
        let line = Line::from(vec![Span::raw("abcdefgh")]).style(selected_style);

        let actual = truncate_line_to_width(&line, 5);

        assert_eq!(
            Line::from(vec![Span::raw("abcd…")]).style(selected_style),
            actual
        );
    }

    #[test]
    fn should_not_split_a_double_width_character_when_truncating_a_line() {
        let line = Line::raw("aああb");

        let actual = truncate_line_to_width(&line, 4);

        assert_eq!(Line::raw("aあ…"), actual);
    }

    #[test]
    fn should_return_only_the_marker_line_when_width_is_one() {
        let line = Line::raw("abcdef");

        let actual = truncate_line_to_width(&line, 1);

        assert_eq!(Line::raw("…"), actual);
    }

    #[test]
    fn should_return_only_the_marker_line_when_width_is_one_and_first_char_is_double_width() {
        let line = Line::raw("あいう");

        let actual = truncate_line_to_width(&line, 1);

        assert_eq!(Line::raw("…"), actual);
    }

    #[test]
    fn should_return_empty_line_when_width_is_zero() {
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);
        let line = Line::from(vec![Span::raw("abcdef")]).style(selected_style);

        let actual = truncate_line_to_width(&line, 0);

        assert_eq!(Line::default().style(selected_style), actual);
    }
}
