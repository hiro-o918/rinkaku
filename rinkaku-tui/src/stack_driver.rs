//! The stacked-PR driver's own pure decision logic (ADR 0075 D4), pulled
//! out of `crate::session::TuiSession::run_stack` so the "what happens
//! next" step is unit-testable without a terminal or a real
//! `PrAnalysisCache` — the driver itself only translates
//! [`SlotOutcome`]/[`AppExit`] into that call and threads the terminal-
//! touching side effects ([`crate::splash`], `run_app`) around it.

use crate::event_loop::AppExit;

/// What the driver should do with a just-resolved layer's [`crate::stack::Slot`]
/// before it can call `run_app` — the caller has already turned `Pending`/
/// `InProgress` into a blocking wait, so by the time this type is built the
/// slot is either ready to view or has failed. The failure message itself
/// is not this function's concern (the caller sets it on the status line
/// directly); only whether the layer resolved matters for the next cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOutcome {
    Ready,
    Failed,
}

/// The driver's next move after handling one layer, decided from
/// [`SlotOutcome`] (before `run_app` even runs) or from `run_app`'s own
/// [`AppExit`] (after it returns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StackStep {
    /// Enter (or re-enter) the layer at this cursor.
    Enter(usize),
    Quit,
    UpdateRequested,
}

/// A [`SlotOutcome::Failed`] layer reports its message on the status line
/// and falls back to `previous_cursor` (ADR 0075 D3: "the cursor stays
/// where it was") rather than advancing — `run_app` is re-entered on the
/// layer the reviewer was already reading, not the one that just failed.
pub(crate) fn step_after_slot(
    outcome: SlotOutcome,
    target: usize,
    previous_cursor: usize,
) -> StackStep {
    match outcome {
        SlotOutcome::Ready => StackStep::Enter(target),
        SlotOutcome::Failed => StackStep::Enter(previous_cursor),
    }
}

/// `run_app`'s own [`AppExit`] maps directly onto the driver's next step:
/// `SwitchPr` re-enters at the requested index, `Quit`/`UpdateRequested`
/// stop the loop.
pub(crate) fn step_after_exit(exit: AppExit) -> StackStep {
    match exit {
        AppExit::SwitchPr(target) => StackStep::Enter(target),
        AppExit::Quit => StackStep::Quit,
        AppExit::UpdateRequested => StackStep::UpdateRequested,
    }
}

/// The `g<Tab>` history to carry into the next layer's
/// [`crate::stack::StackPosition`] (ADR 0075 amendment). Re-entering the
/// layer just rendered (the failed-slot fallback) keeps the history it
/// already had, so an aborted switch does not erase where `g<Tab>` went.
pub(crate) fn next_last_visited(
    rendered_cursor: usize,
    next_cursor: usize,
    current_last_visited: Option<usize>,
) -> Option<usize> {
    if rendered_cursor == next_cursor {
        current_last_visited
    } else {
        Some(rendered_cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_enter_the_target_layer_when_slot_is_ready() {
        let actual = step_after_slot(SlotOutcome::Ready, 2, 0);

        assert_eq!(StackStep::Enter(2), actual);
    }

    #[test]
    fn should_fall_back_to_the_previous_cursor_when_slot_failed() {
        let actual = step_after_slot(SlotOutcome::Failed, 2, 0);

        assert_eq!(StackStep::Enter(0), actual);
    }

    #[test]
    fn should_enter_the_switch_target_when_exit_requests_a_switch() {
        let actual = step_after_exit(AppExit::SwitchPr(3));

        assert_eq!(StackStep::Enter(3), actual);
    }

    #[test]
    fn should_quit_when_exit_is_quit() {
        let actual = step_after_exit(AppExit::Quit);

        assert_eq!(StackStep::Quit, actual);
    }

    #[test]
    fn should_stop_with_update_requested_when_exit_requests_an_update() {
        let actual = step_after_exit(AppExit::UpdateRequested);

        assert_eq!(StackStep::UpdateRequested, actual);
    }

    #[test]
    fn should_carry_the_rendered_layer_as_history_when_moving_to_a_different_layer() {
        let actual = next_last_visited(0, 2, Some(1));

        assert_eq!(Some(0), actual);
    }

    #[test]
    fn should_keep_the_existing_history_when_re_entering_the_same_layer() {
        let actual = next_last_visited(1, 1, Some(0));

        assert_eq!(Some(0), actual);
    }

    #[test]
    fn should_have_no_history_when_re_entering_the_first_layer_of_the_session() {
        let actual = next_last_visited(2, 2, None);

        assert_eq!(None, actual);
    }
}
