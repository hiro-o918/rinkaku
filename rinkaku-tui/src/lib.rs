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
pub mod nav;
pub mod order;
pub mod row_view;
pub mod source;
pub mod tree;
pub mod ui;

use app::{App, BlastRadiusSelection, InputKey, Screen};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use rinkaku_core::render::Report;
use std::time::Duration;

/// Runs the interactive TUI over `report` until the user quits, taking
/// over the terminal for the duration of the call (raw mode + alternate
/// screen via [`ratatui::try_init`], restored on return **and** on panic —
/// `ratatui::try_init`'s own panic hook covers the latter, so a bug in this
/// crate cannot leave the caller's terminal in raw mode).
///
/// Uses `try_init` rather than [`ratatui::init`] specifically so terminal
/// setup failure (e.g. stdin/stdout is not a TTY at all — piped input,
/// `< /dev/null`, a CI runner) surfaces as an `Err` for `main.rs`'s
/// `anyhow` path to print cleanly and exit 1, instead of `ratatui::init`'s
/// own `.expect(...)` panicking with a raw Rust panic message and exit
/// code 101.
///
/// This is the only function in the crate that touches a real terminal or
/// blocks on input; everything it calls into (`App`, `row_view`, `ui`,
/// `source`) is either pure or an isolated, narrowly-scoped IO call (a
/// single source-file read).
///
/// `diff_text` is the exact same raw diff string every `main.rs` input mode
/// already holds before handing it to `rinkaku_core::pipeline::analyze_diff`
/// — passed through unchanged, not re-fetched or re-derived here, so this
/// crate never runs `git` itself (ADR 0016: `rinkaku-core`/adapters own IO,
/// not `rinkaku-tui`'s view layer beyond the one source-file read
/// `crate::source` already makes).
///
/// `entry_path` is `main.rs`'s `--entry <path>` flag (ADR 0019), passed
/// through unchanged when the user combines it with `--tui`: `None` when
/// `--entry` was not given (the ordinary case), `Some(path)` to open
/// straight into [`app::RightPane::BlastRadius`] (ADR 0023) with the cursor
/// already on the matching tree row (`App::with_entry_pivot`) instead of
/// requiring the reviewer to hunt for the row and press `R` themselves. Note
/// this crate does *not* itself re-root `report.graph` — `main.rs` already
/// applied `--entry`'s `pivot_graph` re-rooting to `report` before calling
/// here (the same `Report` both the TUI and Markdown/JSON render from), so
/// this parameter only drives where the TUI *starts*, not what the
/// underlying graph looks like.
///
/// `repo_root` anchors `Report` paths (always repository-root-relative) for
/// the source drill-down's file reads (`crate::source::load_symbol_source`)
/// — `main.rs` resolves it once at startup (`git rev-parse --show-toplevel`,
/// falling back to the process's current directory outside a git
/// repository) rather than this crate ever shelling out to `git` itself
/// (ADR 0016). Without it, the source view would only work when `rinkaku`
/// happens to be invoked from the repository root.
pub fn run(
    report: &Report,
    diff_text: &str,
    entry_path: Option<&str>,
    repo_root: &std::path::Path,
) -> std::io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let result = run_app(&mut terminal, report, diff_text, entry_path, repo_root);
    ratatui::restore();
    result
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    report: &Report,
    diff_text: &str,
    entry_path: Option<&str>,
    repo_root: &std::path::Path,
) -> std::io::Result<()> {
    let mut app = App::new(report);
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

    loop {
        // `ui::draw`'s return value (the right-hand pane's scroll offset as
        // actually clamped and rendered this frame, `ui::draw`'s own doc
        // comment) cannot flow out of the closure itself — `Terminal::draw`
        // requires an `FnOnce(&mut Frame)` returning `()` — so it is
        // captured into this outer binding instead and folded back into
        // `app` right after, via `clamp_right_pane_scroll_after_draw`.
        let mut clamped_scroll = None;
        terminal.draw(|frame| {
            clamped_scroll = ui::draw(
                frame,
                &app,
                report,
                &diff_pane_content,
                &diff_highlights,
                &blast_radius_selection,
                source_content.as_ref(),
            );
        })?;
        app = clamp_right_pane_scroll_after_draw(app, clamped_scroll);

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
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key_event) = event::read()?
            && key_event.kind == KeyEventKind::Press
            && let Some(input_key) = translate_key(key_event.code, key_event.modifiers, &app)
        {
            if let InputKey::Source = input_key {
                app = app.handle_key(input_key);
                if let Screen::Source { symbol_id } = app.screen().clone()
                    && should_reload_source_content(
                        source_content_symbol.as_deref(),
                        source_content.as_ref(),
                        &symbol_id,
                    )
                {
                    let loaded =
                        source::load_highlighted_symbol_source(report, &symbol_id, repo_root);
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
            } else {
                // Every non-`Source` key's dispatch is pure (no IO), so it
                // lives in its own function rather than inline here — see
                // `dispatch_non_source_key`'s own doc comment for why this
                // split exists (ADR 0022's `pending_prefix` regression).
                app = dispatch_non_source_key(app, report, &diff_pane_content, input_key);
            }

            if should_recompute_blast_radius_selection(&app) {
                blast_radius_selection = app.selected_blast_radius_view(report);
            }
            if should_recompute_diff_pane_content(&app) {
                diff_pane_content = diff_shape::build_diff_pane_content(
                    report,
                    &diff_hunks,
                    app.selected_diff_target(report).as_ref(),
                );
            }
        }
    }
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
fn clamp_right_pane_scroll_after_draw(app: App, clamped: Option<usize>) -> App {
    match clamped {
        Some(scroll) => app.with_right_pane_scroll(scroll),
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

/// Translates a raw `crossterm` key press into this crate's
/// terminal-agnostic [`InputKey`], or `None` for a key the app does not
/// react to. Depends on `app.screen()` to disambiguate `q`/Esc (`Quit`/
/// `FocusLeft` on the entry view depending on focus, `Back` on the source
/// view) and on `app.focus()` (ADR 0020) to route Esc between `FocusLeft`
/// and its other meanings — every other mapping is context-free.
///
/// `app.help_open()` (ADR 0020) short-circuits every other rule: while the
/// help overlay is open, `?`/Esc/`q` all translate to `ToggleHelp` (closing
/// it) regardless of what they would otherwise mean, and this check runs
/// before every other arm so none of them — especially `q`, which would
/// otherwise mean `Quit` — can reach past the overlay. `App::handle_key`'s
/// own `help_open` guard is a second, independent layer of the same rule
/// (swallowing every non-`ToggleHelp` key while open) — belt and braces,
/// since "the overlay is a safe action that can never accidentally quit
/// the app" is exactly the property ADR 0020 asks this feature to hold.
///
/// `app.jump_popup()` (ADR 0022) is the next short-circuit, mirroring the
/// help overlay's own structure: while the jump-target popup is open,
/// `j`/`k`/Up/Down move its own selection, Enter confirms (`PopupConfirm`),
/// Esc cancels (`PopupCancel`), and every other key is swallowed.
///
/// `app.pending_prefix()` (ADR 0022) is consulted only for `d`/`r`: when a
/// `g` press is still pending, `d` resolves to `GotoDefinition` and `r` to
/// `GotoReferences` instead of their own ordinary meanings (`ToggleDiff`/
/// unbound) — every other key falls through to its normal translation
/// unconditionally, which is what lets the pending prefix's own state
/// (`App::handle_key`'s blanket clear-unless-`PendingGoto` rule) correctly
/// unwind on any key that is not `d`/`r`.
fn translate_key(code: KeyCode, modifiers: KeyModifiers, app: &App) -> Option<InputKey> {
    if app.help_open() {
        return match code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => Some(InputKey::ToggleHelp),
            _ => None,
        };
    }

    if app.jump_popup().is_some() {
        return match code {
            KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
            KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
            KeyCode::Enter => Some(InputKey::PopupConfirm),
            KeyCode::Esc => Some(InputKey::PopupCancel),
            _ => None,
        };
    }

    let on_source_screen = matches!(app.screen(), Screen::Source { .. });
    let right_focused = app.focus() == app::Focus::Right;

    if app.pending_prefix() == Some(app::PendingPrefix::G) {
        match code {
            KeyCode::Char('d') | KeyCode::Char('D') => return Some(InputKey::GotoDefinition),
            KeyCode::Char('r') | KeyCode::Char('R') => return Some(InputKey::GotoReferences),
            _ => {}
        }
    }

    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
        // Space always means "expand/collapse", never "drill in" — kept
        // distinct from Enter's own `InputKey::Open` (ADR 0020) so Space on
        // a file/symbol row never moves focus. Translated unconditionally
        // here regardless of `app.focus()`, same as every other key this
        // function maps context-free — `App::handle_key`'s own
        // `Focus::Tree`-only arm for `Select` is where the actual
        // Tree-focus requirement lives (mirroring how `NextHunk`/`PrevHunk`
        // are also translated unconditionally but only acted on under
        // certain conditions elsewhere).
        KeyCode::Char(' ') => Some(InputKey::Select),
        KeyCode::Enter => Some(InputKey::Open),
        KeyCode::Char('e') | KeyCode::Char('E') => Some(InputKey::ExpandAll),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::Quit),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(InputKey::CollapseAll),
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::JumpBack),
        // Ctrl-I and Tab share the same control code (0x09) at the terminal
        // protocol level — without Kitty's keyboard-enhancement protocol
        // (which this crate does not enable), a real Ctrl-I keypress
        // arrives here as plain `KeyCode::Tab`, not `KeyCode::Char('i')` +
        // `CONTROL` (confirmed via manual tmux testing against a real
        // terminal, not just documentation: the `Char('i') + CONTROL` arm
        // alone never matched a real Ctrl-I press). Both patterns are kept
        // so this still works correctly in an environment that *does*
        // report the modifier form (e.g. a test harness constructing the
        // event directly, as this module's own tests do).
        KeyCode::Tab => Some(InputKey::JumpForward),
        KeyCode::Char('i') if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(InputKey::JumpForward)
        }
        KeyCode::Char('o') | KeyCode::Char('O') => Some(InputKey::ToggleOrder),
        KeyCode::Char('d') | KeyCode::Char('D') => Some(InputKey::ToggleDiff),
        KeyCode::Char('r') | KeyCode::Char('R') => Some(InputKey::ToggleBlastRadius),
        // `h`, or Esc while the right pane has focus: return focus to the
        // tree (ADR 0020's neovim-style "move left/back"). Checked before
        // the source-screen Esc arm below so `h`/Esc while Right-focused
        // never reaches the source screen (impossible in practice today,
        // since opening the source screen already moves focus to `Right`,
        // but ordered defensively rather than relying on that invariant).
        KeyCode::Char('h') if right_focused => Some(InputKey::FocusLeft),
        KeyCode::Esc if right_focused && !on_source_screen => Some(InputKey::FocusLeft),
        // `]c`/`[c` (vim's hunk-jump idiom) are read here as a single
        // bracket keystroke rather than a buffered two-key chord — this
        // crate's event loop (`run_app`) has no notion of a pending-chord
        // state machine today, and introducing one for exactly one binding
        // would be disproportionate; `]`/`[` alone are otherwise unbound,
        // so no existing gesture is lost by this simplification.
        KeyCode::Char(']') => Some(InputKey::NextHunk),
        KeyCode::Char('[') => Some(InputKey::PrevHunk),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(InputKey::Source),
        // `g` (ADR 0022): the first half of the `gd`/`gr` two-key sequence.
        // Checked after the `pending_prefix` resolution above so a second
        // `g` press (`gg`, not a bound sequence today) simply restarts the
        // pending state rather than doing anything else — `App::handle_key`
        // sets `pending_prefix` from this variant unconditionally.
        KeyCode::Char('g') => Some(InputKey::PendingGoto),
        KeyCode::Char('?') => Some(InputKey::ToggleHelp),
        KeyCode::Esc if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') => Some(InputKey::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rinkaku_core::graph::SymbolGraph;

    fn empty_report() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_translate_ctrl_c_to_quit_regardless_of_screen() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &app);

        assert_eq!(Some(InputKey::Quit), actual);
    }

    #[test]
    fn should_translate_q_to_quit_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Quit), actual);
    }

    #[test]
    fn should_translate_esc_to_none_on_entry_screen() {
        // Esc has no "back" target on the entry screen (App::handle_key's
        // own doc comment) and is not bound to quit there either — quit is
        // 'q'/Ctrl-C only, so Esc is simply not handled at this screen.
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_translate_enter_to_open() {
        // ADR 0020: Enter is `Open` (may move focus), distinct from Space's
        // `Select` (never moves focus) — see the two tests right after this
        // one.
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Open), actual);
    }

    #[test]
    fn should_translate_space_to_select() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char(' '), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Select), actual);
    }

    #[test]
    fn should_translate_h_to_focus_left_when_right_focused() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Open);
        assert_eq!(app::Focus::Right, app.focus());

        let actual = translate_key(KeyCode::Char('h'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::FocusLeft), actual);
    }

    #[test]
    fn should_not_translate_h_at_all_when_tree_focused() {
        // `h` has no meaning while Focus::Tree (ADR 0020 only assigns it a
        // "move left/back" meaning while Focus::Right) — must fall through
        // to `None`, not be swallowed by some other arm.
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(app::Focus::Tree, app.focus());

        let actual = translate_key(KeyCode::Char('h'), KeyModifiers::NONE, &app);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_translate_esc_to_focus_left_when_right_focused_on_entry_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Open);
        assert_eq!(app::Focus::Right, app.focus());

        let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::FocusLeft), actual);
    }

    #[test]
    fn should_translate_lowercase_r_to_toggle_blast_radius() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('r'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleBlastRadius), actual);
    }

    #[test]
    fn should_translate_uppercase_r_to_toggle_blast_radius() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('R'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleBlastRadius), actual);
    }

    #[test]
    fn should_translate_right_bracket_to_next_hunk() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char(']'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::NextHunk), actual);
    }

    #[test]
    fn should_translate_left_bracket_to_prev_hunk() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('['), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::PrevHunk), actual);
    }

    #[test]
    fn should_translate_question_mark_to_toggle_help() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('?'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleHelp), actual);
    }

    #[test]
    fn should_translate_esc_to_toggle_help_when_overlay_is_open() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);

        let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleHelp), actual);
    }

    #[test]
    fn should_translate_q_to_toggle_help_instead_of_quit_when_overlay_is_open() {
        // ADR 0020: `q` must close the overlay, not fall through to its
        // normal `Quit` meaning, while it is open.
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);

        let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleHelp), actual);
    }

    #[test]
    fn should_translate_arbitrary_key_to_none_when_overlay_is_open() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);

        let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_translate_lowercase_j_to_down_regardless_of_focus() {
        // Regression guard: lowercase j/k are always translated to the same
        // `InputKey::Down`/`Up` regardless of focus — `App::handle_key`, not
        // `translate_key`, is what decides whether that means "move cursor"
        // or "scroll" (ADR 0020).
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Down), actual);
    }

    // Regression guard for the per-frame blast-radius recompute bug:
    // `run_app` used to call `App::selected_blast_radius_view` from inside
    // `ui::draw`, which runs on every ~100ms idle poll tick, not only on a
    // key press. Pinning `should_recompute_blast_radius_selection`'s
    // contract (recompute exactly when the blast-radius pane is the active
    // right pane on the entry screen, and nowhere else) is the closest
    // unit-testable proxy for that fix, since `run_app` itself takes a live
    // `ratatui::DefaultTerminal` and cannot be driven directly in a test.
    #[test]
    fn should_recompute_blast_radius_selection_when_blast_radius_pane_is_active_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleBlastRadius);

        let actual = should_recompute_blast_radius_selection(&app);

        assert!(actual);
    }

    #[test]
    fn should_not_recompute_blast_radius_selection_when_right_pane_is_detail() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = should_recompute_blast_radius_selection(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_blast_radius_selection_when_right_pane_is_diff() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleDiff);

        let actual = should_recompute_blast_radius_selection(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_blast_radius_selection_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::ToggleBlastRadius)
            .handle_key(InputKey::Source);

        let actual = should_recompute_blast_radius_selection(&app);

        assert!(!actual);
    }

    fn report_with_one_symbol() -> Report {
        use rinkaku_core::diff::LineRange;
        use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
        use rinkaku_core::render::FileReport;

        Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    id: "lib.rs::foo".to_string(),
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo()".to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                    referenced_names: vec![],
                    dependencies: vec![],
                    omitted_dependency_matches: 0,
                    is_test: false,
                    classification: None,
                    previous_signature: None,
                }],
            }],
            ..empty_report()
        }
    }

    #[test]
    fn should_recompute_diff_pane_content_when_diff_pane_is_active_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(app::RightPane::Diff, app.right_pane()); // ADR 0020 default

        let actual = should_recompute_diff_pane_content(&app);

        assert!(actual);
    }

    #[test]
    fn should_not_recompute_diff_pane_content_when_right_pane_is_detail() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleDiff);

        let actual = should_recompute_diff_pane_content(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_diff_pane_content_when_right_pane_is_blast_radius() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleBlastRadius);

        let actual = should_recompute_diff_pane_content(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_diff_pane_content_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);

        let actual = should_recompute_diff_pane_content(&app);

        assert!(!actual);
    }

    // --- should_reload_source_content ---
    //
    // Regression coverage for the `s`-connot-invariant this guard closes:
    // `run_app`'s [`InputKey::Source`] arm used to call
    // `source::load_highlighted_symbol_source` unconditionally, so pressing
    // `s` a second time on the same row re-read the file and re-ran a full
    // tree-sitter parse — a leak in the "no reparse per user-observable
    // state change" invariant this cache exists to hold, at the explicit-
    // key-press granularity that the idle-poll-tick coverage did not close.

    fn dummy_view(path: &str) -> source::HighlightedSourceView {
        source::HighlightedSourceView {
            view: source::SourceView {
                path: path.to_string(),
                lines: vec![],
                highlight_start: 1,
                highlight_end: 1,
            },
            token_highlights: vec![],
        }
    }

    #[test]
    fn should_reload_source_content_when_cache_is_empty() {
        let actual = should_reload_source_content(None, None, "src/lib.rs::foo");

        assert!(actual);
    }

    #[test]
    fn should_reload_source_content_when_cached_symbol_differs() {
        let cached = Ok(dummy_view("src/lib.rs"));

        let actual =
            should_reload_source_content(Some("src/lib.rs::foo"), Some(&cached), "src/lib.rs::bar");

        assert!(actual);
    }

    #[test]
    fn should_skip_reload_when_cached_ok_matches_next_symbol() {
        let cached = Ok(dummy_view("src/lib.rs"));

        let actual =
            should_reload_source_content(Some("src/lib.rs::foo"), Some(&cached), "src/lib.rs::foo");

        assert!(!actual);
    }

    #[test]
    fn should_reload_source_content_when_cached_err_even_for_same_symbol() {
        // Retryability contract: a failed load remains retryable on the
        // reviewer's next `s` press (e.g. after editing the file back into
        // existence), so a cached `Err(_)` must never suppress the reload —
        // the small parse cost is worth keeping the recovery gesture live.
        let cached: Result<source::HighlightedSourceView, String> =
            Err("failed to read".to_string());

        let actual =
            should_reload_source_content(Some("src/lib.rs::foo"), Some(&cached), "src/lib.rs::foo");

        assert!(actual);
    }

    #[test]
    fn should_reload_source_content_when_cache_has_symbol_but_no_content() {
        // Defensive combination the loop never actually reaches today
        // (`source_content` and `source_content_symbol` are always written
        // together): if they ever fall out of sync, the safe default is to
        // reload rather than trust the stale symbol id alone.
        let actual = should_reload_source_content(Some("src/lib.rs::foo"), None, "src/lib.rs::foo");

        assert!(actual);
    }

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

    // --- clamp_right_pane_scroll_after_draw ---
    //
    // Dogfooding fix: `render_scrollable_pane`'s clamp only ever affected
    // what was drawn, never `App`'s own `right_pane_scroll` — so an
    // overshot scroll request stayed recorded in `App` even once the pane
    // visibly stopped moving, and winding it back down took as many `k`
    // presses as it took to overshoot in the first place. These tests pin
    // the fold-back that keeps `App`'s state in sync with the frame that
    // was actually drawn.

    #[test]
    fn should_overwrite_right_pane_scroll_with_the_clamped_value_when_some() {
        let report = empty_report();
        let app = App::new(&report).with_right_pane_scroll(999);

        let app = clamp_right_pane_scroll_after_draw(app, Some(7));

        assert_eq!(7, app.right_pane_scroll());
    }

    #[test]
    fn should_leave_right_pane_scroll_untouched_when_none() {
        // `None` means the drawn pane had nothing scrollable this frame
        // (`ui::draw`'s own doc comment: the source screen, or a
        // placeholder) — `App`'s own requested scroll must survive
        // unchanged rather than being zeroed or otherwise disturbed by a
        // frame that never consulted it.
        let report = empty_report();
        let app = App::new(&report).with_right_pane_scroll(3);

        let app = clamp_right_pane_scroll_after_draw(app, None);

        assert_eq!(3, app.right_pane_scroll());
    }

    // g-prefix and jump-popup translate_key tests (ADR 0022).

    #[test]
    fn should_translate_g_to_pending_goto() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::PendingGoto), actual);
    }

    #[test]
    fn should_translate_d_to_goto_definition_when_g_is_pending() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::PendingGoto);

        let actual = translate_key(KeyCode::Char('d'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::GotoDefinition), actual);
    }

    #[test]
    fn should_translate_r_to_goto_references_when_g_is_pending() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::PendingGoto);

        let actual = translate_key(KeyCode::Char('r'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::GotoReferences), actual);
    }

    #[test]
    fn should_translate_d_to_toggle_diff_when_no_prefix_is_pending() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('d'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleDiff), actual);
    }

    #[test]
    fn should_fall_through_to_ordinary_meaning_when_a_non_dr_key_follows_pending_goto() {
        // `gj` is not a bound sequence — `j` must still translate to its own
        // ordinary `Down` meaning, not be swallowed just because a prefix
        // was pending (`App::handle_key`'s blanket clear-unless-`PendingGoto`
        // rule is what actually unwinds `pending_prefix` on the next key;
        // this test only pins `translate_key`'s own half of that contract).
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::PendingGoto);

        let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Down), actual);
    }

    #[test]
    fn should_translate_ctrl_o_to_jump_back() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('o'), KeyModifiers::CONTROL, &app);

        assert_eq!(Some(InputKey::JumpBack), actual);
    }

    #[test]
    fn should_translate_ctrl_i_to_jump_forward() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('i'), KeyModifiers::CONTROL, &app);

        assert_eq!(Some(InputKey::JumpForward), actual);
    }

    #[test]
    fn should_translate_tab_to_jump_forward() {
        // A real Ctrl-I keypress arrives here as `KeyCode::Tab`, not
        // `KeyCode::Char('i')` + `CONTROL` — confirmed via manual testing
        // against a real terminal (tmux), since Ctrl-I and Tab share the
        // same control code (0x09) without Kitty's keyboard-enhancement
        // protocol, which this crate does not enable. Without this mapping,
        // Ctrl-i silently did nothing in practice despite the
        // `Char('i') + CONTROL` test above passing.
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Tab, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::JumpForward), actual);
    }

    #[test]
    fn should_translate_plain_o_to_toggle_order_without_control_modifier() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('o'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::ToggleOrder), actual);
    }

    fn candidate(id: &str, name: &str, path: &str) -> app::JumpCandidate {
        app::JumpCandidate {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn should_translate_j_and_k_to_popup_motion_while_jump_popup_is_open() {
        let report = empty_report();
        let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

        assert_eq!(
            Some(InputKey::Down),
            translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app)
        );
        assert_eq!(
            Some(InputKey::Up),
            translate_key(KeyCode::Char('k'), KeyModifiers::NONE, &app)
        );
    }

    #[test]
    fn should_translate_enter_to_popup_confirm_while_jump_popup_is_open() {
        let report = empty_report();
        let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

        let actual = translate_key(KeyCode::Enter, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::PopupConfirm), actual);
    }

    #[test]
    fn should_translate_esc_to_popup_cancel_while_jump_popup_is_open() {
        let report = empty_report();
        let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

        let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::PopupCancel), actual);
    }

    #[test]
    fn should_translate_q_to_none_while_jump_popup_is_open() {
        // `q` must not fall through to `Quit` while the popup is open —
        // mirrors the help overlay's own "swallow everything but the
        // close/confirm/cancel keys" contract.
        let report = empty_report();
        let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

        let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

        assert_eq!(None, actual);
    }

    // resolve_goto tests (ADR 0022): the 0/1/many candidate resolution that
    // needs `report`, extracted so it is unit-testable without a live
    // terminal (this function's own doc comment).

    fn report_with_symbols_and_edges(
        symbols_by_file: Vec<(&str, Vec<&str>)>,
        edges: Vec<(&str, &str)>,
    ) -> Report {
        use rinkaku_core::diff::LineRange;
        use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
        use rinkaku_core::graph::{Edge, Node, SymbolGraph};
        use rinkaku_core::render::FileReport;

        let files: Vec<FileReport> = symbols_by_file
            .iter()
            .map(|(path, names)| FileReport {
                path: path.to_string(),
                symbols: names
                    .iter()
                    .map(|name| ExtractedSymbol {
                        id: format!("{path}::{name}"),
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        signature: format!("fn {name}()"),
                        range: LineRange { start: 1, end: 1 },
                        container: None,
                        referenced_names: vec![],
                        dependencies: vec![],
                        omitted_dependency_matches: 0,
                        is_test: false,
                        classification: None,
                        previous_signature: None,
                    })
                    .collect(),
            })
            .collect();

        let nodes: Vec<Node> = symbols_by_file
            .iter()
            .flat_map(|(path, names)| {
                names.iter().map(move |name| Node {
                    id: format!("{path}::{name}"),
                    path: path.to_string(),
                    name: name.to_string(),
                })
            })
            .collect();

        let graph_edges: Vec<Edge> = edges
            .into_iter()
            .map(|(from, to)| Edge {
                from: from.to_string(),
                to: to.to_string(),
                is_cycle: false,
            })
            .collect();

        Report {
            files,
            graph: SymbolGraph {
                nodes,
                edges: graph_edges,
                roots: vec![],
            },
            ..empty_report()
        }
    }

    #[test]
    fn should_return_no_symbol_selected_when_cursor_is_not_on_a_symbol_row() {
        let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
        let app = App::new(&report); // cursor on "lib.rs" (a File row)

        let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

        assert_eq!(GotoOutcome::NoSymbolSelected, actual);
    }

    #[test]
    fn should_return_no_candidates_when_selected_symbol_has_no_callees() {
        let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
        let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

        let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

        assert_eq!(GotoOutcome::NoCandidates("callees"), actual);
    }

    #[test]
    fn should_return_one_candidate_when_selected_symbol_has_exactly_one_callee() {
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar"])],
            vec![("lib.rs::foo", "lib.rs::bar")],
        );
        let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

        let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

        assert_eq!(
            GotoOutcome::One(candidate("lib.rs::bar", "bar", "lib.rs")),
            actual
        );
    }

    #[test]
    fn should_return_many_candidates_when_selected_symbol_has_multiple_callees() {
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar", "baz"])],
            vec![
                ("lib.rs::foo", "lib.rs::bar"),
                ("lib.rs::foo", "lib.rs::baz"),
            ],
        );
        let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

        let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

        assert_eq!(
            GotoOutcome::Many(vec![
                candidate("lib.rs::bar", "bar", "lib.rs"),
                candidate("lib.rs::baz", "baz", "lib.rs"),
            ]),
            actual
        );
    }

    #[test]
    fn should_resolve_callers_direction_when_goto_references_is_requested() {
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar"])],
            vec![("lib.rs::foo", "lib.rs::bar")],
        );
        // Cursor on "bar" (row 2): "foo" is its one caller.
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down);

        let actual = resolve_goto(&app, &report, InputKey::GotoReferences);

        assert_eq!(
            GotoOutcome::One(candidate("lib.rs::foo", "foo", "lib.rs")),
            actual
        );
    }

    // dispatch_non_source_key regression tests: the `run_app`-equivalent
    // dispatch sequence (as opposed to calling `App::handle_key` directly,
    // which every test above this point does and which was exactly why the
    // `pending_prefix` bug survived until manual/review testing caught it —
    // `App::handle_key`'s own unconditional prefix-clear ran fine in every
    // one of those tests, but `run_app`'s old inline `GotoDefinition`/
    // `GotoReferences` branch skipped calling it in the first place).

    #[test]
    fn should_clear_pending_prefix_so_next_d_toggles_diff_after_a_one_candidate_gd_jump() {
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar"])],
            vec![("lib.rs::foo", "lib.rs::bar")],
        );
        // Cursor on "foo" (row 1): "bar" is its one callee, so `gd` jumps
        // immediately rather than opening the popup.
        let app = App::new(&report).handle_key(InputKey::Down);
        let diff_content = diff_shape::DiffPaneContent::Empty;

        // Simulates the real `g` then `d` key sequence: `translate_key`
        // emits `PendingGoto` for `g`, then (because `pending_prefix` is
        // now set) `GotoDefinition` for the following `d` — both routed
        // through `dispatch_non_source_key`, the same function `run_app`
        // itself calls, rather than `App::handle_key` directly.
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::PendingGoto);
        assert_eq!(Some(app::PendingPrefix::G), app.pending_prefix());
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::GotoDefinition);
        assert_eq!(None, app.pending_prefix(), "gd must clear pending_prefix");

        // The regression itself: a *plain* `d` right after the jump must
        // toggle the right pane (`ToggleDiff`'s own ordinary meaning), not
        // silently re-resolve as another `gd` because `pending_prefix` was
        // still `Some(G)` — `crate::lib::translate_key` only produces
        // `GotoDefinition` for a `d` when `pending_prefix() == Some(G)`, so
        // this assertion on `right_pane()` is an indirect but faithful proxy
        // for "the next `d` meant ToggleDiff, not gd".
        let right_pane_before = app.right_pane();
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::ToggleDiff);
        assert_ne!(
            right_pane_before,
            app.right_pane(),
            "d after gd must toggle the right pane like an ordinary ToggleDiff press"
        );
    }

    #[test]
    fn should_clear_pending_prefix_so_next_d_toggles_diff_after_a_multi_candidate_gr_popup_is_cancelled()
     {
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar", "baz"])],
            vec![
                ("lib.rs::foo", "lib.rs::bar"),
                ("lib.rs::baz", "lib.rs::bar"),
            ],
        );
        // Cursor on "bar" (row 2): both "foo" and "baz" call it, so `gr`
        // opens the popup rather than jumping immediately.
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down);
        let diff_content = diff_shape::DiffPaneContent::Empty;

        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::PendingGoto);
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::GotoReferences);
        assert!(
            app.jump_popup().is_some(),
            "gr with 2 candidates must open the popup"
        );
        assert_eq!(
            None,
            app.pending_prefix(),
            "gr must clear pending_prefix even though it opened a popup instead of jumping"
        );

        // Cancel the popup (Esc) — `App::handle_key`'s own popup-open early
        // return is the second path the #61-review finding flagged: it used
        // to return before the (then-later-positioned) `pending_prefix`
        // clear, so a stale prefix could survive an entire popup session.
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::PopupCancel);
        assert_eq!(None, app.jump_popup());
        assert_eq!(None, app.pending_prefix());

        // Same regression check as the single-candidate test above: a plain
        // `d` after the cancelled popup must toggle the right pane, not
        // silently re-resolve as another `gr`.
        let right_pane_before = app.right_pane();
        let app = dispatch_non_source_key(app, &report, &diff_content, InputKey::ToggleDiff);
        assert_ne!(
            right_pane_before,
            app.right_pane(),
            "d after a cancelled gr popup must toggle the right pane like an ordinary ToggleDiff press"
        );
    }

    #[test]
    fn should_restore_the_scroll_offset_the_reviewer_was_at_when_jumping_back_after_gd() {
        // Independent-review finding: `dispatch_non_source_key` always calls
        // `app.handle_key(GotoDefinition)` first (for the `pending_prefix`
        // clear — see the test group above), and before this fix that call
        // hit `App::handle_key`'s own blanket scroll reset before
        // `App::jump_to_symbol` ever read `right_pane_scroll` to save it into
        // the jumplist entry — so every jumplist entry's saved scroll was
        // always 0 and `Ctrl-o` could never restore a real reading position.
        //
        // This test drives the *real* two-key `g` then `d` sequence
        // (`InputKey::PendingGoto` then `InputKey::GotoDefinition`), each
        // through `dispatch_non_source_key` — not `App::handle_key` directly,
        // and not `GotoDefinition` alone. An earlier version of this test did
        // call `GotoDefinition` alone and passed while the underlying bug was
        // still only half-fixed: `PendingGoto` (the leading `g`) is also
        // dispatched through `handle_key` on its own, one keypress *before*
        // `GotoDefinition`, and its own blanket scroll reset zeroed
        // `right_pane_scroll` before `d` was even pressed — a gap only a
        // real terminal run surfaced (see `InputKey::PendingGoto`'s own doc
        // comment). Scrolls to a nonzero offset, jumps via the real `gd` key
        // sequence, then jumps back via `Ctrl-o` (`InputKey::JumpBack`) and
        // asserts the original scroll offset is restored rather than 0.
        let report = report_with_symbols_and_edges(
            vec![("lib.rs", vec!["foo", "bar"])],
            vec![("lib.rs::foo", "lib.rs::bar")],
        );
        let diff_content = diff_shape::DiffPaneContent::Empty;

        // Cursor on "foo" (row 1), scrolled 5 lines into its Diff pane.
        let mut app = App::new(&report).handle_key(InputKey::Down);
        app = dispatch_non_source_key(app, &report, &diff_content, InputKey::Open); // focus -> Right, RightPane::Diff
        for _ in 0..5 {
            app = dispatch_non_source_key(app, &report, &diff_content, InputKey::Down);
        }
        assert_eq!(5, app.right_pane_scroll());

        // The real `gd` key sequence: `g` (PendingGoto) then `d`
        // (GotoDefinition) — "bar" is "foo"'s one callee, so this jumps
        // immediately (`GotoOutcome::One`) rather than opening the popup.
        app = dispatch_non_source_key(app, &report, &diff_content, InputKey::PendingGoto);
        assert_eq!(
            5,
            app.right_pane_scroll(),
            "the leading g of gd must not disturb scroll either"
        );
        app = dispatch_non_source_key(app, &report, &diff_content, InputKey::GotoDefinition);
        assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
        assert_eq!(
            0,
            app.right_pane_scroll(),
            "the new target's own scroll must start at 0 (App::jump_to_symbol's own reset)"
        );

        // Ctrl-o: jump back to "foo" — the regression this test guards.
        app = dispatch_non_source_key(app, &report, &diff_content, InputKey::JumpBack);

        assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
        assert_eq!(
            5,
            app.right_pane_scroll(),
            "jumping back must restore the scroll offset recorded when gd was pressed, not 0"
        );
    }
}
