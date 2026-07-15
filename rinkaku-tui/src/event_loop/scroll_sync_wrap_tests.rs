//! Regression coverage for the scroll-unit fix (docs/adr/0052): every test
//! in `scroll_sync_tests.rs` uses a pane 160 columns wide, where none of
//! these fixtures' signatures ever wrap — so a bug that only manifests once
//! `crate::ui::scroll::wrap_lines_with_origins`/`pair_wrap_with_origins`
//! actually split a logical line into multiple display rows was invisible
//! there. These tests use a narrow pane and signatures long enough to wrap,
//! driven through the same `dispatch_draw_and_fold` pipeline
//! `scroll_sync_tests.rs` uses for its own end-to-end coverage.
//!
//! - `should_land_symbol_selection_anchor_at_viewport_top_*`: symptom 1
//!   (selecting a symbol did not scroll the diff pane to the corresponding
//!   position) — the anchor row must be the first visible row after
//!   auto-scroll, in both view modes.
//! - `should_resolve_the_correct_symbol_when_scroll_position_lands_inside_a_preceding_wrapped_section`:
//!   symptom 2 (scrolling stuck the tree-cursor sync on the wrong symbol) —
//!   the reverse lookup must agree with what the pane actually has on
//!   screen, not silently resolve past it because a wrapped section
//!   inflated the display-row count relative to the logical-line count the
//!   lookup itself uses.

use super::{apply_diff_pane_selection_effects, clamp_right_pane_scroll_after_draw};
use crate::app::{self, App, InputKey};
use crate::event_loop::tests::empty_report;
use crate::{diff_shape, diff_view};
use pretty_assertions::assert_eq;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::render::{FileReport, Report};

/// Four symbols whose signatures are long enough to wrap at this file's
/// narrow test widths (50-100 columns) — long parameter lists rather than
/// artificially padded names, so the fixture still reads as a plausible
/// signature.
fn report_with_four_long_signature_symbols() -> Report {
    fn symbol(id: &str, name: &str, range: LineRange, params: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}({params}) -> Result<ProcessedOutput, ProcessingError>"),
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
                symbol(
                    "lib.rs::first",
                    "first",
                    LineRange { start: 1, end: 2 },
                    "input: RawInput, config: &ProcessingConfig",
                ),
                symbol(
                    "lib.rs::second",
                    "second",
                    LineRange { start: 10, end: 11 },
                    "input: RawInput, config: &ProcessingConfig, cache: &mut Cache",
                ),
                symbol(
                    "lib.rs::third",
                    "third",
                    LineRange { start: 20, end: 21 },
                    "input: RawInput, config: &ProcessingConfig, cache: &mut Cache, extra: bool",
                ),
                symbol(
                    "lib.rs::fourth",
                    "fourth",
                    LineRange { start: 30, end: 31 },
                    "input: RawInput, config: &ProcessingConfig, cache: &mut Cache, extra: bool, more: u64",
                ),
            ],
        }],
        ..empty_report()
    }
}

fn diff_hunks_with_four_symbol_sections() -> Vec<diff_view::FileHunks> {
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

    vec![diff_view::FileHunks {
        path: "lib.rs".to_string(),
        hunks: vec![
            hunk("@@ -1,1 +1,2 @@", (1, 2), "fn first(..) {}"),
            hunk("@@ -10,1 +10,2 @@", (10, 11), "fn second(..) {}"),
            hunk("@@ -20,1 +20,2 @@", (20, 21), "fn third(..) {}"),
            hunk("@@ -30,1 +30,2 @@", (30, 31), "fn fourth(..) {}"),
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

    let effective_mode = app.diff_view_mode();
    let scroll_before_dispatch = app.right_pane_scroll();
    app = app.handle_key(input_key);
    let effects = apply_diff_pane_selection_effects(
        app,
        report,
        diff_hunks,
        last_diff_focus,
        scroll_before_dispatch,
        effective_mode,
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
/// a symbol's anchor line lands inside the pane's scrollable body, not
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
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");
    diff_pane_rows(&terminal)
}

#[test]
fn should_land_symbol_selection_anchor_at_viewport_top_in_unified_view() {
    // Width 80: right pane inner width ~46 columns, well under `third`'s
    // ~85-column signature — wrapping actually occurs (`ENTRY_RIGHT_WIDTH_PERCENT`'s
    // 60% split plus `Block::bordered`'s 2-column border deduction).
    let report = report_with_four_long_signature_symbols();
    let diff_hunks = diff_hunks_with_four_symbol_sections();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView); // App::new defaults to Split.
    assert_eq!(Some("lib.rs::first"), app.selected_symbol_id());
    let last_diff_focus = app.selected_diff_focus(&report);

    // Two tree-cursor `Down`s land on `third`, past one whole wrapped
    // section (`second`) — ADR 0027's auto-scroll should land the diff
    // pane exactly on `third`'s own anchor row regardless.
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        80,
        20,
    );
    let last_diff_focus = app.selected_diff_focus(&report);
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        80,
        20,
    );
    assert_eq!(Some("lib.rs::third"), app.selected_symbol_id());

    let rows = render_diff_pane_rows(&app, &report, &diff_hunks, 80, 20);
    // `third`'s anchor line must be the first line of the pane's
    // scrollable body — the header lines (identification/stats,
    // `diff_pane_header_lines`) occupy fixed rows above it, so this checks
    // it appears before `fourth`'s own anchor, not that it is at row 0
    // literally.
    let third_row = first_row_containing(&rows, "fn third(")
        .expect("third's anchor line must be visible in the diff pane");
    let fourth_row = first_row_containing(&rows, "fn fourth(");
    if let Some(fourth_row) = fourth_row {
        assert!(
            third_row < fourth_row,
            "third's anchor ({third_row}) must render above fourth's ({fourth_row})"
        );
    }
    // Regression check for the pre-fix bug: `first`/`second`'s own anchor
    // lines must have scrolled out of view once `third` is selected —
    // before this fix, the logical-line scroll target was applied to the
    // wrapped display-row viewport, so an offset short of the true wrapped
    // position could leave earlier sections still on screen instead of
    // scrolling to `third`.
    assert_eq!(None, first_row_containing(&rows, "fn first("));
    assert_eq!(None, first_row_containing(&rows, "fn second("));
}

#[test]
fn should_land_symbol_selection_anchor_at_viewport_top_in_split_view() {
    // Width 170: right pane inner width ~100, split into two ~49-wide
    // columns (`MIN_SPLIT_VIEW_WIDTH` is 100) — `third`'s ~85-column
    // signature still wraps on each side.
    let report = report_with_four_long_signature_symbols();
    let diff_hunks = diff_hunks_with_four_symbol_sections();
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
        20,
    );
    let last_diff_focus = app.selected_diff_focus(&report);
    let app = dispatch_draw_and_fold(
        app,
        &report,
        &diff_hunks,
        last_diff_focus,
        InputKey::Down,
        170,
        20,
    );
    assert_eq!(Some("lib.rs::third"), app.selected_symbol_id());

    let rows = render_diff_pane_rows(&app, &report, &diff_hunks, 170, 20);
    let third_row = first_row_containing(&rows, "fn third(")
        .expect("third's anchor line must be visible in the diff pane");
    let fourth_row = first_row_containing(&rows, "fn fourth(");
    if let Some(fourth_row) = fourth_row {
        assert!(
            third_row < fourth_row,
            "third's anchor ({third_row}) must render above fourth's ({fourth_row})"
        );
    }
    assert_eq!(None, first_row_containing(&rows, "fn first("));
    assert_eq!(None, first_row_containing(&rows, "fn second("));
}

/// A giant symbol (a 20-parameter signature) followed by a one-line symbol
/// — `giant`'s signature alone wraps into many display rows at this file's
/// narrow test widths, so `small`'s logical section-start offset (a small
/// number, e.g. 5) sits many display rows short of where `small` actually
/// renders once `giant` has wrapped.
fn report_with_giant_then_small_symbol() -> Report {
    fn symbol(id: &str, name: &str, range: LineRange, signature: String) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature,
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

    let giant_params = (0..20)
        .map(|index| format!("p{index}: Type{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let giant_signature = format!("fn giant({giant_params}) -> Result<Output, Error>");

    Report {
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol(
                    "lib.rs::giant",
                    "giant",
                    LineRange { start: 1, end: 2 },
                    giant_signature,
                ),
                symbol(
                    "lib.rs::small",
                    "small",
                    LineRange { start: 10, end: 11 },
                    "fn small()".to_string(),
                ),
            ],
        }],
        ..empty_report()
    }
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
                    content: "fn giant(..) {}".to_string(),
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

#[test]
fn should_resolve_the_correct_symbol_when_scroll_position_lands_inside_a_preceding_wrapped_section()
{
    // Symptom 2's regression pin: before the fix, `render_scrollable_pane`
    // clamped/consumed `requested_scroll` directly as a *display-row* index
    // into the wrapped content, with no conversion from the *logical-line*
    // unit `crate::diff_shape::section_start_line_for_symbol` produces it
    // in. Requesting `small`'s logical section-start (a small number) left
    // the rendered viewport still showing `giant`'s own wrapped
    // continuation — `small`'s anchor line was nowhere on screen — while
    // the fold-back nonetheless wrote that same small number back into
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
    let small_start = diff_shape::section_start_line_for_symbol(
        &content,
        "lib.rs::small",
        app::DiffViewMode::Unified,
    )
    .expect("small's section start must resolve");

    // Request `small`'s logical section-start directly (bypassing
    // `apply_diff_pane_selection_effects`'s own gating, since this test
    // targets `render_scrollable_pane`'s own unit contract in isolation)
    // and render at a narrow width where `giant`'s signature wraps.
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
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");
    let rows = diff_pane_rows(&terminal);
    let folded_back_scroll = outcome
        .clamped_right_pane_scroll
        .expect("diff pane must report a clamped scroll");

    assert!(
        first_row_containing(&rows, "fn small(").is_some(),
        "small's anchor line must be visible once requested at its own logical start; rows: {rows:?}"
    );
    let resolved = diff_shape::symbol_id_for_scroll_line(
        &content,
        folded_back_scroll,
        app::DiffViewMode::Unified,
    );
    assert_eq!(
        Some("lib.rs::small"),
        resolved,
        "the reverse lookup fed the folded-back scroll must agree with what the pane actually shows"
    );
}
