//! `translate_key` tests for ADR 0057's Source-view search bindings: `/`
//! starting a query, the composing-mode early return, `n`/`N` being
//! Source-screen-only (freed entirely on the entry screen by ADR 0058,
//! which moved the review-annotation bindings to `a`/`A`), and Esc's dual
//! "cancel search" / "back" meaning.

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
fn should_translate_n_to_none_on_entry_screen() {
    // ADR 0058: `n`/`N` are freed entirely on the entry screen (moved to
    // `a`/`A`), reserved for a future Entry-screen search reusing the same
    // next/prev idiom Source view already has.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('n'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_uppercase_n_to_none_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('N'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_a_to_annotation_compose_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('a'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::AnnotationCompose), actual);
}

#[test]
fn should_translate_uppercase_a_to_annotations_list_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('A'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::AnnotationsList), actual);
}

#[test]
fn should_translate_a_to_annotation_compose_on_source_screen() {
    // `translate_key` maps `a` unconditionally, the same way it mapped `n`
    // before ADR 0058 — `App::handle_key`'s own `Screen::Entry` guard on
    // `AnnotationCompose`/`AnnotationsList` is what actually makes the
    // binding a no-op on the source screen, not this function.
    let report = report_with_one_symbol();
    let app = opened_source_screen(&report);

    let actual = translate_key(KeyCode::Char('a'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::AnnotationCompose), actual);
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
