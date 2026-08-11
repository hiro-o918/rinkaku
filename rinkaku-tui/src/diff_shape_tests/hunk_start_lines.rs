use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_hunk_starts_when_content_is_empty() {
    let actual = hunk_start_lines(&DiffPaneContent::Empty);

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_start_the_first_hunk_at_line_zero() {
    let content = DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -1,1 +1,2 @@",
            Some((1, 2)),
            vec!["fn a() {}", "fn foo() {}"],
        ),
    )]);

    let actual = hunk_start_lines(&content);

    assert_eq!(vec![0], actual);
}

#[test]
fn should_offset_second_hunk_start_by_first_hunk_header_and_body_length() {
    // First hunk: header(0), 2 body lines(1,2) — 3 lines. Blank
    // separator(3), second hunk header(4) — starts at 0, 4.
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk(
                "@@ -1,1 +1,2 @@",
                Some((1, 2)),
                vec!["fn a() {}", "fn b() {}"],
            ),
        ),
        attributed(
            1,
            hunk("@@ -10,1 +11,1 @@", Some((11, 11)), vec!["fn c() {}"]),
        ),
    ]);

    let actual = hunk_start_lines(&content);

    assert_eq!(vec![0, 4], actual);
}

#[test]
fn should_offset_every_hunk_start_for_a_file_with_three_hunks() {
    // Hunk 0: header(0), 1 body line(1) — 2 lines. Blank(2), hunk 1
    // header(3), 1 body line(4) — 2 lines. Blank(5), hunk 2 header(6).
    let content = DiffPaneContent::File(vec![
        attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
        ),
        attributed(
            1,
            hunk("@@ -5,1 +5,1 @@", Some((5, 5)), vec!["use foo::bar;"]),
        ),
        attributed(
            2,
            hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
        ),
    ]);

    let actual = hunk_start_lines(&content);

    assert_eq!(vec![0, 3, 6], actual);
}
