//! Bottom status line (ADR 0020's key-hints and order-mode display),
//! pinned to the last row of the frame regardless of which screen is
//! showing above it.

use crate::app::{App, Screen};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use rinkaku_core::render::Report;

pub(crate) fn draw_status_line(frame: &mut Frame, app: &App, report: &Report, area: Rect) {
    let text = status_line_text(app, report);

    let style = if app.status().is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    frame.render_widget(Paragraph::new(text).style(style), area);
}

/// The status line's full text (ADR 0020): the current order mode is
/// always shown (the real `crate::order::OrderMode` term, not a
/// paraphrase, so it matches what `o` actually toggles between), and the
/// key-hint segment switches on `app.focus()` while [`Screen::Entry`] —
/// Tree-focused hints are navigation-oriented, Right-focused hints are
/// scroll/hunk-jump-oriented, and both end with a `?` mention so the fuller
/// keymap/glossary overlay is always one keypress away. [`Screen::Source`]
/// keeps its own short "esc/q: back" hint, unaffected by focus (drilling
/// into source is reached only via `Focus::Right` already, so a focus
/// distinction there would be redundant).
///
/// The `]/[: next/prev hunk` hint only appears while Right-focused *and*
/// [`crate::app::RightPane::Diff`] is showing — `crate::run_app` only wires up the
/// `]`/`[` jump for that pane/focus combination (it needs the Diff pane's
/// shaped hunk-offset table, which Detail/BlastRadius have no equivalent
/// of), so advertising the key while Detail/BlastRadius is showing would
/// describe a binding that does nothing there.
///
/// Extracted as its own pure function (no `ratatui` types) so the text
/// itself — not just that *something* renders — is unit-testable, mirroring
/// [`super::scroll::clamp_scroll`]/[`super::scroll::scroll_indicator`]'s own precedent in this module for
/// layout-adjacent pure logic.
///
/// The `⚠ N file-size warnings` suffix (ADR 0028) is appended to the help
/// segment whenever `report.file_size_warnings` is non-empty, so the
/// aggregate count is visible from any pane without a dedicated screen.
/// The suffix is dropped when the vec is empty (mirrors ADR 0013's
/// "Hotspots is skipped when empty" rule).
pub(crate) fn status_line_text(app: &App, report: &Report) -> String {
    let help = match app.screen() {
        Screen::Entry => {
            let order = match app.order_mode() {
                crate::order::OrderMode::Topological => "topological",
                crate::order::OrderMode::AlphaNumeric => "alphabetical",
            };
            let keys = match app.focus() {
                crate::app::Focus::Tree => {
                    "j/k: move  enter: open  space: expand  e/c: expand/collapse  o: order  d: diff  r: blast radius  s: source  gd/gr: jump  ?: help  q: quit"
                }
                crate::app::Focus::Right if app.right_pane() == crate::app::RightPane::Diff => {
                    "j/k: scroll  ctrl-d/u: half  gg/G: top/bot  h/esc: back  ]/[: next/prev hunk  d: diff  r: blast radius  gd/gr: jump  ?: help  q: quit"
                }
                crate::app::Focus::Right => {
                    "j/k: scroll  ctrl-d/u: half  gg/G: top/bot  h/esc: back  d: diff  r: blast radius  gd/gr: jump  ?: help  q: quit"
                }
            };
            format!("order: {order}  |  {keys}")
        }
        Screen::Source { .. } => {
            "j/k: scroll  ctrl-d/u: half  gg/G: top/bot  esc/q: back".to_string()
        }
    };

    let help = match report.file_size_warnings.len() {
        0 => help,
        n => format!("{help}  |  \u{26a0} {n} file-size warnings"),
    };

    match app.status() {
        Some(status) => format!("{status}  |  {help}"),
        None => help,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            file_size_warnings: vec![],
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

    fn empty_report_for_status_line() -> Report {
        Report {
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
            file_size_warnings: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_draw_status_line_help_text_on_entry_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        // Wider than the default 80 columns used elsewhere in this test
        // module: the full help text (order mode + Tree-focus key hints,
        // ADR 0020/0022) is ~155 columns and would otherwise be truncated
        // (the status line intentionally does not wrap), hiding the "quit"
        // fragment this test checks for.
        let mut terminal = Terminal::new(TestBackend::new(170, 20)).expect("terminal");

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
        assert!(text.contains("quit"));
        assert!(text.contains("order: topological"));
    }

    // --- status_line_text (pure helper) ---

    #[test]
    fn should_show_topological_order_and_tree_focus_hints_by_default() {
        let report = empty_report_for_status_line();
        let app = App::new(&report);

        let actual = status_line_text(&app, &report);

        assert_eq!(
            "order: topological  |  j/k: move  enter: open  space: expand  e/c: expand/collapse  o: order  d: diff  r: blast radius  s: source  gd/gr: jump  ?: help  q: quit"
                .to_string(),
            actual
        );
    }

    #[test]
    fn should_show_alphabetical_order_after_toggle_order_is_pressed() {
        let report = empty_report_for_status_line();
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleOrder);

        let actual = status_line_text(&app, &report);

        assert!(actual.starts_with("order: alphabetical  |  "));
    }

    #[test]
    fn should_show_right_focus_hints_with_hunk_jump_when_diff_pane_is_showing() {
        let report = report_with_one_symbol();
        // `Open` on the file row (cursor starts there) reaches Focus::Right
        // (ADR 0020) without leaving Screen::Entry, and lands on
        // `RightPane::Diff` (its default, `f3c4b98`) — the pane the
        // `]/[: next/prev hunk` hint actually applies to.
        let app = App::new(&report).handle_key(crate::app::InputKey::Open);

        let actual = status_line_text(&app, &report);

        assert_eq!(
            "order: topological  |  j/k: scroll  ctrl-d/u: half  gg/G: top/bot  h/esc: back  ]/[: next/prev hunk  d: diff  r: blast radius  gd/gr: jump  ?: help  q: quit"
                .to_string(),
            actual
        );
    }

    #[test]
    fn should_show_right_focus_hints_without_hunk_jump_when_detail_pane_is_showing() {
        let report = report_with_one_symbol();
        // `Open` reaches Focus::Right on RightPane::Diff (its default), then
        // `ToggleDiff` (`d`) switches to RightPane::Detail — the hunk-jump
        // hint must disappear here since `crate::run_app` never wires `]`/`[`
        // up for Detail (finding: `]`/`[` used to fire regardless of pane).
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff);
        assert_eq!(crate::app::RightPane::Detail, app.right_pane());

        let actual = status_line_text(&app, &report);

        assert_eq!(
            "order: topological  |  j/k: scroll  ctrl-d/u: half  gg/G: top/bot  h/esc: back  d: diff  r: blast radius  gd/gr: jump  ?: help  q: quit"
                .to_string(),
            actual
        );
    }

    #[test]
    fn should_show_source_view_scroll_hints_on_source_screen_regardless_of_focus() {
        // The source screen is reached only via the dedicated `s` key
        // (`InputKey::Source`) now, not `Enter` — a dogfooding fix to
        // `InputKey::Open`'s own arm (see its doc comment in `crate::app`).
        // ADR 0026 adds this screen's own scroll bindings to the status
        // line so the reviewer can discover them without opening the
        // help overlay first.
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::Source); // opens Screen::Source

        let actual = status_line_text(&app, &report);

        assert_eq!(
            "j/k: scroll  ctrl-d/u: half  gg/G: top/bot  esc/q: back".to_string(),
            actual
        );
    }

    #[test]
    fn should_prefix_status_message_before_the_help_text_when_set() {
        let report = empty_report_for_status_line();
        let mut app = App::new(&report);
        app.set_status("a source read failed");

        let actual = status_line_text(&app, &report);

        assert!(actual.starts_with("a source read failed  |  order: topological  |  "));
    }

    // ADR 0028: a report carrying at least one file-size warning must
    // append the aggregate count to the help text so the reviewer sees
    // the total from any pane. The count is the raw vec length — no
    // per-severity split here, that lives on the detail pane per ADR
    // 0028's "one signal per surface" split.
    #[test]
    fn should_append_file_size_warning_count_to_status_line_when_report_has_warnings() {
        let report = Report {
            file_size_warnings: vec![
                rinkaku_core::file_size::FileSizeWarning {
                    path: "src/big.rs".to_string(),
                    line_count: 1734,
                    severity: rinkaku_core::file_size::FileSizeSeverity::Warn,
                },
                rinkaku_core::file_size::FileSizeWarning {
                    path: "src/huge.rs".to_string(),
                    line_count: 4837,
                    severity: rinkaku_core::file_size::FileSizeSeverity::Split,
                },
            ],
            ..empty_report_for_status_line()
        };
        let app = App::new(&report);

        let actual = status_line_text(&app, &report);

        assert!(
            actual.ends_with("  |  \u{26a0} 2 file-size warnings"),
            "expected trailing warnings segment, got: {actual}",
        );
    }

    // Companion to the test above: an empty `file_size_warnings` vec
    // leaves the help text untouched — mirrors ADR 0013's "Hotspots is
    // skipped when empty" rule for the Markdown surface.
    #[test]
    fn should_not_append_when_report_has_no_warnings() {
        let report = empty_report_for_status_line();
        let app = App::new(&report);

        let actual = status_line_text(&app, &report);

        assert!(
            !actual.contains("file-size warnings"),
            "expected no warnings segment, got: {actual}",
        );
    }
}
