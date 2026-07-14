//! `translate_key` tests: keyboard `KeyCode` → `Option<InputKey>` mapping,
//! covering the plain keymap and the help-overlay / jump-popup "swallow"
//! contracts (ADR 0020, ADR 0022, ADR 0026). Mouse translation lives in
//! `translate_mouse`; the `dispatch_non_source_key` sequence tests live
//! in `goto_dispatch`.

use super::{candidate, empty_report, report_with_one_symbol};
use crate::app::{self, App, InputKey};
use crate::translate_key;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn should_translate_ctrl_c_to_quit_regardless_of_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::Quit), actual);
}

#[test]
fn should_translate_q_to_quit_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Quit), actual);
}

#[test]
fn should_translate_esc_to_none_on_entry_screen() {
    // Esc has no "back" target on the entry screen (App::handle_key's
    // own doc comment) and is not bound to quit there either — quit is
    // 'q'/Ctrl-C only, so Esc is simply not handled at this screen.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_enter_to_open() {
    // ADR 0020: Enter is `Open` (may move focus), distinct from Space's
    // `Select` (never moves focus) — see the two tests right after this
    // one.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Open), actual);
}

#[test]
fn should_translate_space_to_select() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char(' '), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Select), actual);
}

#[test]
fn should_translate_h_to_focus_left_when_right_focused() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(app::Focus::Right, app.focus());

    let actual = translate_key(KeyCode::Char('h'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::FocusLeft), actual);
}

#[test]
fn should_not_translate_h_at_all_when_tree_focused() {
    // `h` has no meaning while Focus::Tree (ADR 0020 only assigns it a
    // "move left/back" meaning while Focus::Right) — must fall through
    // to `None`, not be swallowed by some other arm.
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(app::Focus::Tree, app.focus());

    let actual = translate_key(KeyCode::Char('h'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_esc_to_focus_left_when_right_focused_on_entry_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(app::Focus::Right, app.focus());

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::FocusLeft), actual);
}

#[test]
fn should_translate_lowercase_r_to_toggle_blast_radius() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('r'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleBlastRadius), actual);
}

#[test]
fn should_translate_uppercase_r_to_toggle_blast_radius() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('R'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleBlastRadius), actual);
}

#[test]
fn should_translate_lowercase_v_to_toggle_split_view() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('v'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleSplitView), actual);
}

#[test]
fn should_translate_uppercase_v_to_toggle_split_view() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('V'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleSplitView), actual);
}

#[test]
fn should_translate_right_bracket_to_next_hunk() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char(']'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::NextHunk), actual);
}

#[test]
fn should_translate_left_bracket_to_prev_hunk() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('['), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::PrevHunk), actual);
}

#[test]
fn should_translate_question_mark_to_toggle_help() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('?'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleHelp), actual);
}

#[test]
fn should_translate_esc_to_toggle_help_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleHelp), actual);
}

#[test]
fn should_translate_q_to_toggle_help_instead_of_quit_when_overlay_is_open() {
    // ADR 0020: `q` must close the overlay, not fall through to its
    // normal `Quit` meaning, while it is open.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleHelp), actual);
}

#[test]
fn should_translate_unbound_key_to_none_when_overlay_is_open() {
    // `'j'` used to be this test's example key, back when the overlay
    // had no scroll gestures of its own and swallowed every key but
    // `?`/Esc/`q` (`should_ignore_navigation_keys_while_help_overlay_is_open`'s
    // own App-level counterpart). Scrolling the overlay now maps `'j'`
    // to `InputKey::Down` even while it is open — see
    // `should_translate_j_to_down_for_overlay_scroll_when_overlay_is_open`
    // below — so this test switched to `'z'`, a key with no meaning
    // anywhere in the keymap, to keep pinning "arbitrary keys are
    // swallowed" without asserting something that is no longer true.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('z'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_j_to_down_for_overlay_scroll_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_k_to_up_for_overlay_scroll_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('k'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Up), actual);
}

#[test]
fn should_translate_ctrl_d_to_scroll_half_page_down_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageDown), actual);
}

#[test]
fn should_translate_ctrl_u_to_scroll_half_page_up_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('u'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageUp), actual);
}

#[test]
fn should_translate_uppercase_g_to_scroll_to_bottom_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('G'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToBottom), actual);
}

#[test]
fn should_translate_double_g_to_scroll_to_top_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    let first_g = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);
    assert_eq!(Some(InputKey::PendingGoto), first_g);
    let app = app.handle_key(first_g.expect("first g translates"));

    let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToTop), actual);
}

#[test]
fn should_translate_lowercase_j_to_down_regardless_of_focus() {
    // Regression guard: lowercase j/k are always translated to the same
    // `InputKey::Down`/`Up` regardless of focus — `App::handle_key`, not
    // `translate_key`, is what decides whether that means "move cursor"
    // or "scroll" (ADR 0020).
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Down), actual);
}

// g-prefix and jump-popup translate_key tests (ADR 0022).

#[test]
fn should_translate_g_to_pending_goto() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::PendingGoto), actual);
}

#[test]
fn should_translate_d_to_goto_definition_when_g_is_pending() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::GotoDefinition), actual);
}

#[test]
fn should_translate_r_to_goto_references_when_g_is_pending() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);

    let actual = translate_key(KeyCode::Char('r'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::GotoReferences), actual);
}

#[test]
fn should_translate_d_to_toggle_diff_when_no_prefix_is_pending() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleDiff), actual);
}

#[test]
fn should_fall_through_to_ordinary_meaning_when_a_non_dr_key_follows_pending_goto() {
    // `gj` is not a bound sequence — `j` must still translate to its own
    // ordinary `Down` meaning, not be swallowed just because a prefix
    // was pending (`App::handle_key`'s blanket clear-unless-`PendingGoto`
    // rule is what actually unwinds `pending_prefix` on the next key;
    // this test only pins `translate_key`'s own half of that contract).
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);

    let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_ctrl_o_to_jump_back() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('o'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::JumpBack), actual);
}

#[test]
fn should_translate_ctrl_i_to_jump_forward() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('i'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::JumpForward), actual);
}

#[test]
fn should_translate_tab_to_jump_forward() {
    // A real Ctrl-I keypress arrives here as `KeyCode::Tab`, not
    // `KeyCode::Char('i')` + `CONTROL` — confirmed via manual testing
    // against a real terminal (tmux), since Ctrl-I and Tab share the
    // same control code (0x09) without Kitty's keyboard-enhancement
    // protocol, which this crate does not enable. Without this mapping,
    // Ctrl-i silently did nothing in practice despite the
    // `Char('i') + CONTROL` test above passing.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Tab, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::JumpForward), actual);
}

#[test]
fn should_translate_plain_o_to_toggle_order_without_control_modifier() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('o'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleOrder), actual);
}

// ADR 0026 scroll bindings (translate_key half).

#[test]
fn should_translate_ctrl_d_to_scroll_half_page_down() {
    // Must resolve *before* the plain `Char('d')` arm below (which
    // maps to `ToggleDiff`) — a stale ordering would silently
    // rebind Ctrl-d to Diff-toggle instead of half-page-down.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageDown), actual);
}

#[test]
fn should_translate_ctrl_u_to_scroll_half_page_up() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('u'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageUp), actual);
}

#[test]
fn should_translate_second_g_to_scroll_to_top_when_g_prefix_is_pending() {
    // `gg` (ADR 0026) resolves through the same `pending_prefix`
    // arm `gd`/`gr` do (ADR 0022) — this test pins that a *second*
    // `g` after a pending `g` means "scroll to top", not restart
    // the prefix. Restarting is what a *first* `g` does when no
    // prefix is pending (the `should_translate_g_to_pending_goto`
    // test above); this arm's behavior is the second-key half of
    // the two-key `gg` sequence.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);

    let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToTop), actual);
}

#[test]
fn should_translate_uppercase_g_to_scroll_to_bottom() {
    // Distinct from single-key lowercase `g` (`PendingGoto`, tested
    // above): `G` is a one-key gesture, not the leader of a sequence.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('G'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToBottom), actual);
}

#[test]
fn should_translate_j_and_k_to_popup_motion_while_jump_popup_is_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Char('k'), KeyModifiers::NONE, &app)
    );
}

#[test]
fn should_translate_enter_to_popup_confirm_while_jump_popup_is_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

    let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::PopupConfirm), actual);
}

#[test]
fn should_translate_esc_to_popup_cancel_while_jump_popup_is_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

    let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::PopupCancel), actual);
}

#[test]
fn should_translate_q_to_none_while_jump_popup_is_open() {
    // `q` must not fall through to `Quit` while the popup is open —
    // mirrors the help overlay's own "swallow everything but the
    // close/confirm/cancel keys" contract.
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

    let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

// Full-width key normalization: a reviewer who forgot to switch off a
// Japanese IME sends full-width forms of otherwise-bound ASCII keys —
// normal-mode translation must still resolve them to the same InputKey
// their half-width counterpart would.

#[test]
fn should_translate_fullwidth_n_to_the_same_input_key_as_halfwidth_n() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('ｎ'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::NoteCompose), actual);
}

#[test]
fn should_translate_fullwidth_j_to_down_regardless_of_focus() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('ｊ'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_fullwidth_q_to_quit_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('ｑ'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Quit), actual);
}

#[test]
fn should_translate_fullwidth_question_mark_to_toggle_help() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('？'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ToggleHelp), actual);
}

#[test]
fn should_not_normalize_fullwidth_characters_while_composing_a_note() {
    // The compose buffer is free text (ADR 0048) — a full-width character
    // typed there must land in the note body verbatim, not get folded to
    // its half-width form the way normal-mode single-key gestures are.
    let report = report_with_one_symbol();
    let snapshot = crate::review::SelectionSnapshot {
        path: "lib.rs".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        symbol_name: Some("foo".to_string()),
        range: Some((1, 1)),
        anchor: Some((1, 1)),
        signature: Some("fn foo()".to_string()),
    };
    let review = crate::review::ReviewState::default().begin_compose(snapshot);
    let app = App::new(&report).with_review(review);

    let actual = translate_key(KeyCode::Char('ｎ'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ComposeChar('ｎ')), actual);
}
