//! `?` help overlay and jump-target popup (ADR 0020, ADR 0022) —
//! composited on top of whatever screen was already rendered underneath,
//! after the pane split has drawn everything else.

use super::scroll::windowed_rows_with_indicators;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

/// Draws the `?` help overlay (ADR 0020) centered over `full_area`: a
/// bordered box roughly 70% of the frame's width/height (capped so it
/// never claims more than the frame itself on a small terminal), listing
/// every [`crate::help::HELP_CONTENT`] keymap group followed by the
/// glossary. [`Clear`] is rendered first so the overlay's background is
/// opaque rather than letting the underlying frame's glyphs show through
/// gaps in the overlay's own text.
pub(crate) fn draw_help_overlay(frame: &mut Frame, full_area: Rect) {
    let overlay_area = centered_rect(full_area, 80, 90);
    frame.render_widget(Clear, overlay_area);

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

    let block = Block::bordered().title(" Help (? to close) ");
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, overlay_area);
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
pub(crate) fn draw_jump_popup(frame: &mut Frame, popup: &crate::app::JumpPopup, full_area: Rect) {
    let overlay_area = centered_rect(full_area, 60, 40);
    frame.render_widget(Clear, overlay_area);

    // 2 rows for the top/bottom border, matching `render_scrollable_pane`'s
    // own `saturating_sub(2)` convention for a bordered pane's inner height.
    let viewport_height = overlay_area.height.saturating_sub(2) as usize;
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
                let text = format!("{} ({})", candidate.name, candidate.path);
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
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
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
            hotspots: vec![],
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
