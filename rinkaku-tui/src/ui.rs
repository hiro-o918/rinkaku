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

use crate::app::{App, DiffTarget, RightPane, Screen, SelectedDetail};
use crate::detail::{DetailView, DirDetail, FileDetail, SignatureView};
use crate::diff_view::{DiffLine, DiffLineKind, FileHunks, Hunk, file_hunks, hunks_for_range};
use crate::row_view::{entry_row_line, relative_labels};
use crate::source::{SourceView, load_symbol_source, visible_window};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::extract::Classification;
use rinkaku_core::render::Report;

/// Draws one full frame: the entry view (tree + right pane split) or the
/// source drill-down, depending on `app.screen()`, with a status/help line
/// pinned to the bottom either way. `diff_text` is the raw unified diff the
/// caller built `report` from (threaded through from `main.rs`, same string
/// for every input mode — see `crate::run`'s doc comment) — only consulted
/// when the right pane is in [`RightPane::Diff`] mode.
pub fn draw(frame: &mut Frame, app: &App, report: &Report, diff_text: &str) {
    let area = frame.area();
    let [body, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    match app.screen() {
        Screen::Entry => draw_entry_screen(frame, app, report, diff_text, body),
        Screen::Source { symbol_id } => draw_source_screen(frame, report, symbol_id, body),
    }

    draw_status_line(frame, app, status_area);
}

/// Left entry pane (directory tree) + right pane, split 60/40 — this
/// implementation's own choice (ADR 0015/0016 left the exact ratio open):
/// the tree is the primary navigation surface and typically has more rows
/// than the right pane has fields, so it gets the larger share. The right
/// pane itself shows either the detail view or the diff view depending on
/// `app.right_pane()` (`d`/`D` toggles between them, TUI iteration 2).
fn draw_entry_screen(frame: &mut Frame, app: &App, report: &Report, diff_text: &str, area: Rect) {
    let [tree_area, right_area] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    draw_tree_pane(frame, app, tree_area);
    match app.right_pane() {
        RightPane::Detail => draw_detail_pane(frame, app, report, right_area),
        RightPane::Diff => draw_diff_pane(frame, app, report, diff_text, right_area),
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
    let block = Block::bordered().title(" Detail ");

    let Some(detail) = app.selected_detail(report) else {
        let paragraph = Paragraph::new("(select a row to see its detail)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    let lines = match &detail {
        SelectedDetail::Symbol(detail) => detail_lines(detail),
        SelectedDetail::Dir(detail) => dir_detail_lines(detail),
        SelectedDetail::File(detail) => file_detail_lines(detail),
    };
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Draws the diff pane (TUI iteration 2, [`RightPane::Diff`]): the raw
/// unified-diff hunks touching the row under the cursor — every hunk of the
/// file for a file row, or just the hunks intersecting a symbol's own line
/// range for a symbol row (`App::selected_diff_target`'s own doc comment).
/// A directory row, or a row with nothing to show (no hunks found, e.g. a
/// mismatch between `report` and `diff_text`), falls back to a placeholder
/// message rather than an empty pane.
fn draw_diff_pane(frame: &mut Frame, app: &App, report: &Report, diff_text: &str, area: Rect) {
    let block = Block::bordered().title(" Diff ");

    let Some(target) = app.selected_diff_target(report) else {
        let paragraph = Paragraph::new("(select a symbol or file row to see its diff)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    let files = crate::diff_view::parse_diff_hunks(diff_text);
    let (path, hunks): (&str, Vec<&Hunk>) = match &target {
        DiffTarget::Symbol {
            path,
            range_start,
            range_end,
        } => {
            let hunks = file_hunks(&files, path)
                .map(|fh| hunks_for_range(fh, *range_start, *range_end))
                .unwrap_or_default();
            (path.as_str(), hunks)
        }
        DiffTarget::File { path } => {
            let hunks = file_hunks(&files, path)
                .map(|fh: &FileHunks| fh.hunks.iter().collect())
                .unwrap_or_default();
            (path.as_str(), hunks)
        }
    };

    if hunks.is_empty() {
        let paragraph = Paragraph::new(format!("(no diff hunks found for {path})"))
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    }

    let lines = diff_pane_lines(&hunks);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Formats a list of [`Hunk`]s into styled lines: hunk headers dim, `+`
/// lines green, `-` lines red, context lines unstyled — mirrors the
/// existing signature-diff styling `detail_lines`' `SignatureView::Changed`
/// arm already uses (`Color::Green`/`Color::Red`) so the two diff-styled
/// panes in this crate read consistently.
fn diff_pane_lines(hunks: &[&Hunk]) -> Vec<Line<'static>> {
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
        for line in &hunk.lines {
            lines.push(diff_line(line));
        }
    }
    lines
}

fn diff_line(line: &DiffLine) -> Line<'static> {
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

/// Formats a [`DirDetail`] into displayable lines: a badge breakdown, its
/// own top fan-in symbols, and — only when this directory is in a cycle —
/// the partner directories and the concrete cross-directory edges forming
/// it (TUI iteration 2's answer to "cycle と言われても何が cycle してるか
/// 分からない").
fn dir_detail_lines(detail: &DirDetail) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("Dir {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));
    lines.push(Line::raw(format!(
        "changed symbols: {}",
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

/// Formats a [`FileDetail`] into displayable lines: every symbol changed
/// in this file, with the same classification marker convention
/// `crate::row_view::entry_row_line` uses on symbol rows, plus fan-in.
fn file_detail_lines(detail: &FileDetail) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("File {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));
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
            "j/k: move  enter/space: expand  e/c: expand/collapse all  o: order  d: diff  s: source  q: quit"
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
            .draw(|frame| draw(frame, &app, &report, ""))
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
            .draw(|frame| draw(frame, &app, &report, ""))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("select a row"));
    }

    #[test]
    fn should_draw_dir_detail_content_when_cursor_is_on_a_directory_row() {
        let report = Report {
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
            .draw(|frame| draw(frame, &app, &report, ""))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Dir src"));
        assert!(text.contains("Top fan-in"));
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
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| draw(frame, &app, &report, diff_text))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Diff"));
        assert!(text.contains("+fn foo() {}"));
    }

    #[test]
    fn should_draw_detail_pane_content_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        terminal
            .draw(|frame| draw(frame, &app, &report, ""))
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
            .draw(|frame| draw(frame, &app, &report, ""))
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
            .draw(|frame| draw(frame, &app, &report, ""))
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
