//! `?` help overlay and jump-target popup (ADR 0020, ADR 0022) —
//! composited on top of whatever screen was already rendered underneath,
//! after the pane split has drawn everything else.

use super::scroll::{render_scrollable_pane, truncate_to_width, windowed_rows_with_indicators};
use crate::row_view::{
    band_style, cyan_badge_style, risk_marker_style, split_badge_style, symbol_marker_span,
    symbol_name_style, test_badge_style, warning_badge_style,
};
use crate::tree::SymbolRef;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::file_size::FileSizeBand;

/// The `?` help overlay's content laid out once, independent of the pane's
/// rendered size — extracted from [`draw_help_overlay`] so tests can pin
/// its shape without a live `Frame`, mirroring how `crate::help::HELP_CONTENT`
/// itself is already plain data rather than something computed at draw time.
fn help_overlay_lines() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for group in crate::help::HELP_CONTENT.keymap_groups {
        lines.push(Line::styled(
            group.title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        for binding in group.bindings {
            lines.push(Line::raw(format!(
                "  {:<16} {}",
                binding.keys, binding.description
            )));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "Markers",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.extend(markers_legend_lines());
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Glossary",
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for entry in crate::help::HELP_CONTENT.glossary {
        lines.push(Line::raw(format!(
            "  {:<16} {}",
            entry.term, entry.explanation
        )));
    }
    lines
}

/// One [`SymbolRef`] per marker case the Markers legend needs a real
/// [`crate::row_view::symbol_marker_span`]/[`crate::row_view::symbol_name_style`]
/// swatch for — fields left at their "no signal" default except the one
/// this case is about, mirroring the minimal fixtures `row_view`'s own
/// tests already build (`row_view_tests::plain_symbol`).
fn legend_symbol(
    classification: Option<Classification>,
    removed: bool,
    is_test: bool,
) -> SymbolRef {
    SymbolRef {
        id: "legend".to_string(),
        name: "legend".to_string(),
        kind: SymbolKind::Function,
        classification,
        removed,
        is_test,
    }
}

const MARKER_SWATCH_COLUMN_WIDTH: usize = 40;

/// Builds the Markers section's lines: one row per
/// [`crate::help::HELP_CONTENT`]'s `markers` legend entry, its swatch
/// rendered with the exact [`ratatui::style::Style`]
/// `crate::row_view::entry_row_line` itself would use — a real style, not a
/// prose color name — followed by the entry's explanation. Extracted as its
/// own pure function, mirroring [`help_overlay_lines`]'s own split, so a
/// test can assert on the built `Vec<Line>` without a live `Frame`.
fn markers_legend_lines() -> Vec<Line<'static>> {
    crate::help::HELP_CONTENT
        .markers
        .iter()
        .map(|entry| {
            let swatch = marker_swatch_spans(entry.swatch);
            let swatch_width: usize = swatch.iter().map(|span| span.content.len()).sum();
            let padding = MARKER_SWATCH_COLUMN_WIDTH
                .saturating_sub(swatch_width)
                .max(1);
            let mut spans = vec![Span::raw("  ")];
            spans.extend(swatch);
            spans.push(Span::raw(format!(
                "{}{}",
                " ".repeat(padding),
                entry.explanation
            )));
            Line::from(spans)
        })
        .collect()
}

/// Looks up the real style(s) for one [`crate::help::MarkerLegendEntry::swatch`]
/// value, reusing `crate::row_view`'s own style producers so the legend can
/// never drift from what the tree pane actually renders.
fn marker_swatch_spans(swatch: &'static str) -> Vec<Span<'static>> {
    match swatch {
        "+" => vec![symbol_marker_span(&legend_symbol(
            Some(Classification::Added),
            false,
            false,
        ))],
        "~" => vec![symbol_marker_span(&legend_symbol(
            Some(Classification::SignatureChanged),
            false,
            false,
        ))],
        "x" => vec![symbol_marker_span(&legend_symbol(None, true, false))],
        "(dimmed name)" => vec![Span::styled(
            swatch,
            symbol_name_style(&legend_symbol(Some(Classification::BodyOnly), false, false)),
        )],
        "(dimmed + struck-through name)" => vec![Span::styled(
            swatch,
            symbol_name_style(&legend_symbol(None, true, false)),
        )],
        "(cycle)" => vec![Span::styled(
            swatch,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )],
        "!" => vec![Span::styled(swatch, risk_marker_style())],
        "lines:N" => vec![Span::styled(swatch, band_style(FileSizeBand::Watch))],
        "chg:N" => badge_swatch_spans("chg:", cyan_badge_style()),
        "api:N" => badge_swatch_spans("api:", warning_badge_style()),
        "fan-in:N" => badge_swatch_spans("fan-in:", cyan_badge_style()),
        "warn:N" => badge_swatch_spans("warn:", warning_badge_style()),
        "split:N" => badge_swatch_spans("split:", split_badge_style()),
        "[test] (N symbols)" => vec![Span::styled(swatch, test_badge_style())],
        "N tests" => vec![Span::styled(swatch, Style::default().fg(Color::DarkGray))],
        "(skipped: ...)" => vec![Span::styled(swatch, Style::default().fg(Color::DarkGray))],
        _ => vec![Span::raw(swatch)],
    }
}

/// A `label:N` badge swatch split into a plain label span and an `N`
/// numeral span styled with `number_style` — the same label/number split
/// [`crate::row_view::push_badge_spans`] renders on the real tree row.
fn badge_swatch_spans(label: &'static str, number_style: Style) -> Vec<Span<'static>> {
    vec![Span::raw(label), Span::styled("N", number_style)]
}

/// Draws the `?` help overlay (ADR 0020, scrolling added post-hoc once the
/// keymap grew past what always fit on screen — ADR 0026's own "Source
/// view" group plus the `gd`/`gr`/jumplist entries pushed the pre-glossary
/// content past a typical terminal's height) centered over `full_area`: a
/// bordered box roughly 80%/90% of the frame's width/height (capped so it
/// never claims more than the frame itself on a small terminal), listing
/// every [`crate::help::HELP_CONTENT`] keymap group followed by the
/// glossary. [`Clear`] is rendered first so the overlay's background is
/// opaque rather than letting the underlying frame's glyphs show through
/// gaps in the overlay's own text.
///
/// Scrolled via [`render_scrollable_pane`] — the same clamp/indicator/
/// `Paragraph::scroll` machinery the Detail and Diff panes already share
/// (`crate::ui::scroll`'s own module doc comment), rather than a bespoke
/// mechanism just for this overlay: a terminal short enough that the
/// keymap + glossary overflow the box now scrolls via `j`/`k`/`Ctrl-d`/
/// `Ctrl-u`/`gg`/`G` (`App::handle_key`/`App::handle_scroll_key`'s own
/// `help_open` branches) instead of silently clipping the bottom of the
/// content with no way to reach it.
///
/// Returns the actually-clamped scroll offset and the overlay's own inner
/// height, for `crate::ui::draw` to fold into [`crate::ui::DrawOutcome`]
/// the same way every other scrollable pane's draw call already does.
pub(crate) fn draw_help_overlay(
    frame: &mut Frame,
    full_area: Rect,
    requested_scroll: usize,
) -> (usize, usize) {
    let overlay_area = centered_rect(full_area, 80, 90);
    frame.render_widget(Clear, overlay_area);

    let lines = help_overlay_lines();
    let inner_height = overlay_area.height.saturating_sub(2) as usize;
    // Always drawn as focused: this overlay is modal (composited on top of
    // whatever screen was already showing) and is always the surface `?`'s
    // own scroll keys act on while open, so there is no competing pane to
    // distinguish it from (`render_scrollable_pane`'s own doc comment).
    let scroll = render_scrollable_pane(
        frame,
        " Help (? to close) ",
        &lines,
        requested_scroll,
        overlay_area,
        true,
    );
    (scroll, inner_height)
}

/// A `Rect` centered within `area`, `percent_width`/`percent_height` of
/// `area`'s own dimensions — the standard `ratatui` centered-popup layout
/// recipe (two nested `Layout::vertical`/`horizontal` splits with a
/// `Percentage` constraint sandwiched between two equal `Percentage`
/// margins), extracted as its own pure function so the overlay's sizing
/// rule is nameable and independent of `draw_help_overlay`'s own
/// `Clear`/`Paragraph` concerns.
pub(crate) fn centered_rect(area: Rect, percent_width: u16, percent_height: u16) -> Rect {
    let vertical_margin = (100 - percent_height) / 2;
    let [_, middle, _] = Layout::vertical([
        Constraint::Percentage(vertical_margin),
        Constraint::Percentage(percent_height),
        Constraint::Percentage(vertical_margin),
    ])
    .areas(area);

    let horizontal_margin = (100 - percent_width) / 2;
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage(horizontal_margin),
        Constraint::Percentage(percent_width),
        Constraint::Percentage(horizontal_margin),
    ])
    .areas(middle);

    center
}

/// Draws the jump-target popup (ADR 0022) centered over `full_area`,
/// listing every candidate as `name (path)` with the currently highlighted
/// one shown reversed — the same `Clear`-first, centered-bordered-box
/// compositing `draw_help_overlay` already uses, just a narrower and
/// shorter box (60% x 40%, vs. the help overlay's 80% x 90%) since a
/// candidate list is typically much shorter than the whole keymap.
///
/// Windowed around `popup.cursor` via [`windowed_rows_with_indicators`]
/// (post-#61 review finding: this used to hand every candidate to
/// `Paragraph` unscrolled, so a popup with more candidates than the box's
/// height could select an off-screen candidate with no visual feedback at
/// all) — the same cursor-follow scroll `draw_tree_pane` uses, plus dim
/// "… N more above/below" lines inside the box when the window does not
/// reach an edge of the candidate list.
///
/// Candidate labels are [`truncate_to_width`]-ed to the popup's own inner
/// width rather than wrapped (a second bug found while fixing the first:
/// `windowed_rows_with_indicators`'s window math assumes one candidate is
/// one rendered row, but this used to render with `Paragraph::wrap`
/// enabled — a `"{name} ({path})"` label longer than the box's inner width
/// wrapped onto 2-3 physical rows, so the window's row-count budget
/// silently undercounted and pushed later candidates, including the
/// cursor row, off the bottom of the popup with no visual feedback,
/// exactly the failure mode the windowing fix above exists to prevent).
/// Truncating instead of wrapping restores the "one candidate, one row"
/// invariant the window math relies on.
pub(crate) fn draw_jump_popup(frame: &mut Frame, popup: &crate::app::JumpPopup, full_area: Rect) {
    let overlay_area = centered_rect(full_area, 60, 40);
    frame.render_widget(Clear, overlay_area);

    // 2 rows/columns for the top/bottom and left/right border, matching
    // `render_scrollable_pane`'s own `saturating_sub(2)` convention for a
    // bordered pane's inner height/width.
    let viewport_height = overlay_area.height.saturating_sub(2) as usize;
    let viewport_width = overlay_area.width.saturating_sub(2) as usize;
    let (start, end, above, below) =
        windowed_rows_with_indicators(popup.candidates.len(), popup.cursor, viewport_height);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(above) = above {
        lines.push(Line::styled(
            above,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    lines.extend(
        popup.candidates[start..end]
            .iter()
            .enumerate()
            .map(|(offset, candidate)| {
                let text = truncate_to_width(
                    &format!("{} ({})", candidate.name, candidate.path),
                    viewport_width,
                );
                if start + offset == popup.cursor {
                    Line::styled(
                        text,
                        Style::default()
                            .add_modifier(Modifier::REVERSED)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Line::raw(text)
                }
            }),
    );
    if let Some(below) = below {
        lines.push(Line::styled(
            below,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    let block = Block::bordered().title(" Jump to (enter: go, esc: cancel) ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay_area);
}

#[cfg(test)]
mod tests {
    use super::{FileSizeBand, markers_legend_lines};
    use crate::app::{App, BlastRadiusSelection};
    use crate::ui::draw;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::{FileReport, Report};

    #[test]
    fn should_render_api_badge_swatch_with_yellow_number_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let api_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "api:")
            })
            .expect("api: line present");

        // NOTE: partial assert — a `Line` built from `format!` interpolation
        // doesn't have one clean expected `Line` value to compare as a
        // whole (the explanation half is plain, unstyled text pulled
        // straight from `help::MARKER_LEGEND`), so this only pins the
        // swatch's number span style, which is the thing this test exists
        // to guard.
        let number_span = line_span(api_line, "N");
        assert_eq!(Style::default().fg(Color::Yellow), number_span.style);
    }

    #[test]
    fn should_render_fan_in_badge_swatch_with_cyan_number_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let fan_in_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "fan-in:")
            })
            .expect("fan-in: line present");

        let number_span = line_span(fan_in_line, "N");
        assert_eq!(Style::default().fg(Color::Cyan), number_span.style);
    }

    #[test]
    fn should_render_warn_badge_swatch_with_yellow_number_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let warn_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "warn:")
            })
            .expect("warn: line present");

        let number_span = line_span(warn_line, "N");
        assert_eq!(Style::default().fg(Color::Yellow), number_span.style);
    }

    #[test]
    fn should_render_split_badge_swatch_with_red_number_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let split_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "split:")
            })
            .expect("split: line present");

        let number_span = line_span(split_line, "N");
        assert_eq!(Style::default().fg(Color::Red), number_span.style);
    }

    #[test]
    fn should_render_signature_changed_marker_swatch_with_yellow_tilde_when_building_markers_legend()
     {
        let lines = markers_legend_lines();

        let changed_line = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "~"))
            .expect("~ line present");

        let swatch_span = line_span(changed_line, "~");
        assert_eq!(Style::default().fg(Color::Yellow), swatch_span.style);
    }

    #[test]
    fn should_render_cycle_marker_swatch_bold_yellow_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let cycle_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "(cycle)")
            })
            .expect("(cycle) line present");

        let swatch_span = line_span(cycle_line, "(cycle)");
        assert_eq!(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            swatch_span.style
        );
    }

    #[test]
    fn should_render_added_marker_swatch_with_green_plus_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let added_line = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "+"))
            .expect("+ line present");

        let swatch_span = line_span(added_line, "+");
        assert_eq!(Style::default().fg(Color::Green), swatch_span.style);
    }

    #[test]
    fn should_render_removed_marker_swatch_with_red_x_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let removed_line = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "x"))
            .expect("x line present");

        let swatch_span = line_span(removed_line, "x");
        assert_eq!(Style::default().fg(Color::Red), swatch_span.style);
    }

    #[test]
    fn should_render_risk_marker_swatch_bold_red_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let risk_line = lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "!"))
            .expect("! line present");

        let swatch_span = line_span(risk_line, "!");
        assert_eq!(
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            swatch_span.style
        );
    }

    #[test]
    fn should_render_dimmed_and_crossed_out_removed_name_swatch_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "(dimmed + struck-through name)")
            })
            .expect("removed-name swatch line present");

        let swatch_span = line_span(line, "(dimmed + struck-through name)");
        assert_eq!(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::CROSSED_OUT),
            swatch_span.style
        );
    }

    #[test]
    fn should_reuse_row_view_band_style_for_lines_swatch_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "lines:N")
            })
            .expect("lines:N line present");

        let swatch_span = line_span(line, "lines:N");
        assert_eq!(
            crate::row_view::band_style(FileSizeBand::Watch),
            swatch_span.style
        );
    }

    #[test]
    fn should_render_test_badge_swatch_with_magenta_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "[test] (N symbols)")
            })
            .expect("[test] (N symbols) line present");

        let swatch_span = line_span(line, "[test] (N symbols)");
        assert_eq!(Style::default().fg(Color::Magenta), swatch_span.style);
    }

    #[test]
    fn should_render_test_group_swatch_dark_gray_when_building_markers_legend() {
        let lines = markers_legend_lines();

        let line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref() == "N tests")
            })
            .expect("N tests line present");

        let swatch_span = line_span(line, "N tests");
        assert_eq!(Style::default().fg(Color::DarkGray), swatch_span.style);
    }

    fn line_span<'a>(line: &'a ratatui::text::Line<'static>, content: &str) -> &'a Span<'static> {
        line.spans
            .iter()
            .find(|span| span.content.as_ref() == content)
            .unwrap_or_else(|| panic!("span {content:?} not found in line"))
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
    fn should_draw_help_overlay_with_keymap_markers_and_glossary_when_help_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
        // A 150x70 terminal (up from 100x50): wider so the Markers
        // section's longest explanation line doesn't wrap onto a second
        // row, taller so the overlay's 80% x 90% area fits every keymap
        // group, the Markers legend, *and* the trailing Glossary section
        // without the last section being pushed off the bottom. Grown here
        // rather than narrowing the content itself, same rationale as the
        // 100x40 -> 100x50 growth this test already went through for ADR
        // 0026's keymap additions.
        let mut terminal = Terminal::new(TestBackend::new(150, 70)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Help"));
        assert!(text.contains("Tree focus"));
        assert!(text.contains("Right focus"));
        assert!(text.contains("Source view"));
        assert!(text.contains("Global"));
        assert!(text.contains("Markers"));
        assert!(text.contains("fan-in:N"));
        assert!(text.contains("Glossary"));
        assert!(text.contains("blast radius"));
    }

    #[test]
    fn should_not_show_glossary_when_terminal_is_too_short_to_fit_the_whole_keymap_and_scroll_is_zero()
     {
        // A small terminal (30 rows) whose overlay box cannot fit every
        // keymap group *and* the trailing Glossary section at once — the
        // gap this feature exists to close (previously: the unscrolled
        // `Paragraph` simply clipped the bottom of the content with no way
        // to reach it, `draw_help_overlay`'s pre-scroll doc comment).
        // Pinning that the Glossary is *not* visible at scroll 0 here, and
        // *is* visible after scrolling in the next test, is what proves
        // scrolling actually moves the rendered content rather than the
        // box merely being tall enough by coincidence.
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Tree focus"));
        assert!(!text.contains("Glossary"), "Glossary should not fit yet");
    }

    #[test]
    fn should_reveal_glossary_after_scrolling_down_when_terminal_is_too_short_to_fit_the_whole_keymap()
     {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        // Scroll well past every keymap group — `handle_scroll_key`'s own
        // clamp-free "requested" semantics mean this overshoots on
        // purpose; `render_scrollable_pane`'s clamp inside `draw` below is
        // what brings it back in bounds, mirroring how every other
        // scrollable pane in this crate is exercised in its own tests.
        let app = app.handle_scroll_key(crate::app::InputKey::ScrollToBottom, 20);

        let mut actual_outcome = crate::ui::DrawOutcome::default();
        terminal
            .draw(|frame| {
                actual_outcome = draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(
            text.contains("Glossary"),
            "Glossary should be visible after scrolling to the bottom"
        );
        assert!(
            text.contains("jumplist"),
            "the last glossary entry should be visible at the bottom"
        );
        // The scroll actually applied must be reported back (`DrawOutcome`'s
        // own doc comment on why `crate::run_app` needs this to fold the
        // overshot request back down) rather than staying at the
        // unclamped `usize::MAX` sentinel `ScrollToBottom` set.
        assert!(actual_outcome.clamped_help_scroll.is_some());
        assert_ne!(Some(usize::MAX), actual_outcome.clamped_help_scroll);
        assert!(actual_outcome.help_scroll_viewport_height.is_some());
    }

    #[test]
    fn should_report_none_clamped_help_scroll_and_none_viewport_height_when_help_overlay_is_closed()
    {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        let mut actual_outcome = crate::ui::DrawOutcome::default();
        terminal
            .draw(|frame| {
                actual_outcome = draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        assert_eq!(None, actual_outcome.clamped_help_scroll);
        assert_eq!(None, actual_outcome.help_scroll_viewport_height);
    }

    #[test]
    fn should_not_draw_help_overlay_when_help_is_closed() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(!text.contains("Glossary"));
    }

    #[test]
    fn should_draw_jump_popup_with_every_candidate_when_jump_popup_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).open_jump_popup(vec![
            crate::app::JumpCandidate {
                id: "lib.rs::alpha".to_string(),
                name: "alpha".to_string(),
                path: "lib.rs".to_string(),
            },
            crate::app::JumpCandidate {
                id: "lib.rs::beta".to_string(),
                name: "beta".to_string(),
                path: "lib.rs".to_string(),
            },
        ]);
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Jump to"));
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
    }

    #[test]
    fn should_window_candidates_around_cursor_when_popup_has_more_candidates_than_fit() {
        // #61-review finding: the popup used to hand every candidate to
        // `Paragraph` unscrolled, so a popup with more candidates than the
        // box's own height could highlight an off-screen candidate with no
        // visual feedback at all. 25 candidates, cursor moved to the last
        // one (index 24) via repeated Down, is more than any reasonably
        // sized popup box can show at once.
        let report = report_with_one_symbol();
        let candidates: Vec<crate::app::JumpCandidate> = (0..25)
            .map(|i| crate::app::JumpCandidate {
                id: format!("lib.rs::sym{i}"),
                name: format!("sym{i}"),
                path: "lib.rs".to_string(),
            })
            .collect();
        let mut app = App::new(&report).open_jump_popup(candidates);
        for _ in 0..24 {
            app = app.handle_key(crate::app::InputKey::Down);
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // The highlighted candidate (the last one, cursor at index 24) must
        // always be visible — the whole point of the windowing fix.
        assert!(text.contains("sym24"), "cursor candidate sym24 not visible");
        // The first candidate is far outside the window around index 24, so
        // it must not be rendered, and an overflow indicator must say so.
        assert!(!text.contains("sym0 ("), "sym0 should have scrolled off");
        assert!(text.contains("more above"));
    }

    #[test]
    fn should_keep_highlighted_candidate_visible_when_labels_wrap_across_multiple_rows() {
        // Regression test for the bug this change fixes:
        // `windowed_rows_with_indicators` computes its window assuming one
        // candidate = one rendered row, but `draw_jump_popup` used to hand
        // its lines to `Paragraph` with `Wrap { trim: false }` enabled — a
        // candidate label (`"{name} ({path})"`) longer than the popup's
        // inner width wrapped onto 2-3 physical rows, so the window's
        // row-count math silently undercounted and pushed later
        // candidates, including the cursor row, off the bottom of the
        // popup with no visual feedback at all. An 80x24 terminal (a
        // common real-world size, smaller than every other test in this
        // file's 100-wide terminals) with 40-90 character paths reproduces
        // it: the popup's 60%-width box is nowhere near wide enough to fit
        // "name (path)" on one row unwrapped.
        let report = report_with_one_symbol();
        let candidates: Vec<crate::app::JumpCandidate> = (0..20)
            .map(|i| crate::app::JumpCandidate {
                id: format!("lib.rs::sym{i}"),
                name: format!("very_long_symbol_name_number_{i}"),
                path: format!(
                    "src/very/deeply/nested/module/path/number_{i}/that/is/quite/long/file.rs"
                ),
            })
            .collect();
        let mut app = App::new(&report).open_jump_popup(candidates);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

        // Move the cursor one step at a time, checking after *every* step
        // (not just at the end) that the highlighted candidate is visible —
        // the bug manifested at intermediate cursor positions too, not only
        // at the very last candidate.
        for step in 0..19 {
            app = app.handle_key(crate::app::InputKey::Down);
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        &app,
                        &report,
                        &crate::diff_shape::DiffPaneContent::Empty,
                        &[],
                        &BlastRadiusSelection::NotApplicable,
                        None,
                    );
                })
                .expect("draw");

            let text = buffer_text(&terminal);
            let expected_marker = format!("very_long_symbol_name_number_{}", step + 1);
            assert!(
                text.contains(&expected_marker),
                "cursor candidate {expected_marker} not visible after {} Down presses:\n{text}",
                step + 1
            );
        }
    }

    #[test]
    fn should_not_draw_jump_popup_when_no_jump_is_pending() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &BlastRadiusSelection::NotApplicable,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(!text.contains("Jump to"));
    }
}
