//! `?` help overlay (ADR 0020) — composited on top of whatever screen was
//! already rendered underneath, after the pane split has drawn everything
//! else. Split out of `ui::overlay` (ADR 0028's file-size threshold): this
//! module holds the keymap/markers/glossary content and its own draw
//! function; `ui::overlay` keeps the smaller popups (jump-target,
//! update-available) plus [`super::overlay::centered_rect`], which both
//! modules share.

use super::overlay::centered_rect;
use super::scroll::{Body, render_scrollable_pane};
use crate::app::{Focus, Screen};
use crate::locale::Locale;
use crate::row_view::{
    band_style, cyan_badge_style, risk_marker_style, split_badge_style, symbol_marker_span,
    symbol_name_style, test_badge_style, warning_badge_style,
};
use crate::tree::SymbolRef;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::file_size::FileSizeBand;

/// The `?` help overlay's content laid out once for `locale`/`screen`/
/// `focus`, independent of the pane's rendered size — extracted from
/// [`draw_help_overlay`] so tests can pin its shape without a live `Frame`,
/// mirroring how `crate::help::help_content` itself is already plain data
/// rather than something computed at draw time (ADR 0055:
/// locale-parameterized, not a `const`, since the underlying strings now
/// come from `rust_i18n::t!`). Only the keymap groups are scoped to
/// `screen`/`focus` (`crate::help::applicable_help_groups`) — the Markers
/// legend and Glossary sections stay unconditional, since neither depends on
/// which pane currently has focus.
fn help_overlay_lines(locale: Locale, screen: &Screen, focus: Focus) -> Vec<Line<'static>> {
    let tag = locale.tag();
    let content = crate::help::help_content(locale);
    let keymap_groups = crate::help::applicable_help_groups(locale, screen, focus);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for group in &keymap_groups {
        lines.push(Line::styled(
            group.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        for binding in &group.bindings {
            lines.push(Line::raw(format!(
                "  {:<16} {}",
                binding.keys, binding.description
            )));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        rust_i18n::t!("help.section.markers", locale = tag).into_owned(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.extend(markers_legend_lines(locale));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        rust_i18n::t!("help.section.glossary", locale = tag).into_owned(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    for entry in &content.glossary {
        lines.push(Line::raw(format!(
            "  {:<16} {}",
            entry.term, entry.explanation
        )));
    }
    lines
}

/// One [`SymbolRef`] per marker case the Markers legend needs a real
/// [`crate::row_view::symbol_marker_span`]/[`crate::row_view::symbol_name_style`]
/// swatch for — fields left at their "no signal" default except the one
/// this case is about, mirroring the minimal fixtures `row_view`'s own
/// tests already build (`row_view_tests::plain_symbol`).
fn legend_symbol(
    classification: Option<Classification>,
    removed: bool,
    is_test: bool,
) -> SymbolRef {
    SymbolRef {
        id: "legend".to_string(),
        name: "legend".to_string(),
        kind: SymbolKind::Function,
        classification,
        removed,
        is_test,
    }
}

const MARKER_SWATCH_COLUMN_WIDTH: usize = 40;

/// Builds the Markers section's lines: one row per
/// [`crate::help::help_content`]'s `markers` legend entry for `locale`, its
/// swatch rendered with the exact [`ratatui::style::Style`]
/// `crate::row_view::entry_row_line` itself would use — a real style, not a
/// prose color name — followed by the entry's explanation. Extracted as its
/// own pure function, mirroring [`help_overlay_lines`]'s own split, so a
/// test can assert on the built `Vec<Line>` without a live `Frame`.
fn markers_legend_lines(locale: Locale) -> Vec<Line<'static>> {
    crate::help::help_content(locale)
        .markers
        .iter()
        .map(|entry| {
            let swatch = marker_swatch_spans(entry.swatch);
            let swatch_width: usize = swatch.iter().map(|span| span.content.len()).sum();
            let padding = MARKER_SWATCH_COLUMN_WIDTH
                .saturating_sub(swatch_width)
                .max(1);
            let mut spans = vec![Span::raw("  ")];
            spans.extend(swatch);
            spans.push(Span::raw(format!(
                "{}{}",
                " ".repeat(padding),
                entry.explanation
            )));
            Line::from(spans)
        })
        .collect()
}

/// Looks up the real style(s) for one [`crate::help::MarkerLegendEntry::swatch`]
/// value, reusing `crate::row_view`'s own style producers so the legend can
/// never drift from what the tree pane actually renders.
fn marker_swatch_spans(swatch: &'static str) -> Vec<Span<'static>> {
    match swatch {
        "+" => vec![symbol_marker_span(&legend_symbol(
            Some(Classification::Added),
            false,
            false,
        ))],
        "~" => vec![symbol_marker_span(&legend_symbol(
            Some(Classification::SignatureChanged),
            false,
            false,
        ))],
        "x" => vec![symbol_marker_span(&legend_symbol(None, true, false))],
        "(dimmed name)" => vec![Span::styled(
            swatch,
            symbol_name_style(&legend_symbol(Some(Classification::BodyOnly), false, false)),
        )],
        "(dimmed + struck-through name)" => vec![Span::styled(
            swatch,
            symbol_name_style(&legend_symbol(None, true, false)),
        )],
        "(cycle)" => vec![Span::styled(
            swatch,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )],
        "!" => vec![Span::styled(swatch, risk_marker_style())],
        "lines:N" => vec![Span::styled(swatch, band_style(FileSizeBand::Watch))],
        "chg:N" => badge_swatch_spans("chg:", cyan_badge_style()),
        "api:N" => badge_swatch_spans("api:", warning_badge_style()),
        "fan-in:N" => badge_swatch_spans("fan-in:", cyan_badge_style()),
        "warn:N" => badge_swatch_spans("warn:", warning_badge_style()),
        "split:N" => badge_swatch_spans("split:", split_badge_style()),
        "tests:0" => vec![
            Span::raw("tests:"),
            Span::styled("0", warning_badge_style()),
        ],
        "[test] (N symbols)" => vec![Span::styled(swatch, test_badge_style())],
        "N tests" => vec![Span::styled(swatch, Style::default().fg(Color::DarkGray))],
        "(skipped: ...)" => vec![Span::styled(swatch, Style::default().fg(Color::DarkGray))],
        _ => vec![Span::raw(swatch)],
    }
}

/// A `label:N` badge swatch split into a plain label span and an `N`
/// numeral span styled with `number_style` — the same label/number split
/// [`crate::row_view::push_badge_spans`] renders on the real tree row.
fn badge_swatch_spans(label: &'static str, number_style: Style) -> Vec<Span<'static>> {
    vec![Span::raw(label), Span::styled("N", number_style)]
}

/// Draws the `?` help overlay (ADR 0020, scrolling added post-hoc once the
/// keymap grew past what always fit on screen — ADR 0026's own "Source
/// view" group plus the `gd`/`gr`/jumplist entries pushed the pre-glossary
/// content past a typical terminal's height) centered over `full_area`: a
/// bordered box roughly 80%/90% of the frame's width/height (capped so it
/// never claims more than the frame itself on a small terminal), listing
/// every keymap group applicable to `screen`/`focus`
/// (`crate::help::applicable_help_groups`, scoped this way so the overlay
/// never lists a binding that would be a no-op if pressed right now) for
/// `locale` (ADR 0055) followed by the markers legend and glossary, both
/// unconditional. [`Clear`] is rendered first so the overlay's background is
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
    locale: Locale,
    screen: &Screen,
    focus: Focus,
) -> (usize, usize) {
    let overlay_area = centered_rect(full_area, 80, 90);
    frame.render_widget(Clear, overlay_area);

    let lines = help_overlay_lines(locale, screen, focus);
    let inner_height = overlay_area.height.saturating_sub(2) as usize;
    let title = format!(
        " {} ",
        rust_i18n::t!("help.overlay_title", locale = locale.tag())
    );
    // Always drawn as focused: this overlay is modal (composited on top of
    // whatever screen was already showing) and is always the surface `?`'s
    // own scroll keys act on while open, so there is no competing pane to
    // distinguish it from (`render_scrollable_pane`'s own doc comment).
    let scroll = render_scrollable_pane(
        frame,
        &title,
        &[],
        Body::Single(&lines),
        requested_scroll,
        overlay_area,
        true,
    );
    (scroll, inner_height)
}

#[cfg(test)]
#[path = "help_overlay/tests.rs"]
mod tests;
