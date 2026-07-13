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
        file_size_warnings: vec![],
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
fn should_translate_unbound_key_to_none_when_overlay_is_open() {
    // `'j'` used to be this test's example key, back when the overlay
    // had no scroll gestures of its own and swallowed every key but
    // `?`/Esc/`q` (`should_ignore_navigation_keys_while_help_overlay_is_open`'s
    // own App-level counterpart). Scrolling the overlay now maps `'j'`
    // to `InputKey::Down` even while it is open — see
    // `should_translate_j_to_down_for_overlay_scroll_when_overlay_is_open`
    // below — so this test switched to `'z'`, a key with no meaning
    // anywhere in the keymap, to keep pinning "arbitrary keys are
    // swallowed" without asserting something that is no longer true.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('z'), KeyModifiers::NONE, &app);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_j_to_down_for_overlay_scroll_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_k_to_up_for_overlay_scroll_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('k'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::Up), actual);
}

#[test]
fn should_translate_ctrl_d_to_scroll_half_page_down_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageDown), actual);
}

#[test]
fn should_translate_ctrl_u_to_scroll_half_page_up_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('u'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageUp), actual);
}

#[test]
fn should_translate_uppercase_g_to_scroll_to_bottom_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);

    let actual = translate_key(KeyCode::Char('G'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToBottom), actual);
}

#[test]
fn should_translate_double_g_to_scroll_to_top_when_overlay_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    let first_g = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);
    assert_eq!(Some(InputKey::PendingGoto), first_g);
    let app = app.handle_key(first_g.expect("first g translates"));

    let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToTop), actual);
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

// ADR 0026 scroll bindings (translate_key half).

#[test]
fn should_translate_ctrl_d_to_scroll_half_page_down() {
    // Must resolve *before* the plain `Char('d')` arm below (which
    // maps to `ToggleDiff`) — a stale ordering would silently
    // rebind Ctrl-d to Diff-toggle instead of half-page-down.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('d'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageDown), actual);
}

#[test]
fn should_translate_ctrl_u_to_scroll_half_page_up() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('u'), KeyModifiers::CONTROL, &app);

    assert_eq!(Some(InputKey::ScrollHalfPageUp), actual);
}

#[test]
fn should_translate_second_g_to_scroll_to_top_when_g_prefix_is_pending() {
    // `gg` (ADR 0026) resolves through the same `pending_prefix`
    // arm `gd`/`gr` do (ADR 0022) — this test pins that a *second*
    // `g` after a pending `g` means "scroll to top", not restart
    // the prefix. Restarting is what a *first* `g` does when no
    // prefix is pending (the `should_translate_g_to_pending_goto`
    // test above); this arm's behavior is the second-key half of
    // the two-key `gg` sequence.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::PendingGoto);

    let actual = translate_key(KeyCode::Char('g'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToTop), actual);
}

#[test]
fn should_translate_uppercase_g_to_scroll_to_bottom() {
    // Distinct from single-key lowercase `g` (`PendingGoto`, tested
    // above): `G` is a one-key gesture, not the leader of a sequence.
    let report = empty_report();
    let app = App::new(&report);

    let actual = translate_key(KeyCode::Char('G'), KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ScrollToBottom), actual);
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

// translate_mouse_event tests (mouse wheel scroll support).

#[test]
fn should_translate_scroll_up_to_input_key_up() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollUp);

    assert_eq!(Some(InputKey::Up), actual);
}

#[test]
fn should_translate_scroll_down_to_input_key_down() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollDown);

    assert_eq!(Some(InputKey::Down), actual);
}

#[test]
fn should_translate_scroll_left_to_none() {
    // Horizontal wheel/trackpad input has no mapping — this crate has
    // no horizontally-scrollable pane (this function's own doc comment).
    let actual = translate_mouse_event(event::MouseEventKind::ScrollLeft);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_scroll_right_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::ScrollRight);

    assert_eq!(None, actual);
}

#[test]
fn should_translate_click_to_none() {
    // Clicks/drags/moves are deliberately out of scope (no pane
    // targeting by click position) — this function's own doc comment.
    let actual = translate_mouse_event(event::MouseEventKind::Down(event::MouseButton::Left));

    assert_eq!(None, actual);
}

#[test]
fn should_translate_drag_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::Drag(event::MouseButton::Left));

    assert_eq!(None, actual);
}

#[test]
fn should_translate_mouse_moved_to_none() {
    let actual = translate_mouse_event(event::MouseEventKind::Moved);

    assert_eq!(None, actual);
}
