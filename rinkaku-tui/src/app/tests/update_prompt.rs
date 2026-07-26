use super::empty_report;
use crate::app::{App, InputKey, JumpCandidate};
use pretty_assertions::assert_eq;

// Update-available prompt tests (ADR 0054, amended by ADR 0062 to drop
// the auto-open): `notify_update_available`, `OpenUpdatePrompt`'s gating
// on `update_available`, and the popup's own `PopupConfirm`/`PopupCancel`
// handling.

#[test]
fn should_set_update_available_without_opening_prompt_when_notified() {
    let report = empty_report();
    let mut app = App::new(&report);

    app.notify_update_available("1.2.3");

    assert_eq!(Some("1.2.3"), app.update_available());
    assert_eq!(false, app.update_prompt_open());
}

#[test]
fn should_leave_help_overlay_untouched_when_notified_while_it_is_open() {
    let report = empty_report();
    let mut app = App::new(&report).handle_key(InputKey::ToggleHelp);

    app.notify_update_available("1.2.3");

    assert_eq!(Some("1.2.3"), app.update_available());
    assert_eq!(false, app.update_prompt_open());
    assert_eq!(true, app.help_open());
}

#[test]
fn should_leave_jump_popup_untouched_when_notified_while_it_is_open() {
    let report = empty_report();
    let mut app = App::new(&report).open_jump_popup(vec![JumpCandidate {
        id: "lib.rs::foo".to_string(),
        name: "foo".to_string(),
        path: "lib.rs".to_string(),
    }]);

    app.notify_update_available("1.2.3");

    assert_eq!(Some("1.2.3"), app.update_available());
    assert_eq!(false, app.update_prompt_open());
    assert_eq!(true, app.jump_popup().is_some());
}

#[test]
fn should_replace_the_version_without_opening_prompt_when_notified_again() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");

    app.notify_update_available("1.2.4");

    assert_eq!(Some("1.2.4"), app.update_available());
    assert_eq!(false, app.update_prompt_open());
}

#[test]
fn should_not_open_update_prompt_when_open_update_prompt_is_pressed_and_no_update_is_available() {
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::OpenUpdatePrompt);

    assert_eq!(false, app.update_prompt_open());
}

#[test]
fn should_reopen_update_prompt_when_open_update_prompt_is_pressed_after_dismissal() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");
    let app = app
        .handle_key(InputKey::OpenUpdatePrompt)
        .handle_key(InputKey::PopupCancel);

    let app = app.handle_key(InputKey::OpenUpdatePrompt);

    assert_eq!(true, app.update_prompt_open());
    assert_eq!(false, app.should_quit());
    assert_eq!(false, app.update_requested());
}

#[test]
fn should_set_should_quit_and_update_requested_when_popup_confirm_is_pressed() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");
    let app = app.handle_key(InputKey::OpenUpdatePrompt);

    let app = app.handle_key(InputKey::PopupConfirm);

    assert_eq!(false, app.update_prompt_open());
    assert_eq!(true, app.should_quit());
    assert_eq!(true, app.update_requested());
}

#[test]
fn should_close_popup_without_quitting_when_popup_cancel_is_pressed() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");
    let app = app.handle_key(InputKey::OpenUpdatePrompt);

    let app = app.handle_key(InputKey::PopupCancel);

    assert_eq!(false, app.update_prompt_open());
    assert_eq!(false, app.should_quit());
    assert_eq!(false, app.update_requested());
    // The update hint itself must survive a cancel — only the popup
    // closes, not the underlying availability the status line still
    // advertises.
    assert_eq!(Some("1.2.3"), app.update_available());
}

#[test]
fn should_ignore_other_keys_while_update_prompt_is_open() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");
    let app = app.handle_key(InputKey::OpenUpdatePrompt);

    let app = app.handle_key(InputKey::Down);

    assert_eq!(true, app.update_prompt_open());
    assert_eq!(false, app.should_quit());
}
