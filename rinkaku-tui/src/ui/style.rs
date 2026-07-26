//! Shared token/gap span styling used by the diff pane and the source
//! screen — extracted so [`styled_content_spans`], [`gap_span`], and
//! [`palette_style`] have a single home rather than being duplicated in
//! two panes that need identical "foreground token color + uniform
//! background tint" composition (ADR 0018).

use crate::highlight::{PALETTE, TokenSpan};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

/// Columns between tab stops when rendering source text (ADR 0061).
pub(crate) const TAB_WIDTH: usize = 4;

/// Rewrites `content` with every tab replaced by spaces up to the next
/// [`TAB_WIDTH`] tab stop, returning the expanded text alongside `spans`
/// rebased onto its byte offsets (ADR 0061) — a terminal draws `\t` as zero
/// cells while this crate's width arithmetic counts it as one, so no tab may
/// reach a rendered [`Span`].
pub(crate) fn expand_tabs(content: &str, spans: &[TokenSpan]) -> (String, Vec<TokenSpan>) {
    if !content.contains('\t') {
        return (content.to_string(), spans.to_vec());
    }

    let mut expanded = String::with_capacity(content.len());
    let mut column = 0usize;
    let mut offsets = Vec::with_capacity(content.len() + 1);

    for (byte_index, ch) in content.char_indices() {
        while offsets.len() <= byte_index {
            offsets.push(expanded.len());
        }
        if ch == '\t' {
            let padding = TAB_WIDTH - (column % TAB_WIDTH);
            expanded.extend(std::iter::repeat_n(' ', padding));
            column += padding;
        } else {
            expanded.push(ch);
            // `unwrap_or(0)`, unlike `crate::ui::scroll`'s `unwrap_or(1)`:
            // this column count only feeds the next tab stop, which a
            // zero-width character must not advance.
            column += ch.width().unwrap_or(0);
        }
    }
    while offsets.len() <= content.len() {
        offsets.push(expanded.len());
    }

    let remapped = spans
        .iter()
        .map(|span| TokenSpan {
            start: offsets.get(span.start).copied().unwrap_or(expanded.len()),
            end: offsets.get(span.end).copied().unwrap_or(expanded.len()),
            palette_index: span.palette_index,
        })
        .collect();

    (expanded, remapped)
}

/// [`expand_tabs`] for a path with no token spans to remap.
pub(crate) fn expand_tabs_text(content: &str) -> String {
    expand_tabs(content, &[]).0
}

/// Splits `content` into styled spans per `spans` (byte-offset [`TokenSpan`]s
/// already rebased to `content`'s own coordinates by
/// `highlight::spans_for_line`), coloring each token's foreground by its
/// palette entry (`palette_style`) and applying `bg` uniformly (the diff
/// signal) — any byte range `spans` doesn't cover (whitespace, punctuation
/// the query didn't capture) becomes an unstyled-foreground span with just
/// `bg` applied, so the line's background tint is always contiguous even
/// where token coloring has gaps.
///
/// Both arguments are in `content`'s pre-expansion coordinates: this
/// function applies [`expand_tabs`] itself, so the bytes it slices are not
/// the bytes the caller passed in (ADR 0061).
pub(crate) fn styled_content_spans(
    content: &str,
    spans: &[TokenSpan],
    bg: Option<Color>,
) -> Vec<Span<'static>> {
    let (content, spans) = expand_tabs(content, spans);
    let content = content.as_str();

    let mut result = Vec::new();
    let mut cursor = 0usize;

    let mut sorted_spans = spans;
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
        Some("comment") => Style::default().fg(Color::DarkGray),
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
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::leading_tab("\tx", "    x")]
    #[case::three_leading_tabs("\t\t\tdeep()", "            deep()")]
    #[case::tab_after_one_column("a\tb", "a   b")]
    #[case::tab_after_three_columns("abc\td", "abc d")]
    #[case::tab_on_stop_boundary("abcd\te", "abcd    e")]
    #[case::consecutive_mid_line_tabs("ab\t\tc", "ab      c")]
    #[case::wide_chars_advance_two_columns("あ\tx", "あ  x")]
    #[case::combining_marks_do_not_advance("a\u{0301}\tb", "a\u{0301}   b")]
    #[case::no_tabs_unchanged("plain text", "plain text")]
    #[case::empty_unchanged("", "")]
    fn should_advance_to_next_tab_stop_when_expanding(
        #[case] content: &str,
        #[case] expected: &str,
    ) {
        let actual = expand_tabs_text(content);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_shift_span_offsets_when_tabs_precede_tokens() {
        let content = "\t\tfn name()";
        let spans = vec![
            TokenSpan {
                start: 2,
                end: 4,
                palette_index: 0,
            },
            TokenSpan {
                start: 5,
                end: 9,
                palette_index: 3,
            },
        ];

        let actual = expand_tabs(content, &spans);

        assert_eq!(
            (
                "        fn name()".to_string(),
                vec![
                    TokenSpan {
                        start: 8,
                        end: 10,
                        palette_index: 0,
                    },
                    TokenSpan {
                        start: 11,
                        end: 15,
                        palette_index: 3,
                    },
                ]
            ),
            actual
        );
    }

    #[test]
    fn should_grow_span_range_when_tab_lies_inside_token() {
        let content = "a\tb";
        let spans = vec![TokenSpan {
            start: 0,
            end: 3,
            palette_index: 1,
        }];

        let actual = expand_tabs(content, &spans);

        assert_eq!(
            (
                "a   b".to_string(),
                vec![TokenSpan {
                    start: 0,
                    end: 5,
                    palette_index: 1,
                }]
            ),
            actual
        );
    }

    #[test]
    fn should_keep_offsets_registered_when_multibyte_chars_precede_tab() {
        let content = "あ\tx";
        let spans = vec![TokenSpan {
            start: 4,
            end: 5,
            palette_index: 4,
        }];

        let actual = expand_tabs(content, &spans);

        assert_eq!(
            (
                "あ  x".to_string(),
                vec![TokenSpan {
                    start: 5,
                    end: 6,
                    palette_index: 4,
                }]
            ),
            actual
        );
    }

    #[test]
    fn should_return_spans_unchanged_when_line_has_no_tab() {
        let content = "let x = 1;";
        let spans = vec![TokenSpan {
            start: 0,
            end: 3,
            palette_index: 0,
        }];

        let actual = expand_tabs(content, &spans);

        assert_eq!(("let x = 1;".to_string(), spans), actual);
    }

    #[test]
    fn should_color_tokens_at_expanded_positions_when_content_has_tabs() {
        let content = "\tif x {";
        let spans = vec![TokenSpan {
            start: 1,
            end: 3,
            palette_index: 0,
        }];

        let actual = styled_content_spans(content, &spans, None);

        assert_eq!(
            vec![
                Span::styled("    ".to_string(), Style::default()),
                Span::styled("if".to_string(), Style::default().fg(Color::Magenta)),
                Span::styled(" x {".to_string(), Style::default()),
            ],
            actual
        );
    }

    #[test]
    fn should_map_every_palette_entry_to_its_pinned_style_when_resolved_by_index() {
        // Pins the full palette-index → style table: `palette_style` falls
        // back to unstyled on an unmapped name, so dropping one arm during
        // a future palette edit would otherwise pass `make test` silently.
        let expected = vec![
            ("keyword", Style::default().fg(Color::Magenta)),
            ("string", Style::default().fg(Color::Yellow)),
            ("comment", Style::default().fg(Color::DarkGray)),
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
