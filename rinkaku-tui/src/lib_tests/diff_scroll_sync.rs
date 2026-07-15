//! `sync_target_for_scroll` and `apply_diff_pane_selection_effects` tests
//! (ADR 0030): diff-scroll → tree-cursor auto-sync, and the run_app-level
//! dispatch step that applies it plus prevents the feedback loop back
//! into ADR 0027's tree → diff auto-scroll.

use super::empty_report;
use crate::app::{self, App, InputKey};
use crate::{apply_diff_pane_selection_effects, sync_target_for_scroll};
use crate::{diff_shape, diff_view};
use rinkaku_core::render::Report;

// --- sync_target_for_scroll (ADR 0030) ---

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

/// Two-section [`diff_shape::DiffPaneContent`] matching
/// `report_with_two_symbols`'s two symbols, with the same layout math
/// `diff_shape`'s own tests already use: section 0 (`foo`) spans lines
/// 0-4, section 1 (`bar`) starts at line 5.
fn diff_pane_content_with_two_symbol_sections() -> diff_shape::DiffPaneContent {
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
        diff_shape::DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![diff_shape::AttributedHunk {
                source_index: 0,
                hunk: hunk("@@ -1,1 +1,2 @@", (1, 2), "fn foo() {}"),
            }],
        },
        diff_shape::DiffSection {
            title: "fn bar()".to_string(),
            symbol_id: Some("lib.rs::bar".to_string()),
            contract_header: None,
            hunks: vec![diff_shape::AttributedHunk {
                source_index: 1,
                hunk: hunk("@@ -10,1 +10,2 @@", (10, 11), "fn bar() {}"),
            }],
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
    let content = diff_pane_content_with_two_symbol_sections();
    // scroll_before_dispatch == current scroll: this key's dispatch
    // did not move right_pane_scroll at all (e.g. Enter, d, an
    // unrelated no-op), so there is nothing to sync regardless of
    // which symbol the unchanged offset happens to point at.
    let app = app_focused_on_diff_pane_with_scroll(&report, 5);

    let actual = sync_target_for_scroll(&app, &content, 5);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_tree_is_focused_even_if_scroll_changed() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_symbol_sections();
    let app = App::new(&report).with_right_pane_scroll(5);
    assert_eq!(app::Focus::Tree, app.focus());

    let actual = sync_target_for_scroll(&app, &content, 0);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_right_pane_is_not_diff_even_if_focus_is_right() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_symbol_sections();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::ToggleDiff)
        .with_right_pane_scroll(5);
    assert_eq!(app::RightPane::Detail, app.right_pane());

    let actual = sync_target_for_scroll(&app, &content, 0);

    assert_eq!(None, actual);
}

#[test]
fn should_return_bar_when_scroll_moved_into_bars_section() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_symbol_sections();
    // Cursor still on `foo` (row 0); scroll moved from foo's section
    // (line 2) down into bar's section (line 5, its title line).
    let app = app_focused_on_diff_pane_with_scroll(&report, 5);

    let actual = sync_target_for_scroll(&app, &content, 2);

    assert_eq!(Some("lib.rs::bar".to_string()), actual);
}

#[test]
fn should_return_none_when_scroll_moved_but_stayed_within_the_current_symbols_section() {
    let report = report_with_two_symbols();
    let content = diff_pane_content_with_two_symbol_sections();
    // Cursor on `foo`; scroll moved from line 0 to line 2, both still
    // inside foo's own section (0-4) — nothing to sync.
    let app = app_focused_on_diff_pane_with_scroll(&report, 2);

    let actual = sync_target_for_scroll(&app, &content, 0);

    assert_eq!(None, actual);
}

#[test]
fn should_return_none_when_scroll_moved_into_the_module_level_bucket() {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    let report = report_with_two_symbols();
    let content = diff_shape::DiffPaneContent::File(vec![
        diff_shape::DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![diff_shape::AttributedHunk {
                source_index: 0,
                hunk: Hunk {
                    header: "@@ -1,1 +1,2 @@".to_string(),
                    new_range: Some((1, 2)),
                    lines: vec![DiffLine {
                        kind: DiffLineKind::Context,
                        content: "fn foo() {}".to_string(),
                    }],
                },
            }],
        },
        diff_shape::DiffSection {
            title: diff_shape::MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![diff_shape::AttributedHunk {
                source_index: 1,
                hunk: Hunk {
                    header: "@@ -20,1 +20,2 @@".to_string(),
                    new_range: Some((20, 21)),
                    lines: vec![DiffLine {
                        kind: DiffLineKind::Context,
                        content: "use foo::bar;".to_string(),
                    }],
                },
            }],
        },
    ]);
    // Module-level section starts at line 5 (same layout as the
    // two-symbol fixture).
    let app = app_focused_on_diff_pane_with_scroll(&report, 5);

    let actual = sync_target_for_scroll(&app, &content, 2);

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
/// `build_diff_pane_content` call produces the same two-section shape
/// `diff_pane_content_with_two_symbol_sections` hand-builds for the
/// standalone `sync_target_for_scroll` tests above — this fixture feeds
/// the *real* pipeline instead, since this test exercises the actual
/// sequencing `crate::run_app`'s loop performs, not a hand-shaped
/// content value.
fn diff_hunks_with_two_symbol_sections() -> Vec<diff_view::FileHunks> {
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
fn should_sync_tree_cursor_when_scroll_moves_into_a_different_symbols_section() {
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_sections();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    let last_diff_focus = app.selected_diff_focus(&report);
    assert_eq!(Some("lib.rs::foo"), app.selected_symbol_id());

    // Simulates one `Down` (`InputKey::Down`) scroll key while
    // `Focus::Right`: `right_pane_scroll` moves from 0 to 5 (bar's
    // section start, same layout math as the standalone
    // `sync_target_for_scroll` tests), landing inside bar's section.
    let scroll_before_dispatch = app.right_pane_scroll();
    let app = app.with_right_pane_scroll(5);
    let effects = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        scroll_before_dispatch,
    );

    assert_eq!(Some("lib.rs::bar"), effects.app.selected_symbol_id());
    // The scroll itself must survive the sync unchanged — the whole
    // point of `App::sync_tree_cursor_to_symbol` over `jump_to_symbol`.
    assert_eq!(5, effects.app.right_pane_scroll());
}

#[test]
fn should_not_bounce_scroll_back_on_the_next_key_after_a_sync() {
    // ADR 0030 decision 6's own regression test: without
    // `apply_diff_pane_selection_effects` updating `last_diff_focus` to
    // the *post-sync* focus, a second handled key right after the sync
    // would see `selected_diff_focus` (now bar) differ from a stale
    // `last_diff_focus` (still foo), misread that as a fresh
    // cursor-driven selection change, and auto-scroll `right_pane_scroll`
    // straight back to bar's own section start — undoing whatever the
    // second key's own scroll motion was trying to do.
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_sections();
    let app = app_focused_on_diff_pane_with_scroll(&report, 0);
    let last_diff_focus = app.selected_diff_focus(&report);

    // First key: scroll from 0 to 5, syncing the cursor onto `bar`
    // (previous test's own scenario).
    let scroll_before_first_key = app.right_pane_scroll();
    let app = app.with_right_pane_scroll(5);
    let first = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        scroll_before_first_key,
    );
    assert_eq!(Some("lib.rs::bar"), first.app.selected_symbol_id());
    assert_eq!(5, first.app.right_pane_scroll());

    // Second key: scroll one more line further into bar's own section
    // (5 -> 6, still inside bar's span per the two-symbol fixture's
    // layout). If `last_diff_focus` were stale (still pointing at
    // `foo` instead of the post-sync `bar`), this call would
    // misinterpret the *unchanged* cursor position as a fresh
    // selection change and auto-scroll back to bar's section start (5),
    // clobbering the manual scroll to 6.
    let scroll_before_second_key = first.app.right_pane_scroll();
    let app = first.app.with_right_pane_scroll(6);
    let second = apply_diff_pane_selection_effects(
        app,
        &report,
        &diff_hunks,
        first.last_diff_focus,
        scroll_before_second_key,
    );

    assert_eq!(6, second.app.right_pane_scroll());
    assert_eq!(Some("lib.rs::bar"), second.app.selected_symbol_id());
}

// --- apply_diff_pane_selection_effects (re-entering RightPane::Diff with
// an unchanged cursor: `run_app`'s loop resets `last_diff_focus` to
// `None` while the right pane is not Diff, so re-entry looks like a
// fresh selection to the ADR 0027 auto-scroll branch below) ---

#[test]
fn should_resync_scroll_to_current_symbols_section_when_diff_pane_is_reentered_with_cursor_unchanged()
 {
    let report = report_with_two_symbols();
    let diff_hunks = diff_hunks_with_two_symbol_sections();
    // Cursor on `bar` (row 2), already inside bar's own section — the
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
    let effects = apply_diff_pane_selection_effects(app, &report, &diff_hunks, None, 0);

    // bar's section starts at line 5 (same layout as every other test in
    // this file); landing at 0 would show foo's section under a pinned
    // header that still names `bar` — the mismatch the resync exists to
    // prevent.
    assert_eq!(5, effects.app.right_pane_scroll());
    assert_eq!(Some("lib.rs::bar"), effects.app.selected_symbol_id());
}

// --- apply_diff_pane_selection_effects with adjacent signature-changed
// symbols sharing one hunk (ADR 0029), driven end-to-end through the
// draw+clamp pipeline: the fixtures above use `contract_header: None` and
// two separate hunks, and their tests observe `apply_diff_pane_selection_effects`
// output in isolation. This block covers the interaction between the
// mode-aware anchor rows added by the amendment on ADR 0044 decision 4
// and the actual post-frame scroll — the target computation and the
// clamp-against-`wrap_lines` output are both correct only when the two
// agree on how many rows the anchor occupies in the currently-rendered mode.

/// Two `SignatureChanged` symbols so close that git's line-level diff
/// produces one hunk spanning both — ADR 0029 duplicates it into both
/// sections.
fn report_with_two_adjacent_signature_changed_symbols() -> Report {
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{Classification, ExtractedSymbol, SymbolKind};
    use rinkaku_core::render::FileReport;

    fn changed_symbol(id: &str, name: &str, range: LineRange, previous: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}(a: i32, extra: i32)"),
            range,
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: Some(Classification::SignatureChanged),
            previous_signature: Some(previous.to_string()),
        }
    }

    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                changed_symbol(
                    "lib.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 3 },
                    "fn foo(a: i32)",
                ),
                changed_symbol(
                    "lib.rs::bar",
                    "bar",
                    LineRange { start: 5, end: 7 },
                    "fn bar(a: i32)",
                ),
            ],
        }],
        ..empty_report()
    }
}

fn diff_hunks_with_two_signature_changed_sections() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![Hunk {
            header: "@@ -1,7 +1,7 @@".to_string(),
            new_range: Some((1, 7)),
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Removed,
                    content: "fn foo(a: i32) {}".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Added,
                    content: "fn foo(a: i32, extra: i32) {}".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Context,
                    content: "".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Removed,
                    content: "fn bar(a: i32) {}".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Added,
                    content: "fn bar(a: i32, extra: i32) {}".to_string(),
                },
            ],
        }],
    }]
}

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
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");
    crate::clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll)
}

#[test]
fn should_scroll_into_the_second_signature_changed_section_in_unified_view() {
    // `App::new` defaults to `DiffViewMode::Split` (ADR 0044 amendment);
    // `ToggleSplitView` is what reaches unified rendering here.
    let report = report_with_two_adjacent_signature_changed_symbols();
    let diff_hunks = diff_hunks_with_two_signature_changed_sections();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView);
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
    let expected_scroll = diff_shape::section_start_line_for_symbol(
        &diff_shape::build_diff_pane_content(
            &report,
            &diff_hunks,
            app.selected_diff_target(&report).as_ref(),
        ),
        "lib.rs::bar",
        app.diff_view_mode(),
    )
    .expect("bar's section start must resolve");
    assert_eq!(expected_scroll, app.right_pane_scroll());
}

#[test]
fn should_scroll_into_the_second_signature_changed_section_in_split_view() {
    // `App::new` already defaults to `DiffViewMode::Split`.
    let report = report_with_two_adjacent_signature_changed_symbols();
    let diff_hunks = diff_hunks_with_two_signature_changed_sections();
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
    let expected_scroll = diff_shape::section_start_line_for_symbol(
        &diff_shape::build_diff_pane_content(
            &report,
            &diff_hunks,
            app.selected_diff_target(&report).as_ref(),
        ),
        "lib.rs::bar",
        app.diff_view_mode(),
    )
    .expect("bar's section start must resolve");
    assert_eq!(expected_scroll, app.right_pane_scroll());
}
