use super::*;
use crate::diff_view::{DiffLine, DiffLineKind, FileHunks, Hunk};

fn diff_line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

#[test]
fn should_apply_added_background_and_marker_when_line_was_added_by_the_diff() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn a() {}\nfn foo() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Added, "fn a() {}"),
                diff_line(DiffLineKind::Context, "fn foo() {}"),
            ],
        }],
    }];

    let terminal = draw_source_screen_for_test(&report, dir.path(), &diff_hunks);

    let text = buffer_text(&terminal);
    assert!(text.contains("1+|"));
    let style = find_cell_style(&terminal, "1+|", "fn a");
    assert_eq!(Some(ADDED_BG), style.bg);
    let marker_style = find_cell_style(&terminal, "1+|", "+");
    assert_eq!(Some(Color::Green), marker_style.fg);
}

#[test]
fn should_render_removed_row_with_dash_gutter_and_removed_background() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,2 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn old() {}"),
                diff_line(DiffLineKind::Context, "fn foo() {}"),
            ],
        }],
    }];

    let terminal = draw_source_screen_for_test(&report, dir.path(), &diff_hunks);

    let text = buffer_text(&terminal);
    assert!(text.contains("fn old() {}"));
    let style = find_cell_style(&terminal, "-", "fn old");
    assert_eq!(Some(REMOVED_BG), style.bg);
}

#[test]
fn should_render_plainly_with_no_diff_entry_for_the_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    // Two lines so line 2 falls outside `report_with_one_symbol`'s symbol
    // range (`LineRange { start: 1, end: 1 }`) — line 1 always carries
    // `SOURCE_HIGHLIGHT_BG` regardless of any diff overlay, so asserting
    // "no diff background" needs a line the symbol-range tint doesn't
    // already touch.
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\nfn bar() {}\n").expect("write file");
    let report = report_with_one_symbol();

    let terminal = draw_source_screen_for_test(&report, dir.path(), &[]);

    let text = buffer_text(&terminal);
    assert!(text.contains("fn bar() {}"));
    assert!(!text.contains("diff overlay unavailable"));
    let style = find_cell_style(&terminal, "2 |", "fn bar");
    assert_eq!(Some(Color::Reset), style.bg);
}

#[test]
fn should_fall_back_to_plain_rendering_with_a_title_note_when_file_has_drifted() {
    // Corruption must land on the `Context` line (line 2): `Added` lines
    // are never drift-checked, so only a mismatched `Context` line exercises
    // the detection this test targets.
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn a() {}\nfn edited_since_diff() {}\n",
    )
    .expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Added, "fn a() {}"),
                diff_line(DiffLineKind::Context, "fn foo() {}"),
            ],
        }],
    }];

    let terminal = draw_source_screen_for_test(&report, dir.path(), &diff_hunks);

    let text = buffer_text(&terminal);
    assert!(text.contains("diff overlay unavailable"));
    assert!(text.contains("fn edited_since_diff() {}"));
    let style = find_cell_style(&terminal, "1 |", "fn a");
    assert_eq!(Some(SOURCE_HIGHLIGHT_BG), style.bg);
}
