use super::empty_report;
use crate::app::{App, InputKey};
use crate::stack::{StackEntry, StackPosition};
use pretty_assertions::assert_eq;

fn stack_position(cursor: usize) -> StackPosition {
    StackPosition::new(
        vec![
            StackEntry {
                number: 42,
                title: "auth".to_string(),
                head_ref_name: "auth".to_string(),
            },
            StackEntry {
                number: 43,
                title: "api".to_string(),
                head_ref_name: "api".to_string(),
            },
        ],
        cursor,
    )
}

#[test]
#[ignore = "not implemented"]
fn should_request_switch_to_the_layer_above_when_next_pr_is_pressed_in_stack_mode() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position(0)))
        .handle_key(InputKey::NextPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(Some(1), actual);
}

#[test]
#[ignore = "not implemented"]
fn should_not_request_a_switch_when_prev_pr_is_pressed_at_the_bottom() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position(0)))
        .handle_key(InputKey::PrevPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(None, actual);
}

#[test]
#[ignore = "not implemented"]
fn should_ignore_pr_keys_when_not_in_stack_mode() {
    let report = empty_report();
    let mut app = App::new(&report)
        .handle_key(InputKey::NextPr)
        .handle_key(InputKey::PrevPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(None, actual);
}
