//! Tests for `crate::event_loop::scroll_sync`: `should_apply_hunk_jump`/
//! `jump_scroll_target` (the `]`/`[` hunk-jump dispatch), `clamp_*_after_draw`
//! (post-draw scroll fold-back), and `sync_target_for_scroll`/
//! `apply_diff_pane_selection_effects` (ADR 0030's diff-scroll -> tree-cursor
//! auto-sync and the feedback-loop guard between it and ADR 0027's
//! tree -> diff auto-scroll, both narrowed by ADR 0072 to work against a
//! flat, original-order hunk list rather than per-symbol sections).

use super::{
    apply_diff_pane_selection_effects, clamp_help_scroll_after_draw,
    clamp_right_pane_scroll_after_draw, jump_scroll_target, should_apply_hunk_jump,
    sync_target_for_scroll,
};
use crate::app::{self, App, DiffViewMode, InputKey};
use crate::event_loop::tests::{empty_report, report_with_one_symbol};
use crate::locale::Locale;
use crate::{diff_shape, diff_view};
use pretty_assertions::assert_eq;
use rinkaku_core::render::Report;

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

// --- jump_scroll_target ---

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

// --- clamp_help_scroll_after_draw ---
//
// Same fold-back discipline as `clamp_right_pane_scroll_after_draw`
// above, applied to the `?` help overlay's own independent scroll
// state (this feature).

#[test]
fn should_overwrite_help_scroll_with_the_clamped_value_when_some() {
    let report = empty_report();
    let app = App::new(&report).with_help_scroll(999);

    let app = clamp_help_scroll_after_draw(app, Some(4));

    assert_eq!(4, app.help_scroll());
}

#[test]
fn should_leave_help_scroll_untouched_when_none() {
    let report = empty_report();
    let app = App::new(&report).with_help_scroll(2);

    let app = clamp_help_scroll_after_draw(app, None);

    assert_eq!(2, app.help_scroll());
}

// --- sync_target_for_scroll (ADR 0030, ADR 0072) ---

fn report_with_two_symbols() -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::render::FileReport;

    fn symbol(id: &str, name: &str, range: LineRange) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range,
            container: None,
            referenced_names: vec![],
            referenced_method_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 }),
                symbol("lib.rs::bar", "bar", LineRange { start: 10, end: 11 }),
            ],
        }],
        ..empty_report()
    }
}

/// Two-hunk [`diff_shape::DiffPaneContent`] matching `report_with_two_symbols`'s
/// two symbols: hunk 0 (`foo`, new-side 1-2) spans lines 0-1, hunk 1
/// (`bar`, new-side 10-11) starts at line 3 (header(0), 1 body line(1),
/// blank(2), header(3)).
fn diff_pane_content_with_two_hunks() -> diff_shape::DiffPaneContent {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    fn hunk(header: &str, new_range: (usize, usize), line: &str) -> Hunk {
        Hunk {
            header: header.to_string(),
            new_range: Some(new_range),
            lines: vec![DiffLine {
                kind: DiffLineKind::Context,
                content: line.to_string(),
            }],
        }
    }

    diff_shape::DiffPaneContent::File(vec![
        diff_shape::AttributedHunk {
            source_index: 0,
            hunk: hunk("@@ -1,1 +1,2 @@", (1, 2), "fn foo() {}"),
        },
        diff_shape::AttributedHunk {
            source_index: 1,
            hunk: hunk("@@ -10,1 +10,2 @@", (10, 11), "fn bar() {}"),
        },
    ])
}

/// `App` on `report_with_two_symbols`'s `foo` symbol row, already
/// `Focus::Right` on `RightPane::Diff` (`Open` reaches both at once,
/// same sequence `should_apply_hunk_jump_when_right_focused_on_diff_pane`
/// uses) and at `right_pane_scroll` set to `scroll` directly, bypassing
/// `handle_key` (these tests exercise `sync_target_for_scroll` standalone,
/// not the dispatch that would normally produce that scroll value).
/// `Down` first (row 0 is `lib.rs`'s file row, matching
/// `should_return_none_selected_symbol_id_when_cursor_is_on_a_file_row`'s
/// own row-shape note — row 1 is `foo`) so `selected_symbol_id()`
/// resolves to `Some("lib.rs::foo")`, matching the diff-pane-content
/// fixture the tests below pair this with.
fn app_focused_on_diff_pane_with_scroll(report: &Report, scroll: usize) -> App {
    App::new(report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open)
        .with_right_pane_scroll(scroll)
}

#[test]
fn should_return_none_when_scroll_did_not_change_this_key() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_hunks();
    // scroll_before_dispatch == current scroll: this key's dispatch
    // did not move right_pane_scroll at all (e.g. Enter, d, an
    // unrelated no-op), so there is nothing to sync regardless of
    // which symbol the unchanged offset happens to point at.
    let app = app_focused_on_diff_pane_with_scroll(&report, 1);

    let actual = sync_target_for_scroll(&app, &report, &content, 1, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_tree_is_focused_even_if_scroll_changed() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_hunks();
    let app = App::new(&report).with_right_pane_scroll(3);
    assert_eq!(app::Focus::Tree, app.focus());

    let actual = sync_target_for_scroll(&app, &report, &content, 0, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_right_pane_is_not_diff_even_if_focus_is_right() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_hunks();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::ToggleDiff)
        .with_right_pane_scroll(3);
    assert_eq!(app::RightPane::Detail, app.right_pane());

    let actual = sync_target_for_scroll(&app, &report, &content, 0, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_bar_when_scroll_moved_into_bars_hunk() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_hunks();
    // Cursor still on `foo` (row 0); scroll moved from foo's hunk body
    // (line 1) into bar's hunk body (line 4).
    let app = app_focused_on_diff_pane_with_scroll(&report, 4);

    let actual = sync_target_for_scroll(&app, &report, &content, 1, DiffViewMode::Unified);

    assert_eq!(Some("lib.rs::bar".to_string()), actual);
}

#[test]
fn should_return_none_when_scroll_moved_but_stayed_within_the_current_symbols_hunk() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_hunks();
    // Cursor on `foo`; scroll moved from header(0) to body(1), both
    // still inside foo's own hunk — nothing to sync.
    let app = app_focused_on_diff_pane_with_scroll(&report, 1);

    let actual = sync_target_for_scroll(&app, &report, &content, 0, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_scroll_moved_into_a_hunk_intersecting_no_symbol() {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    let report = report_with_two_symbols();
    let content = diff_shape::DiffPaneContent::File(vec![
        diff_shape::AttributedHunk {
            source_index: 0,
            hunk: Hunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                new_range: Some((1, 2)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: "fn foo() {}".to_string(),
                }],
            },
        },
        diff_shape::AttributedHunk {
            source_index: 1,
            hunk: Hunk {
                header: "@@ -20,1 +20,2 @@".to_string(),
                new_range: Some((20, 21)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: "use foo::bar;".to_string(),
                }],
            },
        },
    ]);
    // Second hunk's body line is at line 4 (header(0), body(1), blank(2),
    // header(3), body(4)).
    let app = app_focused_on_diff_pane_with_scroll(&report, 4);

    let actual = sync_target_for_scroll(&app, &report, &content, 1, DiffViewMode::Unified);

    assert_eq!(None, actual);
}

#[test]
fn should_move_the_tree_cursor_to_bar_when_synced() {
    let report = report_with_two_symbols();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());

    let app = app.sync_tree_cursor_to_symbol("lib.rs::bar");

    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
}

#[test]
fn should_preserve_right_pane_scroll_when_syncing_tree_cursor() {
    // The whole point of `sync_tree_cursor_to_symbol` over
    // `jump_to_symbol`: the scroll offset that triggered the sync must
    // survive it, or the sync would fight its own trigger.
    let report = report_with_two_symbols();
    let app = app_focused_on_diff_pane_with_scroll(&report, 5);

    let app = app.sync_tree_cursor_to_symbol("lib.rs::bar");

    assert_eq!(5, app.right_pane_scroll());
}

#[test]
fn should_leave_cursor_untouched_when_syncing_to_a_symbol_id_with_no_matching_row() {
    let report = report_with_two_symbols();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());

    let app = app.sync_tree_cursor_to_symbol("lib.rs::nonexistent");

    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
}

// --- apply_diff_pane_selection_effects (ADR 0030 decision 6: the
// feedback-loop guard) ---

/// `diff_view::FileHunks` for `lib.rs` matching `report_with_two_symbols`'s
/// two symbol ranges (`foo`: lines 1-2, `bar`: lines 10-11), so
/// `apply_diff_pane_selection_effects`'s own internal
/// `build_diff_pane_content` call produces the same two-hunk shape
/// `diff_pane_content_with_two_hunks` hand-builds for the standalone
/// `sync_target_for_scroll` tests above — this fixture feeds the *real*
/// pipeline instead, since this test exercises the actual sequencing
/// `crate::event_loop::run_app`'s loop performs, not a hand-shaped
/// content value.
fn diff_hunks_with_two_symbol_hunks() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            Hunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                new_range: Some((1, 2)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: "fn foo() {}".to_string(),
                }],
            },
            Hunk {
                header: "@@ -10,1 +10,2 @@".to_string(),
                new_range: Some((10, 11)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: "fn bar() {}".to_string(),
                }],
            },
        ],
    }]
}

#[test]
fn should_sync_tree_cursor_when_scroll_moves_into_a_different_symbols_hunk() {
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_hunks();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    let last_diff_focus = app.selected_diff_focus(&report);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());

    // Simulates scrolling `right_pane_scroll` from 0 (foo's hunk header)
    // to 4 (bar's hunk body, same layout math as the standalone
    // `sync_target_for_scroll` tests).
    let scroll_before_dispatch = app.right_pane_scroll();
    let app = app.with_right_pane_scroll(4);
    let effects = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        scroll_before_dispatch,
        DiffViewMode::Unified,
    );

    assert_eq!(Some("lib.rs::bar"), effects.app.selected_symbol_id());
    // The scroll itself must survive the sync unchanged — the whole
    // point of `App::sync_tree_cursor_to_symbol` over `jump_to_symbol`.
    assert_eq!(4, effects.app.right_pane_scroll());
}

#[test]
fn should_not_bounce_scroll_back_on_the_next_key_after_a_sync() {
    // ADR 0030 decision 6's own regression test: without
    // `apply_diff_pane_selection_effects` updating `last_diff_focus` to
    // the *post-sync* focus, a second handled key right after the sync
    // would see `selected_diff_focus` (now bar) differ from a stale
    // `last_diff_focus` (still foo), misread that as a fresh
    // cursor-driven selection change, and auto-scroll `right_pane_scroll`
    // straight back to bar's own hunk start — undoing whatever the second
    // key's own scroll motion was trying to do.
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_hunks();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    let last_diff_focus = app.selected_diff_focus(&report);

    // First key: scroll from 0 to 4, syncing the cursor onto `bar`
    // (previous test's own scenario).
    let scroll_before_first_key = app.right_pane_scroll();
    let app = app.with_right_pane_scroll(4);
    let first = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        scroll_before_first_key,
        DiffViewMode::Unified,
    );
    assert_eq!(Some("lib.rs::bar"), first.app.selected_symbol_id());
    assert_eq!(4, first.app.right_pane_scroll());

    // Second key: scroll from 4 to bar's own hunk header (3), still
    // inside bar's span per the two-hunk fixture's layout (bar spans
    // lines 3-4). If `last_diff_focus` were stale (still pointing at
    // `foo` instead of the post-sync `bar`), this call would
    // misinterpret the *unchanged* cursor position as a fresh selection
    // change and auto-scroll back to bar's hunk start (3), which happens
    // to coincide here — so this test uses a scroll value inside bar's
    // span but different from its start (4, unchanged) to actually
    // distinguish the two behaviors.
    let scroll_before_second_key = first.app.right_pane_scroll();
    let app = first.app.with_right_pane_scroll(4);
    let second = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        first.last_diff_focus,
        scroll_before_second_key,
        DiffViewMode::Unified,
    );

    assert_eq!(4, second.app.right_pane_scroll());
    assert_eq!(Some("lib.rs::bar"), second.app.selected_symbol_id());
}

// --- apply_diff_pane_selection_effects (re-entering RightPane::Diff with
// an unchanged cursor: `run_app`'s loop resets `last_diff_focus` to
// `None` while the right pane is not Diff, so re-entry looks like a
// fresh selection to the ADR 0027 auto-scroll branch below) ---

#[test]
fn should_resync_scroll_to_current_symbols_hunk_when_diff_pane_is_reentered_with_cursor_unchanged()
{
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_hunks();
    // Cursor on `bar` (row 2), already inside bar's own hunk — the
    // *symbol* did not change, only the right pane's visibility did.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open);
    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());

    // `last_diff_focus: None` and `scroll_before_dispatch: 0` are what
    // `run_app`'s loop passes in after a Diff -> Detail -> Diff toggle
    // with the cursor untouched, per the fix.
    let app = app.with_right_pane_scroll(0);
    let effects = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        None,
        0,
        DiffViewMode::Unified,
    );

    // bar's hunk starts at line 3 (same layout as every other test in
    // this file); landing at 0 would show foo's hunk under a pinned
    // header that still names `bar` — the mismatch the resync exists to
    // prevent.
    assert_eq!(3, effects.app.right_pane_scroll());
    assert_eq!(Some("lib.rs::bar"), effects.app.selected_symbol_id());
}

// --- apply_diff_pane_selection_effects, driven end-to-end through the
// draw+clamp pipeline ---

/// Mirrors one iteration of `crate::run_app`'s loop: dispatch + sync +
/// draw + post-draw fold-back. Caller must size the viewport smaller than
/// the pane's content — if it fits, `crate::ui::clamp_scroll` pins scroll
/// to 0 regardless of the requested target and any regression in
/// `apply_diff_pane_selection_effects`'s target computation is invisible.
fn dispatch_draw_and_fold(
    mut app: App,
    report: &Report,
    diff_hunks: &[diff_view::FileHunks],
    last_diff_focus: Option<app::DiffFocus>,
    input_key: InputKey,
    width: u16,
    height: u16,
) -> App {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let scroll_before_dispatch = app.right_pane_scroll();
    app = app.handle_key(input_key);
    let effects = apply_diff_pane_selection_effects(
        app,
        report,
        diff_hunks,
        last_diff_focus,
        scroll_before_dispatch,
        DiffViewMode::Unified,
    );
    let app = effects.app;
    let diff_pane_content = effects.diff_pane_content;

    let diff_highlights = crate::highlight::highlight_diff_files(diff_hunks);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let mut outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            outcome = crate::ui::draw(
                frame,
                &app,
                report,
                &diff_pane_content,
                &diff_highlights,
                &app::BlastRadiusSelection::NotApplicable,
                None,
                diff_hunks,
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll)
}

#[test]
fn should_scroll_into_the_second_symbols_hunk_when_cursor_moves_past_a_wide_first_hunk() {
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_hunks();
    let app = App::new(&report).handle_key(InputKey::Down);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
    let last_diff_focus = app.selected_diff_focus(&report);

    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        160,
        10,
    );

    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
    let expected_scroll = diff_shape::scroll_target_line_for_symbol(
        &diff_shape::build_diff_pane_content(
            &report,
            &diff_hunks,
            app.selected_diff_target(&report).as_ref(),
        ),
        rinkaku_core::diff::LineRange { start: 10, end: 11 },
        DiffViewMode::Unified,
    )
    .expect("bar's hunk start must resolve");
    assert_eq!(expected_scroll, app.right_pane_scroll());
}

/// `report_with_two_symbols`'s two symbols packed into a *single* hunk —
/// the shape a whole new file (`@@ -0,0 +1,n @@`) always takes, and the one
/// a hunk with generous context routinely takes for adjacent definitions.
fn diff_hunks_with_one_shared_hunk() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -0,0 +1,11 @@".to_string(),
            new_range: Some((1, 11)),
            lines: (1..=11)
                .map(|line_number| DiffLine {
                    kind: DiffLineKind::Added,
                    content: format!("line {line_number}"),
                })
                .collect(),
        }],
    }]
}

#[test]
fn should_scroll_to_the_second_symbols_own_row_when_both_symbols_share_one_hunk() {
    // ADR 0074's end-to-end regression pin. Under ADR 0072's whole-hunk
    // rule both symbols resolved to the shared hunk's header row, so
    // moving the tree cursor from `foo` to `bar` left the pane exactly
    // where it was — the diff pane stopped following the signature list
    // for every file whose changes arrive as one hunk.
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_one_shared_hunk();
    let app = App::new(&report).handle_key(InputKey::Down);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());
    let last_diff_focus = app.selected_diff_focus(&report);

    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        160,
        10,
    );

    // `bar` covers new-side lines 10-11; row 0 is the `@@` header and rows
    // 1..=11 are new-side lines 1..=11, so bar's first row is row 10.
    assert_eq!(Some("lib.rs::bar"), app.selected_symbol_id());
    assert_eq!(10, app.right_pane_scroll());
}

#[test]
fn should_sync_tree_cursor_to_the_second_symbol_when_scroll_moves_within_one_shared_hunk() {
    // The mirror image of the test above: with only whole-hunk resolution,
    // every row of a shared hunk resolved to its first symbol, so scrolling
    // through `bar`'s half of the hunk left the tree cursor stuck on `foo`.
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_one_shared_hunk();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    let last_diff_focus = app.selected_diff_focus(&report);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());

    let app = app.with_right_pane_scroll(10);
    let effects = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        0,
        DiffViewMode::Unified,
    );

    assert_eq!(Some("lib.rs::bar"), effects.app.selected_symbol_id());
}
