//! Review-annotations overlays (ADR 0048): the compose text-input box and
//! the combined annotations-list/export-menu/verdict-menu surface. Follows
//! `ui::overlay`'s existing `draw_help_overlay`/`draw_jump_popup`
//! precedent — `Clear` the popup's `Rect`, then render `Paragraph`/`List`
//! content built from plain data already sitting on [`ReviewState`], fed
//! in by `crate::ui::draw`.

use super::overlay::centered_rect;
use crate::review::{ReviewMode, ReviewState, Verdict, export_menu_entries};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// Draws whichever review overlay `review.mode()` currently calls for
/// (compose, list, export menu, verdict menu), or nothing at all while
/// [`ReviewMode::Idle`] — the single entry point `crate::ui::draw` calls
/// unconditionally as its final compositing step. `sink_a_available`
/// (`App::review_sink_a_available`) is only consulted by the export menu,
/// which must render the same entries [`ReviewState::confirm_export`]
/// resolves the cursor against.
pub(crate) fn draw_review_overlay(
    frame: &mut Frame,
    review: &ReviewState,
    sink_a_available: bool,
    full_area: Rect,
) {
    match review.mode() {
        ReviewMode::Idle => {}
        ReviewMode::Compose { snapshot, buffer } => {
            draw_compose_overlay(frame, full_area, snapshot, buffer)
        }
        ReviewMode::List { cursor } => draw_annotations_overlay(frame, review, *cursor, full_area),
        ReviewMode::ExportMenu { cursor } => {
            draw_export_menu_overlay(frame, sink_a_available, *cursor, full_area)
        }
        ReviewMode::VerdictMenu { cursor } => draw_verdict_menu_overlay(frame, *cursor, full_area),
    }
}

/// The compose overlay: a title naming the selected location, the
/// in-progress buffer (visually wrapped), and a footer key hint.
fn draw_compose_overlay(
    frame: &mut Frame,
    full_area: Rect,
    snapshot: &crate::review::SelectionSnapshot,
    buffer: &str,
) {
    let overlay_area = centered_rect(full_area, 70, 50);
    frame.render_widget(Clear, overlay_area);

    let title = format!(" New annotation: {} ", compose_title_location(snapshot));
    let block = Block::bordered().title(title);
    let mut lines: Vec<Line<'static>> = vec![Line::raw(buffer.to_string())];
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Enter: save  Esc: cancel",
        Style::default().add_modifier(Modifier::DIM),
    ));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, overlay_area);
}

/// The compose overlay's title location text: `"{path}:{start}-{end}
/// {symbol_name}"`, degrading gracefully when a field is absent — the same
/// fallback shape `crate::review`'s own annotation-heading formatting uses
/// (kept separate rather than shared, since that formats an *annotation's*
/// already-resolved anchor/range, while this formats a live
/// [`crate::review::SelectionSnapshot`] still being composed against).
fn compose_title_location(snapshot: &crate::review::SelectionSnapshot) -> String {
    let range = snapshot.anchor.or(snapshot.range).map(|(start, end)| {
        if start == end {
            format!("{start}")
        } else {
            format!("{start}-{end}")
        }
    });
    match (range, &snapshot.symbol_name) {
        (Some(range), Some(name)) => format!("{}:{range} {name}", snapshot.path),
        (Some(range), None) => format!("{}:{range}", snapshot.path),
        (None, Some(name)) => format!("{} {name}", snapshot.path),
        (None, None) => snapshot.path.clone(),
    }
}

/// The annotations-list overlay: every annotation as `{path}:{anchor}
/// {symbol_name}: {body's first line}`, the list cursor highlighted, plus
/// a key-hint footer and (when set) the last export status message.
fn draw_annotations_overlay(
    frame: &mut Frame,
    review: &ReviewState,
    cursor: usize,
    full_area: Rect,
) {
    let overlay_area = centered_rect(full_area, 80, 60);
    frame.render_widget(Clear, overlay_area);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(status) = review.last_status() {
        lines.push(Line::styled(
            status.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(""));
    }
    if review.annotations().is_empty() {
        lines.push(Line::styled(
            "(no annotations yet — press a over a symbol to add one)",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    for (index, annotation) in review.annotations().iter().enumerate() {
        let text = annotations_list_entry_text(annotation);
        if index == cursor {
            lines.push(Line::styled(
                text,
                Style::default().add_modifier(Modifier::REVERSED),
            ));
        } else {
            lines.push(Line::raw(text));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "j/k: move  Enter: export  d: delete  Esc/q: close",
        Style::default().add_modifier(Modifier::DIM),
    ));

    let block = Block::bordered().title(" Review annotations ");
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, overlay_area);
}

/// One annotations-list row's text: `"{path}:{anchor-or-range}
/// {symbol_name}: {body's first line}"`, degrading the location half the
/// same way [`compose_title_location`] does.
fn annotations_list_entry_text(annotation: &crate::review::Annotation) -> String {
    let location = &annotation.location;
    let range = location.anchor.or(location.range).map(|(start, end)| {
        if start == end {
            format!("{start}")
        } else {
            format!("{start}-{end}")
        }
    });
    let location_text = match (range, &location.symbol_name) {
        (Some(range), Some(name)) => format!("{}:{range} {name}", location.path),
        (Some(range), None) => format!("{}:{range}", location.path),
        (None, Some(name)) => format!("{} {name}", location.path),
        (None, None) => location.path.clone(),
    };
    let body_first_line = annotation.body.lines().next().unwrap_or("");
    format!("{location_text}: {body_first_line}")
}

/// The export menu: `GitHub PR review` only when `sink_a_available` (ADR
/// 0048's "no implicit fallback" decision), built from
/// [`export_menu_entries`] — the exact same entry list
/// [`ReviewState::confirm_export`] resolves the menu's cursor against, so
/// what the reviewer sees at a given position and what confirming that
/// position does can never disagree.
fn draw_export_menu_overlay(
    frame: &mut Frame,
    sink_a_available: bool,
    cursor: usize,
    full_area: Rect,
) {
    let overlay_area = centered_rect(full_area, 50, 30);
    frame.render_widget(Clear, overlay_area);

    let entries = export_menu_entries(sink_a_available);
    let lines: Vec<Line<'static>> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| menu_line(entry.label(), index == cursor))
        .collect();

    let block = Block::bordered().title(" Export to (enter: choose, esc: cancel) ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay_area);
}

/// The verdict menu: Approve/Request changes/Comment, mirroring GitHub's
/// own pending-review submit dialog.
fn draw_verdict_menu_overlay(frame: &mut Frame, cursor: usize, full_area: Rect) {
    let overlay_area = centered_rect(full_area, 50, 30);
    frame.render_widget(Clear, overlay_area);

    let entries = [Verdict::Approve, Verdict::RequestChanges, Verdict::Comment];
    let lines: Vec<Line<'static>> = entries
        .iter()
        .enumerate()
        .map(|(index, verdict)| menu_line(verdict_label(*verdict), index == cursor))
        .collect();

    let block = Block::bordered().title(" Submit review as (enter: choose, esc: cancel) ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay_area);
}

fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
        Verdict::Comment => "Comment",
    }
}

/// One menu row, highlighted (reversed) when `selected`.
fn menu_line(label: &str, selected: bool) -> Line<'static> {
    let span = Span::raw(label.to_string());
    let line = Line::from(vec![span]);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

#[cfg(test)]
#[path = "review_overlay_tests.rs"]
mod tests;
