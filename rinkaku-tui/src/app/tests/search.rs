//! Tests for `App::handle_key`'s search dispatch: the original Source-view
//! search (ADR 0057) — `/` starting composing, character/backspace
//! composing, `Esc` cancel (both while composing and after a confirmed
//! search), and the composing-mode priority check taking over the whole key
//! space the same way the review overlay's own check does — plus the tree
//! search amendment (ADR 0057 amendment): the same four dispatch keys
//! (`SearchStart`/`SearchNext`/`SearchPrev`/`SearchCancel`) reaching
//! `Screen::Entry` + `Focus::Tree` instead, and staying a no-op on
//! `Focus::Right`, and the search being cancelled by every key that
//! reshapes the tree's row list or leaves the entry screen (the frozen
//! row-index invariant `App::handle_key`'s `Select` arm documents).
//! `App::with_nav_cursor` (the tree-search jump primitive) is also pinned
//! here, alongside the dispatch it backs.

use super::*;
use crate::app::{Focus, Screen};
use crate::search::{SearchMode, SearchState};
use rstest::rstest;

fn opened_source_screen(report: &Report) -> App {
    App::new(report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
}

#[test]
fn should_start_composing_when_search_start_is_pressed_on_source_screen() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);

    let actual = app.handle_key(InputKey::SearchStart);

    assert_eq!(
        &SearchMode::Composing {
            buffer: String::new()
        },
        actual.search().mode()
    );
}

#[test]
fn should_build_up_the_query_buffer_via_search_char() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report)
        .handle_key(InputKey::SearchStart)
        .handle_key(InputKey::SearchChar('f'))
        .handle_key(InputKey::SearchChar('o'));

    assert_eq!(
        &SearchMode::Composing {
            buffer: "fo".to_string()
        },
        app.search().mode()
    );
}

#[test]
fn should_remove_the_last_character_via_search_backspace() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report)
        .handle_key(InputKey::SearchStart)
        .handle_key(InputKey::SearchChar('f'))
        .handle_key(InputKey::SearchChar('o'))
        .handle_key(InputKey::SearchBackspace);

    assert_eq!(
        &SearchMode::Composing {
            buffer: "f".to_string()
        },
        app.search().mode()
    );
}

#[test]
fn should_cancel_composing_when_search_cancel_is_pressed_while_composing() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report)
        .handle_key(InputKey::SearchStart)
        .handle_key(InputKey::SearchChar('f'))
        .handle_key(InputKey::SearchCancel);

    assert_eq!(&SearchMode::Inactive, app.search().mode());
    assert_eq!(None, app.search().query());
}

#[test]
fn should_clear_a_confirmed_search_when_search_cancel_is_pressed_outside_composing() {
    let report = report_with_one_symbol();
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .confirm(&["fn foo() {}".to_string()], 0);
    let app = opened_source_screen(&report)
        .with_search(search)
        .handle_key(InputKey::SearchCancel);

    assert_eq!(None, app.search().query());
    assert_eq!(&[] as &[crate::search::MatchLine], app.search().matches());
}

#[test]
fn should_advance_to_the_next_match_when_search_next_is_pressed() {
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
    let app = opened_source_screen(&report).with_search(search);
    assert_eq!(Some(0), app.search().current_match());

    let actual = app.handle_key(InputKey::SearchNext);

    assert_eq!(Some(2), actual.search().current_match());
}

#[test]
fn should_retreat_to_the_previous_match_when_search_prev_is_pressed() {
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
    let app = opened_source_screen(&report).with_search(search);

    let actual = app.handle_key(InputKey::SearchPrev);

    assert_eq!(Some(2), actual.search().current_match());
}

#[test]
fn should_swallow_unrelated_keys_while_composing_a_search_query() {
    // Mirrors the review overlay's own "takes over the whole key space"
    // invariant (`App::handle_key`'s doc comment): while composing, a key
    // that would otherwise mean something else (`ToggleHelp`) must not
    // reach its ordinary meaning.
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report)
        .handle_key(InputKey::SearchStart)
        .handle_key(InputKey::ToggleHelp);

    assert_eq!(
        &SearchMode::Composing {
            buffer: String::new()
        },
        app.search().mode()
    );
    assert!(!app.help_open());
}

#[test]
fn should_clear_a_confirmed_search_when_leaving_source_via_back() {
    // Regression: `q` (translated to `InputKey::Back`, same as `Esc` once
    // no confirmed search is active) used to leave `search` untouched,
    // so re-entering Source on a different symbol still showed the old
    // query/matches/highlighting against unrelated content.
    let report = report_with_one_symbol();
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .confirm(&["fn foo() {}".to_string()], 0);
    let app = opened_source_screen(&report).with_search(search);
    assert!(app.search().query().is_some());

    let actual = app.handle_key(InputKey::Back);

    assert_eq!(&SearchMode::Inactive, actual.search().mode());
    assert_eq!(None, actual.search().query());
    assert_eq!(
        &[] as &[crate::search::MatchLine],
        actual.search().matches()
    );
}

#[test]
fn should_not_start_composing_on_the_entry_screen_while_focus_right() {
    // ADR 0057 amendment: tree search is `Focus::Tree`-only on the entry
    // screen — `SearchStart` reaching `handle_key` while `Focus::Right`
    // (defensively, since `crate::input_translate::translate_key` never
    // emits it there) must still be a no-op.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let actual = app.handle_key(InputKey::SearchStart);

    assert_eq!(&SearchMode::Inactive, actual.search().mode());
}

// ADR 0057 amendment: tree search (Entry screen, Focus::Tree).

#[test]
fn should_start_composing_on_the_entry_screen_while_focus_tree() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = app.handle_key(InputKey::SearchStart);

    assert_eq!(
        &SearchMode::Composing {
            buffer: String::new()
        },
        actual.search().mode()
    );
}

#[test]
fn should_advance_to_the_next_tree_match_when_search_next_is_pressed() {
    let report = report_with_one_symbol();
    let texts = vec![
        "foo".to_string(),
        "foobar".to_string(),
        "barfoo".to_string(),
    ];
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&texts, 0);
    let app = App::new(&report).with_search(search);
    assert_eq!(Some(0), app.search().current_match());

    let actual = app.handle_key(InputKey::SearchNext);

    assert_eq!(Some(1), actual.search().current_match());
}

#[test]
fn should_retreat_to_the_previous_tree_match_when_search_prev_is_pressed() {
    let report = report_with_one_symbol();
    let texts = vec!["foo".to_string(), "foobar".to_string()];
    let search = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .confirm(&texts, 0);
    let app = App::new(&report).with_search(search);
    assert_eq!(Some(0), app.search().current_match());

    let actual = app.handle_key(InputKey::SearchPrev);

    assert_eq!(Some(1), actual.search().current_match());
}

#[test]
fn should_clear_a_confirmed_tree_search_when_search_cancel_is_pressed() {
    let report = report_with_one_symbol();
    let search = SearchState::default()
        .start()
        .push_char('f')
        .confirm(&["foo".to_string()], 0);
    let app = App::new(&report).with_search(search);
    assert!(app.search().query().is_some());

    let actual = app.handle_key(InputKey::SearchCancel);

    assert_eq!(None, actual.search().query());
}

fn tree_search_confirmed_on_two_directories(report: &Report) -> App {
    let texts = vec![
        "a".to_string(),
        "a/one.rs".to_string(),
        "foo".to_string(),
        "b".to_string(),
        "b/two.rs".to_string(),
        "bar".to_string(),
    ];
    let search = SearchState::default()
        .start()
        .push_char('o')
        .confirm(&texts, 0);
    App::new(report).with_search(search)
}

#[rstest]
#[case::expand_all(InputKey::ExpandAll)]
#[case::collapse_all(InputKey::CollapseAll)]
#[case::toggle_order(InputKey::ToggleOrder)]
#[case::toggle_expand(InputKey::Select)]
fn should_clear_a_confirmed_tree_search_when_the_row_list_is_reshaped(#[case] key: InputKey) {
    let report = super::report_with_two_directories();
    let app = tree_search_confirmed_on_two_directories(&report);
    assert_eq!(Some("o"), app.search().query());

    let actual = app.handle_key(key);

    assert_eq!(&SearchState::default(), actual.search());
}

#[test]
fn should_clear_a_confirmed_tree_search_when_open_expands_a_directory_row() {
    let report = super::report_with_two_directories();
    let app = tree_search_confirmed_on_two_directories(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = app.handle_key(InputKey::Open);

    assert_eq!(&SearchState::default(), actual.search());
}

#[test]
fn should_clear_a_confirmed_tree_search_when_entering_the_source_screen() {
    let report = report_with_one_symbol();
    let search = SearchState::default()
        .start()
        .push_char('f')
        .confirm(&["lib.rs".to_string(), "foo".to_string()], 0);
    let app = App::new(&report)
        .with_search(search)
        .handle_key(InputKey::Down);

    let actual = app.handle_key(InputKey::Source);

    assert_eq!(
        &Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        actual.screen()
    );
    assert_eq!(&SearchState::default(), actual.search());
}

#[test]
fn should_keep_a_confirmed_tree_search_when_source_is_pressed_on_a_non_symbol_row() {
    let report = report_with_one_symbol();
    let search = SearchState::default()
        .start()
        .push_char('f')
        .confirm(&["lib.rs".to_string(), "foo".to_string()], 0);
    let expected = search.clone();
    // Row 0 of `report_with_one_symbol` is the `lib.rs` File row, which
    // `InputKey::Source`'s guard refuses to open.
    let app = App::new(&report).with_search(search);

    let actual = app.handle_key(InputKey::Source);

    assert_eq!(&Screen::Entry, actual.screen());
    assert_eq!(&expected, actual.search());
}

#[test]
fn should_move_the_tree_cursor_when_with_nav_cursor_is_called() {
    let report = super::report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(0, app.nav().cursor());

    let actual = app.with_nav_cursor(3);

    assert_eq!(3, actual.nav().cursor());
}
