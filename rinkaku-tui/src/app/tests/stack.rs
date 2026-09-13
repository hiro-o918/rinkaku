use super::empty_report;
use crate::app::{App, InputKey};
use crate::stack::{StackEntry, StackPosition};
use pretty_assertions::assert_eq;

fn stack_position(cursor: usize) -> StackPosition {
    StackPosition::new(
        "main".to_string(),
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

fn stack_position_with_last_visited(cursor: usize, last_visited: usize) -> StackPosition {
    stack_position(cursor).with_last_visited(Some(last_visited))
}

#[test]
fn should_request_switch_to_the_layer_above_when_next_pr_is_pressed_in_stack_mode() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position(0)))
        .handle_key(InputKey::NextPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(Some(1), actual);
}

#[test]
fn should_not_request_a_switch_when_prev_pr_is_pressed_at_the_bottom() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position(0)))
        .handle_key(InputKey::PrevPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(None, actual);
}

#[test]
fn should_ignore_pr_keys_when_not_in_stack_mode() {
    let report = empty_report();
    let mut app = App::new(&report)
        .handle_key(InputKey::NextPr)
        .handle_key(InputKey::PrevPr)
        .handle_key(InputKey::LastPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(None, actual);
}

#[test]
fn should_request_switch_to_last_visited_layer_when_last_pr_is_pressed() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position_with_last_visited(1, 0)))
        .handle_key(InputKey::LastPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(Some(0), actual);
}

#[test]
fn should_not_request_a_switch_when_last_pr_is_pressed_without_history() {
    let report = empty_report();
    let mut app = App::new(&report)
        .with_stack(Some(stack_position(0)))
        .handle_key(InputKey::LastPr);

    let actual = app.take_pr_switch_request();

    assert_eq!(None, actual);
}
