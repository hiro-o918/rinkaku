//! Shared token/gap span styling used by the diff pane and the source
//! screen — extracted so [`styled_content_spans`], [`gap_span`], and
//! [`palette_style`] have a single home rather than being duplicated in
//! two panes that need identical "foreground token color + uniform
//! background tint" composition (ADR 0018).

use crate::highlight::{PALETTE, TokenSpan};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// Splits `content` into styled spans per `spans` (byte-offset [`TokenSpan`]s
/// already rebased to `content`'s own coordinates by
/// `highlight::spans_for_line`), coloring each token's foreground by its
/// palette entry (`palette_style`) and applying `bg` uniformly (the diff
/// signal) — any byte range `spans` doesn't cover (whitespace, punctuation
/// the query didn't capture) becomes an unstyled-foreground span with just
/// `bg` applied, so the line's background tint is always contiguous even
/// where token coloring has gaps.
pub(crate) fn styled_content_spans(
    content: &str,
    spans: &[TokenSpan],
    bg: Option<Color>,
) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut cursor = 0usize;

    let mut sorted_spans = spans.to_vec();
    sorted_spans.sort_by_key(|span| span.start);

    for span in &sorted_spans {
        if span.start > cursor {
            result.push(gap_span(&content[cursor..span.start], bg));
        }
        let mut style = palette_style(span.palette_index);
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        result.push(Span::styled(
            content[span.start..span.end].to_string(),
            style,
        ));
        cursor = span.end;
    }
    if cursor < content.len() {
        result.push(gap_span(&content[cursor..], bg));
    }
    if result.is_empty() {
        // Only reachable when `content` is empty AND no token spans exist
        // (a blank added/removed line): non-empty content always yields at
        // least one gap or token span above. The empty span keeps the
        // line's background tint rendering on blank lines too.
        result.push(gap_span("", bg));
    }

    result
}

pub(crate) fn gap_span(text: &str, bg: Option<Color>) -> Span<'static> {
    let mut style = Style::default();
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    Span::styled(text.to_string(), style)
}

/// Border style for a pane's `Block`, keyed only on whether that pane
/// currently has focus (`crate::app::Focus`) — dogfooding finding: every
/// pane's `Block::bordered()` looked identical regardless of which one
/// `Tab`/`h`/`l` had routed motion keys to, so a reviewer had no visual way
/// to tell which pane `j`/`k` would actually move. Centralized here rather
/// than matched inline in each of `draw_tree_pane`/`render_scrollable_pane`/
/// the placeholder `Block`s so the two states can never drift apart between
/// panes.
///
/// Focused uses `Color::Cyan` (the crate's existing accent color — the
/// splash screen's logo/progress gauge and the tree pane's `chg:`/`fan-in:`
/// badge counts, `crate::row_view::push_badge_spans`, already use it) plus
/// `Modifier::BOLD` so the focused border reads as "active" rather than just
/// a different hue. Unfocused is a plain `Color::DarkGray` with no
/// `Modifier::DIM` stacked on top — a sibling fix (comment-token styling,
/// ADR-less color cleanup) is removing that exact double-dimming
/// combination elsewhere in this crate, so a new call site is deliberately
/// not reintroducing it.
pub(crate) fn pane_border_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Maps a [`PALETTE`] index to its display style — the minimal token
/// palette ADR 0018 asks for. Falls back to the default (unstyled)
/// foreground for a palette index this match doesn't special-case (there
/// are none today; `PALETTE`'s entries are all listed below, but keeping
/// this a `match` with a wildcard rather than a same-length array means
/// adding a `PALETTE` entry without a style here degrades to unstyled
/// rather than panicking on an out-of-bounds array index).
pub(crate) fn palette_style(palette_index: usize) -> Style {
    match PALETTE.get(palette_index).copied() {
        Some("keyword") => Style::default().fg(Color::Magenta),
        Some("string") => Style::default().fg(Color::Yellow),
        Some("comment") => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
        Some("function") => Style::default().fg(Color::Blue),
        Some("type") => Style::default().fg(Color::Cyan),
        Some("number") => Style::default().fg(Color::LightRed),
        Some("constant") => Style::default().fg(Color::LightRed),
        Some("property") => Style::default().fg(Color::LightBlue),
        Some("variable") => Style::default(),
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_map_every_palette_entry_to_its_pinned_style_when_resolved_by_index() {
        // Pins the full palette-index → style table: `palette_style` falls
        // back to unstyled on an unmapped name, so dropping one arm during
        // a future palette edit would otherwise pass `make test` silently.
        let expected = vec![
            ("keyword", Style::default().fg(Color::Magenta)),
            ("string", Style::default().fg(Color::Yellow)),
            (
                "comment",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            ("function", Style::default().fg(Color::Blue)),
            ("type", Style::default().fg(Color::Cyan)),
            ("number", Style::default().fg(Color::LightRed)),
            ("constant", Style::default().fg(Color::LightRed)),
            ("property", Style::default().fg(Color::LightBlue)),
            ("variable", Style::default()),
        ];

        let actual: Vec<(&str, Style)> = crate::highlight::PALETTE
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, palette_style(index)))
            .collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_bold_cyan_style_when_pane_is_focused() {
        let expected = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let actual = pane_border_style(true);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_plain_dark_gray_style_when_pane_is_unfocused() {
        // Deliberately no `Modifier::DIM` stacked on top of `DarkGray` — a
        // sibling fix elsewhere in this crate is removing that exact
        // combination from the comment-token style, so this pane border
        // must not reintroduce it (`pane_border_style`'s own doc comment).
        let expected = Style::default().fg(Color::DarkGray);

        let actual = pane_border_style(false);

        assert_eq!(expected, actual);
    }
}
