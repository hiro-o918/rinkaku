//! Overlays a file's diff hunks onto its full-file source view (ADR 0046),
//! reusing [`crate::diff_view`]'s already-parsed [`Hunk`]s rather than
//! re-diffing or growing the diff pane's own hunk-relative coordinate
//! space (ADR 0046's Context section explains why the diff pane itself was
//! rejected as the home for this).

use crate::diff_view::{DiffLineKind, FileHunks, Hunk};

/// One diff-only line an overlay adds beyond what [`crate::source::SourceView`]
/// already renders, either a new-side line already present in the source
/// (`Added`) or a line that no longer exists there (`Removed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayLineKind {
    Added,
    Removed,
}

/// One [`Hunk`] line's position in the overlaid file, in the same 1-based
/// new-side numbering [`crate::source::SourceView`] uses for its own lines.
///
/// `Removed` carries the new-side line number it sits *before* — the same
/// "deletion is a position, not a line" convention
/// [`crate::diff_view::hunk_intersects`] already uses for a pure-deletion
/// hunk's `new_range` (this module's own doc comment), reused here rather
/// than inventing a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkOverlayLine {
    pub position: usize,
    pub kind: OverlayLineKind,
    pub content: String,
}

/// Walks `hunk.lines`, computing each `Added`/`Removed` line's position in
/// the overlaid file. `Context` lines are dropped from the result — the
/// source view's own lines already render them, so the overlay only needs
/// to add information for lines the diff actually changed.
///
/// Returns an empty `Vec` when `hunk.new_range` is `None` (an unreadable
/// header, or a deletion at new-side line 0 — [`Hunk::new_range`]'s own doc
/// comment): neither case has a position to seed the walk from.
pub fn hunk_overlay_lines(hunk: &Hunk) -> Vec<HunkOverlayLine> {
    let Some((range_start, _)) = hunk.new_range else {
        return Vec::new();
    };

    let mut position = range_start;
    let mut result = Vec::new();
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Added => {
                result.push(HunkOverlayLine {
                    position,
                    kind: OverlayLineKind::Added,
                    content: line.content.clone(),
                });
                position += 1;
            }
            DiffLineKind::Context => {
                position += 1;
            }
            DiffLineKind::Removed => {
                result.push(HunkOverlayLine {
                    position,
                    kind: OverlayLineKind::Removed,
                    content: line.content.clone(),
                });
            }
        }
    }
    result
}

/// One display row of the overlaid source view: either an unmodified source
/// line (`line_number` is that line's 1-based number in
/// [`crate::source::SourceView::lines`]), a source line the diff added, or a
/// diff-removed line inserted between two source lines (no `line_number` of
/// its own — [`HunkOverlayLine`]'s own doc comment on why a deletion has a
/// position but not a line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayRow {
    Unchanged { line_number: usize, content: String },
    Added { line_number: usize, content: String },
    Removed { content: String },
}

/// Composites every hunk in `file_hunks` onto `source_lines` (1-based
/// numbering, matching [`crate::source::SourceView::lines`]'s own
/// convention), producing one [`OverlayRow`] per output row: one entry per
/// source line (tagged `Added` when a hunk claims that new-side line
/// number, `Unchanged` otherwise) plus one extra `Removed` entry
/// immediately before the added/unchanged row a deletion's position points
/// at.
///
/// Returns `None` when a hunk's `Context` lines don't match `source_lines`
/// at the position the hunk claims — the file on disk has drifted from the
/// diff (ADR 0046 decision 5) — since composing with a wrong position
/// mapping would place colored lines in the wrong place, worse than no
/// overlay at all.
pub fn overlay_source_lines(
    source_lines: &[String],
    file_hunks: &FileHunks,
) -> Option<Vec<OverlayRow>> {
    for hunk in &file_hunks.hunks {
        if !hunk_matches_source(hunk, source_lines) {
            return None;
        }
    }

    let mut removed_before: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    let mut added_at: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for hunk in &file_hunks.hunks {
        for overlay_line in hunk_overlay_lines(hunk) {
            match overlay_line.kind {
                OverlayLineKind::Added => {
                    added_at.insert(overlay_line.position, overlay_line.content);
                }
                OverlayLineKind::Removed => {
                    removed_before
                        .entry(overlay_line.position)
                        .or_default()
                        .push(overlay_line.content);
                }
            }
        }
    }

    let mut rows = Vec::new();
    for (index, content) in source_lines.iter().enumerate() {
        let line_number = index + 1;
        if let Some(removed) = removed_before.remove(&line_number) {
            rows.extend(
                removed
                    .into_iter()
                    .map(|content| OverlayRow::Removed { content }),
            );
        }
        rows.push(if added_at.contains_key(&line_number) {
            OverlayRow::Added {
                line_number,
                content: content.clone(),
            }
        } else {
            OverlayRow::Unchanged {
                line_number,
                content: content.clone(),
            }
        });
    }
    // A deletion positioned at (source_lines.len() + 1) — the end of the
    // file — has no `Unchanged`/`Added` row above to anchor before; drain
    // whatever remains in source line order for a deterministic tail.
    let mut trailing_positions: Vec<usize> = removed_before.keys().copied().collect();
    trailing_positions.sort_unstable();
    for position in trailing_positions {
        if let Some(removed) = removed_before.remove(&position) {
            rows.extend(
                removed
                    .into_iter()
                    .map(|content| OverlayRow::Removed { content }),
            );
        }
    }

    Some(rows)
}

/// Whether every `Context` line in `hunk` matches `source_lines` at the
/// position [`hunk_overlay_lines`]'s own walk would place it — the drift
/// check backing [`overlay_source_lines`]'s `None` return (ADR 0046
/// decision 5).
fn hunk_matches_source(hunk: &Hunk, source_lines: &[String]) -> bool {
    let Some((range_start, _)) = hunk.new_range else {
        // No position to check against; `hunk_overlay_lines` already
        // returns nothing for this hunk, so it cannot introduce a drifted
        // row either.
        return true;
    };

    let mut position = range_start;
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Context => {
                let Some(actual) = source_lines.get(position - 1) else {
                    return false;
                };
                if actual != &line.content {
                    return false;
                }
                position += 1;
            }
            DiffLineKind::Added => position += 1,
            DiffLineKind::Removed => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
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
}
