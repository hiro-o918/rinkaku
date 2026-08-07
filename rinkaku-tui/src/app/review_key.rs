//! The review overlay's own key dispatch (ADR 0048), split out of
//! `app/handle_key.rs` (ADR 0028): [`App::handle_review_key`] is a
//! self-contained arm reached only via [`App::handle_key`]'s own
//! top-of-function priority check, operating solely on
//! [`crate::review::ReviewState`] — the same "dispatch here, state
//! elsewhere" split `app/jump.rs`'s own doc comment already establishes for
//! the jump popup.

use super::{App, InputKey};

impl App {
    /// Handles one [`InputKey`] while a review overlay mode (ADR 0048) is
    /// open — dispatched by [`Self::handle_key`]'s own top-of-function
    /// priority check. `report`/diff data are never needed here: every
    /// review transition this method reaches is a plain
    /// [`crate::review::ReviewState`] method call, since the one review
    /// action that *does* need external data (opening the compose overlay,
    /// [`InputKey::AnnotationCompose`]) is special-cased by `crate::lib::run_app`
    /// before dispatch and never reaches this method while `review` is
    /// `Idle` — this method only ever runs once a review mode is already
    /// open.
    pub(super) fn handle_review_key(&self, key: InputKey) -> crate::review::ReviewState {
        let review = self.review.clone();
        match review.mode() {
            crate::review::ReviewMode::Compose { .. } => match key {
                InputKey::ComposeChar(c) => review.push_char(c),
                InputKey::ComposeBackspace => review.backspace(),
                InputKey::PopupConfirm => review.confirm_compose(),
                InputKey::PopupCancel => review.cancel_compose(),
                _ => review,
            },
            crate::review::ReviewMode::List { .. } => match key {
                InputKey::Up => review.list_up(),
                InputKey::Down => review.list_down(),
                InputKey::AnnotationDelete => review.delete_selected(),
                InputKey::PopupConfirm => review.open_export_menu(),
                InputKey::PopupCancel => review.close(),
                _ => review,
            },
            crate::review::ReviewMode::ExportMenu { .. } => match key {
                InputKey::Up => review.list_up(),
                InputKey::Down => review.list_down(),
                InputKey::PopupConfirm => review.confirm_export(self.review_sink_a_available),
                InputKey::PopupCancel => review.close(),
                _ => review,
            },
            crate::review::ReviewMode::VerdictMenu { .. } => match key {
                InputKey::Up => review.list_up(),
                InputKey::Down => review.list_down(),
                InputKey::PopupConfirm => review.confirm_verdict(),
                InputKey::PopupCancel => review.close(),
                _ => review,
            },
            crate::review::ReviewMode::Idle => review,
        }
    }
}
