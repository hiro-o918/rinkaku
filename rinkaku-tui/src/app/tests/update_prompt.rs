use super::empty_report;
use crate::app::{App, InputKey, JumpCandidate};
use pretty_assertions::assert_eq;
use rstest::rstest;

// Update-available prompt tests (ADR 0054, amended to auto-open at
// startup): `notify_update_available`, `OpenUpdatePrompt`'s gating on
// `update_available`, and the popup's own `PopupConfirm`/`PopupCancel`
// handling.

#[test]
fn should_set_update_available_and_auto_open_prompt_when_notified() {
    let report = empty_report();
    let mut app = App::new(&report);

    app.notify_update_available("1.2.3");

    assert_eq!(Some("1.2.3"), app.update_available());
    assert_eq!(true, app.update_prompt_open());
}

#[test]
fn should_not_auto_open_prompt_when_notified_while_help_overlay_is_open() {
    let report = empty_report();
    let mut app = App::new(&report).handle_key(InputKey::ToggleHelp);

    app.notify_update_available("1.2.3");

    assert_eq!(Some("1.2.3"), app.update_available());
    assert_eq!(false, app.update_prompt_open());
    assert_eq!(true, app.help_open());
}

#[test]
fn should_not_auto_open_prompt_when_notified_while_jump_popup_is_open() {
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
fn should_not_reopen_prompt_when_notified_again_after_dismissal() {
    let report = empty_report();
    let mut app = App::new(&report);
    app.notify_update_available("1.2.3");
    let mut app = app.handle_key(InputKey::PopupCancel);

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
    let app = app.handle_key(InputKey::PopupCancel);

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

#[rstest]
#[case::should_open_when_available_and_not_dismissed_and_no_other_modal(true, false, true, true)]
#[case::should_not_open_when_unavailable(false, false, true, false)]
#[case::should_not_reopen_when_already_dismissed(true, true, true, false)]
#[case::should_not_open_when_unavailable_and_dismissed(false, true, true, false)]
#[case::should_not_open_when_other_modal_active(true, false, false, false)]
fn should_decide_auto_open_from_availability_dismissal_and_other_modal(
    #[case] update_available: bool,
    #[case] update_prompt_dismissed: bool,
    #[case] no_other_modal_active: bool,
    #[case] expected: bool,
) {
    let actual = super::super::should_auto_open_update_prompt(
        update_available,
        update_prompt_dismissed,
        no_other_modal_active,
    );

    assert_eq!(expected, actual);
}
