//! `gd`/`gr` candidate resolution (ADR 0022): [`resolve_goto`] and its
//! [`GotoOutcome`] result type, split out of `crate::event_loop`'s dispatch
//! module (ADR 0028) since this 0/1/many branching is its own self-contained
//! computation, called from `crate::event_loop::dispatch_non_source_key`.

use crate::app;
use crate::app::{App, InputKey};
use rinkaku_core::render::Report;

/// What `crate::event_loop::run_app` should do next for a pending `gd`/`gr`
/// press (ADR 0022's "0/1/many" branching): no symbol was selected at all,
/// the selected symbol has no candidates in the requested direction
/// (carrying a human-readable direction label, `"callees"`/`"callers"`, for
/// the status message — plain data, not formatted text, matching this
/// crate's own "view-model, not string-building, outside `ui.rs`"
/// convention), exactly one candidate (jump immediately), or more than one
/// (open the popup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GotoOutcome {
    NoSymbolSelected,
    NoCandidates(&'static str),
    One(app::JumpCandidate),
    Many(Vec<app::JumpCandidate>),
}

/// Resolves a pending [`InputKey::GotoDefinition`]/[`InputKey::GotoReferences`]
/// press into a [`GotoOutcome`], given `app`'s current cursor selection and
/// `report`'s graph — the computation `App::handle_key` cannot do itself
/// (ADR 0022's own rationale on `InputKey::GotoDefinition`), extracted as
/// its own pure function (rather than inlined in `run_app`, which takes a
/// live terminal and so cannot be driven directly in a test) so the 0/1/many
/// branching is unit-testable without one, mirroring `jump_scroll_target`'s
/// own precedent in `crate::event_loop::scroll_sync`.
pub(crate) fn resolve_goto(app: &App, report: &Report, direction: InputKey) -> GotoOutcome {
    let Some(symbol_id) = app.selected_symbol_id() else {
        return GotoOutcome::NoSymbolSelected;
    };

    let (mention_direction, label) = match direction {
        InputKey::GotoDefinition => (crate::detail::MentionDirection::Callees, "callees"),
        InputKey::GotoReferences => (crate::detail::MentionDirection::Callers, "callers"),
        // Unreachable: this function's only call site (`dispatch_non_source_key`)
        // already guards on `matches!(input_key, InputKey::GotoDefinition |
        // InputKey::GotoReferences)` before calling here, so `direction` is
        // never anything else in practice. `GotoOutcome::NoSymbolSelected`
        // is a misleading label for this branch specifically (this has
        // nothing to do with whether a symbol is selected — it is a
        // different caller-contract violation entirely), but is reused
        // rather than adding a dedicated `GotoOutcome` variant purely for an
        // unreachable defensive fallback; the important part is that this
        // never panics on a future caller mistake, not that its exact label
        // is semantically precise for a branch that cannot be reached today.
        _ => return GotoOutcome::NoSymbolSelected,
    };

    let mentions = crate::detail::symbol_mentions(report, symbol_id, mention_direction);
    let mut candidates = mentions.iter().map(app::JumpCandidate::from);

    match (candidates.next(), candidates.next()) {
        (None, _) => GotoOutcome::NoCandidates(label),
        (Some(only), None) => GotoOutcome::One(only),
        (Some(first), Some(second)) => {
            let mut all = vec![first, second];
            all.extend(candidates);
            GotoOutcome::Many(all)
        }
    }
}

#[cfg(test)]
#[path = "goto_tests.rs"]
mod tests;
