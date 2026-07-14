use super::*;
use crate::diff_view::{DiffLine, DiffLineKind};
use pretty_assertions::assert_eq;

fn diff_line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

#[test]
fn should_return_default_stats_when_sections_is_empty() {
    let actual = change_stats(&[]);

    assert_eq!(ChangeStats::default(), actual);
}

#[test]
fn should_collect_range_and_counts_from_a_single_hunk() {
    let section = DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 0,
            hunk: Hunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                new_range: Some((23, 41)),
                lines: vec![
                    diff_line(DiffLineKind::Context, "fn a() {}"),
                    diff_line(DiffLineKind::Added, "fn foo() {}"),
                ],
            },
        }],
    };

    let actual = change_stats(&[&section]);

    assert_eq!(
        ChangeStats {
            ranges: vec![(23, 41)],
            added: 1,
            removed: 0,
        },
        actual
    );
}

#[test]
fn should_merge_ranges_and_counts_across_multiple_sections() {
    let first = DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 0,
            hunk: Hunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                new_range: Some((23, 41)),
                lines: vec![diff_line(DiffLineKind::Added, "fn foo() {}")],
            },
        }],
    };
    let second = DiffSection {
        title: "fn bar()".to_string(),
        symbol_id: Some("lib.rs::bar".to_string()),
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 1,
            hunk: Hunk {
                header: "@@ -50,4 +57,4 @@".to_string(),
                new_range: Some((57, 60)),
                lines: vec![
                    diff_line(DiffLineKind::Removed, "fn old_bar() {}"),
                    diff_line(DiffLineKind::Removed, "fn old_bar2() {}"),
                    diff_line(DiffLineKind::Added, "fn bar() {}"),
                ],
            },
        }],
    };

    let actual = change_stats(&[&first, &second]);

    assert_eq!(
        ChangeStats {
            ranges: vec![(23, 41), (57, 60)],
            added: 2,
            removed: 2,
        },
        actual
    );
}

#[test]
fn should_exclude_zero_width_deletion_range_but_still_count_removed_lines() {
    // A pure-deletion hunk's `new_range` is `(start, start - 1)`
    // (`Hunk::new_range`'s own doc comment) — a deliberately empty range
    // that must not appear in `ranges`, but its removed lines still count.
    let section = DiffSection {
        title: MODULE_LEVEL_TITLE.to_string(),
        symbol_id: None,
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 0,
            hunk: Hunk {
                header: "@@ -10,3 +10,0 @@".to_string(),
                new_range: Some((10, 9)),
                lines: vec![
                    diff_line(DiffLineKind::Removed, "use old_module;"),
                    diff_line(DiffLineKind::Removed, "use another_old;"),
                ],
            },
        }],
    };

    let actual = change_stats(&[&section]);

    assert_eq!(
        ChangeStats {
            ranges: vec![],
            added: 0,
            removed: 2,
        },
        actual
    );
}

#[test]
fn should_exclude_range_when_new_range_is_none() {
    let section = DiffSection {
        title: MODULE_LEVEL_TITLE.to_string(),
        symbol_id: None,
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 0,
            hunk: Hunk {
                header: "malformed".to_string(),
                new_range: None,
                lines: vec![diff_line(DiffLineKind::Added, "fn foo() {}")],
            },
        }],
    };

    let actual = change_stats(&[&section]);

    assert_eq!(
        ChangeStats {
            ranges: vec![],
            added: 1,
            removed: 0,
        },
        actual
    );
}
