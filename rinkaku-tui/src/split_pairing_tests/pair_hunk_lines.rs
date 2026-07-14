use super::*;
use crate::diff_view::{DiffLine, DiffLineKind};
use pretty_assertions::assert_eq;

fn line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

fn context(content: &str) -> DiffLine {
    line(DiffLineKind::Context, content)
}

fn added(content: &str) -> DiffLine {
    line(DiffLineKind::Added, content)
}

fn removed(content: &str) -> DiffLine {
    line(DiffLineKind::Removed, content)
}

fn row(left: Option<(DiffLine, usize)>, right: Option<(DiffLine, usize)>) -> SplitRow {
    SplitRow {
        left: left.as_ref().map(|(line, _)| line.clone()),
        left_index: left.map(|(_, index)| index),
        right: right.as_ref().map(|(line, _)| line.clone()),
        right_index: right.map(|(_, index)| index),
    }
}

#[test]
fn should_return_empty_rows_when_hunk_has_no_lines() {
    let actual = pair_hunk_lines(&[]);

    assert_eq!(Vec::<SplitRow>::new(), actual);
}

#[test]
fn should_mirror_context_line_onto_both_sides() {
    let lines = vec![context("fn a() {}")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![row(
            Some((context("fn a() {}"), 0)),
            Some((context("fn a() {}"), 0)),
        )],
        actual
    );
}

#[test]
fn should_pair_equal_length_removed_and_added_runs_positionally() {
    // ADR 0044 decision 4's total-row invariant: 1 removed + 1 added is 2
    // source lines, so the paired row is followed by one filler row even
    // though every removed/added line found a match — row count must
    // always equal `lines.len()`.
    let lines = vec![removed("fn old() {}"), added("fn new() {}")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                Some((removed("fn old() {}"), 0)),
                Some((added("fn new() {}"), 1)),
            ),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_pad_right_side_with_filler_row_when_removed_run_is_longer_than_added_run() {
    // Removed run (2 lines) longer than added run (1 line): the run
    // pairs positionally up to the shorter side's length, then the
    // excess removed line pairs against `None` on the right. Total rows
    // stays at `hunk.lines.len()` (3) — one filler `None`/`None` row
    // absorbs the count the merged pair "saved" (ADR 0044 decision 4).
    let lines = vec![removed("line 1"), removed("line 2"), added("line 1'")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(Some((removed("line 1"), 0)), Some((added("line 1'"), 2)),),
            row(Some((removed("line 2"), 1)), None),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_pad_left_side_with_filler_row_when_added_run_is_longer_than_removed_run() {
    let lines = vec![removed("line 1"), added("line 1'"), added("line 2'")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(Some((removed("line 1"), 0)), Some((added("line 1'"), 1)),),
            row(None, Some((added("line 2'"), 2))),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_pair_pure_deletion_run_against_none_on_the_right() {
    let lines = vec![removed("line 1"), removed("line 2")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(Some((removed("line 1"), 0)), None),
            row(Some((removed("line 2"), 1)), None),
        ],
        actual
    );
}

#[test]
fn should_pair_pure_insertion_run_against_none_on_the_left() {
    let lines = vec![added("line 1"), added("line 2")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(None, Some((added("line 1"), 0))),
            row(None, Some((added("line 2"), 1))),
        ],
        actual
    );
}

#[test]
fn should_pair_context_removed_added_context_sequence_in_source_order() {
    // The removed/added run is a 1/1 pair, so it gets a filler row after it
    // (ADR 0044 decision 4's total-row invariant — see
    // `should_pair_equal_length_removed_and_added_runs_positionally`).
    let lines = vec![
        context("fn a() {}"),
        removed("fn old() {}"),
        added("fn new() {}"),
        context("fn c() {}"),
    ];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                Some((context("fn a() {}"), 0)),
                Some((context("fn a() {}"), 0)),
            ),
            row(
                Some((removed("fn old() {}"), 1)),
                Some((added("fn new() {}"), 2)),
            ),
            row(None, None),
            row(
                Some((context("fn c() {}"), 3)),
                Some((context("fn c() {}"), 3)),
            ),
        ],
        actual
    );
}

#[test]
fn should_treat_two_separate_replace_runs_independently() {
    let lines = vec![
        removed("old 1"),
        added("new 1"),
        context("keep"),
        removed("old 2"),
        added("new 2"),
    ];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(Some((removed("old 1"), 0)), Some((added("new 1"), 1)),),
            row(None, None),
            row(Some((context("keep"), 2)), Some((context("keep"), 2)),),
            row(Some((removed("old 2"), 3)), Some((added("new 2"), 4)),),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_return_hunk_lines_len_rows_regardless_of_run_shape() {
    // ADR 0044 decision 4's own invariant: row count always equals
    // `lines.len()`, so `walk_sections`'s line-counting (and therefore
    // every scroll-sync offset it feeds) never has to branch on
    // `diff_view_mode`.
    let lines = vec![removed("a"), removed("b"), removed("c"), added("a'")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(lines.len(), actual.len());
}

#[test]
fn should_align_matching_signature_past_inserted_leading_comment_lines() {
    // ADR 0044 amendment: positional pairing put the old signature
    // (removed row 0) next to the new side's first added line, which is
    // now a doc comment rather than the shifted signature — the reader's
    // original report. Similarity alignment must still match the two
    // near-identical signature lines to each other and place the two
    // unmatched leading comment lines as their own rows.
    let lines = vec![
        removed("fn compute(a: i32, b: i32) -> i32 {"),
        added("/// Computes the sum of two numbers."),
        added("/// Both arguments must be non-negative."),
        added("fn compute(a: i32, b: i32) -> i64 {"),
    ];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                None,
                Some((added("/// Computes the sum of two numbers."), 1)),
            ),
            row(
                None,
                Some((added("/// Both arguments must be non-negative."), 2)),
            ),
            row(
                Some((removed("fn compute(a: i32, b: i32) -> i32 {"), 0)),
                Some((added("fn compute(a: i32, b: i32) -> i64 {"), 3)),
            ),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_align_matching_line_when_it_is_not_the_first_removed_line() {
    // Mirrors the previous case with the match on the removed side instead
    // of the added side: two removed lines no longer needed, followed by a
    // line that is still present (near-identical) on the added side.
    let lines = vec![
        removed("// stale explanation, no longer accurate"),
        removed("// second stale line"),
        removed("fn compute(a: i32, b: i32) -> i32 {"),
        added("fn compute(a: i32, b: i32) -> i64 {"),
    ];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                Some((removed("// stale explanation, no longer accurate"), 0)),
                None,
            ),
            row(Some((removed("// second stale line"), 1)), None),
            row(
                Some((removed("fn compute(a: i32, b: i32) -> i32 {"), 2)),
                Some((added("fn compute(a: i32, b: i32) -> i64 {"), 3)),
            ),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_fall_back_to_positional_pairing_when_no_lines_are_similar() {
    // No line on either side shares enough tokens with any line on the
    // other side (every word differs) — similarity alignment degrades to
    // exactly the old positional behavior rather than declining to pair
    // anything.
    let lines = vec![removed("alpha beta gamma"), added("delta epsilon zeta")];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                Some((removed("alpha beta gamma"), 0)),
                Some((added("delta epsilon zeta"), 1)),
            ),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_align_every_line_when_removed_and_added_runs_are_identical_text() {
    // Every line has an exact match on the other side — similarity
    // alignment must recover the fully positional pairing. Two matched
    // pairs still carry two filler rows, mirroring
    // `should_pair_equal_length_removed_and_added_runs_positionally`'s
    // existing 1-matched-pair/1-filler-row shape (ADR 0044 decision 4's
    // total-row invariant scales the same way with more matches).
    let lines = vec![
        removed("fn a() {}"),
        removed("fn b() {}"),
        added("fn a() {}"),
        added("fn b() {}"),
    ];

    let actual = pair_hunk_lines(&lines);

    assert_eq!(
        vec![
            row(
                Some((removed("fn a() {}"), 0)),
                Some((added("fn a() {}"), 2)),
            ),
            row(
                Some((removed("fn b() {}"), 1)),
                Some((added("fn b() {}"), 3)),
            ),
            row(None, None),
            row(None, None),
        ],
        actual
    );
}

#[test]
fn should_fall_back_to_positional_pairing_when_run_exceeds_similarity_alignment_cap() {
    // A run longer than `SIMILARITY_ALIGNMENT_MAX_RUN_LEN` skips the O(n*m)
    // DP entirely and uses the old positional algorithm — every line is
    // identical text on both sides (so similarity alignment, if it ran,
    // would produce the same pairing anyway), which isolates this test to
    // pinning the size cap itself rather than an alignment-quality
    // difference.
    let run_len = SIMILARITY_ALIGNMENT_MAX_RUN_LEN + 1;
    let removed_lines: Vec<DiffLine> = (0..run_len)
        .map(|index| removed(&format!("unique line {index}")))
        .collect();
    let added_lines: Vec<DiffLine> = (0..run_len)
        .map(|index| added(&format!("unique line {index}")))
        .collect();
    let lines: Vec<DiffLine> = removed_lines
        .iter()
        .cloned()
        .chain(added_lines.iter().cloned())
        .collect();

    let actual = pair_hunk_lines(&lines);

    let mut expected: Vec<SplitRow> = (0..run_len)
        .map(|index| {
            row(
                Some((removed_lines[index].clone(), index)),
                Some((added_lines[index].clone(), run_len + index)),
            )
        })
        .collect();
    expected.extend((0..run_len).map(|_| row(None, None)));
    assert_eq!(expected, actual);
}
