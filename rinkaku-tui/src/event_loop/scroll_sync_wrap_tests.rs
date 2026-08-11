//! Regression coverage for the scroll-unit fix (docs/adr/0052): every test
//! in `scroll_sync_tests.rs` uses a pane 160 columns wide, where none of
//! these fixtures' signatures ever wrap — so a bug that only manifests once
//! `crate::ui::scroll::wrap_lines_with_origins`/`pair_wrap_with_origins`
//! actually split a logical line into multiple display rows was invisible
//! there. These tests use a narrow pane and hunk header/body text long
//! enough to wrap, driven through the same `dispatch_draw_and_fold` pipeline
//! `scroll_sync_tests.rs` uses for its own end-to-end coverage. ADR 0072
//! removed the diff pane's per-symbol section anchors, so the fixtures below
//! wrap a hunk's own header/body text directly instead of a symbol's
//! signature line — the scroll-unit invariant under test (logical line vs.
//! wrapped display row) is unchanged.
//!
//! - `should_land_symbol_selection_anchor_at_viewport_top_*`: symptom 1
//!   (selecting a symbol did not scroll the diff pane to the corresponding
//!   position) — the target symbol's first intersecting hunk must be the
//!   first visible row after auto-scroll, in both view modes.
//! - `should_resolve_the_correct_symbol_when_scroll_position_lands_inside_a_preceding_wrapped_hunk`:
//!   symptom 2 (scrolling stuck the tree-cursor sync on the wrong symbol) —
//!   the reverse lookup must agree with what the pane actually has on
//!   screen, not silently resolve past it because a wrapped hunk inflated
//!   the display-row count relative to the logical-line count the lookup
//!   itself uses.
//! - `should_advance_scroll_monotonically_past_a_huge_wrapped_leading_line_*`:
//!   a follow-up regression in the symptom-1/2 fix itself — a display-row
//!   clamp that lands *inside* a preceding wrapped span folded back to that
//!   span's own logical line, undoing the request and stalling `Down` at a
//!   fixed point before reaching the last symbol.

use super::{apply_diff_pane_selection_effects, clamp_right_pane_scroll_after_draw};
use crate::app::{self, App, InputKey};
use crate::event_loop::tests::empty_report;
use crate::locale::Locale;
use crate::{diff_shape, diff_view};
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::render::{FileReport, Report};

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

/// Four symbols, each with a one-hunk diff whose body line is long enough
/// to wrap at this file's narrow test widths (40-80 columns) — a long
/// parameter list embedded in the *hunk body text* now that there is no
/// section-title scaffold to wrap instead (ADR 0072).
fn report_with_four_symbols() -> Report {
    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::first", "first", LineRange { start: 1, end: 2 }),
                symbol("lib.rs::second", "second", LineRange { start: 10, end: 11 }),
                symbol("lib.rs::third", "third", LineRange { start: 20, end: 21 }),
                symbol("lib.rs::fourth", "fourth", LineRange { start: 30, end: 31 }),
            ],
        }],
        ..empty_report()
    }
}

fn long_body_line(name: &str) -> String {
    format!(
        "fn {name}(input: RawInput, config: &ProcessingConfig, cache: &mut Cache, extra: bool) -> Result<ProcessedOutput, ProcessingError> {{"
    )
}

fn diff_hunks_with_four_wrapping_sections() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    fn hunk(header: &str, new_range: (usize, usize), name: &str) -> Hunk {
        Hunk {
            header: header.to_string(),
            new_range: Some(new_range),
            lines: vec![DiffLine {
                kind: DiffLineKind::Context,
                content: long_body_line(name),
            }],
        }
    }

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            hunk("@@ -1,1 +1,2 @@", (1, 2), "first"),
            hunk("@@ -10,1 +10,2 @@", (10, 11), "second"),
            hunk("@@ -20,1 +20,2 @@", (20, 21), "third"),
            hunk("@@ -30,1 +30,2 @@", (30, 31), "fourth"),
        ],
    }]
}

/// Mirrors `scroll_sync_tests.rs`'s own `dispatch_draw_and_fold` exactly
/// (one iteration of `crate::run_app`'s loop: dispatch + sync + draw +
/// post-draw fold-back) — duplicated rather than shared because that
/// function is private to its own file and this split (this file's own doc
/// comment) is specifically about keeping the two fixtures apart.
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll)
}

/// The diff pane's rendered text, row by row, so a test can assert which
/// row a given fragment first appears on (`render_scrollable_pane`'s header
/// occupies fixed rows above the scrollable body, so text position within
/// the pane — not just presence anywhere in the buffer — is what pins "at
/// the top of the viewport").
fn diff_pane_rows(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    // The right pane starts at 60% of the terminal width (`ENTRY_TREE_WIDTH_PERCENT`
    // /`ENTRY_RIGHT_WIDTH_PERCENT`) — only that half is relevant to what the
    // diff pane itself shows.
    let right_start = area.width * 40 / 100;
    (0..area.height)
        .map(|y| {
            (right_start..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

/// The first row index (0-based, within the diff pane's own rendered rows)
/// containing `needle`, or `None` if it never appears — used to check that
/// a hunk's own body line lands inside the pane's scrollable body, not
/// scrolled past the top edge into invisibility.
fn first_row_containing(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(needle))
}

/// Renders `app`/`diff_hunks` at `width`x`height` and returns the diff
/// pane's rendered rows (`diff_pane_rows`), rebuilding the same
/// `diff_pane_content` `dispatch_draw_and_fold` would have produced for
/// `app`'s current selection — used after the fold-back loop to inspect
/// the final frame without re-running the dispatch/sync step.
fn render_diff_pane_rows(
    app: &App,
    report: &Report,
    diff_hunks: &[diff_view::FileHunks],
    width: u16,
    height: u16,
) -> Vec<String> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("terminal");
    let diff_pane_content = diff_shape::build_diff_pane_content(
        report,
        diff_hunks,
        app.selected_diff_target(report).as_ref(),
    );
    let diff_highlights = crate::highlight::highlight_diff_files(diff_hunks);
    terminal
        .draw(|frame| {
            crate::ui::draw(
                frame,
                app,
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
    diff_pane_rows(&terminal)
}

#[test]
fn should_land_symbol_selection_anchor_at_viewport_top_in_unified_view() {
    // Width 80: right pane inner width ~46 columns, well under `third`'s
    // wrapping hunk body — wrapping actually occurs (`ENTRY_RIGHT_WIDTH_PERCENT`'s
    // 60% split plus `Block::bordered`'s 2-column border deduction).
    let report = report_with_four_symbols();
    let diff_hunks = diff_hunks_with_four_wrapping_sections();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView); // App::new defaults to Split.
    assert_eq!(Some("lib.rs::first"), app.selected_symbol_id());
    let last_diff_focus = app.selected_diff_focus(&report);

    // Two tree-cursor `Down`s land on `third`, past one whole wrapped
    // hunk (`second`) — ADR 0027's auto-scroll should land the diff pane
    // exactly on `third`'s own hunk regardless. Height 8 keeps the
    // viewport short enough relative to each wrapped hunk's several
    // display rows that scrolling actually pushes earlier hunks off
    // screen (a tall viewport would just show every hunk at once,
    // making this regression invisible).
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        80,
        8,
    );
    let last_diff_focus = app.selected_diff_focus(&report);
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        80,
        8,
    );
    assert_eq!(Some("lib.rs::third"), app.selected_symbol_id());

    let rows = render_diff_pane_rows(&app, &report, &diff_hunks, 80, 8);
    // `third`'s hunk header must be the first hunk-header line of the
    // pane's scrollable body — the header lines (identification/stats,
    // `diff_pane_header_lines`) occupy fixed rows above it, so this checks
    // it appears before `fourth`'s own header, not that it is at row 0
    // literally.
    let third_row = first_row_containing(&rows, "@@ -20,1 +20,2 @@")
        .expect("third's hunk header must be visible in the diff pane");
    let fourth_row = first_row_containing(&rows, "@@ -30,1 +30,2 @@");
    if let Some(fourth_row) = fourth_row {
        assert!(
            third_row < fourth_row,
            "third's hunk ({third_row}) must render above fourth's ({fourth_row})"
        );
    }
    // Regression check for the pre-fix bug: `first`/`second`'s own hunk
    // headers must have scrolled out of view once `third` is selected —
    // before this fix, the logical-line scroll target was applied to the
    // wrapped display-row viewport, so an offset short of the true wrapped
    // position could leave earlier hunks still on screen instead of
    // scrolling to `third`.
    assert_eq!(None, first_row_containing(&rows, "@@ -1,1 +1,2 @@"));
    assert_eq!(None, first_row_containing(&rows, "@@ -10,1 +10,2 @@"));
}

#[test]
fn should_land_symbol_selection_anchor_at_viewport_top_in_split_view() {
    // Width 170: right pane inner width ~100, split into two ~49-wide
    // columns (`MIN_SPLIT_VIEW_WIDTH` is 100) — `third`'s hunk body still
    // wraps on each side.
    let report = report_with_four_symbols();
    let diff_hunks = diff_hunks_with_four_wrapping_sections();
    let app = App::new(&report).handle_key(InputKey::Down);
    assert_eq!(app::DiffViewMode::Split, app.diff_view_mode());
    assert_eq!(Some("lib.rs::first"), app.selected_symbol_id());
    let last_diff_focus = app.selected_diff_focus(&report);

    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        170,
        8,
    );
    let last_diff_focus = app.selected_diff_focus(&report);
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        170,
        8,
    );
    assert_eq!(Some("lib.rs::third"), app.selected_symbol_id());

    let rows = render_diff_pane_rows(&app, &report, &diff_hunks, 170, 8);
    let third_row = first_row_containing(&rows, "@@ -20,1 +20,2 @@")
        .expect("third's hunk header must be visible in the diff pane");
    let fourth_row = first_row_containing(&rows, "@@ -30,1 +30,2 @@");
    if let Some(fourth_row) = fourth_row {
        assert!(
            third_row < fourth_row,
            "third's hunk ({third_row}) must render above fourth's ({fourth_row})"
        );
    }
    assert_eq!(None, first_row_containing(&rows, "@@ -1,1 +1,2 @@"));
    assert_eq!(None, first_row_containing(&rows, "@@ -10,1 +10,2 @@"));
}

/// A giant symbol (a very long hunk body line) followed by a one-line
/// symbol — `giant`'s hunk body alone wraps into many display rows at
/// this file's narrow test widths, so `small`'s logical hunk-start offset
/// (a small number, e.g. 3) sits many display rows short of where `small`
/// actually renders once `giant` has wrapped.
fn report_with_giant_then_small_symbol() -> Report {
    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::giant", "giant", LineRange { start: 1, end: 2 }),
                symbol("lib.rs::small", "small", LineRange { start: 10, end: 11 }),
            ],
        }],
        ..empty_report()
    }
}

fn giant_body_line() -> String {
    let params = (0..20)
        .map(|index| format!("p{index}: Type{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("fn giant({params}) -> Result<Output, Error> {{")
}

fn diff_hunks_with_giant_then_small_sections() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            Hunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                new_range: Some((1, 2)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: giant_body_line(),
                }],
            },
            Hunk {
                header: "@@ -10,1 +10,2 @@".to_string(),
                new_range: Some((10, 11)),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: "fn small() {}".to_string(),
                }],
            },
        ],
    }]
}

fn symbols_for(report: &Report, path: &str) -> Vec<(String, LineRange)> {
    report
        .files
        .iter()
        .find(|file| file.path == path)
        .map(|file| {
            file.symbols
                .iter()
                .map(|symbol| (symbol.id.clone(), symbol.range))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn should_resolve_the_correct_symbol_when_scroll_position_lands_inside_a_preceding_wrapped_hunk() {
    // Symptom 2's regression pin: before the fix, `render_scrollable_pane`
    // clamped/consumed `requested_scroll` directly as a *display-row* index
    // into the wrapped content, with no conversion from the *logical-line*
    // unit `crate::diff_shape::section_start_line_for_symbol` produces it
    // in. Requesting `small`'s logical hunk-start (a small number) left
    // the rendered viewport still showing `giant`'s own wrapped
    // continuation — `small`'s hunk was nowhere on screen — while the
    // fold-back nonetheless wrote that same small number back into
    // `App::right_pane_scroll` unchanged (`clamp_scroll` never *increases*
    // an in-bounds value), so the very next `symbol_id_for_scroll_line`
    // reverse lookup reported `small` as selected despite the pane still
    // showing `giant`: the tree cursor and the diff pane's own content
    // silently disagreed about which symbol was "current".
    let report = report_with_giant_then_small_symbol();
    let diff_hunks = diff_hunks_with_giant_then_small_sections();
    let content = diff_shape::build_diff_pane_content(
        &report,
        &diff_hunks,
        Some(&app::DiffTarget::File {
            path: "lib.rs".to_string(),
        }),
    );
    let small_start =
        diff_shape::section_start_line_for_symbol(&content, LineRange { start: 10, end: 11 })
            .expect("small's hunk start must resolve");

    // Request `small`'s logical hunk-start directly (bypassing
    // `apply_diff_pane_selection_effects`'s own gating, since this test
    // targets `render_scrollable_pane`'s own unit contract in isolation)
    // and render at a narrow width where `giant`'s hunk body wraps.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView)
        .handle_key(InputKey::Open)
        .with_right_pane_scroll(small_start);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_hunks);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 10)).expect("terminal");
    let mut outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            outcome = crate::ui::draw(
                frame,
                &app,
                &report,
                &content,
                &diff_highlights,
                &app::BlastRadiusSelection::NotApplicable,
                None,
                &diff_hunks,
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    let rows = diff_pane_rows(&terminal);
    let folded_back_scroll = outcome
        .clamped_right_pane_scroll
        .expect("diff pane must report a clamped scroll");

    assert!(
        first_row_containing(&rows, "@@ -10,1 +10,2 @@").is_some(),
        "small's hunk header must be visible once requested at its own logical start; rows: {rows:?}"
    );
    let symbols = symbols_for(&report, "lib.rs");
    let resolved = diff_shape::symbol_id_for_scroll_line(&content, folded_back_scroll, &symbols);
    assert_eq!(
        Some("lib.rs::small"),
        resolved,
        "the reverse lookup fed the folded-back scroll must agree with what the pane actually shows"
    );
}

/// A huge symbol (a very long hunk body line, wrapping into dozens of
/// display rows at this file's narrow test widths) followed by three short
/// one-line symbols — the fixture symptom-1/2's own giant-then-small
/// fixture generalizes: a single wrapped leading hunk long enough that
/// `clamp_scroll`'s display-row clamp can land *inside* its own wrapped
/// span for several consecutive `Down` presses in a row, not just one.
fn report_with_giant_then_three_short_symbols() -> Report {
    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::giant", "giant", LineRange { start: 1, end: 2 }),
                symbol("lib.rs::first", "first", LineRange { start: 10, end: 11 }),
                symbol("lib.rs::second", "second", LineRange { start: 20, end: 21 }),
                symbol("lib.rs::third", "third", LineRange { start: 30, end: 31 }),
            ],
        }],
        ..empty_report()
    }
}

fn huge_body_line() -> String {
    let params = (0..80)
        .map(|index| format!("p{index}: Type{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("fn giant({params}) -> Result<Output, Error> {{")
}

fn diff_hunks_with_giant_then_three_short_sections() -> Vec<diff_view::FileHunks> {
    use diff_view::{DiffLine, DiffLineKind, Hunk};

    fn hunk(header: &str, new_range: (usize, usize), line: String) -> Hunk {
        Hunk {
            header: header.to_string(),
            new_range: Some(new_range),
            lines: vec![DiffLine {
                kind: DiffLineKind::Context,
                content: line,
            }],
        }
    }

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            hunk("@@ -1,1 +1,2 @@", (1, 2), huge_body_line()),
            hunk("@@ -10,1 +10,2 @@", (10, 11), "fn first() {}".to_string()),
            hunk("@@ -20,1 +20,2 @@", (20, 21), "fn second() {}".to_string()),
            hunk("@@ -30,1 +30,2 @@", (30, 31), "fn third() {}".to_string()),
        ],
    }]
}

/// Repeatedly presses `Down` while `Focus::Right` on the diff pane (bypassing
/// `apply_diff_pane_selection_effects`'s tree-cursor gating, mirroring
/// `should_resolve_the_correct_symbol_when_scroll_position_lands_inside_a_preceding_wrapped_hunk`'s
/// own direct-request style) and returns `right_pane_scroll` after each
/// press-draw-fold cycle — the sequence a regression test needs to assert
/// monotonic progress against, not just the final value.
fn scroll_positions_after_repeated_down(
    report: &Report,
    diff_hunks: &[diff_view::FileHunks],
    view_mode_toggle: bool,
    width: u16,
    height: u16,
    presses: usize,
) -> Vec<usize> {
    let mut app = App::new(report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open);
    if view_mode_toggle {
        app = app.handle_key(InputKey::ToggleSplitView);
    }
    let content = diff_shape::build_diff_pane_content(
        report,
        diff_hunks,
        app.selected_diff_target(report).as_ref(),
    );
    let diff_highlights = crate::highlight::highlight_diff_files(diff_hunks);

    let mut positions = Vec::new();
    for _ in 0..presses {
        app = app.handle_key(InputKey::Down);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
                .expect("terminal");
        let mut outcome = crate::ui::DrawOutcome::default();
        terminal
            .draw(|frame| {
                outcome = crate::ui::draw(
                    frame,
                    &app,
                    report,
                    &content,
                    &diff_highlights,
                    &app::BlastRadiusSelection::NotApplicable,
                    None,
                    diff_hunks,
                    &crate::annotation_markers::AnnotationMarkers::default(),
                    Locale::English,
                );
            })
            .expect("draw");
        app = clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll);
        positions.push(app.right_pane_scroll());
    }
    positions
}

/// Asserts `positions` never decreases step to step and eventually reaches
/// at least `minimum_final` — the monotonic-progress-and-reaches-the-end
/// contract this regression test exists to pin, shared by every
/// `viewport_height`/view-mode case below. "At least" rather than "exactly"
/// because `minimum_final` is the target symbol's own hunk-start line,
/// while the settled scroll position can land anywhere from there through
/// that symbol's own trailing hunk content — `symbol_id_for_scroll_line`'s
/// reverse lookup, asserted separately by each caller, is what actually
/// pins which symbol the settled position belongs to.
fn assert_monotonic_and_reaches(positions: &[usize], minimum_final: usize) {
    for window in positions.windows(2) {
        assert!(
            window[1] >= window[0],
            "scroll must never regress: {positions:?}"
        );
    }
    let final_position = *positions.last().expect("at least one position recorded");
    assert!(
        final_position >= minimum_final,
        "scroll must reach at least the last symbol's hunk start ({minimum_final}): {positions:?}"
    );
}

#[test]
fn should_advance_scroll_monotonically_past_a_huge_wrapped_leading_line_in_unified_view() {
    let report = report_with_giant_then_three_short_symbols();
    let diff_hunks = diff_hunks_with_giant_then_three_short_sections();
    let content = diff_shape::build_diff_pane_content(
        &report,
        &diff_hunks,
        Some(&app::DiffTarget::File {
            path: "lib.rs".to_string(),
        }),
    );
    let last_line =
        diff_shape::section_start_line_for_symbol(&content, LineRange { start: 30, end: 31 })
            .expect("third's hunk start must resolve");
    let symbols = symbols_for(&report, "lib.rs");

    for viewport_height in [2u16, 3, 4] {
        let positions = scroll_positions_after_repeated_down(
            &report,
            &diff_hunks,
            false,
            40,
            viewport_height + 6,
            60,
        );
        assert_monotonic_and_reaches(&positions, last_line);

        let resolved = diff_shape::symbol_id_for_scroll_line(
            &content,
            *positions.last().expect("at least one press recorded"),
            &symbols,
        );
        assert_eq!(
            Some("lib.rs::third"),
            resolved,
            "reverse sync must resolve to the last symbol once scroll reaches the end (viewport_height={viewport_height})"
        );
    }
}

#[test]
fn should_advance_scroll_monotonically_past_a_huge_wrapped_leading_line_in_split_view() {
    let report = report_with_giant_then_three_short_symbols();
    let diff_hunks = diff_hunks_with_giant_then_three_short_sections();
    let content = diff_shape::build_diff_pane_content(
        &report,
        &diff_hunks,
        Some(&app::DiffTarget::File {
            path: "lib.rs".to_string(),
        }),
    );
    let last_line =
        diff_shape::section_start_line_for_symbol(&content, LineRange { start: 30, end: 31 })
            .expect("third's hunk start must resolve");
    let symbols = symbols_for(&report, "lib.rs");

    for viewport_height in [2u16, 3, 4] {
        let positions = scroll_positions_after_repeated_down(
            &report,
            &diff_hunks,
            true,
            170,
            viewport_height + 6,
            60,
        );
        assert_monotonic_and_reaches(&positions, last_line);

        let resolved = diff_shape::symbol_id_for_scroll_line(
            &content,
            *positions.last().expect("at least one press recorded"),
            &symbols,
        );
        assert_eq!(
            Some("lib.rs::third"),
            resolved,
            "reverse sync must resolve to the last symbol once scroll reaches the end (viewport_height={viewport_height})"
        );
    }
}

#[test]
fn should_not_oscillate_when_alternating_down_and_up_past_a_huge_wrapped_leading_line() {
    let report = report_with_giant_then_three_short_symbols();
    let diff_hunks = diff_hunks_with_giant_then_three_short_sections();
    let content = diff_shape::build_diff_pane_content(
        &report,
        &diff_hunks,
        Some(&app::DiffTarget::File {
            path: "lib.rs".to_string(),
        }),
    );
    let last_line =
        diff_shape::section_start_line_for_symbol(&content, LineRange { start: 30, end: 31 })
            .expect("third's hunk start must resolve");
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_hunks);
    let width = 40;
    let height = 10;

    let mut app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open);
    let draw_and_fold = |app: App, key: InputKey| -> App {
        let app = app.handle_key(key);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
                .expect("terminal");
        let mut outcome = crate::ui::DrawOutcome::default();
        terminal
            .draw(|frame| {
                outcome = crate::ui::draw(
                    frame,
                    &app,
                    &report,
                    &content,
                    &diff_highlights,
                    &app::BlastRadiusSelection::NotApplicable,
                    None,
                    &diff_hunks,
                    &crate::annotation_markers::AnnotationMarkers::default(),
                    Locale::English,
                );
            })
            .expect("draw");
        clamp_right_pane_scroll_after_draw(app, outcome.clamped_right_pane_scroll)
    };

    // Drive all the way down first, recording each step.
    let mut down_positions = Vec::new();
    for _ in 0..60 {
        app = draw_and_fold(app, InputKey::Down);
        down_positions.push(app.right_pane_scroll());
    }
    assert_monotonic_and_reaches(&down_positions, last_line);

    // Then alternate Down/Up from the end: net movement per pair must be
    // zero, and the value must never overshoot past what a single `Down`
    // from the settled end position would produce.
    for _ in 0..5 {
        let before = app.right_pane_scroll();
        app = draw_and_fold(app, InputKey::Down);
        let after_down = app.right_pane_scroll();
        app = draw_and_fold(app, InputKey::Up);
        let after_up = app.right_pane_scroll();
        assert!(
            after_down >= before,
            "Down must not regress scroll: before={before} after_down={after_down}"
        );
        assert!(
            after_up <= after_down,
            "Up must not leave scroll higher than the preceding Down: after_down={after_down} after_up={after_up}"
        );
    }
}
