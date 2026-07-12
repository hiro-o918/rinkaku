//! `ratatui` rendering (stage B, ADR 0015/0016): draws one frame from the
//! current [`App`] state. This is the crate's thin adapter layer — layout
//! decisions live here, but every value drawn (row text/style, detail
//! fields, source lines) comes from a pure view-model computed elsewhere
//! (`crate::app`, `crate::row_view`, `crate::detail`, `crate::source`).
//!
//! Kept deliberately un-unit-tested beyond the coarse `TestBackend`
//! snapshots in this module's own test block (ADR 0016: "rendering itself
//! is covered separately... kept few and coarse — enough to catch a broken
//! layout, not to pin every pixel").

use crate::app::{App, DiffTarget, PivotSelection, RightPane, Screen, SelectedDetail};
use crate::detail::{DetailView, DirDetail, FileDetail, SignatureView};
use crate::diff_view::{DiffLine, DiffLineKind, FileHunks, Hunk, file_hunks, hunks_for_range};
use crate::highlight::{self, HighlightedFile, PALETTE, TokenSpan};
use crate::row_view::{entry_row_line, relative_labels};
use crate::source::{SourceView, load_symbol_source, visible_window};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use rinkaku_core::extract::Classification;
use rinkaku_core::render::{Report, ReportOrigin};
use unicode_width::UnicodeWidthChar;

/// Draws one full frame: the entry view (tree + right pane split) or the
/// source drill-down, depending on `app.screen()`, with a status/help line
/// pinned to the bottom either way. `diff_files` is the whole diff already
/// parsed into per-file hunks once by `crate::run_app` (not re-parsed here
/// on every frame — see that function's doc comment on why parsing lives
/// outside the draw loop), and `diff_highlights` is that same diff's
/// per-line syntax highlighting, computed once alongside it (ADR 0018) —
/// both are only consulted when the right pane is in [`RightPane::Diff`]
/// mode. `pivot_selection` is likewise computed once per handled key by
/// `crate::run_app` (not here) whenever the right pane is in
/// [`RightPane::Pivot`] mode — see `App::selected_pivot_view`'s own doc
/// comment on why this function must not call it itself. `repo_root` is
/// only consulted on [`Screen::Source`] (forwarded to
/// [`load_symbol_source`], see that function's doc comment for why
/// `Report` paths need it) — it is threaded through here rather than
/// resolved lazily so this stays the single frame-drawing entry point.
pub fn draw(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_files: &[FileHunks],
    diff_highlights: &[HighlightedFile],
    pivot_selection: &PivotSelection,
    repo_root: &std::path::Path,
) {
    let area = frame.area();
    let [body, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    match app.screen() {
        Screen::Entry => draw_entry_screen(
            frame,
            app,
            report,
            diff_files,
            diff_highlights,
            pivot_selection,
            body,
        ),
        Screen::Source { symbol_id } => {
            draw_source_screen(frame, report, symbol_id, repo_root, body)
        }
    }

    draw_status_line(frame, app, status_area);
}

/// Left entry pane (directory tree) + right pane, split 60/40 — this
/// implementation's own choice (ADR 0015/0016 left the exact ratio open):
/// the tree is the primary navigation surface and typically has more rows
/// than the right pane has fields, so it gets the larger share. The right
/// pane itself shows either the detail view or the diff view depending on
/// `app.right_pane()` (`d`/`D` toggles between them, TUI iteration 2).
fn draw_entry_screen(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_files: &[FileHunks],
    diff_highlights: &[HighlightedFile],
    pivot_selection: &PivotSelection,
    area: Rect,
) {
    let [tree_area, right_area] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    draw_tree_pane(frame, app, tree_area);
    match app.right_pane() {
        RightPane::Detail => draw_detail_pane(frame, app, report, right_area),
        RightPane::Diff => {
            draw_diff_pane(frame, app, report, diff_files, diff_highlights, right_area)
        }
        RightPane::Pivot => draw_pivot_pane(frame, app, pivot_selection, right_area),
    }
}

fn draw_tree_pane(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app.nav().rows(app.tree());
    let labels = relative_labels(&rows);
    let cursor = app.nav().cursor();

    let ranks = app.ranks();
    let lines: Vec<Line<'static>> = rows
        .iter()
        .zip(labels.iter())
        .enumerate()
        .map(|(index, (row, label))| entry_row_line(row, label, ranks, index == cursor))
        .collect();

    let block = Block::bordered().title(" Entry ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_detail_pane(frame: &mut Frame, app: &App, report: &Report, area: Rect) {
    let Some(detail) = app.selected_detail(report) else {
        let block = Block::bordered().title(" Detail ");
        let paragraph = Paragraph::new("(select a row to see its detail)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    let lines = match &detail {
        SelectedDetail::Symbol(detail) => detail_lines(detail),
        SelectedDetail::Dir(detail) => dir_detail_lines(detail, report.origin),
        SelectedDetail::File(detail) => file_detail_lines(detail),
    };
    render_scrollable_pane(frame, " Detail ", &lines, app.right_pane_scroll(), area);
}

/// Draws the diff pane (TUI iteration 2, [`RightPane::Diff`]): the raw
/// unified-diff hunks touching the row under the cursor — every hunk of the
/// file for a file row, or just the hunks intersecting a symbol's own line
/// range for a symbol row (`App::selected_diff_target`'s own doc comment).
/// A directory row, or a row with nothing to show (no hunks found, e.g. a
/// mismatch between `report` and the diff), falls back to a placeholder
/// message rather than an empty pane. `diff_files` is already parsed and
/// `diff_highlights` already highlighted (`crate::run_app` does both once,
/// up front, not on every call to this function — ADR 0018).
fn draw_diff_pane(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_files: &[FileHunks],
    diff_highlights: &[HighlightedFile],
    area: Rect,
) {
    let Some(target) = app.selected_diff_target(report) else {
        let block = Block::bordered().title(" Diff ");
        let paragraph = Paragraph::new("(select a symbol or file row to see its diff)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    let (path, hunks): (&str, Vec<&Hunk>) = match &target {
        DiffTarget::Symbol {
            path,
            range_start,
            range_end,
        } => {
            let hunks = file_hunks(diff_files, path)
                .map(|fh| hunks_for_range(fh, *range_start, *range_end))
                .unwrap_or_default();
            (path.as_str(), hunks)
        }
        DiffTarget::File { path } => {
            let hunks = file_hunks(diff_files, path)
                .map(|fh: &FileHunks| fh.hunks.iter().collect())
                .unwrap_or_default();
            (path.as_str(), hunks)
        }
    };

    if hunks.is_empty() {
        let block = Block::bordered().title(" Diff ");
        let paragraph = Paragraph::new(format!("(no diff hunks found for {path})"))
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    }

    // `file_hunks` was already resolved above via `DiffTarget`'s match arm,
    // but `hunks_for_range`/the file-row arm both return `&Hunk`s borrowed
    // from it — re-resolving it here (rather than threading it out of the
    // match above) keeps `highlight::lookup_hunk_highlight`'s pointer-
    // identity lookup working against the exact same `FileHunks` the
    // `&Hunk`s in `hunks` were borrowed from.
    let source_file_hunks = file_hunks(diff_files, path);
    let highlighted_file = highlight::highlighted_file(diff_highlights, path);

    let lines = diff_pane_lines(&hunks, source_file_hunks, highlighted_file);
    render_scrollable_pane(frame, " Diff ", &lines, app.right_pane_scroll(), area);
}

/// Draws the pivot pane (ADR 0019, [`RightPane::Pivot`]): the entry-tree
/// text rooted at the directory/file row under the cursor, following the
/// cursor as it moves. `selection` is already computed by `crate::run_app`
/// (via `App::selected_pivot_view`) once per handled key, not here — this
/// function only lays it out, since `terminal.draw` itself runs on every
/// ~100ms idle poll tick as well as on an actual key press, and re-deriving
/// the pivot tree (an O(V+E) graph walk) on every one of those idle ticks
/// was exactly the per-frame recompute this split avoids. A symbol row
/// shows a placeholder asking for a directory/file row instead — pivoting
/// on a single symbol has no directory-scoped meaning (ADR 0019's
/// `path_prefix` is meant to carve out a layer, not re-derive what a single
/// symbol's own detail pane already shows). A directory/file row whose path
/// matches no symbol shows its own "no symbols under `<path>`" message,
/// mirroring `main.rs`'s `--entry` CLI note.
fn draw_pivot_pane(frame: &mut Frame, app: &App, selection: &PivotSelection, area: Rect) {
    match selection {
        PivotSelection::NotApplicable => {
            let block = Block::bordered().title(" Pivot ");
            let paragraph = Paragraph::new("(select a directory or file row to pivot)")
                .block(block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
        PivotSelection::Empty { path } => {
            let block = Block::bordered().title(" Pivot ");
            let paragraph = Paragraph::new(format!("(no symbols under {path})"))
                .block(block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
        PivotSelection::View(view) => {
            let lines = pivot_pane_lines(view);
            render_scrollable_pane(frame, " Pivot ", &lines, app.right_pane_scroll(), area);
        }
    }
}

/// Formats a [`crate::pivot::PivotView`]'s flattened [`crate::pivot::PivotLine`]s
/// into styled [`Line`]s: indentation by depth (same `INDENT_WIDTH`-per-level
/// convention as `crate::row_view::entry_row_line`), a dimmed style for
/// `outside_prefix` lines (reached only by expanding a dependency edge past
/// the pivoted path), a `(see above)` suffix for `already_printed` lines
/// (matching `rinkaku-core::render`'s Markdown tree), and yellow/bold for a
/// cycle-warning line (matching `entry_row_line`'s existing `(cycle)`
/// marker styling).
fn pivot_pane_lines(view: &crate::pivot::PivotView) -> Vec<Line<'static>> {
    const INDENT_WIDTH: usize = 2;
    view.lines
        .iter()
        .map(|line| {
            let indent = " ".repeat(line.depth * INDENT_WIDTH);
            let mut style = Style::default();
            if line.is_cycle_warning {
                style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
            } else if line.outside_prefix {
                style = style.add_modifier(Modifier::DIM);
            }
            let text = if line.already_printed {
                format!("{indent}- {} (see above)", line.label)
            } else {
                format!("{indent}- {}", line.label)
            };
            Line::from(Span::styled(text, style))
        })
        .collect()
}

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
fn render_scrollable_pane(
    frame: &mut Frame,
    title: &str,
    lines: &[Line<'static>],
    requested_scroll: usize,
    area: Rect,
) {
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

    let block = Block::bordered().title(title);
    let paragraph = Paragraph::new(wrapped)
        .block(block)
        .scroll((scroll as u16, 0));
    frame.render_widget(paragraph, area);
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
fn wrap_lines(lines: &[Line<'static>], width: usize) -> Vec<Line<'static>> {
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
fn wrap_one_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
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
fn clamp_scroll(content_len: usize, viewport_height: usize, requested_scroll: usize) -> usize {
    let max_scroll = content_len.saturating_sub(viewport_height);
    requested_scroll.min(max_scroll)
}

/// Builds the `(first-last/total)` title suffix for a pane whose content
/// overflows its viewport, or `None` when everything already fits (nothing
/// to indicate). `scroll` must already be clamped (`clamp_scroll`) — this
/// function does not re-clamp, it only formats.
fn scroll_indicator(content_len: usize, viewport_height: usize, scroll: usize) -> Option<String> {
    if content_len <= viewport_height {
        return None;
    }
    let first_visible = scroll + 1;
    let last_visible = (scroll + viewport_height).min(content_len);
    Some(format!(" ({first_visible}-{last_visible}/{content_len})"))
}

/// Formats a list of [`Hunk`]s into styled lines: hunk headers dim, `+`/`-`
/// marker glyphs keep their existing bold green/red foreground, and each
/// line's own code tokens are colored by [`highlight::lookup_hunk_highlight`]
/// when available (ADR 0018) — falling back to the plain green/red/unstyled
/// line style this pane always had when a hunk has no highlight (unknown
/// extension, parse/query failure) so highlighting can never make a diff
/// harder to read than before.
///
/// `source_file_hunks`/`highlighted_file` are `None` exactly when `hunks`
/// itself would already be empty (`draw_diff_pane` returns before calling
/// this function in that case), so in practice they are always `Some` here
/// — kept as `Option`s anyway (rather than unwrapped) since `file_hunks`
/// returning `None` is a defensive, not-supposed-to-happen case elsewhere
/// in this module too, and threading the same shape through keeps this
/// function's fallback path uniform with `highlight::lookup_hunk_highlight`'s
/// own `None` handling.
fn diff_pane_lines(
    hunks: &[&Hunk],
    source_file_hunks: Option<&FileHunks>,
    highlighted_file: Option<&HighlightedFile>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, hunk) in hunks.iter().enumerate() {
        if index > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            hunk.header.clone(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));

        let hunk_highlight = source_file_hunks
            .and_then(|fh| highlight::lookup_hunk_highlight(highlighted_file, fh, hunk));

        for (line_index, line) in hunk.lines.iter().enumerate() {
            // `hunk_highlight` is `Option<&[LineHighlight]>`, and
            // `LineHighlight` is itself `Option<Vec<TokenSpan>>` (per-line
            // fallback within an otherwise-highlighted hunk) — `flatten`
            // collapses "no highlight data at all for this hunk" and
            // "this specific line had no highlight" into the same `None`
            // `diff_line` already treats as its fallback signal.
            let token_spans = hunk_highlight
                .and_then(|lines| lines.get(line_index).cloned())
                .flatten();
            lines.push(diff_line(line, token_spans));
        }
    }
    lines
}

/// Background tint for an `Added`/`Removed` line (ADR 0018 decision: diff
/// signal moves to the background so a token's own color can carry the
/// foreground). 256-color indexed dark green/red rather than the named
/// `Color::Green`/`Color::Red` used for the `+`/`-` marker itself — a
/// named-color *background* at full saturation would fight with the
/// foreground token colors for attention, whereas these dim indexed tones
/// (in the xterm 256 palette's grayscale-adjacent dark green/red range)
/// stay legible as "this line changed" without competing with the text.
const ADDED_BG: Color = Color::Indexed(22);
const REMOVED_BG: Color = Color::Indexed(52);

/// Builds one display line for a hunk body line, coloring its code tokens
/// per `token_spans` (`None` when highlighting is unavailable for this
/// line — falls back to the pane's original plain style). The `+`/`-`
/// marker glyph itself is always pushed as its own bold-colored span, kept
/// outside of `line.content`'s token coloring so it is never masked by a
/// token span that happens to start at byte 0.
fn diff_line(line: &DiffLine, token_spans: Option<Vec<TokenSpan>>) -> Line<'static> {
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
fn plain_diff_line(line: &DiffLine) -> Line<'static> {
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
fn marker_span(kind: DiffLineKind, bg: Option<Color>) -> Span<'static> {
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

/// Splits `content` into styled spans per `spans` (byte-offset [`TokenSpan`]s
/// already rebased to `content`'s own coordinates by
/// `highlight::spans_for_line`), coloring each token's foreground by its
/// palette entry (`palette_style`) and applying `bg` uniformly (the diff
/// signal) — any byte range `spans` doesn't cover (whitespace, punctuation
/// the query didn't capture) becomes an unstyled-foreground span with just
/// `bg` applied, so the line's background tint is always contiguous even
/// where token coloring has gaps.
fn styled_content_spans(
    content: &str,
    spans: &[TokenSpan],
    bg: Option<Color>,
) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut cursor = 0usize;

    let mut sorted_spans = spans.to_vec();
    sorted_spans.sort_by_key(|span| span.start);

    for span in &sorted_spans {
        if span.start > cursor {
            result.push(gap_span(&content[cursor..span.start], bg));
        }
        let mut style = palette_style(span.palette_index);
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        result.push(Span::styled(
            content[span.start..span.end].to_string(),
            style,
        ));
        cursor = span.end;
    }
    if cursor < content.len() {
        result.push(gap_span(&content[cursor..], bg));
    }
    if result.is_empty() {
        // Only reachable when `content` is empty AND no token spans exist
        // (a blank added/removed line): non-empty content always yields at
        // least one gap or token span above. The empty span keeps the
        // line's background tint rendering on blank lines too.
        result.push(gap_span("", bg));
    }

    result
}

fn gap_span(text: &str, bg: Option<Color>) -> Span<'static> {
    let mut style = Style::default();
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    Span::styled(text.to_string(), style)
}

/// Maps a [`PALETTE`] index to its display style — the minimal token
/// palette ADR 0018 asks for. Falls back to the default (unstyled)
/// foreground for a palette index this match doesn't special-case (there
/// are none today; `PALETTE`'s entries are all listed below, but keeping
/// this a `match` with a wildcard rather than a same-length array means
/// adding a `PALETTE` entry without a style here degrades to unstyled
/// rather than panicking on an out-of-bounds array index).
fn palette_style(palette_index: usize) -> Style {
    match PALETTE.get(palette_index).copied() {
        Some("keyword") => Style::default().fg(Color::Magenta),
        Some("string") => Style::default().fg(Color::Yellow),
        Some("comment") => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
        Some("function") => Style::default().fg(Color::Blue),
        Some("type") => Style::default().fg(Color::Cyan),
        Some("number") => Style::default().fg(Color::LightRed),
        Some("constant") => Style::default().fg(Color::LightRed),
        Some("property") => Style::default().fg(Color::LightBlue),
        Some("variable") => Style::default(),
        _ => Style::default(),
    }
}

/// Formats a [`DirDetail`] into displayable lines: a badge breakdown, its
/// own top fan-in symbols, and — only when this directory is in a cycle —
/// the partner directories and the concrete cross-directory edges forming
/// it (TUI iteration 2's answer to "cycle と言われても何が cycle してるか
/// 分からない").
///
/// `origin` picks the first badge's label: `Report::files`' symbol count is
/// exactly the same aggregation in both modes (`Badges::changed_symbols` is
/// not renamed — ADR 0017 only asks for the label to stop implying a diff),
/// but "changed symbols" would misdescribe a whole-repo outline the same
/// way `render.rs`'s "## Change graph"/"## Repository graph" split avoids
/// for Markdown.
fn dir_detail_lines(detail: &DirDetail, origin: ReportOrigin) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("Dir {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));
    let symbols_label = match origin {
        ReportOrigin::Diff => "changed symbols",
        ReportOrigin::RepoOutline => "symbols",
    };
    lines.push(Line::raw(format!(
        "{symbols_label}: {}",
        detail.badges.changed_symbols
    )));
    lines.push(Line::raw(format!(
        "contract changes: {}",
        detail.badges.contract_changes
    )));
    lines.push(Line::raw(format!("fan-in: {}", detail.badges.fan_in)));

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("Top fan-in ({})", detail.top_fan_in.len()),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for mention in &detail.top_fan_in {
        lines.push(Line::raw(format!("  {} ({})", mention.name, mention.path)));
    }

    if !detail.cycle_partners.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Cycles with",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        for partner in &detail.cycle_partners {
            lines.push(Line::raw(format!("  {partner}")));
        }

        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Cycle edges",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        for edge in &detail.cycle_edges {
            lines.push(Line::raw(format!("  {} -> {}", edge.from, edge.to)));
        }
    }

    lines
}

/// Formats a [`FileDetail`] into displayable lines: every symbol in this
/// file (changed symbols for a diff, every symbol for a whole-repo
/// outline — ADR 0017), with the same classification marker convention
/// `crate::row_view::entry_row_line` uses on symbol rows, plus fan-in. The
/// "Symbols (N)" label itself is already origin-neutral, unlike
/// `dir_detail_lines`'s first badge line, so no `origin` parameter is
/// needed here.
/// Formats a [`FileDetail`] into displayable lines: a `File <path>` header,
/// then either a skipped-file explanation, a whole-test-file explanation, or
/// the ordinary "Symbols (N)" listing — the three are mutually exclusive by
/// construction (`crate::tree::TreeNode`'s own doc comment on
/// `skip_reason`/`test_symbol_count`: an ordinary analyzed file has neither
/// set, a skipped file has no symbols to list, and a whole-test file has no
/// per-symbol data at all, only the count `pipeline::partition_test_symbols`
/// kept). Without this, a skipped or whole-test file's detail pane would
/// show a bare "Symbols (0)" — indistinguishable from a file that genuinely
/// changed nothing, which is exactly the gap this feature closes for the
/// entry-tree row too (see `row_view::entry_row_line`).
fn file_detail_lines(detail: &FileDetail) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("File {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));

    if let Some(reason) = detail.skip_reason {
        lines.push(Line::styled(
            format!(
                "Skipped: {}",
                rinkaku_core::render::skip_reason_label(reason)
            ),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::raw("rinkaku did not extract symbols from this file."));
        return lines;
    }

    if let Some(symbol_count) = detail.test_symbol_count {
        let noun = if symbol_count == 1 {
            "symbol"
        } else {
            "symbols"
        };
        lines.push(Line::styled(
            format!("Test file: {symbol_count} changed test {noun}"),
            Style::default().fg(Color::Magenta),
        ));
        lines.push(Line::raw(
            "Every changed symbol in this file is test code, excluded from the default view (see --include-tests).",
        ));
        return lines;
    }

    lines.push(Line::styled(
        format!("Symbols ({})", detail.symbols.len()),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for symbol in &detail.symbols {
        let marker = if symbol.removed {
            "x"
        } else {
            match symbol.classification {
                Some(Classification::Added) => "+",
                Some(Classification::SignatureChanged) => "~",
                Some(Classification::BodyOnly) | None => " ",
            }
        };
        let fan_in = if symbol.fan_in > 0 {
            format!(" ^{}", symbol.fan_in)
        } else {
            String::new()
        };
        lines.push(Line::raw(format!(
            "  {marker} {} {}{fan_in}",
            kind_abbrev(symbol.kind),
            symbol.name,
        )));
    }

    lines
}

fn kind_abbrev(kind: rinkaku_core::extract::SymbolKind) -> &'static str {
    use rinkaku_core::extract::SymbolKind;
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "iface",
        SymbolKind::TypeAlias => "type",
    }
}

/// Formats a [`DetailView`] into displayable lines: classification,
/// signature (a styled old/new diff when the contract changed, mirroring
/// `render.rs`'s Markdown ` ```diff ` block decision per
/// `crate::detail::SignatureView`'s own doc comment), used-by, callers,
/// callees.
fn detail_lines(detail: &DetailView) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            format!("{:?} ", detail.kind),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(detail.name.clone()),
    ]));
    lines.push(Line::raw(detail.path.clone()));
    if let Some(container) = &detail.container {
        lines.push(Line::raw(format!("in {container}")));
    }
    lines.push(Line::raw(""));

    if let Some(classification) = &detail.classification {
        lines.push(Line::raw(format!("classification: {classification:?}")));
    }

    lines.push(Line::raw(""));
    match &detail.signature {
        SignatureView::Current(signature) => {
            lines.push(Line::raw(signature.clone()));
        }
        SignatureView::Changed { previous, current } => {
            lines.push(Line::styled(
                format!("- {previous}"),
                Style::default().fg(Color::Red),
            ));
            lines.push(Line::styled(
                format!("+ {current}"),
                Style::default().fg(Color::Green),
            ));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("Used by ({})", detail.used_by.len()),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for mention in &detail.used_by {
        lines.push(Line::raw(format!("  {} ({})", mention.name, mention.path)));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("Callees ({})", detail.callees.len()),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for mention in &detail.callees {
        lines.push(Line::raw(format!("  {} ({})", mention.name, mention.path)));
    }

    lines
}

/// Draws the source drill-down for `symbol_id`. Re-reads the file on every
/// frame (via [`load_symbol_source`]) rather than caching the result on
/// `App` — a source file read is cheap relative to a terminal redraw, this
/// module has no cache-invalidation story of its own, and re-reading keeps
/// the view correct across a terminal resize (which redraws without a new
/// key event) without the event loop needing to distinguish "just entered
/// this screen" from "still on it". A read failure here is a fallback
/// display path only: `crate::run`'s event loop already attempts the same
/// read when the user first presses `s` and records a failure on `app`'s
/// status line (`App::set_status`) via that same code path, so a failure
/// mid-session (e.g. the file was deleted after opening the view) is
/// shown in the pane itself too, not just silently on the status line.
fn draw_source_screen(
    frame: &mut Frame,
    report: &Report,
    symbol_id: &str,
    repo_root: &std::path::Path,
    area: Rect,
) {
    let title = format!(" Source: {symbol_id} ");
    let block = Block::bordered().title(title);

    let source = match load_symbol_source(report, symbol_id, repo_root) {
        Ok(source) => source,
        Err(message) => {
            // `.wrap(Wrap { trim: false })`: the error message (full path +
            // io error + the "not present in the working tree" hint added
            // alongside repo-root resolution) routinely exceeds one line at
            // ordinary pane widths. Without wrapping, `Paragraph` silently
            // truncates instead of overflowing, cutting the hint off
            // exactly where it explains the failure. `trim: false` keeps
            // the message's own leading whitespace (there isn't any here,
            // but matches this pane's other `Paragraph` usages that don't
            // opt into trimming either).
            let paragraph = Paragraph::new(message)
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
            return;
        }
    };

    let viewport_height = area.height.saturating_sub(2) as usize; // borders
    let (start, end) = visible_window(
        source.lines.len(),
        source.highlight_start,
        source.highlight_end,
        viewport_height,
    );

    let lines = source_lines(&source, start, end);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn source_lines(source: &SourceView, start: usize, end: usize) -> Vec<Line<'static>> {
    source.lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, text)| {
            let line_number = start + offset + 1;
            let is_highlighted =
                line_number >= source.highlight_start && line_number <= source.highlight_end;
            let style = if is_highlighted {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Line::styled(format!("{line_number:>5} | {text}"), style)
        })
        .collect()
}

fn draw_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.screen() {
        Screen::Entry => {
            "j/k: move  enter/space: expand  e/c: expand/collapse  o: order  d: diff  p: pivot  J/K: scroll  s: source  q: quit"
        }
        Screen::Source { .. } => "esc/q: back",
    };

    let text = match app.status() {
        Some(status) => format!("{status}  |  {help}"),
        None => help.to_string(),
    };

    let style = if app.status().is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    frame.render_widget(Paragraph::new(text).style(style), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::FileReport;

    fn symbol(id: &str, name: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn report_with_one_symbol() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    /// A placeholder repo root for tests that don't exercise the source
    /// drill-down's actual file read (every `draw` test except the two
    /// `draw_source_screen` ones below) — `draw` still requires the
    /// parameter, but nothing under this path is ever read.
    fn test_repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from("/repo")
    }

    /// Flattens a `TestBackend`'s buffer into one string (rows joined by
    /// `\n`), so a snapshot assertion can check for expected substrings
    /// (pane titles, row content) without pinning every cell — the coarse
    /// check ADR 0016 asks for, not a pixel-perfect comparison.
    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn should_draw_entry_and_detail_panes_with_titles_on_entry_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Entry"));
        assert!(text.contains("Detail"));
        assert!(text.contains("lib.rs"));
        // The cursor starts on row 0, the "lib.rs" file row (TUI iteration
        // 2: a file row now renders its own `FileDetail` instead of the
        // "select a row" placeholder), so this coarse layout check confirms
        // the detail pane actually rendered file-detail content rather than
        // asserting on the placeholder text that used to show here.
        assert!(text.contains("Symbols"));
    }

    #[test]
    fn should_draw_placeholder_message_when_there_are_no_rows_at_all() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("select a row"));
    }

    #[test]
    fn should_draw_dir_detail_content_when_cursor_is_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Dir src"));
        assert!(text.contains("changed symbols:"));
        assert!(text.contains("Top fan-in"));
    }

    // ADR 0017: a whole-repo outline's directory detail must not say
    // "changed symbols" — nothing changed in that mode — so this pins
    // `dir_detail_lines`'s label switching on `report.origin`, using the
    // same report shape as
    // `should_draw_dir_detail_content_when_cursor_is_on_a_directory_row`
    // above (differing only in `origin`) so the two tests read as a pair.
    #[test]
    fn should_draw_symbols_label_without_changed_wording_when_origin_is_repo_outline() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::RepoOutline,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Dir src"));
        assert!(text.contains("symbols:"));
        assert!(!text.contains("changed symbols:"));
    }

    /// A [`Report`] whose only entry is a skipped file (no `files`, no
    /// `tests`) — pairs with `report_with_one_symbol` for the detail-pane
    /// tests below.
    fn report_with_one_skipped_file() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_draw_skip_reason_in_detail_pane_when_cursor_is_on_a_skipped_file_row() {
        let report = report_with_one_skipped_file();
        // Row 0 is the collapsing "assets" dir (single child, see
        // `crate::tree::build_tree`'s collapsing rule); row 1 is the
        // skipped "logo.png" file itself.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("File assets/logo.png"));
        assert!(text.contains("Skipped: binary"));
        assert!(!text.contains("Symbols ("));
    }

    /// A [`Report`] whose only entry is a whole-test-file summary (no
    /// `files`, no `skipped`).
    fn report_with_one_test_file() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![rinkaku_core::render::TestFileSummary {
                path: "src/lib_test.go".to_string(),
                symbol_count: 3,
            }],
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_draw_test_symbol_count_in_detail_pane_when_cursor_is_on_a_whole_test_file_row() {
        let report = report_with_one_test_file();
        // Row 0 is the collapsing "src" dir; row 1 is the whole test file.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("File src/lib_test.go"));
        // "Test file: 3 changed test symbols" wraps across two rendered
        // lines at this terminal's pane width, so assert on a substring
        // that survives the wrap rather than the whole phrase.
        assert!(text.contains("Test file: 3 changed test"));
        assert!(!text.contains("Symbols ("));
    }

    #[test]
    fn should_draw_diff_pane_with_hunk_lines_when_toggled_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Diff"));
        assert!(text.contains("+fn foo() {}"));
    }

    // Dynamic-verification note (see CLAUDE.md's reviewing-changes
    // section): this pins that a skipped file's diff pane still resolves
    // real hunks from the raw diff text — `App::selected_diff_target`
    // scopes a `NodeKind::File` row to `DiffTarget::File { path }`
    // regardless of `skip_reason` (see the `app.rs` unit test
    // `should_return_file_diff_target_when_cursor_is_on_a_skipped_file_row`),
    // and `draw_diff_pane` looks hunks up by that path alone — so a
    // skipped file (which has no `FileReport`/symbols to key off of) must
    // not silently fall back to the "no diff hunks found" placeholder just
    // because rinkaku didn't extract symbols from it.
    #[test]
    fn should_draw_diff_pane_with_hunk_lines_when_toggled_on_a_skipped_file_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
            files: vec![],
        };
        // Row 0 is the collapsing "assets" dir; row 1 is the skipped file.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // A binary file has no hunks at all in the diff text itself (git
        // reports "Binary files ... differ" instead of `@@` hunks), so the
        // correct, honest behavior is the pane's own "no diff hunks found"
        // placeholder — this test's real assertion is that it names the
        // right path, not a stale/mismatched one, confirming the lookup
        // reached this row's `path` at all. Checked as two substrings
        // rather than the whole phrase since it wraps across rendered
        // lines at this terminal's pane width.
        assert!(text.contains("no diff hunks found for"));
        assert!(text.contains("assets/logo.png"));
    }

    /// Sibling of the binary-skip test above, using an unsupported-language
    /// skip (a real text file with real hunks in the raw diff) to confirm
    /// the diff pane actually renders content — not just the placeholder —
    /// for a skipped-but-textual file.
    #[test]
    fn should_draw_diff_pane_with_hunk_lines_for_an_unsupported_language_skipped_file() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "vendor/lib.zig".to_string(),
                reason: rinkaku_core::render::SkipReason::UnsupportedLanguage,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
            files: vec![],
        };
        // Row 0 is the collapsing "vendor" dir; row 1 is the skipped file.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/vendor/lib.zig b/vendor/lib.zig
index e69de29..4b825dc 100644
--- a/vendor/lib.zig
+++ b/vendor/lib.zig
@@ -1,1 +1,2 @@
 const a = 1;
+const b = 2;
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Diff"));
        assert!(text.contains("+const b = 2;"));
    }

    #[test]
    fn should_draw_pivot_pane_with_tree_lines_when_toggled_on_a_file_row() {
        // Same fixture shape as `report_with_one_symbol`, but with a graph
        // actually populated (that fixture leaves `graph` empty since most
        // of this module's tests don't need one) so pivoting on "lib.rs"
        // yields a real `PivotSelection::View` instead of `Empty`.
        let report = Report {
            graph: SymbolGraph {
                nodes: vec![rinkaku_core::graph::Node {
                    id: "lib.rs::foo".to_string(),
                    path: "lib.rs".to_string(),
                    name: "foo".to_string(),
                }],
                edges: vec![],
                roots: vec!["lib.rs::foo".to_string()],
            },
            ..report_with_one_symbol()
        };
        // Row 0 is the "lib.rs" file row itself (cursor starts there).
        let app = App::new(&report).handle_key(crate::app::InputKey::TogglePivot);
        // `crate::run_app` computes this once per handled key and hands it
        // into `draw` (see `draw`'s own doc comment on why `draw` itself
        // must not call `App::selected_pivot_view`) — this test recreates
        // that same one-shot computation rather than a per-frame one.
        let pivot_selection = app.selected_pivot_view(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &pivot_selection,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Pivot"));
        assert!(text.contains("fn foo (lib.rs)"));
    }

    #[test]
    fn should_draw_pivot_placeholder_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::TogglePivot);
        let pivot_selection = app.selected_pivot_view(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &pivot_selection,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Pivot"));
        // Not the full placeholder sentence: the pane is narrow enough
        // (40% of an 80-column terminal) that `Paragraph`'s word-wrapping
        // splits it across rows, and `buffer_text` joins rows with `\n` —
        // a multi-row substring would never match. "directory" alone fits
        // on one wrapped line and is unique enough to confirm the
        // placeholder rendered, mirroring this module's other coarse
        // fragment checks (e.g. `should_draw_entry_and_detail_panes_...`'s
        // "Symbols" check).
        assert!(text.contains("directory"));
    }

    /// Finds the buffer cell for `token`'s first character within the row
    /// that contains `line_needle`, scanning row by row — used by the
    /// highlight tests below to inspect a specific token's actual `Style`
    /// (`buffer_text` only exposes glyphs, not styling). `line_needle`
    /// disambiguates which row to sample when `token` alone could match
    /// more than one (e.g. the left tree pane's cursor row also happens to
    /// render a truncated "fn foo" label for this test module's one-symbol
    /// fixture).
    ///
    /// Deliberately indexes by *character* position, not `str::find`'s byte
    /// offset: this pane's border glyphs (`│`) are multi-byte UTF-8, so a
    /// byte offset into the flattened row string does not line up with the
    /// buffer's `x` column once even one border character precedes the
    /// match — using `char_indices`/`chars().count()` keeps this aligned
    /// with the single-width-per-cell column space `TestBackend` itself
    /// uses (every char in this test module's fixtures is single-width
    /// ASCII, so column count and char count coincide).
    fn find_cell_style(terminal: &Terminal<TestBackend>, line_needle: &str, token: &str) -> Style {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        for y in 0..area.height {
            let row: String = (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            let Some(needle_byte_offset) = row.find(line_needle) else {
                continue;
            };
            // Search for `token` starting from `line_needle`'s own match,
            // not row-wide: this pane's two side-by-side panes can each
            // contain `token` (e.g. the left tree pane's cursor row renders
            // a truncated "fn foo" label that also contains "fn"), so a
            // row-wide `find` could resolve to the wrong pane entirely.
            let Some(token_byte_offset) = row[needle_byte_offset..].find(token) else {
                continue;
            };
            let byte_offset = needle_byte_offset + token_byte_offset;
            // Convert the byte offset `str::find` returned into a char
            // (= column) index by counting chars before it — this pane's
            // border glyphs (`│`) are multi-byte UTF-8, so the byte offset
            // itself does not line up with the buffer's `x` column once
            // even one border character precedes the match.
            let column = row[..byte_offset].chars().count() as u16;
            return buffer[(column, y)].style();
        }
        panic!("expected to find {token:?} within a row containing {line_needle:?}");
    }

    #[test]
    fn should_apply_added_background_tint_and_keyword_foreground_in_diff_pane() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        // The added line's "fn" keyword: foreground colored by the keyword
        // palette entry, background tinted with `ADDED_BG` — both signals
        // present on the same cell, per ADR 0018's "fg is token color, bg is
        // diff signal" decision. Disambiguated against the row via
        // "+fn foo() {}" (the marker plus full added line): the left-hand
        // tree pane's cursor row also happens to render a truncated "fn
        // foo" label for this fixture's one symbol.
        let keyword_style = find_cell_style(&terminal, "+fn foo() {}", "fn");
        assert_eq!(Some(ADDED_BG), keyword_style.bg);
        assert_eq!(Some(Color::Magenta), keyword_style.fg);
    }

    #[test]
    fn should_apply_removed_background_tint_in_diff_pane() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,1 @@
 fn a() {}
-fn foo() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let keyword_style = find_cell_style(&terminal, "-fn foo() {}", "fn");
        assert_eq!(Some(REMOVED_BG), keyword_style.bg);
        assert_eq!(Some(Color::Magenta), keyword_style.fg);
    }

    #[test]
    fn should_keep_context_line_unstyled_background_in_diff_pane() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        // Context line "fn a() {}" keeps its keyword token color but must
        // not carry either diff background tint (`Style::bg` reports an
        // unset background as `Some(Color::Reset)`, not `None` — ratatui's
        // own `Cell` defaults every cell's `bg` field to `Color::Reset`
        // rather than leaving it absent). Disambiguated the same way as
        // the added-line test above (a leading space marker rather than
        // `+`/`-`, matching `diff_line`'s context-line rendering).
        let context_style = find_cell_style(&terminal, " fn a() {}", "fn");
        assert_eq!(Some(Color::Reset), context_style.bg);
        assert_eq!(Some(Color::Magenta), context_style.fg);
    }

    #[test]
    fn should_keep_hunk_header_dim_when_diff_pane_is_highlighted() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let header_style = find_cell_style(&terminal, "@@ -1,1 +1,2 @@", "@@");
        assert_eq!(Some(Color::DarkGray), header_style.fg);
        assert!(header_style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn should_fall_back_to_plain_diff_style_when_file_extension_is_unrecognized() {
        // A symbol whose path has no known extension (mirrors an unbuilt
        // language, e.g. YAML): `App::selected_diff_target` reads the path
        // straight off the symbol/file row, so this only needs a report
        // whose file path is unrecognized by `highlight::config_for_path`,
        // not a real diff for an actual YAML grammar.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "config.yaml".to_string(),
                symbols: vec![symbol("config.yaml::foo", "foo")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
        let diff_text = "\
diff --git a/config.yaml b/config.yaml
index e69de29..4b825dc 100644
--- a/config.yaml
+++ b/config.yaml
@@ -1,1 +1,2 @@
 a: 1
+b: 2
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_files,
                    &diff_highlights,
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("+b: 2"));

        // Falls back to the pane's original plain green foreground with no
        // background tint at all (`Some(Color::Reset)` is ratatui's
        // "unset" — see the context-line test above for why this isn't
        // `None`) — highlighting failing (or, here, never applying) must
        // never break the pre-existing diff styling.
        let added_style = find_cell_style(&terminal, "+b: 2", "b");
        assert_eq!(Some(Color::Reset), added_style.bg);
        assert_eq!(Some(Color::Green), added_style.fg);
    }

    #[test]
    fn should_draw_detail_pane_content_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("foo"));
        assert!(text.contains("Used by"));
    }

    #[test]
    fn should_draw_status_line_help_text_on_entry_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        // Wider than the default 80 columns used elsewhere in this test
        // module: the full help text (now including the J/K scroll hint and
        // the `p: pivot` hint, ADR 0019) is ~114 columns and would
        // otherwise be truncated (the status line intentionally does not
        // wrap), hiding the "quit" fragment this test checks for.
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("quit"));
    }

    #[test]
    fn should_draw_source_screen_title_and_error_message_when_file_is_missing() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::Source);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        // "lib.rs" does not exist under `test_repo_root()`'s placeholder
        // path, so this exercises `draw_source_screen`'s error-message
        // fallback path rather than needing a real file on disk.
        let text = buffer_text(&terminal);
        assert!(text.contains("Source: lib.rs::foo"));
        assert!(text.contains("failed to read"));
        assert!(text.contains("back"));
    }

    #[test]
    fn should_wrap_source_error_message_instead_of_truncating_it_in_a_narrow_pane() {
        // Regression test: `source::load_symbol_source`'s error message
        // (full path + io error + the "not present in the working tree"
        // hint) routinely exceeds one line, and `Paragraph` without
        // `.wrap(...)` silently truncates rather than overflowing — cutting
        // the hint off exactly where it explains the failure. A narrow
        // (40-column) pane makes the message wrap across multiple rows
        // whether or not `.wrap(...)` is set, but only *with* it does the
        // hint's text actually appear anywhere in the buffer; without it,
        // the trailing text is simply dropped.
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::Source);
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        // `buffer_text` joins rows with `\n`, so a phrase that happens to
        // wrap exactly at a row boundary (as "in the" / "present" do at
        // this width) would not appear as one contiguous substring even
        // though every word is visible — asserting on words rather than a
        // multi-word phrase keeps this test robust to exactly where the
        // wrap point falls, while still failing if `.wrap(...)` were
        // removed (the words after "working tree" would be dropped
        // entirely, not just split across rows).
        let text = buffer_text(&terminal);
        assert!(text.contains("Source: lib.rs::foo"));
        assert!(text.contains("present"));
        assert!(text.contains("working tree"));
        assert!(text.contains("historical commit not checked out"));
        assert!(text.contains("locally)"));
    }

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

    // --- rendered scroll behavior (TestBackend) ---

    /// A report whose single file has `count` symbols, each referencing
    /// `report_with_one_symbol`'s pattern but repeated enough times that
    /// `file_detail_lines` produces more lines than a typical test
    /// viewport's height — used to exercise `draw_detail_pane`'s scrolling
    /// and overflow-indicator paths, which need content that does not fit
    /// in one screen.
    fn report_with_many_symbols(count: usize) -> Report {
        let symbols: Vec<ExtractedSymbol> = (0..count)
            .map(|i| symbol(&format!("lib.rs::sym{i}"), &format!("sym{i}")))
            .collect();
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols,
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_show_overflow_indicator_in_detail_pane_title_when_content_exceeds_viewport() {
        // Row 0 is the "lib.rs" file row itself: `file_detail_lines` lists
        // a "File lib.rs" header, a blank line, a "Symbols (40)" header,
        // then all 40 symbols (43 lines total) — comfortably more than a
        // 20-row terminal's inner pane height can show at once.
        let report = report_with_many_symbols(40);
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // Exact bounds depend on the pane's inner height (20 - 2 for the
        // status line/border layout), so this only pins the shape/start
        // rather than the literal end number, keeping the test robust to
        // an unrelated layout tweak elsewhere in this module.
        assert!(text.contains("Detail (1-"));
        assert!(text.contains("/43)"));
    }

    #[test]
    fn should_not_show_overflow_indicator_when_content_fits_the_viewport() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains(" Detail "));
        assert!(!text.contains("Detail ("));
    }

    #[test]
    fn should_scroll_detail_pane_content_down_when_scroll_down_is_pressed() {
        let report = report_with_many_symbols(40);
        let app = App::new(&report).handle_key(crate::app::InputKey::ScrollDown);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // One line scrolled down: the first visible content line is now 2
        // instead of 1, and the "File lib.rs" header line (the very first
        // content line, before the two blank/"Symbols (40)" header lines
        // that precede the actual symbol list) has scrolled out of view.
        assert!(text.contains("Detail (2-"));
        assert!(!text.contains("File lib.rs"));
    }

    #[test]
    fn should_clamp_detail_pane_scroll_at_the_last_page() {
        // Request an enormous scroll far past the end of a 40-symbol
        // report; the pane must clamp to its last full page rather than
        // showing a mostly-blank pane past the end of the content.
        let report = report_with_many_symbols(40);
        let mut app = App::new(&report);
        for _ in 0..1000 {
            app = app.handle_key(crate::app::InputKey::ScrollDown);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // The last symbol must be visible once clamped to the final page.
        assert!(text.contains("sym39"));
    }

    #[test]
    fn should_reset_scroll_indicator_when_cursor_moves_to_a_different_row() {
        // Scroll down on the file row's detail, then move the cursor onto
        // a symbol row: `App::handle_key`'s reset-on-cursor-move rule means
        // the newly selected row's own (short) detail must render from the
        // top, not carry over the file row's scroll offset.
        let report = report_with_many_symbols(40);
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::ScrollDown)
            .handle_key(crate::app::InputKey::ScrollDown)
            .handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // A single symbol's own detail (used-by/callees, both empty here)
        // fits well within the viewport, so no overflow indicator should
        // appear even though the file row's detail definitely overflowed.
        assert!(text.contains(" Detail "));
        assert!(!text.contains("Detail ("));
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

    // --- long-line scroll reachability regression (TestBackend) ---

    #[test]
    fn should_reach_the_last_wrapped_line_of_content_via_scrolling_when_a_logical_line_is_long_enough_to_wrap()
     {
        // A narrow pane (30 inner columns after the 2-column border) with a
        // single logical line far longer than that — mirrors a real fan-in
        // entry's full path being too long for the pane. Before wrapping was
        // applied before the scroll offset, the scroll unit (logical lines)
        // and the render unit (wrapped rows) disagreed, so a marker placed
        // near the end of this one long logical line was unreachable at any
        // scroll offset. Regression coverage for that desync.
        let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: long_line.clone(),
                symbols: vec![],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let mut app = App::new(&report);
        // Scroll far enough down to reach the wrapped tail of the long path
        // line, however many wrapped rows that turns out to be.
        for _ in 0..200 {
            app = app.handle_key(crate::app::InputKey::ScrollDown);
        }
        let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("TAIL_MARKER"));
    }

    #[test]
    fn should_report_indicator_total_as_wrapped_row_count_not_logical_line_count_when_a_line_wraps()
    {
        // Same narrow pane/long-path setup as the reachability regression
        // above: the file row's detail is exactly 2 logical lines ("File
        // <path>" plus a blank line, since this report has no symbols), but
        // the long path line wraps into several rows — the indicator's
        // "/total" must count wrapped rows, not the 2 logical lines, or the
        // indicator would (wrongly) claim everything fits and hide it
        // entirely.
        let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: long_line,
                symbols: vec![],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        };
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &[],
                    &[],
                    &PivotSelection::NotApplicable,
                    &test_repo_root(),
                )
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // Inner width is 34 - 2 = 32 columns; the long line alone wraps into
        // ceil(211 / 32) = 7 rows, well over the "/2" a logical-line count
        // would have produced.
        assert!(text.contains("Detail (1-"));
        assert!(!text.contains("/2)"));
    }

    #[test]
    fn should_map_every_palette_entry_to_its_pinned_style_when_resolved_by_index() {
        // Pins the full palette-index → style table: `palette_style` falls
        // back to unstyled on an unmapped name, so dropping one arm during
        // a future palette edit would otherwise pass `make test` silently.
        let expected = vec![
            ("keyword", Style::default().fg(Color::Magenta)),
            ("string", Style::default().fg(Color::Yellow)),
            (
                "comment",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            ("function", Style::default().fg(Color::Blue)),
            ("type", Style::default().fg(Color::Cyan)),
            ("number", Style::default().fg(Color::LightRed)),
            ("constant", Style::default().fg(Color::LightRed)),
            ("property", Style::default().fg(Color::LightBlue)),
            ("variable", Style::default()),
        ];

        let actual: Vec<(&str, Style)> = crate::highlight::PALETTE
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, palette_style(index)))
            .collect();

        assert_eq!(expected, actual);
    }
}
