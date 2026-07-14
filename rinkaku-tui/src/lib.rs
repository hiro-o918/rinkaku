//! rinkaku's interactive terminal UI (ADR 0015/0016).
//!
//! Two layers, kept deliberately separate:
//!
//! - **View-models** (`tree`, `nav`, `order`, `detail`, `app`, `row_view`):
//!   plain data and pure functions/state machines derived from
//!   [`rinkaku_core::render::Report`]. `tree`/`nav`/`order`/`detail` carry
//!   no `ratatui`/`crossterm` types at all (ADR 0016 decision 3). `app`
//!   and `row_view` are the stage B additions that compose those
//!   view-models into one navigable state machine and format its rows —
//!   `row_view` uses `ratatui::text`/`style` types (`Line`/`Span`/`Style`),
//!   which are plain, comparable data rather than a live `Frame`/
//!   `Terminal`, so building one from a row stays a pure, unit-testable
//!   transformation. `app` stays entirely free of `ratatui`/`crossterm`
//!   types, translating real key events at the boundary instead (see
//!   `run`'s event loop).
//! - **Terminal adapter** (`ui`, `source`, [`run`]): draws `App`'s state
//!   with `ratatui`, reads source files for the drill-down view, and owns
//!   the terminal lifecycle (raw mode, alternate screen, the event loop).
//!   This is the only layer that performs IO or holds a live `Terminal`.
//!   [`run_app`]'s own event loop delegates two responsibilities to
//!   sibling modules (ADR 0028, split out once this file grew past the
//!   file-size threshold): `input_translate` (raw `crossterm` events →
//!   [`app::InputKey`]) and `review_flow` (ADR 0048's review-notes
//!   composing/exporting/caching glue).
//!
//! [`run`] is the crate's single public entry point for the CLI binary:
//! `rinkaku`'s `main.rs` hands it a [`rinkaku_core::render::Report`] once
//! `--tui` is passed, in place of rendering Markdown/JSON. It also hands in
//! the raw unified diff text `main.rs` already has in hand for every input
//! mode (stdin / `--base` / `--pr`) — TUI iteration 2's diff pane
//! (`d`/`D`, `crate::diff_view`) slices hunks straight out of that same
//! string rather than reconstructing a diff from `Report` (which no longer
//! carries hunk text once extraction has run).

pub mod app;
pub mod blast_radius;
pub mod detail;
pub mod diff_shape;
pub mod diff_view;
pub mod help;
pub mod highlight;
mod input_translate;
pub mod nav;
pub mod note_markers;
pub mod order;
pub mod review;
mod review_flow;
pub mod row_view;
mod session;
pub mod source;
pub mod source_diff;
pub mod splash;
mod split_pairing;
pub mod tree;
pub mod ui;

pub use session::{TuiSession, run};

use app::{App, BlastRadiusSelection, InputKey, Screen};
use input_translate::{translate_key, translate_mouse_event};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use review::PrContext;
use review::ports::{ClipboardSink, ReviewSubmitter};
use review_flow::{
    derive_selection_snapshot, dispatch_note_compose_key, perform_export,
    should_recompute_note_markers,
};
use rinkaku_core::render::Report;
use std::time::Duration;

// Re-exported at the crate root purely so `lib_tests/note_snapshot.rs`'s
// and `lib_tests/perform_export.rs`'s existing `use crate::{...}` imports
// (predating the ADR 0028 split of these two into `review_flow`) keep
// working unchanged.
#[cfg(test)]
pub(crate) use review_flow::{OSC52_SIZE_GUARD_BYTES, clipboard_export_status, first_anchor_run};

/// Review-notes export wiring (ADR 0048), assembled once by `main.rs`'s
/// composition root and threaded through unchanged from
/// [`crate::session::TuiSession::run`] to [`run_app`]: `pr_context`/
/// `submitter` are both `Some`/`None` together (sink A's own "absent when
/// no PR context" rule — [`crate::app::App::with_review_sink_a_available`]'s
/// own doc comment), `clipboard` is always present since sink B never
/// depends on a PR.
pub struct ReviewPorts<'a> {
    pub pr_context: Option<PrContext>,
    pub submitter: Option<&'a dyn ReviewSubmitter>,
    pub clipboard: &'a dyn ClipboardSink,
}

/// The event loop [`TuiSession::run`] (`crate::session`) drives once it has
/// taken over the terminal — see [`run`]'s doc comment (re-exported from
/// `crate::session`, which retains the full terminal-lifecycle rationale
/// that used to live here) for what `report`, `diff_text`, `entry_path`,
/// `repo_root`, and `source_reader` mean. `pub(crate)` rather than private:
/// `crate::session` is a sibling module, not a submodule, so it needs this
/// visibility to call in.
pub(crate) fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    report: &Report,
    diff_text: &str,
    entry_path: Option<&str>,
    repo_root: &std::path::Path,
    source_reader: &dyn source::SourceReader,
    review_ports: ReviewPorts<'_>,
) -> std::io::Result<()> {
    let mut app = App::new(report).with_review_sink_a_available(review_ports.pr_context.is_some());
    if let Some(path) = entry_path {
        app = app.with_entry_pivot(path);
    }
    // Parsed once up front rather than inside the draw loop: `diff_text`
    // does not change for the lifetime of this session, but the loop below
    // redraws on every ~100ms poll timeout (not just on an actual key
    // press), so re-running `parse_diff_hunks` inside `ui::draw` would
    // re-walk the whole diff (unbounded in PR size) roughly ten times a
    // second even while idle.
    let diff_hunks = diff_view::parse_diff_hunks(diff_text);
    // Highlighted once immediately after, for the same reason (ADR 0018):
    // highlighting is a full tree-sitter parse per hunk side, strictly
    // more expensive than the hunk parse above, so it must not run inside
    // the render loop either.
    let diff_highlights = highlight::highlight_diff_files(&diff_hunks);
    // Computed once up front (then on demand below, once per handled key —
    // unlike `diff_hunks`/`diff_highlights` above, the blast-radius view
    // depends on `app`'s cursor position and right-pane mode, both of which
    // change as keys are handled) and cached here across idle poll ticks for
    // the same reason those two are computed outside the draw loop at all:
    // `ui::draw` itself runs on every ~100ms idle poll timeout, not just on
    // an actual key press, and `crate::blast_radius::build_blast_radius_view`
    // is an O(V+E) graph walk — recomputing it on every one of those idle
    // ticks while the blast-radius pane merely sits on screen was the
    // per-frame recompute bug this cache exists to fix
    // (`App::selected_blast_radius_view`'s own doc comment). The up-front
    // computation (rather than starting at `NotApplicable` unconditionally)
    // matters specifically for `--entry --tui`: when `entry_path` above
    // already opened `RightPane::BlastRadius`, the very first frame must
    // show the blast-radius tree immediately, not an empty placeholder until
    // the first key press recomputes it.
    let mut blast_radius_selection = if should_recompute_blast_radius_selection(&app) {
        app.selected_blast_radius_view(report)
    } else {
        BlastRadiusSelection::NotApplicable
    };
    // Computed once up front then on demand below, once per handled key —
    // same reasoning and cache-on-selection-change discipline as
    // `blast_radius_selection` above (ADR 0020: `crate::diff_shape`'s own doc
    // comment on why this must not be recomputed inside `ui::draw`, after
    // the blast-radius pane's own past per-frame recompute bug). The
    // up-front computation matters for the ordinary (non-`--entry`) startup
    // path too now, since ADR 0020 also made Diff the default right pane:
    // the very first frame must already show shaped diff content, not an
    // empty placeholder until the first key press recomputes it.
    let mut diff_pane_content = if should_recompute_diff_pane_content(&app) {
        diff_shape::build_diff_pane_content(
            report,
            &diff_hunks,
            app.selected_diff_target(report).as_ref(),
        )
    } else {
        diff_shape::DiffPaneContent::Empty
    };
    // ADR 0027 decision 2: apply the same "auto-scroll to the focused
    // symbol's section" rule the loop below runs on selection change, so
    // `--tui` startup on a symbol row (e.g. `App::new`'s default cursor
    // when the first row happens to be a symbol) already opens with the
    // correct scroll offset rather than an unrelated 0.
    let mut last_diff_focus: Option<app::DiffFocus> = app.selected_diff_focus(report);
    if let Some(target_scroll) = auto_scroll_for_diff_focus(&app, report, &diff_pane_content) {
        app = app.with_right_pane_scroll(target_scroll);
    }
    // The source screen's (`s` key) file read + syntax highlight, computed
    // once when `Screen::Source` is entered (the `InputKey::Source` arm
    // below) and cached here across every subsequent draw — including the
    // idle ~100ms poll ticks `ui::draw` runs on regardless of a key press —
    // for the same reason `diff_highlights` above must not run inside the
    // render loop: highlighting is a full tree-sitter parse, and repeating
    // it roughly ten times a second while the screen merely sits open would
    // reintroduce exactly the per-frame recompute bug ADR 0018 already had
    // to fix once for the diff pane. `None` on startup (the source screen is
    // never the initial screen — reached only via `s` from the entry view),
    // so there is no up-front computation to mirror `diff_pane_content`'s/
    // `blast_radius_selection`'s own `--entry`-driven initial state.
    let mut source_content: Option<Result<source::HighlightedSourceView, String>> = None;
    // The symbol id `source_content` was computed for, kept alongside so
    // the `s` key can skip the reload when re-pressed on the same row (see
    // the re-entry guard inside the loop below). Held separately rather
    // than folded into `HighlightedSourceView` itself: `HighlightedSourceView`
    // carries the file *path*, not the symbol id, and two distinct symbols
    // in one file would otherwise share a cache entry indistinguishably.
    let mut source_content_symbol: Option<String> = None;
    // The inner height (borders excluded) of whichever pane was scrollable
    // as of the last drawn frame (`ui::DrawOutcome::scroll_viewport_height`),
    // remembered here so the next `Ctrl-d`/`Ctrl-u`/`gg`/`G` key press
    // (ADR 0026) can size its step against the same viewport the reviewer
    // just saw — the very first keypress before any frame has drawn (a
    // near-impossible edge case, but guarded rather than defaulting to a
    // zero step) falls back to [`DEFAULT_SCROLL_VIEWPORT_HEIGHT`].
    let mut last_scroll_viewport_height: Option<usize> = None;
    // The `?` help overlay's own last-drawn inner height, tracked
    // separately from `last_scroll_viewport_height` above: the overlay can
    // be open on top of either screen, and its box is a different size
    // than whichever pane sits underneath it — sizing a `Ctrl-d` press
    // while the overlay is open against the *underlying* pane's height
    // would produce a step that does not match what the reviewer is
    // actually looking at.
    let mut last_help_scroll_viewport_height: Option<usize> = None;
    // ADR 0048's `NoteMarkers` cache-on-change, mirroring
    // `blast_radius_selection`/`diff_pane_content`'s own up-front-then-
    // gated-recompute shape: built once from `App::new`'s initial (empty)
    // `review` state, then only recomputed when `should_recompute_note_markers`
    // reports the note set actually changed. `last_note_markers_revision`
    // starts at `app.review().revision()` itself (0 on a fresh session)
    // rather than a sentinel, so the first key press that does not touch
    // `review` correctly skips a redundant recompute of an already-empty
    // table.
    let mut note_markers = note_markers::build_note_markers(app.review().notes());
    let mut last_note_markers_revision = app.review().revision();

    loop {
        // `ui::draw`'s return value (`DrawOutcome`, `ui::draw`'s own doc
        // comment) cannot flow out of the closure itself — `Terminal::draw`
        // requires an `FnOnce(&mut Frame)` returning `()` — so it is
        // captured into this outer binding instead and folded back into
        // `app`/`last_scroll_viewport_height` right after.
        let mut outcome = ui::DrawOutcome::default();
        terminal.draw(|frame| {
            outcome = ui::draw(
                frame,
                &app,
                report,
                &diff_pane_content,
                &diff_highlights,
                &blast_radius_selection,
                source_content.as_ref(),
                &diff_hunks,
                &note_markers,
            );
        })?;
        app = clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll);
        if outcome.scroll_viewport_height.is_some() {
            last_scroll_viewport_height = outcome.scroll_viewport_height;
        }
        app = clamp_help_scroll_after_draw(app, outcome.clamped_help_scroll);
        if outcome.help_scroll_viewport_height.is_some() {
            last_help_scroll_viewport_height = outcome.help_scroll_viewport_height;
        }

        if app.should_quit() {
            return Ok(());
        }

        // A 100ms poll timeout keeps the loop responsive to terminal
        // resize events without busy-spinning — `event::read()` alone
        // would block indefinitely on a genuinely idle terminal, which is
        // fine for input but would also delay reacting to anything else
        // this loop might grow to check in the future (kept short as a
        // resize/redraw responsiveness margin, not a correctness
        // requirement of the key-handling itself).
        //
        // Both a keyboard press and a mouse wheel tick resolve to the same
        // `Option<InputKey>` here (`translate_key`/`translate_mouse_event`
        // respectively) and share every downstream dispatch branch below —
        // a wheel scroll is just another way to produce `InputKey::Up`/
        // `Down`, not a second, parallel handling path. Every other
        // `Event` variant (`FocusGained`/`FocusLost`/`Paste`/`Resize`, and
        // click/drag/move mouse events `translate_mouse_event` maps to
        // `None`) falls through to `None` and the loop simply redraws.
        let translated_input_key = if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    translate_key(key_event.code, key_event.modifiers, &app)
                }
                Event::Mouse(mouse_event) => translate_mouse_event(mouse_event.kind),
                _ => None,
            }
        } else {
            None
        };

        if let Some(input_key) = translated_input_key {
            // ADR 0030: captured before dispatch so the scroll->tree sync
            // below can tell whether *this* key actually moved
            // `right_pane_scroll` — comparing against the value at loop-top
            // rather than re-deriving it from `input_key` itself, since
            // several different keys (`Up`/`Down`, the four ADR 0026 scroll
            // variants, `]c`/`[c`) can all change the scroll offset and this
            // sync applies uniformly to any of them.
            let scroll_before_dispatch = app.right_pane_scroll();
            if let InputKey::NoteCompose = input_key {
                // ADR 0048: needs a `SelectionSnapshot` derived from
                // `report`/`diff_hunks`, which `App::handle_key` has no
                // access to (`InputKey::NoteCompose`'s own doc comment) —
                // derived here, mirroring `InputKey::Source`'s own "IO/
                // derivation stays outside `App`" precedent just below, then
                // applied via `dispatch_note_compose_key` (extracted for the
                // same "sequencing needs its own regression coverage"
                // reason `dispatch_non_source_key`'s own doc comment gives —
                // an earlier version of this arm left `app` completely
                // untouched on a `None` snapshot, which skipped
                // `App::handle_key`'s unconditional `pending_prefix` clear
                // the same way the ADR 0022 bug `dispatch_non_source_key`
                // documents did).
                let snapshot = derive_selection_snapshot(&app, report, &diff_hunks);
                app = dispatch_note_compose_key(app, snapshot);
            } else if let InputKey::Source = input_key {
                app = app.handle_key(input_key);
                if let Screen::Source { symbol_id, .. } = app.screen().clone()
                    && should_reload_source_content(
                        source_content_symbol.as_deref(),
                        source_content.as_ref(),
                        &symbol_id,
                    )
                {
                    let loaded = source::load_highlighted_symbol_source(
                        report,
                        &symbol_id,
                        repo_root,
                        source_reader,
                    );
                    // A failure is surfaced on the status line right away
                    // (rather than only discovered on the next redraw)
                    // *and* cached into `source_content` below so
                    // `ui::draw`'s `draw_source_screen` shows the same
                    // error message in the pane itself — mirrors the
                    // pre-caching behavior, which attempted this same
                    // read eagerly for the same early-feedback reason.
                    if let Err(message) = &loaded {
                        app.set_status(message.clone());
                    }
                    source_content = Some(loaded);
                    source_content_symbol = Some(symbol_id);
                }
                // ADR 0026: back-fill `Screen::Source::scroll_top` with the
                // centered starting position `crate::source::visible_window`
                // computes, now that the file has been loaded and its line
                // count is known. `App::handle_key(InputKey::Source)` above
                // sets `scroll_top = 0` (it has no access to the file), so
                // without this back-fill the first frame after `s` would
                // land at the file's very top rather than centered on the
                // symbol's definition — the old auto-centering behavior
                // this ADR preserves as the initial position.
                if let (Screen::Source { .. }, Some(Ok(highlighted))) =
                    (app.screen(), source_content.as_ref())
                {
                    let viewport_height =
                        last_scroll_viewport_height.unwrap_or(DEFAULT_SCROLL_VIEWPORT_HEIGHT);
                    let (start, _end) = source::visible_window(
                        highlighted.view.lines.len(),
                        highlighted.view.highlight_start,
                        highlighted.view.highlight_end,
                        viewport_height,
                    );
                    app = app.with_source_scroll_top(start);
                }
            } else if is_scroll_input_key(input_key) {
                // ADR 0026 two-step dispatch: `handle_key` first for the
                // blanket `status`/`pending_prefix`/`preserve_scroll`
                // bookkeeping every key needs (mirroring how
                // `dispatch_non_source_key` also calls `handle_key`
                // unconditionally for `GotoDefinition`/`GotoReferences`
                // for the same reason), then `handle_scroll_key` with
                // the last-drawn viewport height for the actual scroll
                // mutation. `handle_key`'s own arm for these four
                // variants is deliberately a no-op; the state change
                // lives here.
                //
                // While the help overlay is open, both steps size against
                // and act on the overlay's own scroll state instead of
                // whichever pane is underneath it (`App::handle_key`/
                // `App::handle_scroll_key`'s own `help_open` branches) —
                // `last_help_scroll_viewport_height` is this loop's mirror
                // of `last_scroll_viewport_height` for that surface.
                app = app.handle_key(input_key);
                let viewport_height = if app.help_open() {
                    last_help_scroll_viewport_height.unwrap_or(DEFAULT_SCROLL_VIEWPORT_HEIGHT)
                } else {
                    last_scroll_viewport_height.unwrap_or(DEFAULT_SCROLL_VIEWPORT_HEIGHT)
                };
                app = app.handle_scroll_key(input_key, viewport_height);
            } else {
                // Every non-`Source` key's dispatch is pure (no IO), so it
                // lives in its own function rather than inline here — see
                // `dispatch_non_source_key`'s own doc comment for why this
                // split exists (ADR 0022's `pending_prefix` regression).
                app = dispatch_non_source_key(app, report, &diff_pane_content, input_key);
            }

            // ADR 0048: performs the export this key requested, if any —
            // the one place `review`'s plain `ExportRequest` data actually
            // reaches a port (`gh api`/OSC 52), since `review` itself
            // never calls one. Runs once per handled key, after every
            // review-state transition above (`begin_compose`/`handle_review_key`)
            // has already produced the export request via the verdict/
            // export menu's own confirm step.
            let mut review = app.review().clone();
            if let Some(export) = review.take_pending_export() {
                review = perform_export(review, &review_ports, export);
                app = app.with_review(review);
            }

            if should_recompute_note_markers(&app, last_note_markers_revision) {
                note_markers = note_markers::build_note_markers(app.review().notes());
                last_note_markers_revision = app.review().revision();
            }

            if should_recompute_blast_radius_selection(&app) {
                blast_radius_selection = app.selected_blast_radius_view(report);
            }
            if should_recompute_diff_pane_content(&app) {
                let effects = apply_diff_pane_selection_effects(
                    app,
                    report,
                    &diff_hunks,
                    last_diff_focus,
                    scroll_before_dispatch,
                );
                app = effects.app;
                diff_pane_content = effects.diff_pane_content;
                last_diff_focus = effects.last_diff_focus;
            } else {
                // Without this, toggling away from RightPane::Diff and back
                // with the cursor unchanged left `last_diff_focus` equal to
                // the still-current symbol, so re-entry skipped both ADR
                // 0027's auto-scroll and ADR 0030's sync-back branch and
                // redrew at a stale `right_pane_scroll`. Resetting to `None`
                // makes re-entry look like a fresh selection.
                last_diff_focus = None;
            }
        }
    }
}

/// The result of [`apply_diff_pane_selection_effects`]: the next `App`,
/// the diff pane's freshly rebuilt shaped content, and the `last_diff_focus`
/// value `crate::run_app`'s loop should carry into the next handled key.
/// Grouped into one struct rather than a tuple so the three fields keep
/// their names at every call site (all three change together, and a
/// positional tuple would invite a `(app, content, focus)` vs.
/// `(app, focus, content)` mix-up the first time this function's argument
/// order is touched).
struct DiffPaneSelectionEffects {
    app: App,
    diff_pane_content: diff_shape::DiffPaneContent,
    last_diff_focus: Option<app::DiffFocus>,
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
fn apply_diff_pane_selection_effects(
    mut app: App,
    report: &Report,
    diff_hunks: &[diff_view::FileHunks],
    last_diff_focus: Option<app::DiffFocus>,
    scroll_before_dispatch: usize,
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
        if let Some(target_scroll) = auto_scroll_for_diff_focus(&app, report, &diff_pane_content) {
            app = app.with_right_pane_scroll(target_scroll);
        }
        next_focus
    } else if let Some(target_symbol_id) =
        sync_target_for_scroll(&app, &diff_pane_content, scroll_before_dispatch)
    {
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

/// The symbol id `crate::run_app`'s scroll->tree sync (ADR 0030) should
/// move the tree cursor to, or `None` when no sync should happen this key —
/// extracted as its own pure function (mirroring
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
fn sync_target_for_scroll(
    app: &App,
    diff_pane_content: &diff_shape::DiffPaneContent,
    scroll_before_dispatch: usize,
) -> Option<String> {
    if !should_apply_hunk_jump(app) {
        return None;
    }
    if app.right_pane_scroll() == scroll_before_dispatch {
        return None;
    }
    let target_symbol_id =
        diff_shape::symbol_id_for_scroll_line(diff_pane_content, app.right_pane_scroll())?;
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
/// *every* draw, including the idle ~100ms poll ticks `crate::run_app`'s doc
/// comment already notes, not only after a key press — so shrinking the
/// terminal (fewer visible rows, a smaller `max_scroll`) permanently clamps
/// `App`'s own scroll offset down, and growing the terminal back afterward
/// does not restore the pre-shrink position; there is no separate "requested
/// vs. actually-applied" pair of fields to fall back to; `right_pane_scroll`
/// is single-valued by design (that field's own doc comment). A reviewer who
/// shrinks their terminal mid-read and then grows it back finds the pane
/// scrolled less far than before the resize — judged an acceptable, rare
/// edge case relative to the far more common overshoot this fold-back fixes.
/// Fallback viewport height used by [`run_app`]'s ADR 0026 scroll dispatch
/// (`Ctrl-d`/`Ctrl-u`/`gg`/`G`) when a key press arrives *before* any frame
/// has drawn — a near-impossible edge case in practice (a terminal that
/// finished initializing and accepted its very first key press without ever
/// polling for an idle draw once, or a `run_app` invocation whose first
/// user action is somehow one of these keys with no intervening frame), but
/// still gets a sensible half-page step (12 lines is roughly half a typical
/// 24-row terminal) rather than a 0-line no-op. Not user-visible past this
/// one edge; the very next frame's `DrawOutcome::scroll_viewport_height`
/// replaces it with the real inner-pane height.
const DEFAULT_SCROLL_VIEWPORT_HEIGHT: usize = 24;

/// Whether `input_key` is one of ADR 0026's four scroll variants — the
/// ones [`run_app`] routes through [`App::handle_scroll_key`] (which needs
/// the viewport height) instead of the ordinary
/// [`dispatch_non_source_key`] path. Kept as its own predicate rather than
/// inlined into `run_app` so the two-step dispatch's exact set of variants
/// stays in one obvious place, and so a future addition to that set
/// (e.g. `ScrollLineToCenter`) is a one-line change here rather than a
/// hunt through `run_app`.
fn is_scroll_input_key(input_key: InputKey) -> bool {
    matches!(
        input_key,
        InputKey::ScrollHalfPageDown
            | InputKey::ScrollHalfPageUp
            | InputKey::ScrollToTop
            | InputKey::ScrollToBottom,
    )
}

fn clamp_right_pane_scroll_after_draw(app: App, clamped: Option<usize>) -> App {
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
fn clamp_help_scroll_after_draw(app: App, clamped: Option<usize>) -> App {
    match clamped {
        Some(scroll) => app.with_help_scroll(scroll),
        None => app,
    }
}

/// Dispatches one [`InputKey`] that is not [`InputKey::Source`] (the one
/// key `crate::run_app`'s loop handles inline instead, since it needs a
/// real file read — ADR 0016's "IO isolated to one function" discipline
/// keeps that read out of this otherwise-pure function) against `app`,
/// given `report` and the already-cached `diff_pane_content` both
/// `NextHunk`/`PrevHunk` and `GotoDefinition`/`GotoReferences` need but
/// `App::handle_key` itself has no access to (`InputKey::NextHunk`'s own
/// doc comment). Returns the next `App`.
///
/// Extracted out of `run_app`'s loop body specifically to make this
/// dispatch sequence unit-testable without a live `ratatui::DefaultTerminal`
/// — `run_app` itself cannot be driven directly in a test, so a bug in the
/// *sequencing* of this dispatch (as opposed to a bug in one arm's own
/// logic, which the arm's own unit tests already cover) had no regression
/// coverage before this function existed. That gap is exactly how ADR
/// 0022's `pending_prefix` bug survived review: every existing test called
/// `App::handle_key` directly, which always runs its own unconditional
/// `pending_prefix` clear — the bug was specifically that `run_app`'s old
/// inline `GotoDefinition`/`GotoReferences` branch *skipped* that call
/// entirely, a defect only visible by testing this exact dispatch sequence,
/// not `handle_key` in isolation.
fn dispatch_non_source_key(
    mut app: App,
    report: &Report,
    diff_pane_content: &diff_shape::DiffPaneContent,
    input_key: InputKey,
) -> App {
    if let InputKey::NextHunk | InputKey::PrevHunk = input_key
        && should_apply_hunk_jump(&app)
    {
        // Hunk jumping needs the shaped diff content already cached by the
        // caller (to know where each hunk starts — `App::handle_key` itself
        // has no notion of that content), so the jump target is computed
        // here rather than inside `App`.
        let scroll = diff_shape::hunk_start_lines(diff_pane_content);
        let next = jump_scroll_target(&scroll, app.right_pane_scroll(), input_key);
        if let Some(target) = next {
            return app.handle_key(input_key).with_right_pane_scroll(target);
        }
        return app;
    }

    if let InputKey::GotoDefinition | InputKey::GotoReferences = input_key {
        // `gd`/`gr` candidate resolution needs `report.graph.edges`
        // (`crate::detail::symbol_mentions`), which `App::handle_key` has no
        // access to (ADR 0022, mirroring `NextHunk`/`PrevHunk`'s own
        // precedent just above) — resolved here, then applied via
        // `App::jump_to_symbol`/`App::open_jump_popup`/`App::set_status`
        // depending on the candidate count (`resolve_goto`'s own doc
        // comment on the three-way split).
        //
        // `app.handle_key(input_key)` is called first (post-review finding),
        // even though its own match arm for these two variants is a no-op
        // stub — what matters is not that arm, it is `handle_key`'s own
        // unconditional `pending_prefix` clear at the top of the function.
        // Skipping this call, an earlier version of this dispatch did,
        // meant `gd`/`gr` was the one key in this whole function that never
        // reached `App::handle_key` at all, so a `gd`/`gr` press left
        // `pending_prefix` stuck at `Some(G)` and the *next* `d`/`r` the
        // reviewer typed for its own ordinary reason (`ToggleDiff`, or
        // nothing at all) silently re-resolved as another `GotoDefinition`/
        // `GotoReferences` instead of its own meaning — violating ADR
        // 0022's own "any other key discards the pending prefix" guarantee.
        // Reading `app.selected_symbol_id()`/cursor state via `resolve_goto`
        // *after* this call is safe: the no-op stub touches nothing but
        // `pending_prefix` for these two variants.
        app = app.handle_key(input_key);
        return match resolve_goto(&app, report, input_key) {
            GotoOutcome::NoSymbolSelected => {
                app.set_status("note: no symbol selected");
                app
            }
            GotoOutcome::NoCandidates(direction) => {
                app.set_status(format!("note: no {direction}"));
                app
            }
            GotoOutcome::One(candidate) => app.jump_to_symbol(&candidate.id),
            GotoOutcome::Many(candidates) => app.open_jump_popup(candidates),
        };
    }

    app.handle_key(input_key)
}

/// Whether `crate::run_app`'s event loop should recompute the diff pane's
/// shaped content this key, rather than keep showing the previously cached
/// one — mirrors `should_recompute_blast_radius_selection`'s own contract and
/// reasoning, just for [`RightPane::Diff`] instead of `RightPane::BlastRadius`.
fn should_recompute_diff_pane_content(app: &App) -> bool {
    matches!(app.screen(), Screen::Entry) && app.right_pane() == app::RightPane::Diff
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
fn auto_scroll_for_diff_focus(
    app: &App,
    report: &Report,
    diff_pane_content: &diff_shape::DiffPaneContent,
) -> Option<usize> {
    let focus = app.selected_diff_focus(report)?;
    diff_shape::section_start_line_for_symbol(diff_pane_content, &focus.symbol_id)
}

/// Whether `crate::run_app`'s [`InputKey::Source`] arm should re-run
/// [`crate::source::load_highlighted_symbol_source`] this press, given the
/// cache's current `(cached_symbol, cached_content)` pair and the
/// `next_symbol` the just-pressed `s` would open. The general "must not
/// reparse inside the render loop" invariant this cache holds against idle
/// poll ticks is really a facet of a sharper rule — "no reparse per
/// user-observable state change" — which idle poll ticks satisfy trivially
/// (they change nothing) but explicit `s`-on-the-same-row presses also
/// satisfy (the reviewer observes the same screen either way).
///
/// Returns `true` (reload) when:
/// - nothing is cached yet (first `s` press);
/// - the cache holds a *different* symbol (drilling into a new row);
/// - or the cache holds an `Err(_)` for the same symbol — a previously
///   failed load must remain retryable (`s` again after editing the file
///   back into existence is the reviewer's own retry gesture, and denying
///   it would be worse than the one-shot reparse cost).
///
/// Returns `false` (skip reload) only when the cache already holds a
/// successful `Ok(_)` for this exact `next_symbol` — the "same-row
/// re-entry" case this guard exists to save.
///
/// Extracted as its own pure function (rather than inlined in `run_app`,
/// which takes a live `ratatui::DefaultTerminal` and so cannot be driven
/// directly in a test) so this exact decision is unit-testable without a
/// terminal — mirrors `should_recompute_diff_pane_content`'s own
/// precedent just above.
fn should_reload_source_content(
    cached_symbol: Option<&str>,
    cached_content: Option<&Result<source::HighlightedSourceView, String>>,
    next_symbol: &str,
) -> bool {
    !matches!(
        (cached_symbol, cached_content),
        (Some(cached_id), Some(Ok(_))) if cached_id == next_symbol,
    )
}

/// Whether `crate::run_app`'s event loop should act on an
/// [`InputKey::NextHunk`]/[`InputKey::PrevHunk`] press by jumping
/// `diff_pane_content`'s scroll offset, rather than treating the key as a
/// no-op. `true` only while [`app::Focus::Right`] *and* [`app::RightPane::Diff`]
/// is showing — gating on focus alone let `]`/`[` scroll the Detail/BlastRadius
/// pane using `diff_pane_content`'s hunk-start table, which is only ever
/// recomputed for the Diff pane (`should_recompute_diff_pane_content` above),
/// so it goes stale (pinned to whichever file/symbol was selected the last
/// time Diff was shown) the moment the user switches away from Diff. That
/// produced a jump with no relation to what is actually on screen.
///
/// Extracted as its own pure function, mirroring `should_recompute_blast_radius_selection`'s
/// own reasoning, so this exact gate is unit-testable without a live
/// `ratatui::DefaultTerminal`.
fn should_apply_hunk_jump(app: &App) -> bool {
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
fn jump_scroll_target(
    hunk_starts: &[usize],
    current_scroll: usize,
    direction: InputKey,
) -> Option<usize> {
    match direction {
        InputKey::NextHunk => hunk_starts
            .iter()
            .copied()
            .find(|&start| start > current_scroll),
        InputKey::PrevHunk => hunk_starts
            .iter()
            .copied()
            .rfind(|&start| start < current_scroll),
        _ => None,
    }
}

/// What `crate::run_app` should do next for a pending `gd`/`gr` press (ADR
/// 0022's "0/1/many" branching): no symbol was selected at all, the
/// selected symbol has no candidates in the requested direction (carrying a
/// human-readable direction label, `"callees"`/`"callers"`, for the status
/// message — plain data, not formatted text, matching this crate's own
/// "view-model, not string-building, outside `ui.rs`" convention), exactly
/// one candidate (jump immediately), or more than one (open the popup).
#[derive(Debug, Clone, PartialEq, Eq)]
enum GotoOutcome {
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
/// own precedent just above.
fn resolve_goto(app: &App, report: &Report, direction: InputKey) -> GotoOutcome {
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

/// Whether `crate::run_app`'s event loop should recompute the blast-radius
/// selection this key, rather than keep showing the previously cached one
/// (this function's own extraction is what makes that decision
/// unit-testable without a live `ratatui::DefaultTerminal` — `run_app`
/// itself takes one and so cannot be driven directly in a test). `true`
/// only when the blast-radius pane is actually the active right pane on the
/// entry screen; every other key/screen combination leaves the cached value
/// untouched rather than resetting it to `NotApplicable`, so switching away
/// from and back to the blast-radius pane (e.g. `R` -> `d` -> `R`) does not
/// need a wasted recompute on the `d` press that briefly leaves it.
fn should_recompute_blast_radius_selection(app: &App) -> bool {
    matches!(app.screen(), Screen::Entry) && app.right_pane() == app::RightPane::BlastRadius
}

#[cfg(test)]
#[path = "lib_tests/mod.rs"]
mod tests;
