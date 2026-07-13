//! Detail right-pane: classification/signature/used-by/callees for a
//! selected symbol, badge breakdown for a directory row, or the file-level
//! summary for a file row. Layout only — the underlying view models
//! (`DetailView`, `DirDetail`, `FileDetail`) come from `crate::detail`.

use super::scroll::render_scrollable_pane;
use crate::app::{App, SelectedDetail};
use crate::detail::{DetailView, DirDetail, FileDetail, SignatureView};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use rinkaku_core::extract::Classification;
use rinkaku_core::render::{Report, ReportOrigin};

/// Returns the clamped scroll offset actually applied (`render_scrollable_pane`'s
/// own doc comment on why the caller — ultimately `crate::run_app` — needs
/// this), or `None` when the placeholder path was taken (nothing scrollable
/// was rendered at all).
pub(crate) fn draw_detail_pane(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    area: Rect,
) -> Option<usize> {
    let Some(detail) = app.selected_detail(report) else {
        let block = Block::bordered().title(" Detail ");
        let paragraph = Paragraph::new("(select a row to see its detail)")
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return None;
    };

    let lines = match &detail {
        SelectedDetail::Symbol(detail) => detail_lines(detail),
        SelectedDetail::Dir(detail) => dir_detail_lines(detail, report.origin),
        SelectedDetail::File(detail) => file_detail_lines(detail),
    };
    Some(render_scrollable_pane(
        frame,
        " Detail ",
        &lines,
        app.right_pane_scroll(),
        area,
    ))
}

/// Formats a [`DirDetail`] into displayable lines: a badge breakdown, its
/// own top fan-in symbols, and — only when this directory is in a cycle —
/// the partner directories and the concrete cross-directory edges forming
/// it (TUI iteration 2's answer to "cycle と言われても何が cycle してるか
/// 分からない").
///
/// `origin` picks the first badge's label: `Report::files`' symbol count is
/// exactly the same aggregation in both modes (`Badges::changed_symbols` is
/// not renamed — ADR 0017 only asks for the label to stop implying a diff),
/// but "changed symbols" would misdescribe a whole-repo outline the same
/// way `render.rs`'s "## Change graph"/"## Repository graph" split avoids
/// for Markdown.
pub(crate) fn dir_detail_lines(detail: &DirDetail, origin: ReportOrigin) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("Dir {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));
    let symbols_label = match origin {
        ReportOrigin::Diff => "changed symbols",
        ReportOrigin::RepoOutline => "symbols",
    };
    lines.push(Line::raw(format!(
        "{symbols_label}: {}",
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

/// Formats a [`FileDetail`] into displayable lines: a `File <path>` header,
/// then a skipped-file explanation (returning early — a skipped file has no
/// `symbols`/`test_symbol_count` of its own to show alongside it,
/// `crate::tree::TreeNode::skip_reason`'s own doc comment on why that half
/// of the split is a true either/or), followed by a whole/mixed-test-file
/// note when `test_symbol_count` is set, followed by the ordinary "Symbols
/// (N)" listing when `symbols` is non-empty. The last two are **not**
/// mutually exclusive — `pipeline::partition_test_symbols` can populate both
/// a `FileReport` (real, non-test symbols) and a `TestFileSummary` (a test
/// count) for the same mixed-test-code path (`TreeNode::test_symbol_count`'s
/// own doc comment), so a mixed file shows both the test note and its real
/// symbols rather than one silently hiding the other. Without the
/// skip/test-note lines at all, a skipped or whole-test file's detail pane
/// would show a bare "Symbols (0)" — indistinguishable from a file that
/// genuinely changed nothing, which is exactly the gap this feature closes
/// for the entry-tree row too (see `row_view::entry_row_line`).
pub(crate) fn file_detail_lines(detail: &FileDetail) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::styled(
        format!("File {}", detail.path),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));

    if let Some(reason) = detail.skip_reason {
        lines.push(Line::styled(
            format!(
                "Skipped: {}",
                rinkaku_core::render::skip_reason_label(reason)
            ),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::raw("rinkaku did not extract symbols from this file."));
        return lines;
    }

    if let Some(symbol_count) = detail.test_symbol_count {
        let noun = if symbol_count == 1 {
            "symbol"
        } else {
            "symbols"
        };
        lines.push(Line::styled(
            format!("Test file: {symbol_count} changed test {noun}"),
            Style::default().fg(Color::Magenta),
        ));
        lines.push(Line::raw(
            "Changed test-code symbols in this file are excluded from the view because --exclude-tests is set (omit it to include them).",
        ));
        if !detail.symbols.is_empty() {
            lines.push(Line::raw(""));
        }
    }

    // ADR 0028: an oversized-file warning renders above the "Symbols"
    // listing, matching how `top_fan_in` sits above the equivalent
    // rows on `DirDetail`. Skipped/deleted files never reach this point
    // (the skip-reason arm returns early above), so the warning only
    // shows for files rinkaku actually measured.
    if let Some(warning) = &detail.size_warning {
        lines.push(Line::styled(
            file_size_warning_line(warning),
            Style::default().fg(match warning.severity {
                rinkaku_core::file_size::FileSizeSeverity::Warn => Color::Yellow,
                rinkaku_core::file_size::FileSizeSeverity::Split => Color::Red,
            }),
        ));
        lines.push(Line::raw(""));
    }

    if !detail.symbols.is_empty() || detail.test_symbol_count.is_none() {
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
    }

    lines
}

/// Formats one file-size warning (ADR 0028) as the single line the Detail
/// pane shows above the "Symbols" listing. The glyph tracks severity
/// (`⚠` for `Warn`, `🚨` for `Split`) and the trailing hint names the
/// threshold that was crossed, mirroring the Markdown surface's wording.
fn file_size_warning_line(warning: &rinkaku_core::file_size::FileSizeWarning) -> String {
    use rinkaku_core::file_size::{FileSizeSeverity, SPLIT_LINE_THRESHOLD, WARN_LINE_THRESHOLD};
    match warning.severity {
        FileSizeSeverity::Warn => format!(
            "\u{26a0} {} lines \u{2014} consider splitting (>{WARN_LINE_THRESHOLD} watch)",
            warning.line_count,
        ),
        FileSizeSeverity::Split => format!(
            "\u{1f6a8} {} lines \u{2014} over the {SPLIT_LINE_THRESHOLD}-line split threshold",
            warning.line_count,
        ),
    }
}

pub(crate) fn kind_abbrev(kind: rinkaku_core::extract::SymbolKind) -> &'static str {
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
pub(crate) fn detail_lines(detail: &DetailView) -> Vec<Line<'static>> {
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

#[cfg(test)]
mod tests {
    use super::file_detail_lines;
    use crate::app::{App, BlastRadiusSelection};
    use crate::detail::FileDetail;
    use crate::ui::{DrawOutcome, draw};
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

    #[test]
    fn should_draw_placeholder_message_when_there_are_no_rows_at_all() {
        let report = Report {
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
        };
        // ADR 0020 defaults the right pane to Diff, whose own placeholder
        // text differs ("select a symbol or file row..."); `ToggleDiff`
        // switches to Detail, whose placeholder is what this test checks.
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
        assert!(text.contains("select a row"));
    }

    #[test]
    fn should_draw_dir_detail_content_when_cursor_is_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
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
            file_size_warnings: vec![],
            removed: vec![],
        };
        // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
        // Detail, which is what this test actually exercises. (A directory
        // row has no diff-specific content of its own, so leaving it on the
        // default Diff pane would just show that pane's placeholder rather
        // than the dir-detail content this test checks for.)
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
        assert!(text.contains("Dir src"));
        assert!(text.contains("changed symbols:"));
        assert!(text.contains("Top fan-in"));
    }

    // ADR 0017: a whole-repo outline's directory detail must not say
    // "changed symbols" — nothing changed in that mode — so this pins
    // `dir_detail_lines`'s label switching on `report.origin`, using the
    // same report shape as
    // `should_draw_dir_detail_content_when_cursor_is_on_a_directory_row`
    // above (differing only in `origin`) so the two tests read as a pair.
    #[test]
    fn should_draw_symbols_label_without_changed_wording_when_origin_is_repo_outline() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::RepoOutline,
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
            file_size_warnings: vec![],
            removed: vec![],
        };
        // See the sibling test above for why `ToggleDiff` is needed to
        // reach the Detail pane this test actually exercises.
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
        assert!(text.contains("Dir src"));
        assert!(text.contains("symbols:"));
        assert!(!text.contains("changed symbols:"));
    }

    /// A [`Report`] whose only entry is a skipped file (no `files`, no
    /// `tests`) — pairs with `report_with_one_symbol` for the detail-pane
    /// tests below.
    fn report_with_one_skipped_file() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
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
        }
    }

    #[test]
    fn should_draw_skip_reason_in_detail_pane_when_cursor_is_on_a_skipped_file_row() {
        let report = report_with_one_skipped_file();
        // Row 0 is the collapsing "assets" dir (single child, see
        // `crate::tree::build_tree`'s collapsing rule); row 1 is the
        // skipped "logo.png" file itself. ADR 0020 defaults the right pane
        // to Diff, so `ToggleDiff` is needed here to reach Detail (unlike
        // the pre-v2 default this test originally relied on).
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
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
        assert!(text.contains("File assets/logo.png"));
        assert!(text.contains("Skipped: binary"));
        assert!(!text.contains("Symbols ("));
    }

    /// A [`Report`] whose only entry is a whole-test-file summary (no
    /// `files`, no `skipped`).
    fn report_with_one_test_file() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![rinkaku_core::render::TestFileSummary {
                path: "src/lib_test.go".to_string(),
                symbol_count: 3,
            }],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_draw_test_symbol_count_in_detail_pane_when_cursor_is_on_a_whole_test_file_row() {
        let report = report_with_one_test_file();
        // Row 0 is the collapsing "src" dir; row 1 is the whole test file.
        // ADR 0020 defaults the right pane to Diff, so `ToggleDiff` is
        // needed here to reach Detail.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
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
        assert!(text.contains("File src/lib_test.go"));
        // "Test file: 3 changed test symbols" wraps across two rendered
        // lines at this terminal's pane width, so assert on a substring
        // that survives the wrap rather than the whole phrase.
        assert!(text.contains("Test file: 3 changed test"));
        assert!(!text.contains("Symbols ("));
    }

    // Regression test (post-rebase integration check): a mixed file — real
    // symbols in `report.files` *and* a test-symbol count in `report.tests`
    // for the same path (`pipeline::partition_test_symbols`'s legitimate
    // output for a file with both production and test code changed) — must
    // show both the test-file note and the real "Symbols (N)" listing in
    // the detail pane, not just one. This is the exact shape that caused a
    // live panic (`rinkaku-tui/src/app.rs` in this repo's own diff) before
    // `TreeBuilder::insert_test_file` stopped rejecting a file that already
    // carries real symbols.
    #[test]
    fn should_draw_both_test_note_and_real_symbols_in_detail_pane_when_file_is_mixed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "app.rs".to_string(),
                symbols: vec![symbol("app.rs::handle_key", "handle_key")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![rinkaku_core::render::TestFileSummary {
                path: "app.rs".to_string(),
                symbol_count: 5,
            }],
            hotspots: vec![],
            file_size_warnings: vec![],
            removed: vec![],
        };
        // Row 0 is the "app.rs" file row itself.
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
        assert!(text.contains("File app.rs"));
        assert!(text.contains("Test file: 5 changed test"));
        assert!(text.contains("Symbols (1)"));
        assert!(text.contains("handle_key"));
    }

    #[test]
    fn should_draw_detail_pane_content_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
        // Detail, which is what this test actually exercises.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::ToggleDiff);
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
        assert!(text.contains("foo"));
        assert!(text.contains("Used by"));
    }

    // --- rendered scroll behavior (TestBackend) ---

    /// A report whose single file has `count` symbols, each referencing
    /// `report_with_one_symbol`'s pattern but repeated enough times that
    /// `file_detail_lines` produces more lines than a typical test
    /// viewport's height — used to exercise `draw_detail_pane`'s scrolling
    /// and overflow-indicator paths, which need content that does not fit
    /// in one screen.
    fn report_with_many_symbols(count: usize) -> Report {
        let symbols: Vec<ExtractedSymbol> = (0..count)
            .map(|i| symbol(&format!("lib.rs::sym{i}"), &format!("sym{i}")))
            .collect();
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols,
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

    #[test]
    fn should_show_overflow_indicator_in_detail_pane_title_when_content_exceeds_viewport() {
        // Row 0 is the "lib.rs" file row itself: `file_detail_lines` lists
        // a "File lib.rs" header, a blank line, a "Symbols (40)" header,
        // then all 40 symbols (43 lines total) — comfortably more than a
        // 20-row terminal's inner pane height can show at once.
        let report = report_with_many_symbols(40);
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
        // Exact bounds depend on the pane's inner height (20 - 2 for the
        // status line/border layout), so this only pins the shape/start
        // rather than the literal end number, keeping the test robust to
        // an unrelated layout tweak elsewhere in this module.
        assert!(text.contains("Detail (1-"));
        assert!(text.contains("/43)"));
    }

    #[test]
    fn should_not_show_overflow_indicator_when_content_fits_the_viewport() {
        let report = report_with_one_symbol();
        // See the test above for why `ToggleDiff` is needed to reach the
        // Detail pane this test actually exercises.
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
        assert!(text.contains(" Detail "));
        assert!(!text.contains("Detail ("));
    }

    #[test]
    fn should_scroll_detail_pane_content_down_when_scroll_down_is_pressed() {
        let report = report_with_many_symbols(40);
        // `Open` on the file row (cursor starts there) reaches Focus::Right
        // (ADR 0020) without changing the selected row itself, so `Down`
        // afterward scrolls instead of moving the cursor. `ToggleDiff`
        // switches from the default Diff pane to Detail, which is what this
        // test actually exercises.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff)
            .handle_key(crate::app::InputKey::Down);
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
        // One line scrolled down: the first visible content line is now 2
        // instead of 1, and the "File lib.rs" header line (the very first
        // content line, before the two blank/"Symbols (40)" header lines
        // that precede the actual symbol list) has scrolled out of view.
        assert!(text.contains("Detail (2-"));
        assert!(!text.contains("File lib.rs"));
    }

    #[test]
    fn should_clamp_detail_pane_scroll_at_the_last_page() {
        // Request an enormous scroll far past the end of a 40-symbol
        // report; the pane must clamp to its last full page rather than
        // showing a mostly-blank pane past the end of the content.
        let report = report_with_many_symbols(40);
        // `ToggleDiff` switches from the default Diff pane to Detail, which
        // is what this test actually exercises.
        let mut app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff);
        for _ in 0..1000 {
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

        let text = buffer_text(&terminal);
        // The last symbol must be visible once clamped to the final page.
        assert!(text.contains("sym39"));
    }

    #[test]
    fn should_return_the_clamped_scroll_from_draw_when_requested_scroll_overshoots() {
        // Dogfooding fix: `draw` must hand back the *clamped* offset it
        // actually rendered (not the caller's unclamped `right_pane_scroll`
        // request), since `crate::run_app` folds this return value back into
        // `App` so an overshot scroll request cannot silently outlive the
        // frame that visibly clamped it.
        let report = report_with_many_symbols(40);
        let mut app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff);
        for _ in 0..1000 {
            app = app.handle_key(crate::app::InputKey::Down);
        }
        assert!(
            app.right_pane_scroll() > 100,
            "the unclamped request must actually have overshot for this test to be meaningful"
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

        let mut actual = DrawOutcome::default();
        terminal
            .draw(|frame| {
                actual = draw(
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

        let clamped = actual
            .clamped_right_pane_scroll
            .expect("right pane rendered scrollable content, so a clamped offset must be reported");
        assert!(
            clamped < app.right_pane_scroll(),
            "clamped scroll must be strictly less than the overshot request"
        );
    }

    #[test]
    fn should_reset_scroll_indicator_when_cursor_moves_to_a_different_row() {
        // Scroll down on the file row's detail, then move the cursor onto
        // a symbol row: `App::handle_key`'s reset-on-cursor-move rule means
        // the newly selected row's own (short) detail must render from the
        // top, not carry over the file row's scroll offset.
        let report = report_with_many_symbols(40);
        // `ToggleDiff` switches from the default Diff pane to Detail, which
        // is what this test actually exercises.
        let app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::Down)
            .handle_key(crate::app::InputKey::FocusLeft)
            .handle_key(crate::app::InputKey::Down);
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
        // A single symbol's own detail (used-by/callees, both empty here)
        // fits well within the viewport, so no overflow indicator should
        // appear even though the file row's detail definitely overflowed.
        assert!(text.contains(" Detail "));
        assert!(!text.contains("Detail ("));
    }

    // --- long-line scroll reachability regression (TestBackend) ---

    #[test]
    fn should_reach_the_last_wrapped_line_of_content_via_scrolling_when_a_logical_line_is_long_enough_to_wrap()
     {
        // A narrow pane (30 inner columns after the 2-column border) with a
        // single logical line far longer than that — mirrors a real fan-in
        // entry's full path being too long for the pane. Before wrapping was
        // applied before the scroll offset, the scroll unit (logical lines)
        // and the render unit (wrapped rows) disagreed, so a marker placed
        // near the end of this one long logical line was unreachable at any
        // scroll offset. Regression coverage for that desync.
        let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: long_line.clone(),
                symbols: vec![],
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
        // ADR 0020 defaults the right pane to Diff, whose own placeholder
        // text also happens to embed the file path (`"(no diff hunks found
        // for <path>)"`) — but not through this test's actual target,
        // `render_scrollable_pane`'s wrap-before-scroll behavior, so
        // `ToggleDiff` switches to Detail to keep exercising that.
        let mut app = App::new(&report)
            .handle_key(crate::app::InputKey::Open)
            .handle_key(crate::app::InputKey::ToggleDiff);
        // Scroll far enough down to reach the wrapped tail of the long path
        // line, however many wrapped rows that turns out to be.
        for _ in 0..200 {
            app = app.handle_key(crate::app::InputKey::Down);
        }
        let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

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
        assert!(text.contains("TAIL_MARKER"));
    }

    #[test]
    fn should_report_indicator_total_as_wrapped_row_count_not_logical_line_count_when_a_line_wraps()
    {
        // Same narrow pane/long-path setup as the reachability regression
        // above: the file row's detail is exactly 2 logical lines ("File
        // <path>" plus a blank line, since this report has no symbols), but
        // the long path line wraps into several rows — the indicator's
        // "/total" must count wrapped rows, not the 2 logical lines, or the
        // indicator would (wrongly) claim everything fits and hide it
        // entirely.
        let long_line = format!("{}TAIL_MARKER", "x".repeat(200));
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: long_line,
                symbols: vec![],
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
        // ADR 0020 defaults the right pane to Diff; `ToggleDiff` switches to
        // Detail, which is what this test actually exercises.
        let app = App::new(&report).handle_key(crate::app::InputKey::ToggleDiff);
        let mut terminal = Terminal::new(TestBackend::new(34, 12)).expect("terminal");

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
        // Inner width is 34 - 2 = 32 columns; the long line alone wraps into
        // ceil(211 / 32) = 7 rows, well over the "/2" a logical-line count
        // would have produced.
        assert!(text.contains("Detail (1-"));
        assert!(!text.contains("/2)"));
    }

    // ADR 0028: a `FileDetail` carrying a `size_warning` renders one
    // extra line above the "Symbols" listing, with a severity-matched
    // glyph and the crossed threshold named in the trailing hint. The
    // `⚠` and `>1500 watch` wording pinned here is the same shape the
    // Markdown surface uses, so a reviewer skimming either output reads
    // the same signal.
    #[test]
    fn should_render_size_warning_line_when_file_detail_has_size_warning() {
        let detail = FileDetail {
            path: "src/big.rs".to_string(),
            symbols: vec![],
            skip_reason: None,
            test_symbol_count: None,
            size_warning: Some(rinkaku_core::file_size::FileSizeWarning {
                path: "src/big.rs".to_string(),
                line_count: 1734,
                severity: rinkaku_core::file_size::FileSizeSeverity::Warn,
            }),
        };

        let lines = file_detail_lines(&detail);

        // NOTE: partial assert — the "File src/big.rs" header, the blank
        // line, and the "Symbols (0)" listing come from unrelated arms
        // of `file_detail_lines`; this test only pins the warning-line
        // portion (a whole-vec compare would double-book coverage the
        // sibling tests already have).
        let rendered: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line == "\u{26a0} 1734 lines \u{2014} consider splitting (>1500 watch)"),
            "expected watch-severity warning line among: {rendered:?}",
        );
    }

    // Companion to the Warn test above: the `Split` variant swaps the
    // glyph to `🚨` and names the split threshold in the trailing hint.
    #[test]
    fn should_render_split_severity_glyph_when_file_detail_size_warning_is_split() {
        let detail = FileDetail {
            path: "src/huge.rs".to_string(),
            symbols: vec![],
            skip_reason: None,
            test_symbol_count: None,
            size_warning: Some(rinkaku_core::file_size::FileSizeWarning {
                path: "src/huge.rs".to_string(),
                line_count: 4837,
                severity: rinkaku_core::file_size::FileSizeSeverity::Split,
            }),
        };

        let lines = file_detail_lines(&detail);

        let rendered: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line
                    == "\u{1f6a8} 4837 lines \u{2014} over the 2000-line split threshold"),
            "expected split-severity warning line among: {rendered:?}",
        );
    }
}
