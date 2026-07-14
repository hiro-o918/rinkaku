use super::*;
use crate::row_view::{BadgeContext, push_badge_spans};
use pretty_assertions::assert_eq;

fn bold(text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

fn tree_badge_line(badges: &Badges) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    push_badge_spans(&mut spans, badges, BadgeContext::File);
    Line::from(spans)
}

fn range_line(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("range: "),
        Span::styled(
            text.to_string(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ])
}

#[test]
fn should_join_symbol_name_and_path_on_first_header_line_when_selection_name_is_present() {
    let badges = Badges::default();

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &[], 80);

    assert_eq!(vec![bold("foo · src/lib.rs")], actual);
}

#[test]
fn should_show_bare_path_on_first_header_line_when_no_selection_name() {
    let badges = Badges::default();

    let actual = diff_pane_header_lines(None, "src/lib.rs", &badges, &[], 80);

    assert_eq!(vec![bold("src/lib.rs")], actual);
}

#[test]
fn should_render_line_2_span_for_span_from_the_same_badges_the_tree_row_would_render() {
    // Single-source-of-truth check: the diff pane's line 2 must be
    // exactly what `push_badge_spans(..., BadgeContext::File)` produces
    // for the same badges — no drift between the tree row and the header.
    let badges = Badges {
        changed_symbols: 1,
        contract_changes: 1,
        fan_in: 4,
        own_file_size_band: None,
        own_file_line_count: None,
        file_size_warn_count: 0,
        file_size_split_count: 0,
    };

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &[], 80);

    assert_eq!(
        vec![bold("foo · src/lib.rs"), tree_badge_line(&badges)],
        actual
    );
}

#[test]
fn should_omit_line_2_when_badges_are_all_zero() {
    let badges = Badges::default();

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &[], 80);

    assert_eq!(vec![bold("foo · src/lib.rs")], actual);
}

#[test]
fn should_append_range_line_when_ranges_is_non_empty() {
    let badges = Badges::default();
    let ranges = [(23, 41), (57, 60)];

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &ranges, 80);

    assert_eq!(
        vec![bold("foo · src/lib.rs"), range_line("23-41, 57-60")],
        actual
    );
}

#[test]
fn should_render_single_line_range_without_a_dash() {
    let badges = Badges::default();
    let ranges = [(5, 5)];

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &ranges, 80);

    assert_eq!(vec![bold("foo · src/lib.rs"), range_line("5")], actual);
}

#[test]
fn should_omit_range_line_entirely_when_ranges_is_empty() {
    let badges = Badges {
        changed_symbols: 1,
        ..Badges::default()
    };

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &badges, &[], 80);

    assert_eq!(
        vec![bold("foo · src/lib.rs"), tree_badge_line(&badges)],
        actual
    );
}

#[test]
fn should_truncate_first_header_line_keeping_the_tail_when_it_overflows_width() {
    let badges = Badges::default();

    let actual = diff_pane_header_lines(
        Some("very_long_symbol_name_here"),
        "src/very/deeply/nested/module/lib.rs",
        &badges,
        &[],
        20,
    );

    assert_eq!(vec![bold("…ested/module/lib.rs")], actual);
}

#[test]
fn should_truncate_range_list_from_the_head_when_the_joined_list_overflows_width() {
    let badges = Badges::default();
    let ranges = [(1, 5), (10, 20), (100, 200), (300, 400)];

    let actual = diff_pane_header_lines(None, "lib.rs", &badges, &ranges, 20);

    // The `"range: "` label stays fixed and the *range list itself* is
    // head-truncated (`…` at the start), keeping the later line numbers
    // — the ones the reviewer scrolled to see — visible at the tail.
    assert_eq!(vec![bold("lib.rs"), range_line("…200, 300-400")], actual);
}
