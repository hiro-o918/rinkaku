//! Entry screen (tree + right pane split, ADR 0015/0016) and its tree
//! pane rendering. The right-pane variant switching (Detail/Diff/BlastRadius)
//! dispatches to the sibling modules that own each pane's own layout.

use super::blast_radius::draw_blast_radius_pane;
use super::detail_pane::draw_detail_pane;
use super::diff_pane::draw_diff_pane;
use super::scroll::{scroll_indicator, truncate_line_to_width, windowed_rows_with_indicators};
use super::style::pane_border_style;
use super::{ENTRY_RIGHT_WIDTH_PERCENT, ENTRY_TREE_WIDTH_PERCENT};
use crate::app::{App, BlastRadiusSelection, Focus, RightPane};
use crate::highlight::HighlightedFile;
use crate::row_view::{entry_row_line, relative_labels};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::render::Report;

/// Left entry pane (directory tree) + right pane, split 60/40 — this
/// implementation's own choice (ADR 0015/0016 left the exact ratio open):
/// the tree is the primary navigation surface and typically has more rows
/// than the right pane has fields, so it gets the larger share. The right
/// pane itself shows either the detail view or the diff view depending on
/// `app.right_pane()` (`d`/`D` toggles between them, TUI iteration 2).
///
/// Returns the clamped right-pane scroll offset actually applied — whichever
/// of `draw_detail_pane`/`draw_diff_pane`/`draw_blast_radius_pane` ran for
/// `app.right_pane()` (`render_scrollable_pane`'s doc comment on why
/// `crate::run_app` needs this).
pub(crate) fn draw_entry_screen(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_content: &crate::diff_shape::DiffPaneContent,
    diff_highlights: &[HighlightedFile],
    blast_radius_selection: &BlastRadiusSelection,
    area: Rect,
) -> Option<usize> {
    let [tree_area, right_area] = Layout::horizontal([
        Constraint::Percentage(ENTRY_TREE_WIDTH_PERCENT),
        Constraint::Percentage(ENTRY_RIGHT_WIDTH_PERCENT),
    ])
    .areas(area);

    draw_tree_pane(frame, app, tree_area);
    match app.right_pane() {
        RightPane::Detail => draw_detail_pane(frame, app, report, right_area),
        RightPane::Diff => draw_diff_pane(
            frame,
            app,
            report,
            diff_content,
            diff_highlights,
            right_area,
        ),
        RightPane::BlastRadius => {
            draw_blast_radius_pane(frame, app, blast_radius_selection, right_area)
        }
    }
}

/// Draws the entry tree, windowed around the cursor
/// ([`windowed_rows_with_indicators`]) so the cursor row is always inside
/// the viewport — post-#61 review finding: this used to hand `Nav::rows`'
/// entire row list to `Paragraph` unscrolled, so moving the cursor past the
/// bottom of the initial viewport (via repeated `j`, or a `gd`/`gr` jump
/// landing far from the current scroll position) looked like the keypress
/// had no effect at all, since the screen kept showing exactly the same
/// rows. A `(first-last/total)` title suffix mirrors `render_scrollable_pane`'s
/// own `scroll_indicator` convention, and the windowing also grows a dim
/// "… N more above/below" line inside the pane itself when the window does
/// not start/end at the list's own edge — belt and braces with the title
/// suffix, since the title is easy to miss but the in-pane line sits right
/// where the reviewer's eye already is.
pub(crate) fn draw_tree_pane(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app.nav().rows(app.tree());
    let labels = relative_labels(&rows);
    let cursor = app.nav().cursor();
    let ranks = app.ranks();

    // 2 rows/columns reserved for the pane's own border.
    let viewport_height = area.height.saturating_sub(2) as usize;
    let viewport_width = area.width.saturating_sub(2) as usize;
    let (start, end, above, below) =
        windowed_rows_with_indicators(rows.len(), cursor, viewport_height);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(above) = above {
        lines.push(Line::styled(
            above,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    lines.extend(
        rows[start..end]
            .iter()
            .zip(labels[start..end].iter())
            .enumerate()
            .map(|(offset, (row, label))| {
                entry_row_line(row, label, ranks, start + offset == cursor)
            }),
    );
    if let Some(below) = below {
        lines.push(Line::styled(
            below,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    // No `.wrap(...)`: `windowed_rows_with_indicators` requires one row
    // per item, so overflow is truncated with `…` instead.
    let lines: Vec<Line<'static>> = lines
        .iter()
        .map(|line| truncate_line_to_width(line, viewport_width))
        .collect();

    // `end - start`, not the raw `viewport_height`, is the title
    // indicator's own "how many rows are actually visible" — the two can
    // differ once `windowed_rows_with_indicators` has reserved rows for the
    // in-pane "… N more" lines, and `scroll_indicator` reporting the
    // unreserved `viewport_height` would overstate the last visible row.
    let title = match scroll_indicator(rows.len(), end - start, start) {
        Some(indicator) => format!("{}{indicator} ", " Entry".trim_end()),
        None => " Entry ".to_string(),
    };
    let block = Block::bordered()
        .title(title)
        .border_style(pane_border_style(app.focus() == Focus::Tree));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use crate::app::{App, BlastRadiusSelection};
    use crate::ui::draw;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
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

    fn report_with_many_files(count: usize) -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: (0..count)
                .map(|i| FileReport {
                    path: format!("f{i}.rs"),
                    symbols: vec![symbol(&format!("f{i}.rs::sym{i}"), &format!("sym{i}"))],
                })
                .collect(),
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
    fn should_draw_entry_and_detail_panes_with_titles_on_entry_screen() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
        // Detail, which is what this test actually exercises.
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

    // NOTE: asserts only `fg`/`add_modifier` rather than the whole `Style`
    // returned by `buffer[..].style()` — a rendered `Cell`'s `Style` always
    // carries ratatui's own default `bg`/`underline_color` (`Color::Reset`)
    // filled in, unlike the bare `Style` `pane_border_style` constructs, so
    // a fully-qualified comparison would fail for a reason unrelated to
    // this test's actual claim. Mirrors the same partial-comparison
    // precedent `diff_pane`/`source_screen`'s own `find_cell_style` tests
    // already use for the identical reason.
    #[test]
    fn should_swap_border_emphasis_between_tree_and_right_pane_when_focus_moves_right() {
        // Dogfooding finding: every pane's `Block` looked identical
        // regardless of which one currently received motion keys, so a
        // reviewer had no visual way to tell which pane `j`/`k` would move.
        // This pins the actual rendered border cell style (not just the
        // pure `pane_border_style` helper in isolation) swapping sides as
        // `Focus` changes.
        let report = report_with_one_symbol();
        let app = App::new(&report);
        assert_eq!(crate::app::Focus::Tree, app.focus());
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

        // Tree pane's own top-left border corner is at (0, 0); the right
        // pane's 60/40 split (`ENTRY_TREE_WIDTH_PERCENT`) puts its top-left
        // corner at column 48 of an 80-column terminal.
        let buffer = terminal.backend().buffer();
        let tree_border_style = buffer[(0, 0)].style();
        let right_border_style = buffer[(48, 0)].style();
        assert_eq!(Some(Color::Cyan), tree_border_style.fg);
        assert!(tree_border_style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(Some(Color::DarkGray), right_border_style.fg);
        assert!(!right_border_style.add_modifier.contains(Modifier::BOLD));

        let app = app.handle_key(crate::app::InputKey::Open);
        assert_eq!(crate::app::Focus::Right, app.focus());
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

        let buffer = terminal.backend().buffer();
        let tree_border_style = buffer[(0, 0)].style();
        let right_border_style = buffer[(48, 0)].style();
        assert_eq!(Some(Color::DarkGray), tree_border_style.fg);
        assert!(!tree_border_style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(Some(Color::Cyan), right_border_style.fg);
        assert!(right_border_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn should_follow_cursor_downward_in_tree_pane_when_scrolling_past_the_bottom_of_the_viewport() {
        // #61-review finding: the tree pane used to hand `Nav::rows`' entire
        // row list to `Paragraph` unscrolled, so moving the cursor past the
        // bottom of the initial viewport looked like the keypress had no
        // effect (the screen kept showing exactly the same rows). A 60-file
        // tree (each file has a symbol row too, so ~120 rows) in a 20-row
        // terminal (18-row inner viewport after the border) is far taller
        // than any single viewport.
        let report = report_with_many_files(60);
        let mut app = App::new(&report);
        for _ in 0..100 {
            app = app.handle_key(crate::app::InputKey::Down);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

        // The cursor's own row label must appear in the rendered pane — read
        // back from `App` rather than hardcoding a guess, since which row
        // 100 `Down` presses lands on is not a fixed, predictable filename
        // in the first place (sibling files can be reordered by the default
        // topological order, ADR 0016). A file row's own label is its path
        // (`row_view::entry_row_line`'s `NodeKind::File` arm); a symbol
        // row's is just its name, not the containing file's path.
        let cursor_row = &app.nav().rows(app.tree())[app.nav().cursor()];
        let cursor_label = match &cursor_row.node.kind {
            crate::tree::NodeKind::Symbol(symbol_ref) => symbol_ref.name.clone(),
            crate::tree::NodeKind::File | crate::tree::NodeKind::Dir => {
                cursor_row.node.path.clone()
            }
            // Unreachable in this test fixture (no test files), but
            // required to stay exhaustive.
            crate::tree::NodeKind::Section(section_kind) => section_kind.label().to_string(),
        };
        let text = buffer_text(&terminal);
        assert!(
            text.contains(&cursor_label),
            "cursor row {cursor_label} not visible in:\n{text}"
        );
        // The very first file must have scrolled off given how far down the
        // cursor moved, and an overflow indicator must say so.
        assert!(!text.contains("f0.rs"), "f0.rs should have scrolled off");
        assert!(text.contains("more above"));
    }

    #[test]
    fn should_show_jump_target_row_in_tree_pane_when_it_lands_far_from_the_current_scroll_position()
    {
        // The exact user-facing scenario the #61-review finding describes:
        // a gd/gr jump landing far down a long tree must actually scroll
        // the tree pane there, not just move the cursor in state while the
        // screen keeps showing the old scroll position.
        let report = report_with_many_files(60);
        let app = App::new(&report).jump_to_symbol("f55.rs::sym55");
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

        // The jump must have actually landed on "sym55" (defensive: proves
        // the rest of this assertion is exercising the intended row, not
        // silently passing because the jump itself failed).
        let cursor_row = &app.nav().rows(app.tree())[app.nav().cursor()];
        match &cursor_row.node.kind {
            crate::tree::NodeKind::Symbol(symbol_ref) => assert_eq!("sym55", symbol_ref.name),
            other => panic!("expected jump to land on a Symbol row, got {other:?}"),
        }

        let text = buffer_text(&terminal);
        assert!(text.contains("sym55"), "jump target row not visible");
    }

    /// A single-file report whose path overflows an 80-column terminal's
    /// tree pane (60% width, `ENTRY_TREE_WIDTH_PERCENT`, minus 2 columns
    /// of border).
    fn report_with_long_path_symbol() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/very/deeply/nested/directory/tree/that/does/not/fit.rs".to_string(),
                symbols: vec![symbol(
                    "src/very/deeply/nested/directory/tree/that/does/not/fit.rs::foo",
                    "foo",
                )],
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
            removed: vec![],
        }
    }

    #[test]
    fn should_truncate_tree_row_with_trailing_ellipsis_when_it_overflows_the_pane_width() {
        let report = report_with_long_path_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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
        assert!(
            text.contains('…'),
            "expected an ellipsis-truncated row in:\n{text}"
        );
        assert!(
            !text.contains("src/very/deeply/nested/directory/tree/that/does/not/fit.rs"),
            "the full overflowing path should not fit on screen:\n{text}"
        );
    }

    #[test]
    fn should_not_truncate_tree_row_when_it_fits_within_the_pane_width() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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
        assert!(
            text.contains("lib.rs"),
            "short file row should render in full"
        );
        assert!(
            !text.contains('…'),
            "no row should need truncation in:\n{text}"
        );
    }

    #[test]
    fn should_preserve_selection_highlight_style_on_a_truncated_cursor_row() {
        let report = report_with_long_path_symbol();
        let app = App::new(&report);
        assert_eq!(0, app.nav().cursor());
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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

        // (1, 1): one column/row past the pane's own left/top border.
        let buffer = terminal.backend().buffer();
        let cell_style = buffer[(1, 1)].style();
        assert!(
            cell_style.add_modifier.contains(Modifier::REVERSED),
            "cursor row cell should stay reversed after truncation, got {cell_style:?}"
        );
    }
}
