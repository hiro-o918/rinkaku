//! Diff-pane scroll synchronization (ADR 0027/ADR 0030), split out of
//! `crate::event_loop`'s dispatch module (ADR 0028): tree-cursor -> diff
//! auto-scroll, diff-scroll -> tree-cursor auto-sync, the hunk-jump (`]`/
//! `[`) target computation, and the post-draw scroll fold-back both panes
//! need. Grouped together because all of these functions cooperate on the
//! same `right_pane_scroll` state — a bug in one direction's gating tends to
//! show up as a feedback loop with another, so their tests and
//! implementations stay side by side.

use crate::app::{self, App};
use crate::{diff_shape, diff_view};
use rinkaku_core::render::Report;

/// The result of [`apply_diff_pane_selection_effects`]: the next `App`,
/// the diff pane's freshly rebuilt shaped content, and the `last_diff_focus`
/// value `crate::event_loop::run_app`'s loop should carry into the next
/// handled key. Grouped into one struct rather than a tuple so the three
/// fields keep their names at every call site (all three change together,
/// and a positional tuple would invite a `(app, content, focus)` vs.
/// `(app, focus, content)` mix-up the first time this function's argument
/// order is touched).
pub(crate) struct DiffPaneSelectionEffects {
    pub(crate) app: App,
    pub(crate) diff_pane_content: diff_shape::DiffPaneContent,
    pub(crate) last_diff_focus: Option<app::DiffFocus>,
}

/// Rebuilds the diff pane's shaped content for the just-dispatched key and
/// applies both directions of ADR 0027/ADR 0030's cursor<->scroll sync,
/// given `last_diff_focus` (the tree-cursor-driven focus as of the
/// *previous* handled key) and `scroll_before_dispatch` (`right_pane_scroll`
/// as of *before* this key's dispatch, ADR 0030's own doc comment on why
/// scroll-vs-focus need two different "before" snapshots).
///
/// Extracted out of `run_app`'s loop body for the same reason
/// `dispatch_non_source_key` was (that function's own doc comment): a bug
/// in the *sequencing* of "rebuild content, then decide which sync
/// direction fires" has no regression coverage when every existing test
/// only exercises `auto_scroll_for_diff_focus`/`sync_target_for_scroll` in
/// isolation — ADR 0030 decision 6's feedback-loop guard specifically
/// depends on `last_diff_focus` being updated *within* this same step, a
/// property only a test that calls this exact function back-to-back for
/// two simulated keys can pin.
///
/// **Exactly one** of ADR 0027's tree->diff auto-scroll or ADR 0030's
/// diff->tree cursor sync can fire per call, never both: the auto-scroll
/// branch only runs when `selected_diff_focus` changed since
/// `last_diff_focus` (a cursor-driven selection change), and the sync
/// branch's `else` only runs when it did not — the two conditions are
/// complements of each other by construction, matching decision 6's "the
/// two directions must not re-trigger one another in the same step" rule.
pub(crate) fn apply_diff_pane_selection_effects(
    mut app: App,
    report: &Report,
    diff_hunks: &[diff_view::FileHunks],
    last_diff_focus: Option<app::DiffFocus>,
    scroll_before_dispatch: usize,
    effective_diff_view_mode: app::DiffViewMode,
) -> DiffPaneSelectionEffects {
    let diff_pane_content = diff_shape::build_diff_pane_content(
        report,
        diff_hunks,
        app.selected_diff_target(report).as_ref(),
    );
    // ADR 0027 decision 2: auto-scroll to the focused symbol's section
    // start only when the focus *actually changed* since the previous key.
    // The caller's `should_recompute_diff_pane_content` gate is `true` on
    // every key while Diff is showing (it just gates the cache rebuild,
    // not a selection-change signal), so firing auto-scroll
    // unconditionally here would overwrite the reviewer's own
    // `j`/`k`/`Ctrl-d` scrolling immediately after they pressed it
    // (dogfooding finding: Enter into Focus::Right + subsequent `j`/`k`
    // had no visible effect because this override snapped back to the
    // section start every keystroke). Only when the tree cursor lands on
    // a different symbol do we retarget the pane.
    let next_focus = app.selected_diff_focus(report);
    let last_diff_focus = if next_focus != last_diff_focus {
        if let Some(target_scroll) =
            auto_scroll_for_diff_focus(&app, report, &diff_pane_content, effective_diff_view_mode)
        {
            app = app.with_right_pane_scroll(target_scroll);
        }
        next_focus
    } else if let Some(target_symbol_id) = sync_target_for_scroll(
        &app,
        &diff_pane_content,
        scroll_before_dispatch,
        effective_diff_view_mode,
    ) {
        // ADR 0030: the mirror-image sync — this key's dispatch did not
        // itself move the cursor (`next_focus == last_diff_focus`, the `if`
        // branch above), but it did move `right_pane_scroll` onto a
        // *different* symbol's section than the cursor is currently on, so
        // move the cursor to match. The returned `last_diff_focus` is the
        // post-sync focus, not the pre-sync `next_focus` — critical to
        // avoid the auto-scroll-vs-scroll-sync feedback loop decision 6
        // documents: without this, the next handled key would see
        // `selected_diff_focus` (now the synced symbol) differ from a
        // stale `last_diff_focus` (still the pre-scroll symbol), treat that
        // as a fresh cursor-driven selection change, and auto-scroll
        // `right_pane_scroll` right back to that symbol's section start —
        // undoing the very scroll that triggered this sync.
        app = app.sync_tree_cursor_to_symbol(&target_symbol_id);
        app.selected_diff_focus(report)
    } else {
        next_focus
    };
    DiffPaneSelectionEffects {
        app,
        diff_pane_content,
        last_diff_focus,
    }
}

/// The symbol id `crate::event_loop::run_app`'s scroll->tree sync (ADR
/// 0030) should move the tree cursor to, or `None` when no sync should
/// happen this key — extracted as its own pure function (mirroring
/// [`auto_scroll_for_diff_focus`]/[`jump_scroll_target`]'s own precedent)
/// so the gating logic is unit-testable without a live
/// `ratatui::DefaultTerminal`.
///
/// `None` when: the right pane isn't showing the diff pane with focus on it
/// (`should_apply_hunk_jump`'s own focus/pane gate — scrolling a pane that
/// isn't the diff pane, e.g. Detail/BlastRadius, has no per-symbol section
/// to sync against); this key's dispatch did not actually change
/// `right_pane_scroll` from `scroll_before_dispatch` (a key that reached
/// this point already passed the "focus didn't change" check above, but
/// plenty of keys — `Enter`, `d`, `?`, arbitrary no-ops — reach here without
/// touching scroll at all, and re-deriving the same symbol id every one of
/// those keys would be wasted work, not just a correctness no-op);
/// [`diff_shape::symbol_id_for_scroll_line`] resolves to `None` (ADR 0030
/// decision 3: the module-level bucket or past-content overscroll, where
/// there is no principled symbol to sync to); or the resolved symbol id is
/// already the one currently focused (nothing to do — most scroll keys that
/// stay within the same section land here).
pub(crate) fn sync_target_for_scroll(
    app: &App,
    diff_pane_content: &diff_shape::DiffPaneContent,
    scroll_before_dispatch: usize,
    effective_diff_view_mode: app::DiffViewMode,
) -> Option<String> {
    if !should_apply_hunk_jump(app) {
        return None;
    }
    if app.right_pane_scroll() == scroll_before_dispatch {
        return None;
    }
    let target_symbol_id = diff_shape::symbol_id_for_scroll_line(
        diff_pane_content,
        app.right_pane_scroll(),
        effective_diff_view_mode,
    )?;
    if Some(target_symbol_id) == app.selected_symbol_id() {
        return None;
    }
    Some(target_symbol_id.to_string())
}

/// Folds `ui::draw`'s clamped scroll offset (`ui::draw`'s own doc comment)
/// back into `app`, given `clamped` — `Some(scroll)` when the active right
/// pane rendered scrollable content this frame, `None` on the source screen
/// or a placeholder pane.
///
/// Dogfooding finding: `App::right_pane_scroll` is deliberately an
/// *unclamped* "requested" offset (that field's own doc comment) — `App`
/// has no notion of the pane's rendered height, so clamping was left to
/// `crate::ui` at draw time (`ui::clamp_scroll`). That is still correct as
/// far as *what gets drawn*, but left `App`'s own notion of "how far down
/// the user asked to scroll" free to run past the content's actual end:
/// holding `j` past the bottom kept incrementing the request with no
/// visible change once the pane was already showing its last screenful, so
/// the very next `k` had to first unwind that whole invisible overshoot
/// before the pane visibly moved at all — indistinguishable, from the
/// keyboard, from the key simply not responding. Writing the clamped value
/// straight back after every draw keeps `App`'s own state in sync with what
/// is actually on screen, so the next `j`/`k` always has an immediate,
/// visible effect.
///
/// Applied unconditionally (not gated on `clamped != app.right_pane_scroll()`)
/// since `App::with_right_pane_scroll` is a plain field write — the branch
/// would only save a redundant assignment, not a meaningfully different
/// state, so it is not worth the extra branch.
///
/// Deliberate trade-off (recorded in ADR 0020's Amendment too): this runs on
/// *every* draw, including the idle ~100ms poll ticks `crate::event_loop`'s
/// doc comment already notes, not only after a key press — so shrinking the
/// terminal (fewer visible rows, a smaller `max_scroll`) permanently clamps
/// `App`'s own scroll offset down, and growing the terminal back afterward
/// does not restore the pre-shrink position; there is no separate "requested
/// vs. actually-applied" pair of fields to fall back to; `right_pane_scroll`
/// is single-valued by design (that field's own doc comment). A reviewer who
/// shrinks their terminal mid-read and then grows it back finds the pane
/// scrolled less far than before the resize — judged an acceptable, rare
/// edge case relative to the far more common overshoot this fold-back fixes.
pub(crate) fn clamp_right_pane_scroll_after_draw(app: App, clamped: Option<usize>) -> App {
    match clamped {
        Some(scroll) => app.with_right_pane_scroll(scroll),
        None => app,
    }
}

/// The `?` help overlay's own version of [`clamp_right_pane_scroll_after_draw`]
/// — folds `ui::DrawOutcome::clamped_help_scroll` back into `App` after every
/// draw so an overshot request (repeated `j` past the overlay's own last
/// line) never survives past the frame that visibly clamped it, mirroring
/// that function's own reasoning for the right pane.
pub(crate) fn clamp_help_scroll_after_draw(app: App, clamped: Option<usize>) -> App {
    match clamped {
        Some(scroll) => app.with_help_scroll(scroll),
        None => app,
    }
}

/// The auto-scroll target for the diff pane (ADR 0027 decision 2 + 4):
/// [`crate::diff_shape::section_start_line_for_symbol`] on the currently
/// focused symbol's section, or `None` when there is nothing to auto-scroll
/// to — a file/directory row has no `DiffFocus`, and a symbol whose id
/// contributed no section (e.g. its hunks were absorbed into an adjacent
/// symbol's section via first-match attribution, or the file has no diff
/// hunks at all) has no section start to jump to.
///
/// Extracted as its own pure function (mirroring
/// [`jump_scroll_target`]/[`should_apply_hunk_jump`]'s own precedent) so this
/// rule stays unit-testable without a live `ratatui::DefaultTerminal`, and
/// so the "when do we auto-scroll?" gate is in one obvious place rather than
/// scattered through `run_app`'s loop body.
pub(crate) fn auto_scroll_for_diff_focus(
    app: &App,
    report: &Report,
    diff_pane_content: &diff_shape::DiffPaneContent,
    effective_diff_view_mode: app::DiffViewMode,
) -> Option<usize> {
    let focus = app.selected_diff_focus(report)?;
    diff_shape::section_start_line_for_symbol(
        diff_pane_content,
        &focus.symbol_id,
        effective_diff_view_mode,
    )
}

/// Whether `crate::event_loop::run_app`'s event loop should act on an
/// [`InputKey::NextHunk`]/[`InputKey::PrevHunk`] press by jumping
/// `diff_pane_content`'s scroll offset, rather than treating the key as a
/// no-op. `true` only while [`app::Focus::Right`] *and* [`app::RightPane::Diff`]
/// is showing — gating on focus alone let `]`/`[` scroll the Detail/BlastRadius
/// pane using `diff_pane_content`'s hunk-start table, which is only ever
/// recomputed for the Diff pane (`should_recompute_diff_pane_content`), so
/// it goes stale (pinned to whichever file/symbol was selected the last
/// time Diff was shown) the moment the user switches away from Diff. That
/// produced a jump with no relation to what is actually on screen.
///
/// Extracted as its own pure function, mirroring `should_recompute_blast_radius_selection`'s
/// own reasoning, so this exact gate is unit-testable without a live
/// `ratatui::DefaultTerminal`.
///
/// [`InputKey::NextHunk`]: crate::app::InputKey::NextHunk
/// [`InputKey::PrevHunk`]: crate::app::InputKey::PrevHunk
pub(crate) fn should_apply_hunk_jump(app: &App) -> bool {
    app.focus() == app::Focus::Right && app.right_pane() == app::RightPane::Diff
}

/// The scroll offset [`InputKey::NextHunk`]/[`InputKey::PrevHunk`] should
/// jump to, given `hunk_starts` (each hunk's starting logical-line offset
/// within the diff pane's shaped content, `crate::diff_shape::hunk_start_lines`'s
/// own doc comment) and the pane's `current_scroll`. `None` when there is
/// nowhere to jump (`hunk_starts` is empty, or already at the first/last
/// hunk in the requested direction) — a no-op, not a clamp to the nearest
/// edge, since silently landing back on the same hunk would look like the
/// keypress did nothing anyway.
///
/// Extracted as its own pure function (rather than inlined in `run_app`,
/// which takes a live `ratatui::DefaultTerminal` and so cannot be driven
/// directly in a test) so the jump direction/boundary logic is
/// unit-testable without a terminal.
///
/// [`InputKey::NextHunk`]: crate::app::InputKey::NextHunk
/// [`InputKey::PrevHunk`]: crate::app::InputKey::PrevHunk
pub(crate) fn jump_scroll_target(
    hunk_starts: &[usize],
    current_scroll: usize,
    direction: app::InputKey,
) -> Option<usize> {
    match direction {
        app::InputKey::NextHunk => hunk_starts
            .iter()
            .copied()
            .find(|&start| start > current_scroll),
        app::InputKey::PrevHunk => hunk_starts
            .iter()
            .copied()
            .rfind(|&start| start < current_scroll),
        _ => None,
    }
}

#[cfg(test)]
#[path = "scroll_sync_tests.rs"]
mod tests;
