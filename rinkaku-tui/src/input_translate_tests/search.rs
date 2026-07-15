//! `translate_key` tests for ADR 0057's Source-view search bindings: `/`
//! starting a query, the composing-mode early return, `n`/`N` being
//! Source-screen-only (and not colliding with the entry screen's own
//! review-note `n`/`N`), and Esc's dual "cancel search" / "back" meaning.

use super::{empty_report, report_with_one_symbol};
use crate::app::{App, InputKey};
use crate::input_translate::translate_key;
use crate::search::SearchState;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

fn opened_source_screen(report: &rinkaku_core::render::Report) -> App {
    App::new(report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
}

#[test]
fn should_translate_slash_to_search_start_on_source_screen() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);

    let actual = translate_key(KeyCode::Char('/'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchStart), actual);
}

#[test]
fn should_not_translate_slash_at_all_on_entry_screen() {
    // ADR 0057: search is Source-screen-only — `/` has no meaning on the
    // entry screen (diff-pane search is explicit future work, not this
    // ADR's scope).
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('/'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_n_to_search_next_on_source_screen() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);

    let actual = translate_key(KeyCode::Char('n'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchNext), actual);
}

#[test]
fn should_translate_uppercase_n_to_search_prev_on_source_screen() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);

    let actual = translate_key(KeyCode::Char('N'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchPrev), actual);
}

#[test]
fn should_translate_n_to_note_compose_on_entry_screen() {
    // Unaffected by ADR 0057: the entry screen's own `n` (ADR 0048) keeps
    // its pre-existing meaning, since the search bindings are gated on
    // `on_source_screen`.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('n'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::NoteCompose), actual);
}

#[test]
fn should_translate_uppercase_n_to_notes_list_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('N'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::NotesList), actual);
}

#[test]
fn should_translate_printable_char_to_search_char_while_composing() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Char('f'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchChar('f')), actual);
}

#[test]
fn should_translate_question_mark_to_search_char_while_composing() {
    // Mirrors the review overlay's own "a literal `?` must reach the
    // buffer, not the help overlay" precedent (`translate_key`'s own doc
    // comment) — the same must hold for a search query.
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Char('?'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchChar('?')), actual);
}

#[test]
fn should_translate_backspace_to_search_backspace_while_composing() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Backspace, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchBackspace), actual);
}

#[test]
fn should_translate_enter_to_search_confirm_while_composing() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchConfirm), actual);
}

#[test]
fn should_translate_esc_to_search_cancel_while_composing() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchCancel), actual);
}

#[test]
fn should_translate_esc_to_search_cancel_when_a_confirmed_search_is_active() {
    // ADR 0057: Esc's first press clears an active confirmed search
    // rather than immediately leaving the screen.
    let search = SearchState::default()
        .start()
        .push_char('f')
        .confirm(&["fn foo() {}".to_string()], 0);
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report).with_search(search);

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchCancel), actual);
}

#[test]
fn should_translate_esc_to_back_when_no_search_is_active_on_source_screen() {
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);
    assert_eq!(None, app.search().query());

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Back), actual);
}
