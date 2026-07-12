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

use crate::app::{App, Screen};
use crate::detail::{DetailView, SignatureView};
use crate::row_view::{entry_row_line, relative_labels};
use crate::source::{SourceView, load_symbol_source, visible_window};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::render::Report;

/// Draws one full frame: the entry view (tree + detail pane split) or the
/// source drill-down, depending on `app.screen()`, with a status/help line
/// pinned to the bottom either way.
pub fn draw(frame: &mut Frame, app: &App, report: &Report) {
    let area = frame.area();
    let [body, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    match app.screen() {
        Screen::Entry => draw_entry_screen(frame, app, report, body),
        Screen::Source { symbol_id } => draw_source_screen(frame, report, symbol_id, body),
    }

    draw_status_line(frame, app, status_area);
}

/// Left entry pane (directory tree) + right detail pane, split 60/40 —
/// this implementation's own choice (ADR 0015/0016 left the exact ratio
/// open): the tree is the primary navigation surface and typically has
/// more rows than the detail pane has fields, so it gets the larger share.
fn draw_entry_screen(frame: &mut Frame, app: &App, report: &Report, area: Rect) {
    let [tree_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    draw_tree_pane(frame, app, tree_area);
    draw_detail_pane(frame, app, report, detail_area);
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
    let block = Block::bordered().title(" Detail ");

    let Some(detail) = app.selected_detail(report) else {
        let paragraph = Paragraph::new("(select a symbol row to see its detail)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    let lines = detail_lines(&detail);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
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
fn draw_source_screen(frame: &mut Frame, report: &Report, symbol_id: &str, area: Rect) {
    let title = format!(" Source: {symbol_id} ");
    let block = Block::bordered().title(title);

    let source = match load_symbol_source(report, symbol_id) {
        Ok(source) => source,
        Err(message) => {
            let paragraph = Paragraph::new(message).block(block);
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
            "j/k: move  enter/space: expand  e/c: expand/collapse all  o: order  s: source  q: quit"
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
            .draw(|frame| draw(frame, &app, &report))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Entry"));
        assert!(text.contains("Detail"));
        assert!(text.contains("lib.rs"));
        // The full "(select a symbol row to see its detail)" message can
        // legitimately wrap across two buffer rows depending on the
        // detail pane's width, which would defeat a substring check
        // against `buffer_text`'s newline-joined rows — "select a symbol"
        // is short enough to always land on one row regardless of exact
        // wrap point, which is all this coarse layout check needs.
        assert!(text.contains("select a symbol"));
    }

    #[test]
    fn should_draw_detail_pane_content_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| draw(frame, &app, &report))
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
        // module: the full help text is ~86 columns and would otherwise
        // be truncated (the status line intentionally does not wrap),
        // hiding the "quit" fragment this test checks for.
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).expect("terminal");

        terminal
            .draw(|frame| draw(frame, &app, &report))
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
            .draw(|frame| draw(frame, &app, &report))
            .expect("draw");

        // "lib.rs" does not exist relative to the test process's cwd, so
        // this exercises `draw_source_screen`'s error-message fallback
        // path rather than needing a real file on disk.
        let text = buffer_text(&terminal);
        assert!(text.contains("Source: lib.rs::foo"));
        assert!(text.contains("failed to read"));
        assert!(text.contains("back"));
    }
}
