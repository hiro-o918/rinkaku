//! [`run_app`]'s event loop (ADR 0015/0016) and its dispatch machinery, split
//! out of `lib.rs` (ADR 0028) once that file grew past the file-size
//! threshold. Two sibling modules hold specific pieces of the loop's own
//! logic: [`scroll_sync`] (ADR 0027/ADR 0030's diff-pane <-> tree-cursor
//! scroll synchronization, the hunk-jump target, and the post-draw scroll
//! fold-back) and [`goto`] (ADR 0022's `gd`/`gr` candidate resolution).
//! `crate::input_translate` (raw `crossterm` events -> `InputKey`) and
//! `crate::review_flow` (ADR 0048's review-notes composing/exporting/caching
//! glue) are further siblings this loop delegates to but does not own.

mod goto;
mod scroll_sync;

use crate::app::{App, BlastRadiusSelection, InputKey, Screen};
use crate::review::PrContext;
use crate::review::ports::{BrowserOpener, ClipboardSink, ReviewSubmitter};
use crate::review_flow::{
    derive_selection_snapshot, dispatch_note_compose_key, open_pr_in_browser, perform_export,
    should_recompute_note_markers,
};
use crate::{diff_shape, diff_view, highlight, input_translate, note_markers, source, ui};
use goto::{GotoOutcome, resolve_goto};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use rinkaku_core::render::Report;
use scroll_sync::{
    apply_diff_pane_selection_effects, auto_scroll_for_diff_focus, clamp_help_scroll_after_draw,
    clamp_right_pane_scroll_after_draw, jump_scroll_target, should_apply_hunk_jump,
};
use std::time::Duration;

/// Review-notes export wiring (ADR 0048), assembled once by `main.rs`'s
/// composition root and threaded through unchanged from
/// [`crate::session::TuiSession::run`] to [`run_app`]: `pr_context`/
/// `submitter` are both `Some`/`None` together (sink A's own "absent when
/// no PR context" rule — [`crate::app::App::with_review_sink_a_available`]'s
/// own doc comment), `clipboard` is always present since sink B never
/// depends on a PR. `browser` (ADR 0050) is likewise always present — `w` is
/// a global key regardless of `pr_context`, so the port itself always
/// exists; only the `PrContext` it needs to build a URL may be absent.
pub struct ReviewPorts<'a> {
    pub pr_context: Option<PrContext>,
    pub submitter: Option<&'a dyn ReviewSubmitter>,
    pub clipboard: &'a dyn ClipboardSink,
    pub browser: &'a dyn BrowserOpener,
}

/// The event loop [`TuiSession::run`] (`crate::session`) drives once it has
/// taken over the terminal — see [`crate::run`]'s doc comment (re-exported
/// from `crate::session`, which retains the full terminal-lifecycle
/// rationale that used to live in `lib.rs`) for what `report`, `diff_text`,
/// `entry_path`, `repo_root`, and `source_reader` mean. `pub(crate)` rather
/// than private: `crate::session` is a sibling module, not a submodule, so
/// it needs this visibility to call in.
///
/// [`TuiSession::run`]: crate::session::TuiSession::run
// `update_check` (ADR 0054) pushed this past clippy's 7-argument
// threshold; every parameter is already independently load-bearing (see
// this function's own doc comment and `TuiSession::run`'s, which has the
// same allow for the same reason), so bundling them into a struct now
// would only rename the same values one level deeper.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    report: &Report,
    diff_text: &str,
    entry_path: Option<&str>,
    repo_root: &std::path::Path,
    source_reader: &dyn source::SourceReader,
    review_ports: ReviewPorts<'_>,
    update_check: Option<std::sync::mpsc::Receiver<String>>,
) -> std::io::Result<bool> {
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
    let mut last_diff_focus: Option<crate::app::DiffFocus> = app.selected_diff_focus(report);
    if let Some(target_scroll) =
        auto_scroll_for_diff_focus(&app, report, &diff_pane_content, app.diff_view_mode())
    {
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
    // The diff pane's *effective* view mode as of the last drawn frame
    // (`ui::DrawOutcome::effective_diff_view_mode`) — the ADR 0044
    // decision 7 fallback (`MIN_SPLIT_VIEW_WIDTH`) means a requested
    // `Split` can render as `Unified` on a narrow pane, and the ADR
    // 0044 decision 4 amendment's mode-aware `walk_sections` needs the
    // effective mode (not the requested one) for its scroll math to
    // match what is on screen. Initialized to the requested mode so
    // the very first key press before any frame has drawn (a rare edge
    // case at startup) still has a sensible value to feed in.
    let mut last_effective_diff_view_mode: crate::app::DiffViewMode = app.diff_view_mode();
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
        if let Some(mode) = outcome.effective_diff_view_mode {
            last_effective_diff_view_mode = mode;
        }

        if app.should_quit() {
            return Ok(app.update_requested());
        }

        // ADR 0054: a non-blocking check of the background version-check
        // thread's channel, once per loop iteration alongside the poll
        // timeout below — `try_recv` never blocks, so this cannot delay
        // input handling the way waiting on the thread itself would.
        // `recv()`'s `Err` (the sender dropped without ever sending, e.g.
        // the check found nothing newer) is silently ignored, same as
        // `check_update_available`'s own "silent on failure" contract.
        if let Some(receiver) = &update_check
            && let Ok(version) = receiver.try_recv()
        {
            app.notify_update_available(version);
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
                    input_translate::translate_key(key_event.code, key_event.modifiers, &app)
                }
                Event::Mouse(mouse_event) => {
                    input_translate::translate_mouse_event(mouse_event.kind)
                }
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
            } else if let InputKey::OpenPrInBrowser = input_key {
                // ADR 0050: needs `review_ports.pr_context`/`.browser`,
                // neither of which `App::handle_key` has access to (mirrors
                // `InputKey::NoteCompose`'s own precedent just above).
                // `app.handle_key(input_key)` still runs first for the
                // blanket `status`/`pending_prefix` reset every key needs
                // (`App::handle_key`'s own doc comment) — its own arm for
                // this variant is a no-op stub, same as `NoteCompose`'s.
                app = app.handle_key(input_key);
                app = open_pr_in_browser(app, &review_ports);
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
                app = dispatch_non_source_key(
                    app,
                    report,
                    &diff_pane_content,
                    input_key,
                    last_effective_diff_view_mode,
                );
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
                    last_effective_diff_view_mode,
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

/// Dispatches one [`InputKey`] that is not [`InputKey::Source`] (the one
/// key `crate::event_loop::run_app`'s loop handles inline instead, since it
/// needs a real file read — ADR 0016's "IO isolated to one function"
/// discipline keeps that read out of this otherwise-pure function) against
/// `app`, given `report` and the already-cached `diff_pane_content` both
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
    effective_diff_view_mode: crate::app::DiffViewMode,
) -> App {
    if let InputKey::NextHunk | InputKey::PrevHunk = input_key
        && should_apply_hunk_jump(&app)
    {
        // Hunk jumping needs the shaped diff content already cached by the
        // caller (to know where each hunk starts — `App::handle_key` itself
        // has no notion of that content), so the jump target is computed
        // here rather than inside `App`.
        let scroll = diff_shape::hunk_start_lines(diff_pane_content, effective_diff_view_mode);
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

/// Whether `crate::event_loop::run_app`'s event loop should recompute the
/// diff pane's shaped content this key, rather than keep showing the
/// previously cached one — mirrors `should_recompute_blast_radius_selection`'s
/// own contract and reasoning, just for [`RightPane::Diff`] instead of
/// `RightPane::BlastRadius`.
///
/// [`RightPane::Diff`]: crate::app::RightPane::Diff
fn should_recompute_diff_pane_content(app: &App) -> bool {
    matches!(app.screen(), Screen::Entry) && app.right_pane() == crate::app::RightPane::Diff
}

/// Whether `crate::event_loop::run_app`'s [`InputKey::Source`] arm should
/// re-run [`crate::source::load_highlighted_symbol_source`] this press,
/// given the cache's current `(cached_symbol, cached_content)` pair and the
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

/// Whether `crate::event_loop::run_app`'s event loop should recompute the
/// blast-radius selection this key, rather than keep showing the previously
/// cached one (this function's own extraction is what makes that decision
/// unit-testable without a live `ratatui::DefaultTerminal` — `run_app`
/// itself takes one and so cannot be driven directly in a test). `true`
/// only when the blast-radius pane is actually the active right pane on the
/// entry screen; every other key/screen combination leaves the cached value
/// untouched rather than resetting it to `NotApplicable`, so switching away
/// from and back to the blast-radius pane (e.g. `R` -> `d` -> `R`) does not
/// need a wasted recompute on the `d` press that briefly leaves it.
fn should_recompute_blast_radius_selection(app: &App) -> bool {
    matches!(app.screen(), Screen::Entry) && app.right_pane() == crate::app::RightPane::BlastRadius
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
