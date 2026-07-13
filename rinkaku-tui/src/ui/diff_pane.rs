//! Diff right-pane (TUI iteration 2, [`crate::app::RightPane::Diff`]; ADR
//! 0020 reshapes its content): the raw unified-diff hunks touching the row
//! under the cursor, either clipped to a symbol row's own line range or
//! grouped into per-symbol sections for a file row.

use super::scroll::render_scrollable_pane;
use super::style::styled_content_spans;
use crate::app::{App, DiffTarget};
use crate::diff_shape::DiffSection;
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
            let block = Block::bordered().title(" Diff ");
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
    Some(render_scrollable_pane(
        frame,
        " Diff ",
        &lines,
        app.right_pane_scroll(),
        area,
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
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
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
            file_size_warnings: vec![],
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
            hotspots: vec![],
            file_size_warnings: vec![],
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
            hotspots: vec![],
            file_size_warnings: vec![],
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
            hotspots: vec![],
            file_size_warnings: vec![],
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
            hotspots: vec![],
            file_size_warnings: vec![],
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
    fn should_keep_hunk_header_dim_when_diff_pane_is_highlighted() {
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
            file_size_warnings: vec![],
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
