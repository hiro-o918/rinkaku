//! `?` help overlay and jump-target popup (ADR 0020, ADR 0022) —
//! composited on top of whatever screen was already rendered underneath,
//! after the pane split has drawn everything else.

use super::scroll::{render_scrollable_pane, truncate_to_width, windowed_rows_with_indicators};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

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
        &[],
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
    use crate::app::{App, BlastRadiusSelection};
    use crate::ui::draw;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::{FileReport, Report};

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
    fn should_draw_help_overlay_with_keymap_and_glossary_when_help_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
        // A 100x50 terminal (up from 100x40 before ADR 0026) so the
        // overlay's 80% x 90% area (about 80x45 inner) fits every keymap
        // group *and* the trailing Glossary section without the last
        // section being pushed off the bottom. ADR 0026 added the
        // "Source view" group and three extra "Right focus" entries
        // (gg/G, Ctrl-d/u), which grew the pre-glossary content past
        // the old 36-row overlay ceiling. Grown here rather than
        // narrowing the keymap itself since discoverability of the new
        // bindings is the whole point of adding them to the overlay in
        // the first place.
        let mut terminal = Terminal::new(TestBackend::new(100, 50)).expect("terminal");

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
