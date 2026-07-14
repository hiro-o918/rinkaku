use super::*;
use crate::diff_view::DiffLine;
use pretty_assertions::assert_eq;

fn diff_line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

// --- hunk_overlay_lines ---

#[test]
fn should_return_empty_when_hunk_has_no_new_range() {
    let hunk = Hunk {
        header: "@@ garbage @@".to_string(),
        new_range: None,
        lines: vec![diff_line(DiffLineKind::Context, "fn a() {}")],
    };

    let actual = hunk_overlay_lines(&hunk);

    assert_eq!(Vec::<HunkOverlayLine>::new(), actual);
}

#[test]
fn should_position_added_lines_by_new_side_line_number_for_a_pure_addition() {
    let hunk = Hunk {
        header: "@@ -1,1 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Added, "fn b() {}"),
            diff_line(DiffLineKind::Added, "fn c() {}"),
        ],
    };

    let expected = vec![
        HunkOverlayLine {
            position: 2,
            kind: OverlayLineKind::Added,
            content: "fn b() {}".to_string(),
        },
        HunkOverlayLine {
            position: 3,
            kind: OverlayLineKind::Added,
            content: "fn c() {}".to_string(),
        },
    ];
    let actual = hunk_overlay_lines(&hunk);

    assert_eq!(expected, actual);
}

#[test]
fn should_position_removed_lines_before_the_new_side_line_they_used_to_precede() {
    let hunk = Hunk {
        header: "@@ -1,3 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn old() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    };

    let expected = vec![HunkOverlayLine {
        position: 2,
        kind: OverlayLineKind::Removed,
        content: "fn old() {}".to_string(),
    }];
    let actual = hunk_overlay_lines(&hunk);

    assert_eq!(expected, actual);
}

#[test]
fn should_position_removed_line_at_file_end_when_hunk_is_a_trailing_pure_deletion() {
    let hunk = Hunk {
        header: "@@ -3,1 +2,0 @@".to_string(),
        new_range: Some((3, 2)),
        lines: vec![diff_line(DiffLineKind::Removed, "fn tail() {}")],
    };

    let expected = vec![HunkOverlayLine {
        position: 3,
        kind: OverlayLineKind::Removed,
        content: "fn tail() {}".to_string(),
    }];
    let actual = hunk_overlay_lines(&hunk);

    assert_eq!(expected, actual);
}

#[test]
fn should_position_mixed_added_and_removed_lines_in_one_replace_hunk() {
    let hunk = Hunk {
        header: "@@ -1,3 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn old() {}"),
            diff_line(DiffLineKind::Added, "fn new() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    };

    let expected = vec![
        HunkOverlayLine {
            position: 2,
            kind: OverlayLineKind::Removed,
            content: "fn old() {}".to_string(),
        },
        HunkOverlayLine {
            position: 2,
            kind: OverlayLineKind::Added,
            content: "fn new() {}".to_string(),
        },
    ];
    let actual = hunk_overlay_lines(&hunk);

    assert_eq!(expected, actual);
}

// --- overlay_source_lines ---

fn file_hunks(hunks: Vec<Hunk>) -> FileHunks {
    FileHunks {
        path: "src/lib.rs".to_string(),
        hunks,
    }
}

#[test]
fn should_return_empty_rows_when_file_has_no_hunks() {
    let source_lines = vec!["fn a() {}".to_string()];
    let hunks = file_hunks(vec![]);

    let expected = vec![OverlayRow::Unchanged {
        line_number: 1,
        content: "fn a() {}".to_string(),
    }];
    let actual = overlay_source_lines(&source_lines, &hunks).expect("no drift to detect");

    assert_eq!(expected, actual);
}

#[test]
fn should_tag_a_pure_addition_as_added_rows_at_their_source_positions() {
    let source_lines = vec![
        "fn a() {}".to_string(),
        "fn b() {}".to_string(),
        "fn c() {}".to_string(),
    ];
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,3 @@".to_string(),
        new_range: Some((1, 3)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Added, "fn b() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let expected = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "fn a() {}".to_string(),
        },
        OverlayRow::Added {
            line_number: 2,
            content: "fn b() {}".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 3,
            content: "fn c() {}".to_string(),
        },
    ];
    let actual = overlay_source_lines(&source_lines, &hunks).expect("no drift to detect");

    assert_eq!(expected, actual);
}

#[test]
fn should_insert_removed_row_immediately_before_its_new_side_position() {
    let source_lines = vec!["fn a() {}".to_string(), "fn c() {}".to_string()];
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Removed, "fn old() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let expected = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "fn a() {}".to_string(),
        },
        OverlayRow::Removed {
            content: "fn old() {}".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 2,
            content: "fn c() {}".to_string(),
        },
    ];
    let actual = overlay_source_lines(&source_lines, &hunks).expect("no drift to detect");

    assert_eq!(expected, actual);
}

#[test]
fn should_append_removed_rows_at_the_tail_when_deletion_is_at_file_end() {
    let source_lines = vec!["fn a() {}".to_string(), "fn b() {}".to_string()];
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,3 +1,2 @@".to_string(),
        new_range: Some((3, 2)),
        lines: vec![diff_line(DiffLineKind::Removed, "fn tail() {}")],
    }]);

    let expected = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "fn a() {}".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 2,
            content: "fn b() {}".to_string(),
        },
        OverlayRow::Removed {
            content: "fn tail() {}".to_string(),
        },
    ];
    let actual = overlay_source_lines(&source_lines, &hunks).expect("no drift to detect");

    assert_eq!(expected, actual);
}

#[test]
fn should_composite_multiple_hunks_across_the_same_file() {
    let source_lines = vec![
        "fn a() {}".to_string(),
        "fn b() {}".to_string(),
        "fn x() {}".to_string(),
        "fn y() {}".to_string(),
    ];
    let hunks = file_hunks(vec![
        Hunk {
            header: "@@ -1,1 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Context, "fn a() {}"),
                diff_line(DiffLineKind::Added, "fn b() {}"),
            ],
        },
        Hunk {
            header: "@@ -8,1 +9,2 @@".to_string(),
            new_range: Some((3, 4)),
            lines: vec![
                diff_line(DiffLineKind::Context, "fn x() {}"),
                diff_line(DiffLineKind::Added, "fn y() {}"),
            ],
        },
    ]);

    let expected = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "fn a() {}".to_string(),
        },
        OverlayRow::Added {
            line_number: 2,
            content: "fn b() {}".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 3,
            content: "fn x() {}".to_string(),
        },
        OverlayRow::Added {
            line_number: 4,
            content: "fn y() {}".to_string(),
        },
    ];
    let actual = overlay_source_lines(&source_lines, &hunks).expect("no drift to detect");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_none_when_context_line_does_not_match_source_at_its_position() {
    let source_lines = vec!["fn a_edited() {}".to_string(), "fn c() {}".to_string()];
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,2 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Context, "fn c() {}"),
        ],
    }]);

    let actual = overlay_source_lines(&source_lines, &hunks);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_context_line_position_is_past_the_end_of_source() {
    let source_lines = vec!["fn a() {}".to_string()];
    let hunks = file_hunks(vec![Hunk {
        header: "@@ -1,1 +1,2 @@".to_string(),
        new_range: Some((1, 2)),
        lines: vec![
            diff_line(DiffLineKind::Context, "fn a() {}"),
            diff_line(DiffLineKind::Context, "fn missing() {}"),
        ],
    }]);

    let actual = overlay_source_lines(&source_lines, &hunks);

    assert_eq!(None, actual);
}

// --- rows_in_source_range ---

#[test]
fn should_return_empty_slice_when_rows_is_empty() {
    let rows: Vec<OverlayRow> = vec![];

    let actual = rows_in_source_range(&rows, 1, 10);

    assert_eq!(Vec::<OverlayRow>::new(), actual.to_vec());
}

#[test]
fn should_slice_rows_to_the_requested_source_line_range() {
    let rows = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "a".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 2,
            content: "b".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 3,
            content: "c".to_string(),
        },
    ];

    let actual = rows_in_source_range(&rows, 2, 2);

    assert_eq!(
        vec![OverlayRow::Unchanged {
            line_number: 2,
            content: "b".to_string(),
        }],
        actual.to_vec()
    );
}

#[test]
fn should_include_a_removed_row_anchored_to_an_in_range_row() {
    let rows = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "a".to_string(),
        },
        OverlayRow::Removed {
            content: "old".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 2,
            content: "b".to_string(),
        },
    ];

    let actual = rows_in_source_range(&rows, 2, 2);

    assert_eq!(
        vec![
            OverlayRow::Removed {
                content: "old".to_string(),
            },
            OverlayRow::Unchanged {
                line_number: 2,
                content: "b".to_string(),
            },
        ],
        actual.to_vec()
    );
}

#[test]
fn should_exclude_a_removed_row_anchored_to_an_out_of_range_row() {
    let rows = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "a".to_string(),
        },
        OverlayRow::Removed {
            content: "old".to_string(),
        },
        OverlayRow::Unchanged {
            line_number: 2,
            content: "b".to_string(),
        },
    ];

    let actual = rows_in_source_range(&rows, 1, 1);

    assert_eq!(
        vec![OverlayRow::Unchanged {
            line_number: 1,
            content: "a".to_string(),
        }],
        actual.to_vec()
    );
}

#[test]
fn should_include_trailing_removed_rows_when_range_covers_the_last_source_line() {
    let rows = vec![
        OverlayRow::Unchanged {
            line_number: 1,
            content: "a".to_string(),
        },
        OverlayRow::Removed {
            content: "tail".to_string(),
        },
    ];

    let actual = rows_in_source_range(&rows, 1, 1);

    assert_eq!(
        vec![
            OverlayRow::Unchanged {
                line_number: 1,
                content: "a".to_string(),
            },
            OverlayRow::Removed {
                content: "tail".to_string(),
            },
        ],
        actual.to_vec()
    );
}

#[test]
fn should_return_empty_slice_when_start_line_is_past_every_row() {
    let rows = vec![OverlayRow::Unchanged {
        line_number: 1,
        content: "a".to_string(),
    }];

    let actual = rows_in_source_range(&rows, 5, 10);

    assert_eq!(Vec::<OverlayRow>::new(), actual.to_vec());
}
