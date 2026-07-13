//! Detail right-pane: classification/signature/used-by/callees for a
//! selected symbol, badge breakdown for a directory row, or the file-level
//! summary for a file row. Layout only — the underlying view models
//! (`DetailView`, `DirDetail`, `FileDetail`) come from `crate::detail`.

use super::scroll::render_scrollable_pane;
use super::style::pane_border_style;
use crate::app::{App, Focus, SelectedDetail};
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
    let focused = app.focus() == Focus::Right;
    let Some(detail) = app.selected_detail(report) else {
        let block = Block::bordered()
            .title(" Detail ")
            .border_style(pane_border_style(focused));
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
        focused,
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
            // ADR 0013 amendment (2026-07-13, relabeled by ADR 0034): the
            // compact fan-in badge matches `row_view::push_badge_spans`'
            // `fan-in:N` labeling so the two panes agree on the badge
            // legend.
            let fan_in = if symbol.fan_in > 0 {
                format!(" fan-in:{}", symbol.fan_in)
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
/// pane shows above the "Symbols" listing. Uses a text label
/// (`Warn:` / `Split:`) instead of an emoji glyph — terminal emoji
/// rendering width is inconsistent enough to distort the pane's
/// column layout, and severity is already legible from the whole-line
/// color (yellow for Warn, red for Split; applied at the call site so
/// this pure formatter stays a plain `String`). The trailing hint
/// names the threshold that was crossed, mirroring the Markdown
/// surface's wording without the glyph.
fn file_size_warning_line(warning: &rinkaku_core::file_size::FileSizeWarning) -> String {
    use rinkaku_core::file_size::{FileSizeSeverity, SPLIT_LINE_THRESHOLD, WARN_LINE_THRESHOLD};
    match warning.severity {
        FileSizeSeverity::Warn => format!(
            "Warn: {} lines \u{2014} consider splitting (>{WARN_LINE_THRESHOLD} watch)",
            warning.line_count,
        ),
        FileSizeSeverity::Split => format!(
            "Split: {} lines \u{2014} over the {SPLIT_LINE_THRESHOLD}-line split threshold",
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
#[path = "detail_pane_tests/mod.rs"]
mod tests;
