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
pub mod detail;
pub mod diff_view;
pub mod highlight;
pub mod nav;
pub mod order;
pub mod pivot;
pub mod row_view;
pub mod source;
pub mod tree;
pub mod ui;

use app::{App, InputKey, PivotSelection, Screen};
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
/// straight into [`app::RightPane::Pivot`] with the cursor already on the
/// matching tree row (`App::with_entry_pivot`) instead of requiring the
/// reviewer to hunt for the row and press `p` themselves. Note this crate
/// does *not* itself re-root `report.graph` — `main.rs` already applied
/// `--entry`'s `pivot_graph` re-rooting to `report` before calling here (the
/// same `Report` both the TUI and Markdown/JSON render from), so this
/// parameter only drives where the TUI *starts*, not what the underlying
/// graph looks like.
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
    // unlike `diff_hunks`/`diff_highlights` above, the pivot view depends on
    // `app`'s cursor position and right-pane mode, both of which change as
    // keys are handled) and cached here across idle poll ticks for the same
    // reason those two are computed outside the draw loop at all: `ui::draw`
    // itself runs on every ~100ms idle poll timeout, not just on an actual
    // key press, and `crate::pivot::build_pivot_view` is an O(V+E) graph
    // walk — recomputing it on every one of those idle ticks while the
    // pivot pane merely sits on screen was the per-frame recompute bug this
    // cache exists to fix (`App::selected_pivot_view`'s own doc comment).
    // The up-front computation (rather than starting at `NotApplicable`
    // unconditionally) matters specifically for `--entry --tui`: when
    // `entry_path` above already opened `RightPane::Pivot`, the very first
    // frame must show the pivot tree immediately, not an empty placeholder
    // until the first key press recomputes it.
    let mut pivot_selection = if should_recompute_pivot_selection(&app) {
        app.selected_pivot_view(report)
    } else {
        PivotSelection::NotApplicable
    };

    loop {
        terminal.draw(|frame| {
            ui::draw(
                frame,
                &app,
                report,
                &diff_hunks,
                &diff_highlights,
                &pivot_selection,
                repo_root,
            )
        })?;

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
                if let Screen::Source { symbol_id } = app.screen().clone() {
                    match source::load_symbol_source(report, &symbol_id, repo_root) {
                        // The `SourceView` itself is discarded here — only
                        // used to detect a failure early so it can be
                        // surfaced on the status line right away, rather
                        // than silently on the next redraw. `ui::draw`'s
                        // `draw_source_screen` re-reads the file itself
                        // when it renders the screen (see that function's
                        // doc comment for why it re-reads instead of
                        // caching this result).
                        Ok(_) => {}
                        Err(message) => app.set_status(message),
                    }
                }
            } else {
                app = app.handle_key(input_key);
            }

            if should_recompute_pivot_selection(&app) {
                pivot_selection = app.selected_pivot_view(report);
            }
        }
    }
}

/// Whether `crate::run_app`'s event loop should recompute the pivot
/// selection this key, rather than keep showing the previously cached one
/// (this function's own extraction is what makes that decision
/// unit-testable without a live `ratatui::DefaultTerminal` — `run_app`
/// itself takes one and so cannot be driven directly in a test). `true`
/// only when the pivot pane is actually the active right pane on the entry
/// screen; every other key/screen combination leaves the cached value
/// untouched rather than resetting it to `NotApplicable`, so switching away
/// from and back to the pivot pane (e.g. `p` -> `d` -> `p`) does not need a
/// wasted recompute on the `d` press that briefly leaves it.
fn should_recompute_pivot_selection(app: &App) -> bool {
    matches!(app.screen(), Screen::Entry) && app.right_pane() == app::RightPane::Pivot
}

/// Translates a raw `crossterm` key press into this crate's
/// terminal-agnostic [`InputKey`], or `None` for a key the app does not
/// react to. Depends on `app.screen()` to disambiguate `q`/Esc (`Quit`/
/// `FocusLeft` on the entry view depending on focus, `Back` on the source
/// view) and on `app.focus()` (ADR 0020) to route Esc between `FocusLeft`
/// and its other meanings — every other mapping is context-free.
fn translate_key(code: KeyCode, modifiers: KeyModifiers, app: &App) -> Option<InputKey> {
    let on_source_screen = matches!(app.screen(), Screen::Source { .. });
    let right_focused = app.focus() == app::Focus::Right;

    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
        // Space always means "expand/collapse", never "drill in" — kept
        // distinct from Enter's own `InputKey::Open` (ADR 0020) so Space on
        // a file/symbol row never moves focus.
        KeyCode::Char(' ') => Some(InputKey::Select),
        KeyCode::Enter => Some(InputKey::Open),
        KeyCode::Char('e') | KeyCode::Char('E') => Some(InputKey::ExpandAll),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::Quit),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(InputKey::CollapseAll),
        KeyCode::Char('o') | KeyCode::Char('O') => Some(InputKey::ToggleOrder),
        KeyCode::Char('d') | KeyCode::Char('D') => Some(InputKey::ToggleDiff),
        KeyCode::Char('p') | KeyCode::Char('P') => Some(InputKey::TogglePivot),
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

    // Regression guard for the per-frame pivot recompute bug: `run_app`
    // used to call `App::selected_pivot_view` from inside `ui::draw`, which
    // runs on every ~100ms idle poll tick, not only on a key press. Pinning
    // `should_recompute_pivot_selection`'s contract (recompute exactly when
    // the pivot pane is the active right pane on the entry screen, and
    // nowhere else) is the closest unit-testable proxy for that fix, since
    // `run_app` itself takes a live `ratatui::DefaultTerminal` and cannot be
    // driven directly in a test.
    #[test]
    fn should_recompute_pivot_selection_when_pivot_pane_is_active_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::TogglePivot);

        let actual = should_recompute_pivot_selection(&app);

        assert!(actual);
    }

    #[test]
    fn should_not_recompute_pivot_selection_when_right_pane_is_detail() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = should_recompute_pivot_selection(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_pivot_selection_when_right_pane_is_diff() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleDiff);

        let actual = should_recompute_pivot_selection(&app);

        assert!(!actual);
    }

    #[test]
    fn should_not_recompute_pivot_selection_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::TogglePivot)
            .handle_key(InputKey::Source);

        let actual = should_recompute_pivot_selection(&app);

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
}
