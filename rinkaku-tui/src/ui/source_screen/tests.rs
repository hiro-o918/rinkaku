use super::*;
use crate::app::{App, BlastRadiusSelection};
use crate::ui::{DrawOutcome, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

fn symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
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
    }
}

fn report_with_one_symbol() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
        }],
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

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_cell_style(terminal: &Terminal<TestBackend>, line_needle: &str, token: &str) -> Style {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    for y in 0..area.height {
        let row: String = (0..area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        let Some(needle_byte_offset) = row.find(line_needle) else {
            continue;
        };
        let Some(token_byte_offset) = row[needle_byte_offset..].find(token) else {
            continue;
        };
        let byte_offset = needle_byte_offset + token_byte_offset;
        let column = row[..byte_offset].chars().count() as u16;
        return buffer[(column, y)].style();
    }
    panic!("expected to find {token:?} within a row containing {line_needle:?}");
}

/// A real `Err` from `crate::source::load_symbol_source`/
/// `load_highlighted_symbol_source`, produced by actually attempting a
/// read under a placeholder repo root nothing on disk matches — used to
/// build `source_content` for `draw_source_screen` tests below rather
/// than fabricating the error string by hand, so these tests stay
/// pinned to the real message format `crate::source` produces.
fn missing_file_source_content(
    report: &Report,
    symbol_id: &str,
) -> Option<Result<crate::source::HighlightedSourceView, String>> {
    Some(crate::source::load_highlighted_symbol_source(
        report,
        symbol_id,
        std::path::Path::new("/repo"),
    ))
}

#[test]
fn should_draw_source_screen_title_and_error_message_when_file_is_missing() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");
    let source_content = missing_file_source_content(&report, "lib.rs::foo");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // "lib.rs" does not exist under the placeholder repo root above, so
    // this exercises `draw_source_screen`'s error-message fallback path
    // rather than needing a real file on disk.
    let text = buffer_text(&terminal);
    assert!(text.contains("Source: lib.rs::foo"));
    assert!(text.contains("failed to read"));
    assert!(text.contains("back"));
}

#[test]
fn should_wrap_source_error_message_instead_of_truncating_it_in_a_narrow_pane() {
    // Regression test: `source::load_symbol_source`'s error message
    // (full path + io error + the "not present in the working tree"
    // hint) routinely exceeds one line, and `Paragraph` without
    // `.wrap(...)` silently truncates rather than overflowing — cutting
    // the hint off exactly where it explains the failure. A narrow
    // (40-column) pane makes the message wrap across multiple rows
    // whether or not `.wrap(...)` is set, but only *with* it does the
    // hint's text actually appear anywhere in the buffer; without it,
    // the trailing text is simply dropped.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(40, 20)).expect("terminal");
    let source_content = missing_file_source_content(&report, "lib.rs::foo");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // `buffer_text` joins rows with `\n`, so a phrase that happens to
    // wrap exactly at a row boundary (as "in the" / "present" do at
    // this width) would not appear as one contiguous substring even
    // though every word is visible — asserting on words rather than a
    // multi-word phrase keeps this test robust to exactly where the
    // wrap point falls, while still failing if `.wrap(...)` were
    // removed (the words after "working tree" would be dropped
    // entirely, not just split across rows).
    let text = buffer_text(&terminal);
    assert!(text.contains("Source: lib.rs::foo"));
    assert!(text.contains("present"));
    assert!(text.contains("working tree"));
    assert!(text.contains("historical commit not checked out"));
    assert!(text.contains("locally)"));
}

// --- draw_source_screen: syntax highlighting ---

#[test]
fn should_apply_keyword_foreground_and_symbol_range_background_in_source_screen() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn a() {}\nfn foo() {}\n").expect("write file");

    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange { start: 2, end: 2 },
                ..symbol("lib.rs::foo", "foo")
            }],
        }],
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
    };
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        "lib.rs::foo",
        dir.path(),
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // Line 2 ("fn foo() {}") is the symbol's own highlighted range: its
    // "fn" keyword must carry both signals composited together — the
    // token's own foreground color (ADR 0018's palette, extended to
    // this screen) *and* the symbol-range background tint — mirroring
    // how the diff pane composites a token's foreground with its
    // added/removed background rather than one replacing the other.
    let keyword_style = find_cell_style(&terminal, "2 | fn foo() {}", "fn");
    assert_eq!(Some(Color::Magenta), keyword_style.fg);
    assert_eq!(Some(SOURCE_HIGHLIGHT_BG), keyword_style.bg);

    // Line 1 ("fn a() {}") is outside the symbol's range: its own "fn"
    // keyword must still be colored (highlighting applies to every
    // line, not just the highlighted range) but without the background
    // tint, since only the drilled-into symbol's own lines get it.
    // `Style::bg` reports an unset background as `Some(Color::Reset)`,
    // not `None` (ratatui's own `Cell` default — see
    // `should_keep_context_line_unstyled_background_in_diff_pane`'s own
    // doc comment for this same convention on the diff pane).
    let outside_range_style = find_cell_style(&terminal, "1 | fn a() {}", "fn");
    assert_eq!(Some(Color::Magenta), outside_range_style.fg);
    assert_eq!(Some(Color::Reset), outside_range_style.bg);
}

#[test]
fn should_fall_back_to_plain_source_style_when_file_extension_is_unrecognized() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("config.yaml"), "key: value\n").expect("write file");

    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "config.yaml".to_string(),
            symbols: vec![symbol("config.yaml::foo", "foo")],
        }],
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
    };
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        "config.yaml::foo",
        dir.path(),
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
            );
        })
        .expect("draw");

    // No highlight configuration exists for `.yaml`
    // (`highlight::config_for_path`), so the line still renders — with
    // the symbol-range background tint (highlighting failing must never
    // regress the pane below its pre-ADR-0018 plain style) but no
    // per-token foreground coloring (`Style::fg` reports an unset
    // foreground as `Some(Color::Reset)`, not `None` — same ratatui
    // `Cell` default convention as `bg`, see the test above).
    let text = buffer_text(&terminal);
    assert!(text.contains("key: value"));
    let style = find_cell_style(&terminal, "1 | key: value", "key");
    assert_eq!(Some(SOURCE_HIGHLIGHT_BG), style.bg);
    assert_eq!(Some(Color::Reset), style.fg);
}

#[test]
fn should_report_none_clamped_scroll_but_source_viewport_height_when_source_screen_is_open() {
    // ADR 0026: the source screen scrolls via its own
    // `Screen::Source::scroll_top`, not `App::right_pane_scroll`, so
    // `DrawOutcome::clamped_right_pane_scroll` must stay `None` on this
    // screen (otherwise `crate::run_app`'s
    // `clamp_right_pane_scroll_after_draw` would fold a source-screen
    // offset back into the wrong field). At the same time the source
    // pane's inner height must be surfaced via
    // `scroll_viewport_height` — otherwise `Ctrl-d`/`Ctrl-u`/`G` from
    // the source screen would have no viewport to size their step
    // against, defaulting to `DEFAULT_SOURCE_VIEWPORT_HEIGHT`.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    let mut actual = DrawOutcome {
        clamped_right_pane_scroll: Some(999),
        scroll_viewport_height: None,
        clamped_help_scroll: None,
        help_scroll_viewport_height: None,
    };
    terminal
        .draw(|frame| {
            actual = draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
            );
        })
        .expect("draw");

    assert_eq!(None, actual.clamped_right_pane_scroll);
    // A 20-row terminal with a 1-row status line leaves 19 rows for
    // the body; the source pane's bordered box takes 2 of them,
    // leaving 17 rows inside. Pinned exactly (rather than a range)
    // so a future layout refactor that silently changes the split
    // is caught, matching the specificity `ADR 0020`'s own
    // `right_pane_viewport_height` shares with `draw_entry_screen`.
    assert_eq!(Some(17), actual.scroll_viewport_height);
}
