use super::*;
use crate::app::InputKey;
use crate::diff_view::{DiffLine, DiffLineKind, FileHunks, Hunk};

fn diff_line(kind: DiffLineKind, content: &str) -> DiffLine {
    DiffLine {
        kind,
        content: content.to_string(),
    }
}

fn draw_source_screen_split_for_test(
    report: &Report,
    repo_root: &std::path::Path,
    diff_hunks: &[FileHunks],
    width: u16,
) -> Terminal<TestBackend> {
    let app = App::new(report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        report,
        "lib.rs::foo",
        repo_root,
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(width, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                diff_hunks,
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    terminal
}

#[test]
fn should_draw_old_and_new_lines_side_by_side_by_default() {
    // ADR 0044 amendment: split is now the default `DiffViewMode`, and ADR
    // 0049 extends it to `Screen::Source` too, so no `ToggleSplitView`
    // press is needed to reach it here.
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\nfn bar() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,2 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn old_foo() {}"),
                diff_line(DiffLineKind::Added, "fn foo() {}"),
                diff_line(DiffLineKind::Context, "fn bar() {}"),
            ],
        }],
    }];

    // Wide enough that the source pane comfortably clears
    // `MIN_SPLIT_VIEW_WIDTH`.
    let terminal = draw_source_screen_split_for_test(&report, dir.path(), &diff_hunks, 200);

    let text = buffer_text(&terminal);
    let paired_row = text
        .lines()
        .find(|line| line.contains("old_foo") && line.contains("fn foo()"))
        .unwrap_or_else(|| panic!("expected a row with both sides, got:\n{text}"));
    assert!(paired_row.contains("fn old_foo() {}"));
    assert!(paired_row.contains("fn foo() {}"));

    // The unchanged context line ("fn bar() {}") still mirrors on both
    // sides, same as the diff pane's own split view.
    let context_row = text
        .lines()
        .filter(|line| line.contains("fn bar() {}"))
        .count();
    assert_eq!(
        1, context_row,
        "context line should render on one row, mirrored on both sides:\n{text}"
    );
}

#[test]
fn should_apply_token_highlighting_to_the_new_side_of_a_changed_row() {
    // Regression: a `Changed` row's new-side cell is still new-side
    // source text with a valid `token_highlights` index, the same as an
    // `Unchanged` row's — it must not lose that signal just because it's
    // part of a paired row.
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn old_foo() {}"),
                diff_line(DiffLineKind::Added, "fn foo() {}"),
            ],
        }],
    }];

    let terminal = draw_source_screen_split_for_test(&report, dir.path(), &diff_hunks, 200);

    let style = find_cell_style(&terminal, "fn foo() {}", "fn");
    assert_eq!(Some(Color::Magenta), style.fg);
}

#[test]
fn should_fall_back_to_unified_when_pane_is_narrower_than_the_split_view_minimum() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn old_foo() {}"),
                diff_line(DiffLineKind::Added, "fn foo() {}"),
            ],
        }],
    }];

    // Narrower than `MIN_SPLIT_VIEW_WIDTH` (100): the pane must render
    // unified (ADR 0049 decision 6, mirroring ADR 0044 decision 7) even
    // though `diff_view_mode` defaults to `Split`.
    let terminal = draw_source_screen_split_for_test(&report, dir.path(), &diff_hunks, 80);

    let text = buffer_text(&terminal);
    assert!(text.contains("split view needs a wider pane"));
    assert!(text.contains("fn old_foo() {}"));
    assert!(text.contains("fn foo() {}"));
}

#[test]
fn should_render_unified_when_split_view_is_toggled_off() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\n").expect("write file");
    let report = report_with_one_symbol();
    let diff_hunks = vec![FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![
                diff_line(DiffLineKind::Removed, "fn old_foo() {}"),
                diff_line(DiffLineKind::Added, "fn foo() {}"),
            ],
        }],
    }];
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .handle_key(InputKey::ToggleSplitView);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        "lib.rs::foo",
        dir.path(),
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(200, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &diff_hunks,
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(!text.contains("split view needs a wider pane"));
    // Unified overlay's own removed-row rendering (ADR 0046): a `-`
    // gutter, no line number.
    let style = find_cell_style(&terminal, "-", "fn old_foo");
    assert_eq!(Some(REMOVED_BG), style.bg);
}

#[test]
fn should_fall_back_to_unified_when_file_has_drifted_since_the_diff_was_produced() {
    // Corruption must land on the `Context` line: `Added` lines are never
    // drift-checked (`crate::source_split::reconstruct_old_lines`'s own
    // doc comment, mirroring the unified overlay's precedent).
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

    let terminal = draw_source_screen_split_for_test(&report, dir.path(), &diff_hunks, 200);

    let text = buffer_text(&terminal);
    assert!(text.contains("diff overlay unavailable"));
    assert!(!text.contains("split view needs a wider pane"));
    assert!(text.contains("fn edited_since_diff() {}"));
}
