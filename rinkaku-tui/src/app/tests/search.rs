//! Tests for `App::handle_key`'s Source-view search dispatch (ADR 0057):
//! `/` starting composing, character/backspace composing, `Esc` cancel
//! (both while composing and after a confirmed search), and the
//! composing-mode priority check taking over the whole key space the same
//! way the review overlay's own check does.

use super::*;
use crate::search::{SearchMode, SearchState};

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
fn should_not_start_composing_on_the_entry_screen() {
    // ADR 0057: search is Source-screen-only — `SearchStart` reaching
    // `handle_key` while `Screen::Entry` (defensively, since
    // `crate::input_translate::translate_key` never emits it there) must
    // be a no-op.
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let actual = app.handle_key(InputKey::SearchStart);

    assert_eq!(&SearchMode::Inactive, actual.search().mode());
}
