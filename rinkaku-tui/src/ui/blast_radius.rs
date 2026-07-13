//! Blast-radius right-pane (ADR 0019 for the re-rooting algorithm, ADR
//! 0023 for the naming): the entry-tree text rooted at the directory/file
//! row under the cursor. Layout only — the actual re-rooted view is
//! computed once per handled key by `crate::run_app` and handed in.

use super::scroll::render_scrollable_pane;
use crate::app::{App, BlastRadiusSelection};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

/// Draws the blast-radius pane (ADR 0019 for the re-rooting algorithm, ADR
/// 0023 for the "blast radius" naming — [`crate::app::RightPane::BlastRadius`]): the
/// entry-tree text rooted at the directory/file row under the cursor,
/// following the cursor as it moves. `selection` is already computed by
/// `crate::run_app` (via `App::selected_blast_radius_view`) once per handled
/// key, not here — this function only lays it out, since `terminal.draw`
/// itself runs on every ~100ms idle poll tick as well as on an actual key
/// press, and re-deriving the blast-radius tree (an O(V+E) graph walk) on
/// every one of those idle ticks was exactly the per-frame recompute this
/// split avoids. A symbol row shows a placeholder asking for a directory/
/// file row instead — measuring blast radius from a single symbol has no
/// directory-scoped meaning (ADR 0019's `path_prefix` is meant to carve out
/// a layer, not re-derive what a single symbol's own detail pane already
/// shows). A directory/file row whose path matches no symbol shows its own
/// "nothing under `<path>` is reachable" message, mirroring `main.rs`'s
/// `--entry` CLI note.
///
/// The pane's title states the question the tree answers ("Blast radius of
/// `<path>`") rather than the re-rooting mechanism, per ADR 0023's own
/// rationale for the rename — a reviewer opening the pane for the first
/// time should not need `?`'s glossary to understand what it shows.
///
/// Returns the clamped scroll offset actually applied, or `None` when a
/// placeholder path was taken — mirrors `draw_detail_pane`'s own return
/// value for the identical reason (`render_scrollable_pane`'s doc comment).
pub(crate) fn draw_blast_radius_pane(
    frame: &mut Frame,
    app: &App,
    selection: &BlastRadiusSelection,
    area: Rect,
) -> Option<usize> {
    match selection {
        BlastRadiusSelection::NotApplicable => {
            let block = Block::bordered().title(" Blast radius ");
            let paragraph =
                Paragraph::new("(select a directory or file row to see its blast radius)")
                    .block(block)
                    .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
            None
        }
        BlastRadiusSelection::Empty { path } => {
            let block = Block::bordered().title(format!(" Blast radius of {path} "));
            let paragraph = Paragraph::new(format!("(nothing under {path} is reachable)"))
                .block(block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(paragraph, area);
            None
        }
        BlastRadiusSelection::View(view) => {
            let lines = blast_radius_pane_lines(view);
            let title = format!(" Blast radius of {} ", view.path);
            Some(render_scrollable_pane(
                frame,
                &title,
                &lines,
                app.right_pane_scroll(),
                area,
            ))
        }
    }
}

/// Formats a [`crate::blast_radius::BlastRadiusView`]'s flattened [`crate::blast_radius::BlastRadiusLine`]s
/// into styled [`Line`]s: indentation by depth (same `INDENT_WIDTH`-per-level
/// convention as `crate::row_view::entry_row_line`), a dimmed style for
/// `outside_prefix` lines (reached only by expanding a dependency edge past
/// the pivoted path), a `(see above)` suffix for `already_printed` lines
/// (matching `rinkaku-core::render`'s Markdown tree), and yellow/bold for a
/// cycle-warning line (matching `entry_row_line`'s existing `(cycle)`
/// marker styling).
pub(crate) fn blast_radius_pane_lines(
    view: &crate::blast_radius::BlastRadiusView,
) -> Vec<Line<'static>> {
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

#[cfg(test)]
mod tests {
    use crate::app::App;
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
    fn should_draw_blast_radius_pane_with_tree_lines_when_toggled_on_a_file_row() {
        // Same fixture shape as `report_with_one_symbol`, but with a graph
        // actually populated (that fixture leaves `graph` empty since most
        // of this module's tests don't need one) so opening the blast-radius
        // pane on "lib.rs" yields a real `BlastRadiusSelection::View` instead
        // of `Empty`.
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
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleBlastRadius);
        // `crate::run_app` computes this once per handled key and hands it
        // into `draw` (see `draw`'s own doc comment on why `draw` itself
        // must not call `App::selected_blast_radius_view`) — this test recreates
        // that same one-shot computation rather than a per-frame one.
        let blast_radius_selection = app.selected_blast_radius_view(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &blast_radius_selection,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Blast radius of lib.rs"));
        assert!(text.contains("fn foo (lib.rs)"));
    }

    #[test]
    fn should_draw_blast_radius_placeholder_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleBlastRadius);
        let blast_radius_selection = app.selected_blast_radius_view(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &blast_radius_selection,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Blast radius"));
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

    #[test]
    fn should_draw_blast_radius_empty_message_when_file_row_path_matches_no_graph_node() {
        // `report_with_one_symbol`'s `graph.nodes` is empty (that fixture
        // exists for Detail/Diff pane tests that don't need a populated
        // graph), so the cursor's default position on the "lib.rs" file row
        // matches no graph node — the real-world trigger for
        // `BlastRadiusSelection::Empty` (`App`'s own
        // `should_return_empty_blast_radius_selection_when_file_row_path_matches_no_graph_node`
        // test pins the same fixture shape at the `App` layer; this test
        // pins the rendered pane text ADR 0023 promises, which nothing
        // previously asserted).
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleBlastRadius);
        let blast_radius_selection = app.selected_blast_radius_view(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &app,
                    &report,
                    &crate::diff_shape::DiffPaneContent::Empty,
                    &[],
                    &blast_radius_selection,
                    None,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Blast radius of lib.rs"));
        // NOTE: asserting a substring, not the full parenthesized sentence —
        // the pane is narrow enough (40% of an 80-column terminal) that
        // `Paragraph`'s word-wrapping can split the sentence across rows,
        // and `buffer_text` joins rows with `\n`, so a wrapped multi-row
        // substring would never match a single-line `contains` check
        // (mirroring `should_draw_blast_radius_placeholder_when_cursor_is_on_a_symbol_row`'s
        // own "directory" fragment check just above, for the same reason).
        // Still pins the exact wording ADR 0023 specifies ("is reachable",
        // not the earlier "depends on anything" this test was added to
        // catch a future drift away from).
        assert!(text.contains("nothing under lib.rs is"));
        assert!(text.contains("reachable"));
    }
}
