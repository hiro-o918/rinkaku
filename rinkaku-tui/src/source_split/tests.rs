//! Tests for `crate::source_split` (ADR 0049), split from the source file
//! per this project's file-size discipline.

use super::*;
use crate::diff_view::{DiffLine, Hunk};
use pretty_assertions::assert_eq;

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|s| s.to_string()).collect()
}

fn diff_line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

fn file_hunks(hunks: Vec<Hunk>) -> FileHunks {
    FileHunks {
        path: "lib.rs".to_string(),
        hunks,
    }
}

// --- reconstruct_old_lines ---

#[test]
fn should_return_new_lines_unchanged_when_file_hunks_has_no_hunks() {
    let new_lines = lines(&["fn a() {}", "fn b() {}"]);
    let hunks = file_hunks(vec![]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(Some(new_lines), actual);
}

#[test]
fn should_drop_added_line_and_keep_removed_line_when_hunk_is_a_pure_replace() {
    let new_lines = lines(&["fn a() {}", "fn new_name() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn old_name() {}"),
            diff_line(DiffLineKind::Added, "fn new_name() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(
        Some(lines(&["fn a() {}", "fn old_name() {}", "fn c() {}"])),
        actual
    );
}

#[test]
fn should_reconstruct_pure_insertion_by_dropping_the_added_line() {
    let new_lines = lines(&["fn a() {}", "fn b() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Added, "fn b() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(Some(lines(&["fn a() {}", "fn c() {}"])), actual);
}

#[test]
fn should_reconstruct_pure_deletion_by_inserting_the_removed_line() {
    let new_lines = lines(&["fn a() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn b() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(
        Some(lines(&["fn a() {}", "fn b() {}", "fn c() {}"])),
        actual
    );
}

#[test]
fn should_reconstruct_across_multiple_hunks_preserving_unchanged_gaps() {
    let new_lines = lines(&[
        "fn a() {}",
        "fn b2() {}",
        "fn c() {}",
        "fn d2() {}",
        "fn e() {}",
    ]);
    let hunks = file_hunks(vec![
        Hunk {
            header: "@@ -1,2 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Context, "fn a() {}"),
                diff_line(DiffLineKind::Removed, "fn b() {}"),
                diff_line(DiffLineKind::Added, "fn b2() {}"),
            ],
        },
        Hunk {
            header: "@@ -3,2 +3,2 @@".to_string(),
            new_range: Some((4, 5)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn d() {}"),
                diff_line(DiffLineKind::Added, "fn d2() {}"),
                diff_line(DiffLineKind::Context, "fn e() {}"),
            ],
        },
    ]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(
        Some(lines(&[
            "fn a() {}",
            "fn b() {}",
            "fn c() {}",
            "fn d() {}",
            "fn e() {}",
        ])),
        actual
    );
}

#[test]
fn should_return_none_when_context_line_does_not_match_new_lines_at_computed_position() {
    // Corruption must land on the `Context` line: an `Added` line is never
    // drift-checked (mirrors `crate::source_diff::hunk_matches_source`'s
    // own precedent), so only a mismatched `Context` line exercises this.
    let new_lines = lines(&["fn a() {}", "fn edited_since_diff() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Added, "fn a() {}"),
            diff_line(DiffLineKind::Context, "fn foo() {}"),
        ],
    }]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_hunk_has_no_new_range() {
    let new_lines = lines(&["fn a() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ garbage @@".to_string(),
        new_range: None,
        lines: vec![diff_line(DiffLineKind::Context, "fn a() {}")],
    }]);

    let actual = reconstruct_old_lines(&new_lines, &hunks);

    assert_eq!(None, actual);
}

// --- split_source_rows ---

#[test]
fn should_return_all_unchanged_rows_when_file_hunks_has_no_hunks() {
    let new_lines = lines(&["fn a() {}", "fn b() {}"]);
    let hunks = file_hunks(vec![]);

    let actual = split_source_rows(&new_lines, &hunks);

    assert_eq!(
        Some(vec![
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn b() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn b() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
        ]),
        actual
    );
}

#[test]
fn should_pair_replace_run_and_mirror_surrounding_context_when_hunk_is_a_single_line_replace() {
    let new_lines = lines(&["fn a() {}", "fn new_name() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn old_name() {}"),
            diff_line(DiffLineKind::Added, "fn new_name() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = split_source_rows(&new_lines, &hunks);

    assert_eq!(
        Some(vec![
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn old_name() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn new_name() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Changed,
            },
            // `pair_hunk_lines`'s total-row invariant (ADR 0044 decision
            // 4): one filler row per matched pair, so this run's rendered
            // row count still matches its two source lines.
            SourceSplitRow {
                left: None,
                right: None,
                kind: SourceSplitRowKind::Filler,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 3,
                    content: "fn c() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 3,
                    content: "fn c() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
        ]),
        actual
    );
}

#[test]
fn should_place_pure_insertion_against_filler_on_the_left() {
    let new_lines = lines(&["fn a() {}", "fn b() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Added, "fn b() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = split_source_rows(&new_lines, &hunks);

    assert_eq!(
        Some(vec![
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
            SourceSplitRow {
                left: None,
                right: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn b() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Changed,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn c() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 3,
                    content: "fn c() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
        ]),
        actual
    );
}

#[test]
fn should_place_pure_deletion_against_filler_on_the_right() {
    let new_lines = lines(&["fn a() {}", "fn c() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn b() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = split_source_rows(&new_lines, &hunks);

    assert_eq!(
        Some(vec![
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 1,
                    content: "fn a() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn b() {}".to_string(),
                }),
                right: None,
                kind: SourceSplitRowKind::Changed,
            },
            SourceSplitRow {
                left: Some(SourceSplitLine {
                    line_number: 3,
                    content: "fn c() {}".to_string(),
                }),
                right: Some(SourceSplitLine {
                    line_number: 2,
                    content: "fn c() {}".to_string(),
                }),
                kind: SourceSplitRowKind::Unchanged,
            },
        ]),
        actual
    );
}

#[test]
fn should_advance_line_numbers_across_multiple_hunks_with_unchanged_gaps() {
    let new_lines = lines(&[
        "fn a() {}",
        "fn b2() {}",
        "fn c() {}",
        "fn d2() {}",
        "fn e() {}",
    ]);
    let hunks = file_hunks(vec![
        Hunk {
            header: "@@ -1,2 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Context, "fn a() {}"),
                diff_line(DiffLineKind::Removed, "fn b() {}"),
                diff_line(DiffLineKind::Added, "fn b2() {}"),
            ],
        },
        Hunk {
            header: "@@ -3,2 +3,2 @@".to_string(),
            new_range: Some((4, 5)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn d() {}"),
                diff_line(DiffLineKind::Added, "fn d2() {}"),
                diff_line(DiffLineKind::Context, "fn e() {}"),
            ],
        },
    ]);

    let actual = split_source_rows(&new_lines, &hunks);

    let expected = vec![
        SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: 1,
                content: "fn a() {}".to_string(),
            }),
            right: Some(SourceSplitLine {
                line_number: 1,
                content: "fn a() {}".to_string(),
            }),
            kind: SourceSplitRowKind::Unchanged,
        },
        SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: 2,
                content: "fn b() {}".to_string(),
            }),
            right: Some(SourceSplitLine {
                line_number: 2,
                content: "fn b2() {}".to_string(),
            }),
            kind: SourceSplitRowKind::Changed,
        },
        // `pair_hunk_lines`'s total-row invariant (ADR 0044 decision 4):
        // one filler row per matched pair.
        SourceSplitRow {
            left: None,
            right: None,
            kind: SourceSplitRowKind::Filler,
        },
        SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: 3,
                content: "fn c() {}".to_string(),
            }),
            right: Some(SourceSplitLine {
                line_number: 3,
                content: "fn c() {}".to_string(),
            }),
            kind: SourceSplitRowKind::Unchanged,
        },
        SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: 4,
                content: "fn d() {}".to_string(),
            }),
            right: Some(SourceSplitLine {
                line_number: 4,
                content: "fn d2() {}".to_string(),
            }),
            kind: SourceSplitRowKind::Changed,
        },
        SourceSplitRow {
            left: None,
            right: None,
            kind: SourceSplitRowKind::Filler,
        },
        SourceSplitRow {
            left: Some(SourceSplitLine {
                line_number: 5,
                content: "fn e() {}".to_string(),
            }),
            right: Some(SourceSplitLine {
                line_number: 5,
                content: "fn e() {}".to_string(),
            }),
            kind: SourceSplitRowKind::Unchanged,
        },
    ];
    assert_eq!(Some(expected), actual);
}

#[test]
fn should_return_none_when_file_has_drifted_since_the_diff_was_produced() {
    // Corruption must land on the `Context` line — see
    // `reconstruct_old_lines`'s own drift test above for why.
    let new_lines = lines(&["fn a() {}", "fn edited_since_diff() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Added, "fn a() {}"),
            diff_line(DiffLineKind::Context, "fn foo() {}"),
        ],
    }]);

    let actual = split_source_rows(&new_lines, &hunks);

    assert_eq!(None, actual);
}

#[test]
fn should_emit_filler_row_for_each_line_similarity_alignment_merges_onto_one_row() {
    // A leading doc comment inserted ahead of an otherwise unchanged
    // signature (ADR 0044's amendment motivating case): similarity
    // alignment merges the unchanged signature line onto one row, so the
    // run must still contribute one row per source line via a filler row —
    // asserted here as the row *count* rather than every field, since the
    // point under test is the total-row invariant
    // (`crate::split_pairing::pair_hunk_lines`'s own doc comment), which
    // the individual-row tests above already cover in detail.
    let new_lines = lines(&["/// doc comment", "fn signature() {}"]);
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,1 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Added, "/// doc comment"),
            diff_line(DiffLineKind::Removed, "fn signature() {}"),
            diff_line(DiffLineKind::Added, "fn signature() {}"),
        ],
    }]);

    let actual = split_source_rows(&new_lines, &hunks).expect("reconstruction should succeed");

    assert_eq!(hunks.hunks[0].lines.len(), actual.len());
    assert_eq!(
        SourceSplitRowKind::Filler,
        actual.last().expect("at least one row").kind
    );
}
