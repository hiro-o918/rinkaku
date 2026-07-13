//! `should_apply_hunk_jump` and `jump_scroll_target` tests: the two-step
//! `]`/`[` hunk-jump dispatch (application gate + pure scroll-target
//! computation) used by the diff pane on the entry screen.

use super::report_with_one_symbol;
use crate::app::{self, App, InputKey};
use crate::{jump_scroll_target, should_apply_hunk_jump};

// --- should_apply_hunk_jump ---
//
// Regression coverage for the cross-pane key-leak this gate was added
// to fix: `]`/`[` used to fire (scrolling `diff_pane_content`'s cached
// hunk-offset table) whenever `Focus::Right` held, regardless of which
// right pane was actually showing — so opening a file (Focus::Right,
// RightPane::Diff by default), pressing `d` to switch to Detail, then
// pressing `]`, silently jumped the Detail pane's scroll to a Diff-pane
// offset that has no meaning there. `should_recompute_blast_radius_selection`'s
// own existing tests only pin cache-staleness for the blast-radius pane's
// *recompute* trigger; none of them cover this key's *application* gate,
// which is a separate condition (`run_app` applies the jump only when
// this returns true, independent of whether anything gets recomputed).
#[test]
fn should_apply_hunk_jump_when_right_focused_on_diff_pane() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(app::Focus::Right, app.focus());
    assert_eq!(app::RightPane::Diff, app.right_pane()); // ADR 0020 default

    let actual = should_apply_hunk_jump(&app);

    assert!(actual);
}

#[test]
fn should_not_apply_hunk_jump_when_right_focused_on_detail_pane() {
    let report = report_with_one_symbol();
    // Open reaches Focus::Right on RightPane::Diff (its default), then
    // ToggleDiff ('d') switches to RightPane::Detail without touching
    // focus — exactly the sequence (Enter -> d -> ]) the bug report
    // describes.
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::ToggleDiff);
    assert_eq!(app::Focus::Right, app.focus());
    assert_eq!(app::RightPane::Detail, app.right_pane());

    let actual = should_apply_hunk_jump(&app);

    assert!(!actual);
}

#[test]
fn should_not_apply_hunk_jump_when_right_focused_on_blast_radius_pane() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::ToggleBlastRadius);
    assert_eq!(app::Focus::Right, app.focus());
    assert_eq!(app::RightPane::BlastRadius, app.right_pane());

    let actual = should_apply_hunk_jump(&app);

    assert!(!actual);
}

#[test]
fn should_not_apply_hunk_jump_when_tree_focused_even_if_right_pane_is_diff() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(app::Focus::Tree, app.focus());
    assert_eq!(app::RightPane::Diff, app.right_pane()); // ADR 0020 default

    let actual = should_apply_hunk_jump(&app);

    assert!(!actual);
}

#[test]
fn should_jump_to_the_next_hunk_start_strictly_after_current_scroll() {
    let hunk_starts = vec![0, 5, 12];

    let actual = jump_scroll_target(&hunk_starts, 5, InputKey::NextHunk);

    assert_eq!(Some(12), actual);
}

#[test]
fn should_return_none_when_next_hunk_is_pressed_at_the_last_hunk() {
    let hunk_starts = vec![0, 5, 12];

    let actual = jump_scroll_target(&hunk_starts, 12, InputKey::NextHunk);

    assert_eq!(None, actual);
}

#[test]
fn should_jump_to_the_previous_hunk_start_strictly_before_current_scroll() {
    let hunk_starts = vec![0, 5, 12];

    let actual = jump_scroll_target(&hunk_starts, 12, InputKey::PrevHunk);

    assert_eq!(Some(5), actual);
}

#[test]
fn should_return_none_when_prev_hunk_is_pressed_at_the_first_hunk() {
    let hunk_starts = vec![0, 5, 12];

    let actual = jump_scroll_target(&hunk_starts, 0, InputKey::PrevHunk);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_hunk_starts_is_empty() {
    let hunk_starts: Vec<usize> = vec![];

    let actual = jump_scroll_target(&hunk_starts, 0, InputKey::NextHunk);

    assert_eq!(None, actual);
}

#[test]
fn should_jump_to_the_first_hunk_after_scroll_lands_between_two_hunks() {
    // Scroll sitting mid-hunk (not exactly on a hunk boundary) still
    // finds the next hunk strictly after it, not the one it's inside.
    let hunk_starts = vec![0, 10];

    let actual = jump_scroll_target(&hunk_starts, 3, InputKey::NextHunk);

    assert_eq!(Some(10), actual);
}
