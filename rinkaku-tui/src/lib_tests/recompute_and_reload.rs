//! Per-frame recompute-gate tests for the entry-screen right pane:
//! `should_recompute_diff_pane_content` (Diff pane) and
//! `should_recompute_blast_radius_selection` (blast-radius pane), plus the
//! source-cache reload gate `should_reload_source_content` used by the
//! Source screen's `s`-press handler.

use super::{dummy_view, empty_report, report_with_one_symbol};
use crate::app::{self, App, InputKey};
use crate::source;
use crate::{
    should_recompute_blast_radius_selection, should_recompute_diff_pane_content,
    should_reload_source_content,
};

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
    let cached: Result<source::HighlightedSourceView, String> = Err("failed to read".to_string());

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
