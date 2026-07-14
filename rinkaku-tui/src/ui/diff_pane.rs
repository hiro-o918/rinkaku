//! Diff right-pane (TUI iteration 2, [`crate::app::RightPane::Diff`]; ADR
//! 0020 reshapes its content): the raw unified-diff hunks touching the row
//! under the cursor, either clipped to a symbol row's own line range or
//! grouped into per-symbol sections for a file row.

use super::scroll::{render_scrollable_pane, truncate_to_width_keeping_tail};
use super::style::{pane_border_style, styled_content_spans};
use crate::app::{App, DiffTarget, Focus};
use crate::diff_shape::{ChangeStats, DiffSection};
use crate::diff_view::{DiffLine, DiffLineKind};
use crate::highlight::{self, HighlightedFile, TokenSpan};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::render::Report;

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

/// Builds the Diff pane's 2-line identification/stats header — pinned above
/// the scrollable hunk content by `render_scrollable_pane`'s `header_lines`
/// parameter so it never scrolls out of view, since knowing *which* symbol
/// or file is being read stays useful no matter how far into its hunks the
/// reviewer has scrolled.
///
/// Line 1 identifies the selection: `"<symbol name> · <path>"` for a symbol
/// row (`·` chosen over `::`/`/`, both of which collide with characters
/// that already appear inside a symbol name or path), or the bare `path`
/// for a file/skipped-file row
/// (`selection_name` is `None` there — nothing to pair the path with). The
/// whole line is truncated from the *head* when it overflows `width`
/// ([`truncate_to_width_keeping_tail`]): the tail — the symbol's own name,
/// or a path's basename — is what tells the two apart when many files share
/// leading directories, so that is the part kept visible.
///
/// Line 2 reports `stats` as `"chg: <ranges> (+A/-R)"`, omitted entirely
/// when `stats` has no ranges and no added/removed lines to show (an empty
/// selection already took the placeholder path in [`draw_diff_pane`], so
/// this only happens for a selection whose hunks this fold could not
/// attribute a range to — better to show nothing than a misleadingly empty
/// `"chg: "` line).
pub(crate) fn diff_pane_header_lines(
    selection_name: Option<&str>,
    path: &str,
    stats: &ChangeStats,
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

    if !stats.ranges.is_empty() || stats.added > 0 || stats.removed > 0 {
        let ranges = stats
            .ranges
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
        let counts = format!("(+{}/-{})", stats.added, stats.removed);
        let stats_text = if ranges.is_empty() {
            format!("chg: {counts}")
        } else {
            format!("chg: {ranges} {counts}")
        };
        lines.push(Line::styled(
            truncate_to_width_keeping_tail(&stats_text, width),
            Style::default().add_modifier(Modifier::DIM),
        ));
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
    // ADR 0027: `DiffPaneContent` no longer has a symbol-clip variant, so
    // the diff pane always renders with section headers on. `diff_pane_lines`'s
    // `show_section_headers` parameter is now always `true` at this call
    // site, kept as a parameter to leave that layout knob visible in one
    // place rather than hard-coding it inside `diff_pane_lines`.
    let lines = diff_pane_lines(&sections, true, highlighted_file);

    // A symbol row's stats scope to that symbol's own section only; a file
    // row (no `selected_diff_focus`) scopes to every section in the file —
    // mirrors `App::selected_diff_target`'s own file-vs-symbol row scoping.
    let focus = app.selected_diff_focus(report);
    let stats_sections: Vec<&crate::diff_shape::DiffSection> = match &focus {
        Some(focus) => sections
            .iter()
            .filter(|section| section.symbol_id.as_deref() == Some(focus.symbol_id.as_str()))
            .copied()
            .collect(),
        None => sections.clone(),
    };
    let stats = crate::diff_shape::change_stats(&stats_sections);
    let header_width = area.width.saturating_sub(2) as usize;
    // `selected_diff_title_name` returns the row's *path* itself on a file
    // row (its own doc comment) — reusing it as the header's symbol-name
    // pairing would duplicate the path against itself, so it is only passed
    // through for an actual symbol row (`focus.is_some()`).
    let selection_name = if focus.is_some() {
        app.selected_diff_title_name()
    } else {
        None
    };
    let header_lines = diff_pane_header_lines(selection_name, path, &stats, header_width);

    Some(render_scrollable_pane(
        frame,
        DIFF_PANE_TITLE,
        &header_lines,
        &lines,
        app.right_pane_scroll(),
        area,
        focused,
    ))
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
mod tests {
    use super::*;
    use crate::app::{App, BlastRadiusSelection};
    use crate::ui::draw;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{Classification, ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::FileReport;

    // --- diff_pane_header_lines (pure helper) ---

    #[test]
    fn should_join_symbol_name_and_path_on_first_header_line_when_selection_name_is_present() {
        let stats = ChangeStats::default();

        let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

        assert_eq!(
            vec![Line::styled(
                "foo · src/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            )],
            actual
        );
    }

    #[test]
    fn should_show_bare_path_on_first_header_line_when_no_selection_name() {
        let stats = ChangeStats::default();

        let actual = diff_pane_header_lines(None, "src/lib.rs", &stats, 80);

        assert_eq!(
            vec![Line::styled(
                "src/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            )],
            actual
        );
    }

    #[test]
    fn should_add_change_stats_line_when_stats_has_ranges_and_counts() {
        let stats = ChangeStats {
            ranges: vec![(23, 41), (57, 60)],
            added: 18,
            removed: 4,
        };

        let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

        assert_eq!(
            vec![
                Line::styled(
                    "foo · src/lib.rs".to_string(),
                    Style::default().add_modifier(Modifier::BOLD)
                ),
                Line::styled(
                    "chg: 23-41, 57-60 (+18/-4)".to_string(),
                    Style::default().add_modifier(Modifier::DIM)
                ),
            ],
            actual
        );
    }

    #[test]
    fn should_format_single_line_range_without_a_dash() {
        let stats = ChangeStats {
            ranges: vec![(5, 5)],
            added: 1,
            removed: 0,
        };

        let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

        assert_eq!(
            vec![
                Line::styled(
                    "foo · src/lib.rs".to_string(),
                    Style::default().add_modifier(Modifier::BOLD)
                ),
                Line::styled(
                    "chg: 5 (+1/-0)".to_string(),
                    Style::default().add_modifier(Modifier::DIM)
                ),
            ],
            actual
        );
    }

    #[test]
    fn should_omit_change_stats_line_when_stats_is_entirely_empty() {
        let stats = ChangeStats::default();

        let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

        assert_eq!(1, actual.len());
    }

    #[test]
    fn should_show_counts_without_ranges_when_ranges_is_empty_but_counts_are_nonzero() {
        // A pure-deletion selection: `ChangeStats::ranges` excludes the
        // zero-width deletion range (`change_stats`'s own doc comment), but
        // the removed count is still real and worth reporting.
        let stats = ChangeStats {
            ranges: vec![],
            added: 0,
            removed: 2,
        };

        let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

        assert_eq!(
            vec![
                Line::styled(
                    "foo · src/lib.rs".to_string(),
                    Style::default().add_modifier(Modifier::BOLD)
                ),
                Line::styled(
                    "chg: (+0/-2)".to_string(),
                    Style::default().add_modifier(Modifier::DIM)
                ),
            ],
            actual
        );
    }

    #[test]
    fn should_truncate_first_header_line_keeping_the_tail_when_it_overflows_width() {
        let stats = ChangeStats::default();

        let actual = diff_pane_header_lines(
            Some("very_long_symbol_name_here"),
            "src/very/deeply/nested/module/lib.rs",
            &stats,
            20,
        );

        assert_eq!(
            vec![Line::styled(
                "…ested/module/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            )],
            actual
        );
    }

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
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        }
    }

    fn diff_content_for(
        report: &Report,
        diff_files: &[crate::diff_view::FileHunks],
        app: &App,
    ) -> crate::diff_shape::DiffPaneContent {
        crate::diff_shape::build_diff_pane_content(
            report,
            diff_files,
            app.selected_diff_target(report).as_ref(),
        )
    }

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
            let Some(token_byte_offset) = row[needle_byte_offset..].find(token) else {
                continue;
            };
            let byte_offset = needle_byte_offset + token_byte_offset;
            let column = row[..byte_offset].chars().count() as u16;
            return buffer[(column, y)].style();
        }
        panic!("expected to find {token:?} within a row containing {line_needle:?}");
    }

    #[test]
    fn should_draw_diff_pane_with_hunk_lines_when_toggled_on_a_symbol_row() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
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
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
            files: vec![],
        };
        // Row 0 is the collapsing "assets" dir; row 1 is the skipped file.
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let diff_text = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
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
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
            files: vec![],
        };
        // Row 0 is the collapsing "vendor" dir; row 1 is the skipped file.
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Diff"));
        assert!(text.contains("+const b = 2;"));
    }

    #[test]
    fn should_draw_per_symbol_section_headers_when_diff_pane_shows_a_file_selection() {
        // Cursor stays on row 0, the "lib.rs" file row itself — a file
        // selection (ADR 0020) groups hunks under each symbol's own
        // signature as a section header, unlike a symbol selection (the
        // sibling test above), which shows no header at all.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::foo", "foo"),
                    ExtractedSymbol {
                        range: LineRange { start: 10, end: 10 },
                        ..symbol("lib.rs::bar", "bar")
                    },
                ],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };
        let app = App::new(&report);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn a() {}
+fn foo() {}
@@ -9,1 +10,1 @@
-fn old_bar() {}
+fn bar() {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("fn foo()"));
        assert!(text.contains("fn bar()"));
        assert!(text.contains("+fn foo() {}"));
        assert!(text.contains("+fn bar() {}"));
    }

    #[test]
    fn should_draw_contract_header_before_hunks_when_symbol_signature_changed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    previous_signature: Some("fn foo(a: i32)".to_string()),
                    signature: "fn foo(a: i32, b: i32)".to_string(),
                    ..symbol("lib.rs::foo", "foo")
                }],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };
        // Row 0 is the "lib.rs" file row, row 1 is the "foo" symbol.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn foo(a: i32) {}
+fn foo(a: i32, b: i32) {}
";
        let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
        let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // The 2-line old/new contract header precedes the hunk body itself
        // (ADR 0020's outline-before-implementation disclosure order).
        assert!(text.contains("- fn foo(a: i32)"));
        assert!(text.contains("+ fn foo(a: i32, b: i32)"));
        assert!(text.contains("-fn foo(a: i32) {}"));
        assert!(text.contains("+fn foo(a: i32, b: i32) {}"));
    }

    #[test]
    fn should_apply_added_background_tint_and_keyword_foreground_in_diff_pane() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
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
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let keyword_style = find_cell_style(&terminal, "-fn foo() {}", "fn");
        assert_eq!(Some(REMOVED_BG), keyword_style.bg);
        assert_eq!(Some(Color::Magenta), keyword_style.fg);
    }

    #[test]
    fn should_keep_context_line_unstyled_background_in_diff_pane() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
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
    fn should_keep_hunk_header_dark_gray_when_diff_pane_is_highlighted() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let header_style = find_cell_style(&terminal, "@@ -1,1 +1,2 @@", "@@");
        assert_eq!(Some(Color::DarkGray), header_style.fg);
        // DarkGray alone gives sufficient contrast; stacking `Modifier::DIM`
        // on top of it double-dims the header to near-invisibility on many
        // terminal themes (especially light backgrounds).
        assert!(!header_style.add_modifier.contains(Modifier::DIM));
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
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };
        // ADR 0020 defaults the right pane to Diff already, so no
        // `ToggleDiff` press is needed to reach it here.
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
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
        let diff_content = diff_content_for(&report, &diff_files, &app);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &diff_content,
                    &diff_highlights,
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
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
}
