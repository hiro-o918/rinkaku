use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_join_symbol_name_and_path_on_first_header_line_when_selection_name_is_present() {
    let stats = ChangeStats::default();

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

    assert_eq!(
        vec![Line::styled(
            "foo · src/lib.rs".to_string(),
            Style::default().add_modifier(Modifier::BOLD)
        )],
        actual
    );
}

#[test]
fn should_show_bare_path_on_first_header_line_when_no_selection_name() {
    let stats = ChangeStats::default();

    let actual = diff_pane_header_lines(None, "src/lib.rs", &stats, 80);

    assert_eq!(
        vec![Line::styled(
            "src/lib.rs".to_string(),
            Style::default().add_modifier(Modifier::BOLD)
        )],
        actual
    );
}

#[test]
fn should_add_change_stats_line_when_stats_has_ranges_and_counts() {
    let stats = ChangeStats {
        ranges: vec![(23, 41), (57, 60)],
        added: 18,
        removed: 4,
    };

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

    assert_eq!(
        vec![
            Line::styled(
                "foo · src/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            ),
            Line::styled(
                "chg: 23-41, 57-60 (+18/-4)".to_string(),
                Style::default().add_modifier(Modifier::DIM)
            ),
        ],
        actual
    );
}

#[test]
fn should_format_single_line_range_without_a_dash() {
    let stats = ChangeStats {
        ranges: vec![(5, 5)],
        added: 1,
        removed: 0,
    };

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

    assert_eq!(
        vec![
            Line::styled(
                "foo · src/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            ),
            Line::styled(
                "chg: 5 (+1/-0)".to_string(),
                Style::default().add_modifier(Modifier::DIM)
            ),
        ],
        actual
    );
}

#[test]
fn should_omit_change_stats_line_when_stats_is_entirely_empty() {
    let stats = ChangeStats::default();

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

    assert_eq!(1, actual.len());
}

#[test]
fn should_show_counts_without_ranges_when_ranges_is_empty_but_counts_are_nonzero() {
    // A pure-deletion selection: `ChangeStats::ranges` excludes the
    // zero-width deletion range (`change_stats`'s own doc comment), but
    // the removed count is still real and worth reporting.
    let stats = ChangeStats {
        ranges: vec![],
        added: 0,
        removed: 2,
    };

    let actual = diff_pane_header_lines(Some("foo"), "src/lib.rs", &stats, 80);

    assert_eq!(
        vec![
            Line::styled(
                "foo · src/lib.rs".to_string(),
                Style::default().add_modifier(Modifier::BOLD)
            ),
            Line::styled(
                "chg: (+0/-2)".to_string(),
                Style::default().add_modifier(Modifier::DIM)
            ),
        ],
        actual
    );
}

#[test]
fn should_truncate_first_header_line_keeping_the_tail_when_it_overflows_width() {
    let stats = ChangeStats::default();

    let actual = diff_pane_header_lines(
        Some("very_long_symbol_name_here"),
        "src/very/deeply/nested/module/lib.rs",
        &stats,
        20,
    );

    assert_eq!(
        vec![Line::styled(
            "…ested/module/lib.rs".to_string(),
            Style::default().add_modifier(Modifier::BOLD)
        )],
        actual
    );
}
