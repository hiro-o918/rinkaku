//! `translate_key` tests for ADR 0057's Source-view search bindings: `/`
//! starting a query, the composing-mode early return, `n`/`N`, and Esc's
//! dual "cancel search" / "back" meaning — plus the ADR 0057 amendment's
//! tree search, which reuses the exact same bindings on `Screen::Entry` +
//! `Focus::Tree` (ADR 0058 having freed `n`/`N` there in anticipation of
//! exactly this) while leaving `Focus::Right` untranslated, reserved for a
//! possible future Diff-pane search.

use super::{empty_report, report_with_one_symbol};
use crate::app::{App, Focus, InputKey};
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
fn should_translate_slash_to_search_start_on_entry_screen_while_focus_tree() {
    // ADR 0057 amendment: tree search reuses `/` on `Screen::Entry` +
    // `Focus::Tree` (the entry view's default focus).
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = translate_key(KeyCode::Char('/'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchStart), actual);
}

#[test]
fn should_not_translate_slash_at_all_on_entry_screen_while_focus_right() {
    // Diff/Detail pane search is still explicit future work (ADR 0057's own
    // Alternatives) — `/` stays reserved, untranslated, while `Focus::Right`.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

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
fn should_translate_n_to_search_next_on_entry_screen_while_focus_tree() {
    // ADR 0058 freed `n`/`N` on the entry screen precisely so the ADR 0057
    // amendment's tree search could reuse them without colliding with
    // anything (they used to be the review-annotation bindings, moved to
    // `a`/`A`).
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = translate_key(KeyCode::Char('n'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchNext), actual);
}

#[test]
fn should_translate_uppercase_n_to_search_prev_on_entry_screen_while_focus_tree() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('N'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchPrev), actual);
}

#[test]
fn should_translate_n_to_none_on_entry_screen_while_focus_right() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    let actual = translate_key(KeyCode::Char('n'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_uppercase_n_to_none_on_entry_screen_while_focus_right() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);

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

// ADR 0057 amendment: tree search composing on `Screen::Entry` +
// `Focus::Tree` mirrors the Source-view composing tests above exactly —
// the composing-mode early return in `translate_key` is screen-agnostic
// once entered (its own doc comment).

#[test]
fn should_translate_printable_char_to_search_char_while_composing_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Char('f'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchChar('f')), actual);
}

#[test]
fn should_translate_enter_to_search_confirm_while_composing_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report).with_search(SearchState::default().start());

    let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchConfirm), actual);
}

#[test]
fn should_translate_esc_to_search_cancel_when_a_confirmed_tree_search_is_active() {
    let search = SearchState::default()
        .start()
        .push_char('f')
        .confirm(&["foo".to_string()], 0);
    let report = empty_report();
    let app = App::new(&report).with_search(search);

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::SearchCancel), actual);
}
