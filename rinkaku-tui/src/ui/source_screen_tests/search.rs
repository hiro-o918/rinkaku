//! Tests for the Source-view search match highlighting (ADR 0057):
//! `crate::ui::source_screen`'s reuse of the existing diff-overlay
//! bg-blend mechanism (`SEARCH_MATCH_BG`/`SEARCH_CURRENT_MATCH_BG`) for a
//! non-current and the current match respectively.

use super::*;
use crate::search::SearchState;

fn draw_source_screen_with_search(
    report: &Report,
    repo_root: &std::path::Path,
    search: SearchState,
) -> Terminal<TestBackend> {
    let app = App::new(report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source)
        .with_search(search);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        report,
        "lib.rs::foo",
        repo_root,
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

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
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    terminal
}

#[test]
fn should_apply_current_match_background_to_the_confirmed_current_match_line() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn foo() {}\nfn bar() {}\nfn foo_helper() {}\n",
    )
    .expect("write file");
    let report = report_with_one_symbol();
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);

    let terminal = draw_source_screen_with_search(&report, dir.path(), search);

    // Line 1 (index 0) is the current match.
    let style = find_cell_style(&terminal, "1 | fn foo() {}", "fn");
    assert_eq!(Some(SEARCH_CURRENT_MATCH_BG), style.bg);
}

#[test]
fn should_apply_plain_match_background_to_a_non_current_match_line() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn foo() {}\nfn bar() {}\nfn foo_helper() {}\n",
    )
    .expect("write file");
    let report = report_with_one_symbol();
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);

    let terminal = draw_source_screen_with_search(&report, dir.path(), search);

    // Line 3 (index 2) is a match but not the current one (current is
    // line 1) — must carry the dimmer non-current tint, distinguishable
    // from `SEARCH_CURRENT_MATCH_BG`.
    let style = find_cell_style(&terminal, "3 | fn foo_helper() {}", "fn");
    assert_eq!(Some(SEARCH_MATCH_BG), style.bg);
}

#[test]
fn should_not_apply_any_match_background_to_a_non_matching_line() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn foo() {}\nfn bar() {}\nfn foo_helper() {}\n",
    )
    .expect("write file");
    let report = report_with_one_symbol();
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);

    let terminal = draw_source_screen_with_search(&report, dir.path(), search);

    // Line 2 ("fn bar() {}") contains neither the query nor falls inside
    // the symbol's own highlighted range (`report_with_one_symbol`'s
    // `LineRange { start: 1, end: 1 }`), so it must render with no
    // background tint at all.
    let style = find_cell_style(&terminal, "2 | fn bar() {}", "fn");
    assert_eq!(Some(Color::Reset), style.bg);
}

#[test]
fn should_apply_no_match_background_when_search_is_inactive() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn foo() {}\nfn bar() {}\n").expect("write file");
    let report = report_with_one_symbol();

    let terminal = draw_source_screen_with_search(&report, dir.path(), SearchState::default());

    let style = find_cell_style(&terminal, "2 | fn bar() {}", "fn");
    assert_eq!(Some(Color::Reset), style.bg);
}
