use super::*;
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;

#[test]
fn should_return_unsplit_hunk_when_only_one_symbol_intersects() {
    let original = hunk(
        "@@ -1,2 +1,2 @@",
        1,
        &[
            (DiffLineKind::Context, "fn foo() {"),
            (DiffLineKind::Added, "    body();"),
        ],
    );
    let symbols = [(0, LineRange { start: 1, end: 5 })];

    let actual = split_hunk(&original, &symbols);

    assert_eq!(
        vec![(
            Some(0),
            SubHunk {
                header: original.header.clone(),
                new_range: original.new_range,
                lines: original.lines.clone(),
                origin_offset: 0,
            }
        )],
        actual
    );
}

#[test]
fn should_return_unsplit_hunk_with_none_owner_when_no_symbol_intersects() {
    let original = hunk(
        "@@ -1,1 +1,1 @@",
        1,
        &[(DiffLineKind::Context, "use foo::bar;")],
    );
    let symbols = [(0, LineRange { start: 10, end: 20 })];

    let actual = split_hunk(&original, &symbols);

    assert_eq!(
        vec![(
            None,
            SubHunk {
                header: original.header.clone(),
                new_range: original.new_range,
                lines: original.lines.clone(),
                origin_offset: 0,
            }
        )],
        actual
    );
}

#[test]
fn should_split_added_and_context_lines_at_symbol_boundary() {
    // Two adjacent symbols sharing one hunk, no Removed lines: `foo` owns
    // new-file lines 1-2, `bar` owns line 3.
    let original = hunk(
        "@@ -1,3 +1,3 @@",
        1,
        &[
            (DiffLineKind::Context, "fn foo() {}"),
            (DiffLineKind::Added, "fn foo2() {}"),
            (DiffLineKind::Context, "fn bar() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 2 }),
        (1, LineRange { start: 3, end: 3 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -1,1 +1,2 @@",
                Some((1, 2)),
                &[
                    (DiffLineKind::Context, "fn foo() {}"),
                    (DiffLineKind::Added, "fn foo2() {}"),
                ],
                0,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -2,1 +3,1 @@",
                Some((3, 3)),
                &[(DiffLineKind::Context, "fn bar() {}")],
                2,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_route_gap_between_symbols_to_module_level_bucket() {
    // `foo` owns line 1, an unowned import sits at line 2, `bar` owns line
    // 3 — three sub-hunks, the middle one with a `None` owner.
    let original = hunk(
        "@@ -1,3 +1,3 @@",
        1,
        &[
            (DiffLineKind::Context, "fn foo() {}"),
            (DiffLineKind::Added, "use foo::bar;"),
            (DiffLineKind::Context, "fn bar() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 1 }),
        (1, LineRange { start: 3, end: 3 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -1,1 +1,1 @@",
                Some((1, 1)),
                &[(DiffLineKind::Context, "fn foo() {}")],
                0,
            ),
        ),
        (
            None,
            sub(
                "@@ -2,0 +2,1 @@",
                Some((2, 2)),
                &[(DiffLineKind::Added, "use foo::bar;")],
                1,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -2,1 +3,1 @@",
                Some((3, 3)),
                &[(DiffLineKind::Context, "fn bar() {}")],
                2,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_attribute_removed_only_run_to_the_previous_owner() {
    // `foo` (lines 1-2) is followed by a Removed-only run (no new-file
    // line of its own) before `bar` (line 3) resumes — the removed run
    // must inherit `foo`'s ownership (the run immediately preceding it),
    // not `bar`'s.
    let original = hunk(
        "@@ -1,4 +1,3 @@",
        1,
        &[
            (DiffLineKind::Context, "fn foo() {}"),
            (DiffLineKind::Added, "fn foo2() {}"),
            (DiffLineKind::Removed, "fn old_helper() {}"),
            (DiffLineKind::Context, "fn bar() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 2 }),
        (1, LineRange { start: 3, end: 3 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -1,2 +1,2 @@",
                Some((1, 2)),
                &[
                    (DiffLineKind::Context, "fn foo() {}"),
                    (DiffLineKind::Added, "fn foo2() {}"),
                    (DiffLineKind::Removed, "fn old_helper() {}"),
                ],
                0,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -3,1 +3,1 @@",
                Some((3, 3)),
                &[(DiffLineKind::Context, "fn bar() {}")],
                3,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_retroactively_attribute_leading_removed_run_to_the_next_owner() {
    // The hunk opens with Removed lines before any Added/Context line has
    // appeared — those lines have no "previous owner" yet, so they must
    // take the owner of the next resolved (Added/Context) line, which is
    // `bar`.
    let original = hunk(
        "@@ -1,3 +1,2 @@",
        1,
        &[
            (DiffLineKind::Removed, "fn old_top_level() {}"),
            (DiffLineKind::Context, "fn foo() {}"),
            (DiffLineKind::Context, "fn bar() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 1 }),
        (1, LineRange { start: 2, end: 2 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -1,2 +1,1 @@",
                Some((1, 1)),
                &[
                    (DiffLineKind::Removed, "fn old_top_level() {}"),
                    (DiffLineKind::Context, "fn foo() {}"),
                ],
                0,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -3,1 +2,1 @@",
                Some((2, 2)),
                &[(DiffLineKind::Context, "fn bar() {}")],
                2,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_split_brand_new_file_single_hunk_into_one_sub_hunk_per_symbol() {
    // ADR 0029's own regression scenario: a brand-new file is always
    // exactly one hunk spanning the whole file, so every symbol it
    // defines shares that one hunk. ADR 0053 now splits it into one
    // sub-hunk per symbol instead of cloning the whole hunk into every
    // section.
    let original = hunk(
        "@@ -0,0 +1,3 @@",
        1,
        &[
            (DiffLineKind::Added, "fn foo() {}"),
            (DiffLineKind::Added, "fn bar() {}"),
            (DiffLineKind::Added, "fn baz() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 1 }),
        (1, LineRange { start: 2, end: 2 }),
        (2, LineRange { start: 3, end: 3 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -0,0 +1,1 @@",
                Some((1, 1)),
                &[(DiffLineKind::Added, "fn foo() {}")],
                0,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -0,0 +2,1 @@",
                Some((2, 2)),
                &[(DiffLineKind::Added, "fn bar() {}")],
                1,
            ),
        ),
        (
            Some(2),
            sub(
                "@@ -0,0 +3,1 @@",
                Some((3, 3)),
                &[(DiffLineKind::Added, "fn baz() {}")],
                2,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_recompute_old_side_header_across_multiple_sub_hunks() {
    // Old-side start must accumulate: the second sub-hunk's `-` start is
    // the original hunk's old start (10) plus the first sub-hunk's own
    // old-side line count (2: one context + one removed).
    let original = hunk(
        "@@ -10,3 +10,3 @@",
        10,
        &[
            (DiffLineKind::Context, "fn foo() {}"),
            (DiffLineKind::Removed, "fn old() {}"),
            (DiffLineKind::Added, "fn bar() {}"),
        ],
    );
    let symbols = [
        (0, LineRange { start: 10, end: 10 }),
        (1, LineRange { start: 11, end: 11 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected = vec![
        (
            Some(0),
            sub(
                "@@ -10,2 +10,1 @@",
                Some((10, 10)),
                &[
                    (DiffLineKind::Context, "fn foo() {}"),
                    (DiffLineKind::Removed, "fn old() {}"),
                ],
                0,
            ),
        ),
        (
            Some(1),
            sub(
                "@@ -12,0 +11,1 @@",
                Some((11, 11)),
                &[(DiffLineKind::Added, "fn bar() {}")],
                2,
            ),
        ),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_emit_one_sub_hunk_per_symbol_when_symbol_ranges_overlap() {
    // Pathological input (a real extractor would not normally produce
    // overlapping symbol ranges), but ADR 0029's "attribute to every
    // intersecting symbol" contract still applies: a line whose new-file
    // position falls inside both `foo` (1-5) and `bar` (3-8) must appear
    // in both symbols' sub-hunks rather than only the first one found.
    let original = hunk(
        "@@ -1,1 +1,5 @@",
        3,
        &[(DiffLineKind::Context, "shared line")],
    );
    let symbols = [
        (0, LineRange { start: 1, end: 5 }),
        (1, LineRange { start: 3, end: 8 }),
    ];

    let actual = split_hunk(&original, &symbols);

    let expected_sub = sub(
        "@@ -1,1 +3,1 @@",
        Some((3, 3)),
        &[(DiffLineKind::Context, "shared line")],
        0,
    );
    let expected = vec![(Some(0), expected_sub.clone()), (Some(1), expected_sub)];
    assert_eq!(expected, actual);
}
