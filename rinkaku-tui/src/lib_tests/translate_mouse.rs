//! `translate_mouse_event` tests: mouse-wheel/click → `Option<InputKey>`
//! mapping. Horizontal wheel input and clicks/drags/moves all resolve to
//! `None` (this crate has no click-targeting).

use crate::app::InputKey;
use crate::translate_mouse_event;
use ratatui::crossterm::event;

#[test]
fn should_translate_scroll_up_to_input_key_up() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollUp);

    assert_eq!(Some(InputKey::Up), actual);
}

#[test]
fn should_translate_scroll_down_to_input_key_down() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollDown);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_scroll_left_to_none() {
    // Horizontal wheel/trackpad input has no mapping — this crate has
    // no horizontally-scrollable pane (this function's own doc comment).
    let actual = translate_mouse_event(event::MouseEventKind::ScrollLeft);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_scroll_right_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollRight);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_click_to_none() {
    // Clicks/drags/moves are deliberately out of scope (no pane
    // targeting by click position) — this function's own doc comment.
    let actual = translate_mouse_event(event::MouseEventKind::Down(event::MouseButton::Left));

    assert_eq!(None, actual);
}

#[test]
fn should_translate_drag_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::Drag(event::MouseButton::Left));

    assert_eq!(None, actual);
}

#[test]
fn should_translate_mouse_moved_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::Moved);

    assert_eq!(None, actual);
}
