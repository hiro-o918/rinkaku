//! Tests for `crate::event_loop`'s own dispatch machinery: the per-frame
//! recompute gates (`should_recompute_diff_pane_content`,
//! `should_recompute_blast_radius_selection`), the source-cache reload gate
//! (`should_reload_source_content`), the scroll-key classifier
//! (`is_scroll_input_key`), and `dispatch_non_source_key`'s `gd`/`gr`
//! pending-prefix/jumplist regression coverage. `resolve_goto` and
//! `apply_diff_pane_selection_effects`/`sync_target_for_scroll`/
//! `should_apply_hunk_jump`/`jump_scroll_target` have their own test trees
//! in the sibling `goto`/`scroll_sync` submodules.

use crate::app::{self, App, InputKey};
use crate::diff_shape;
use crate::event_loop::{dispatch_non_source_key, is_scroll_input_key};
use pretty_assertions::assert_eq;
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::Report;

pub(super) fn empty_report() -> Report {
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
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    }
}

pub(super) fn report_with_one_symbol() -> Report {
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

pub(super) fn report_with_symbols_and_edges(
    symbols_by_file: Vec<(&str, Vec<&str>)>,
    edges: Vec<(&str, &str)>,
) -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::{Edge, SymbolGraph};
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

    let nodes: Vec<rinkaku_core::graph::Node> = symbols_by_file
        .iter()
        .flat_map(|(path, names)| {
            names.iter().map(move |name| rinkaku_core::graph::Node {
                id: format!("{path}::{name}"),
                path: path.to_string(),
                name: name.to_string(),
                is_test: false,
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

pub(super) fn dummy_view(path: &str) -> crate::source::HighlightedSourceView {
    crate::source::HighlightedSourceView {
        view: crate::source::SourceView {
            path: path.to_string(),
            lines: vec![],
            highlight_start: 1,
            highlight_end: 1,
        },
        token_highlights: vec![],
    }
}

// --- should_recompute_blast_radius_selection ---
//
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

    let actual = crate::event_loop::should_recompute_blast_radius_selection(&app);

    assert!(actual);
}

#[test]
fn should_not_recompute_blast_radius_selection_when_right_pane_is_detail() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = crate::event_loop::should_recompute_blast_radius_selection(&app);

    assert!(!actual);
}

#[test]
fn should_not_recompute_blast_radius_selection_when_right_pane_is_diff() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleDiff);

    let actual = crate::event_loop::should_recompute_blast_radius_selection(&app);

    assert!(!actual);
}

#[test]
fn should_not_recompute_blast_radius_selection_while_source_screen_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleBlastRadius)
        .handle_key(InputKey::Source);

    let actual = crate::event_loop::should_recompute_blast_radius_selection(&app);

    assert!(!actual);
}

// --- should_recompute_diff_pane_content ---

#[test]
fn should_recompute_diff_pane_content_when_diff_pane_is_active_on_entry_screen() {
    let report = empty_report();
    let app = App::new(&report);
    assert_eq!(app::RightPane::Diff, app.right_pane()); // ADR 0020 default

    let actual = crate::event_loop::should_recompute_diff_pane_content(&app);

    assert!(actual);
}

#[test]
fn should_not_recompute_diff_pane_content_when_right_pane_is_detail() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleDiff);

    let actual = crate::event_loop::should_recompute_diff_pane_content(&app);

    assert!(!actual);
}

#[test]
fn should_not_recompute_diff_pane_content_when_right_pane_is_blast_radius() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleBlastRadius);

    let actual = crate::event_loop::should_recompute_diff_pane_content(&app);

    assert!(!actual);
}

#[test]
fn should_not_recompute_diff_pane_content_while_source_screen_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);

    let actual = crate::event_loop::should_recompute_diff_pane_content(&app);

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

#[test]
fn should_reload_source_content_when_cache_is_empty() {
    let actual = crate::event_loop::should_reload_source_content(None, None, "src/lib.rs::foo");

    assert!(actual);
}

#[test]
fn should_reload_source_content_when_cached_symbol_differs() {
    let cached = Ok(dummy_view("src/lib.rs"));

    let actual = crate::event_loop::should_reload_source_content(
        Some("src/lib.rs::foo"),
        Some(&cached),
        "src/lib.rs::bar",
    );

    assert!(actual);
}

#[test]
fn should_skip_reload_when_cached_ok_matches_next_symbol() {
    let cached = Ok(dummy_view("src/lib.rs"));

    let actual = crate::event_loop::should_reload_source_content(
        Some("src/lib.rs::foo"),
        Some(&cached),
        "src/lib.rs::foo",
    );

    assert!(!actual);
}

#[test]
fn should_reload_source_content_when_cached_err_even_for_same_symbol() {
    // Retryability contract: a failed load remains retryable on the
    // reviewer's next `s` press (e.g. after editing the file back into
    // existence), so a cached `Err(_)` must never suppress the reload —
    // the small parse cost is worth keeping the recovery gesture live.
    let cached: Result<crate::source::HighlightedSourceView, String> =
        Err("failed to read".to_string());

    let actual = crate::event_loop::should_reload_source_content(
        Some("src/lib.rs::foo"),
        Some(&cached),
        "src/lib.rs::foo",
    );

    assert!(actual);
}

#[test]
fn should_reload_source_content_when_cache_has_symbol_but_no_content() {
    // Defensive combination the loop never actually reaches today
    // (`source_content` and `source_content_symbol` are always written
    // together): if they ever fall out of sync, the safe default is to
    // reload rather than trust the stale symbol id alone.
    let actual = crate::event_loop::should_reload_source_content(
        Some("src/lib.rs::foo"),
        None,
        "src/lib.rs::foo",
    );

    assert!(actual);
}

// --- is_scroll_input_key ---

#[test]
fn should_treat_the_four_adr_0026_scroll_variants_as_scroll_input_keys() {
    for key in [
        InputKey::ScrollHalfPageDown,
        InputKey::ScrollHalfPageUp,
        InputKey::ScrollToTop,
        InputKey::ScrollToBottom,
    ] {
        assert!(is_scroll_input_key(key), "{key:?} should be a scroll key");
    }
}

#[test]
fn should_not_treat_up_or_down_as_scroll_input_keys() {
    // `Up`/`Down` scroll the help overlay too, but through the ordinary
    // `dispatch_non_source_key` path (`App::handle_key`'s own
    // `help_open` branch), not the two-step `handle_scroll_key`
    // dispatch reserved for the four ADR 0026 variants — this pins
    // that boundary stays where `run_app`'s own dispatch expects it.
    assert!(!is_scroll_input_key(InputKey::Up));
    assert!(!is_scroll_input_key(InputKey::Down));
}

// --- dispatch_non_source_key regression tests: the `run_app`-equivalent
// dispatch sequence (as opposed to calling `App::handle_key` directly,
// which was exactly why the `pending_prefix` bug survived until manual/
// review testing caught it — `App::handle_key`'s own unconditional
// prefix-clear ran fine in every isolated test, but `run_app`'s old
// inline `GotoDefinition`/`GotoReferences` branch skipped calling it in
// the first place). ---

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
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    assert_eq!(Some(app::PendingPrefix::G), app.pending_prefix());
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoDefinition,
        app::DiffViewMode::Split,
    );
    assert_eq!(None, app.pending_prefix(), "gd must clear pending_prefix");

    // The regression itself: a *plain* `d` right after the jump must
    // toggle the right pane (`ToggleDiff`'s own ordinary meaning), not
    // silently re-resolve as another `gd` because `pending_prefix` was
    // still `Some(G)` — `crate::input_translate::translate_key` only
    // produces `GotoDefinition` for a `d` when
    // `pending_prefix() == Some(G)`, so this assertion on `right_pane()`
    // is an indirect but faithful proxy for "the next `d` meant
    // ToggleDiff, not gd".
    let right_pane_before = app.right_pane();
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::ToggleDiff,
        app::DiffViewMode::Split,
    );
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

    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoReferences,
        app::DiffViewMode::Split,
    );
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
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PopupCancel,
        app::DiffViewMode::Split,
    );
    assert_eq!(None, app.jump_popup());
    assert_eq!(None, app.pending_prefix());

    // Same regression check as the single-candidate test above: a plain
    // `d` after the cancelled popup must toggle the right pane, not
    // silently re-resolve as another `gr`.
    let right_pane_before = app.right_pane();
    let app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::ToggleDiff,
        app::DiffViewMode::Split,
    );
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
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::Open,
        app::DiffViewMode::Split,
    ); // focus -> Right, RightPane::Diff
    for _ in 0..5 {
        app = dispatch_non_source_key(
            app,
            &report,
            &diff_content,
            InputKey::Down,
            app::DiffViewMode::Split,
        );
    }
    assert_eq!(5, app.right_pane_scroll());

    // The real `gd` key sequence: `g` (PendingGoto) then `d`
    // (GotoDefinition) — "bar" is "foo"'s one callee, so this jumps
    // immediately (`GotoOutcome::One`) rather than opening the popup.
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::PendingGoto,
        app::DiffViewMode::Split,
    );
    assert_eq!(
        5,
        app.right_pane_scroll(),
        "the leading g of gd must not disturb scroll either"
    );
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::GotoDefinition,
        app::DiffViewMode::Split,
    );
    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
    assert_eq!(
        0,
        app.right_pane_scroll(),
        "the new target's own scroll must start at 0 (App::jump_to_symbol's own reset)"
    );

    // Ctrl-o: jump back to "foo" — the regression this test guards.
    app = dispatch_non_source_key(
        app,
        &report,
        &diff_content,
        InputKey::JumpBack,
        app::DiffViewMode::Split,
    );

    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
    assert_eq!(
        5,
        app.right_pane_scroll(),
        "jumping back must restore the scroll offset recorded when gd was pressed, not 0"
    );
}
