use super::{FileSizeBand, markers_legend_lines};
use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::ui::draw;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

#[test]
fn should_render_api_badge_swatch_with_yellow_number_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let api_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "api:")
        })
        .expect("api: line present");

    // NOTE: partial assert — a `Line` built from `format!` interpolation
    // doesn't have one clean expected `Line` value to compare as a
    // whole (the explanation half is plain, unstyled text pulled
    // straight from `help::MARKER_LEGEND`), so this only pins the
    // swatch's number span style, which is the thing this test exists
    // to guard.
    let number_span = line_span(api_line, "N");
    assert_eq!(Style::default().fg(Color::Yellow), number_span.style);
}

#[test]
fn should_render_fan_in_badge_swatch_with_cyan_number_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let fan_in_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "fan-in:")
        })
        .expect("fan-in: line present");

    let number_span = line_span(fan_in_line, "N");
    assert_eq!(Style::default().fg(Color::Cyan), number_span.style);
}

#[test]
fn should_render_warn_badge_swatch_with_yellow_number_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let warn_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "warn:")
        })
        .expect("warn: line present");

    let number_span = line_span(warn_line, "N");
    assert_eq!(Style::default().fg(Color::Yellow), number_span.style);
}

#[test]
fn should_render_split_badge_swatch_with_red_number_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let split_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "split:")
        })
        .expect("split: line present");

    let number_span = line_span(split_line, "N");
    assert_eq!(Style::default().fg(Color::Red), number_span.style);
}

#[test]
fn should_render_signature_changed_marker_swatch_with_yellow_tilde_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let changed_line = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "~"))
        .expect("~ line present");

    let swatch_span = line_span(changed_line, "~");
    assert_eq!(Style::default().fg(Color::Yellow), swatch_span.style);
}

#[test]
fn should_render_cycle_marker_swatch_bold_yellow_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let cycle_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "(cycle)")
        })
        .expect("(cycle) line present");

    let swatch_span = line_span(cycle_line, "(cycle)");
    assert_eq!(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        swatch_span.style
    );
}

#[test]
fn should_render_added_marker_swatch_with_green_plus_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let added_line = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "+"))
        .expect("+ line present");

    let swatch_span = line_span(added_line, "+");
    assert_eq!(Style::default().fg(Color::Green), swatch_span.style);
}

#[test]
fn should_render_removed_marker_swatch_with_red_x_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let removed_line = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "x"))
        .expect("x line present");

    let swatch_span = line_span(removed_line, "x");
    assert_eq!(Style::default().fg(Color::Red), swatch_span.style);
}

#[test]
fn should_render_risk_marker_swatch_bold_red_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let risk_line = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content.as_ref() == "!"))
        .expect("! line present");

    let swatch_span = line_span(risk_line, "!");
    assert_eq!(
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        swatch_span.style
    );
}

#[test]
fn should_render_dimmed_and_crossed_out_removed_name_swatch_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "(dimmed + struck-through name)")
        })
        .expect("removed-name swatch line present");

    let swatch_span = line_span(line, "(dimmed + struck-through name)");
    assert_eq!(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT),
        swatch_span.style
    );
}

#[test]
fn should_reuse_row_view_band_style_for_lines_swatch_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "lines:N")
        })
        .expect("lines:N line present");

    let swatch_span = line_span(line, "lines:N");
    assert_eq!(
        crate::row_view::band_style(FileSizeBand::Watch),
        swatch_span.style
    );
}

#[test]
fn should_render_test_badge_swatch_with_magenta_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "[test] (N symbols)")
        })
        .expect("[test] (N symbols) line present");

    let swatch_span = line_span(line, "[test] (N symbols)");
    assert_eq!(Style::default().fg(Color::Magenta), swatch_span.style);
}

#[test]
fn should_render_test_group_swatch_dark_gray_when_building_markers_legend() {
    let lines = markers_legend_lines(Locale::English);

    let line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "N tests")
        })
        .expect("N tests line present");

    let swatch_span = line_span(line, "N tests");
    assert_eq!(Style::default().fg(Color::DarkGray), swatch_span.style);
}

fn line_span<'a>(line: &'a ratatui::text::Line<'static>, content: &str) -> &'a Span<'static> {
    line.spans
        .iter()
        .find(|span| span.content.as_ref() == content)
        .unwrap_or_else(|| panic!("span {content:?} not found in line"))
}

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

/// Every [`crate::help::HelpGroup`] title string, in [`crate::help::keymap_groups`]'s
/// own display order — the full, unfiltered set this crate's group-filtering
/// tests below check subsets of.
const ALL_GROUP_TITLES: [&str; 5] = [
    "Tree focus",
    "Right focus",
    "Source view",
    "Review",
    "Global",
];

/// Which of [`ALL_GROUP_TITLES`] are present in `text` (the overlay's
/// rendered buffer), preserving [`ALL_GROUP_TITLES`]'s own order — the
/// fully-qualified value each context test below compares against, rather
/// than one `assert!(text.contains(...))` per heading.
fn present_group_titles(text: &str) -> Vec<&'static str> {
    ALL_GROUP_TITLES
        .into_iter()
        .filter(|title| text.contains(title))
        .collect()
}

#[test]
fn should_draw_help_overlay_with_keymap_markers_and_glossary_when_help_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    assert_eq!(crate::app::Focus::Tree, app.focus());
    // A 150x76 terminal (up from 150x74): taller again so the
    // overlay's 80% x 90% area still fits every keymap group —
    // now including the `U` update-prompt binding (ADR 0054) in the
    // "Global" group — the Markers legend, and the trailing Glossary
    // section without the last section being pushed off the bottom.
    // Grown here rather than narrowing the content itself, same
    // rationale as the 100x40 -> 100x50 -> 150x70 -> 150x74 growths
    // this test already went through for earlier keymap additions.
    let mut terminal = Terminal::new(TestBackend::new(150, 76)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Help"));
    // Tree focus (Focus::Tree, ADR 0020) hides the Right-focus and
    // Source-view groups, which are only ever reachable from a different
    // focus/screen (`crate::help::is_group_applicable`'s own doc comment).
    assert_eq!(
        vec!["Tree focus", "Review", "Global"],
        present_group_titles(&text)
    );
    assert!(text.contains("Markers"));
    assert!(text.contains("fan-in:N"));
    assert!(text.contains("Glossary"));
    assert!(text.contains("blast radius"));
}

#[test]
fn should_hide_tree_focus_and_source_view_groups_when_right_focused_on_diff_pane() {
    let report = report_with_one_symbol();
    // `Open` on the symbol row (cursor starts there, `report_with_one_symbol`'s
    // own single file/symbol tree) reaches Focus::Right + RightPane::Diff
    // (ADR 0020) without leaving Screen::Entry.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleHelp);
    assert_eq!(crate::app::Focus::Right, app.focus());
    let mut terminal = Terminal::new(TestBackend::new(150, 76)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert_eq!(
        vec!["Right focus", "Review", "Global"],
        present_group_titles(&text)
    );
}

#[test]
fn should_show_only_source_view_and_global_groups_on_source_screen() {
    let report = report_with_one_symbol();
    // `report_with_one_symbol`'s tree starts with the cursor on the file
    // row; `Down` moves it onto the symbol row `Source` (`s`) requires
    // (`App::handle_key`'s own `InputKey::Source` arm only fires on a
    // `NodeKind::Symbol` row) — the same sequence
    // `should_show_source_view_scroll_hints_on_source_screen_regardless_of_focus`
    // in `ui::status`'s own tests already uses.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source)
        .handle_key(crate::app::InputKey::ToggleHelp);
    assert!(matches!(app.screen(), crate::app::Screen::Source { .. }));
    let mut terminal = Terminal::new(TestBackend::new(150, 76)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // Review is hidden on the source screen: `n`/`N` (NoteCompose/NotesList)
    // only dispatch on Screen::Entry (`review_flow::derive_selection_snapshot`,
    // `App::handle_key`'s own `NotesList` arm).
    assert_eq!(vec!["Source view", "Global"], present_group_titles(&text));
}

#[test]
fn should_draw_help_overlay_in_japanese_when_locale_is_japanese() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(150, 76)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::Japanese,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // `TestBackend`'s buffer devotes two cells to every double-width CJK
    // glyph (the second cell an empty-symbol placeholder that
    // `Cell::symbol` renders as a literal space), so a run of CJK
    // characters reads back with one extra space after each double-width
    // char — `widened` reproduces exactly that spacing (single-width
    // chars are left as-is) so the expected/actual comparison does not
    // have to special-case it per assertion.
    let widened = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if unicode_width::UnicodeWidthChar::width(c) == Some(2) {
                    format!("{c} ")
                } else {
                    c.to_string()
                }
            })
            .collect()
    };

    // One key-binding description (move-cursor, "Tree focus" group).
    assert!(text.contains(&widened("カーソルを移動")));
    // One marker-legend entry (fan-in:N's explanation).
    assert!(text.contains(&widened("高 fan-in シンボルの used_by 数の合計")));
    // One glossary entry (blast radius).
    assert!(text.contains(&widened("依存ツリー")));
    // The overlay title and section headings switch too, proving this is
    // a locale switch and not just one string happening to match.
    assert!(text.contains(&widened("ヘルプ")));
    assert!(text.contains(&widened("用語集")));
    // English text from the same keys must not leak into the Japanese
    // render.
    assert!(!text.contains("Move the cursor"));
}

#[test]
fn should_not_show_glossary_when_terminal_is_too_short_to_fit_the_whole_keymap_and_scroll_is_zero()
{
    // A small terminal (30 rows) whose overlay box cannot fit every
    // keymap group *and* the trailing Glossary section at once — the
    // gap this feature exists to close (previously: the unscrolled
    // `Paragraph` simply clipped the bottom of the content with no way
    // to reach it, `draw_help_overlay`'s pre-scroll doc comment).
    // Pinning that the Glossary is *not* visible at scroll 0 here, and
    // *is* visible after scrolling in the next test, is what proves
    // scrolling actually moves the rendered content rather than the
    // box merely being tall enough by coincidence.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Tree focus"));
    assert!(!text.contains("Glossary"), "Glossary should not fit yet");
}

#[test]
fn should_reveal_glossary_after_scrolling_down_when_terminal_is_too_short_to_fit_the_whole_keymap()
{
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    // Scroll well past every keymap group — `handle_scroll_key`'s own
    // clamp-free "requested" semantics mean this overshoots on
    // purpose; `render_scrollable_pane`'s clamp inside `draw` below is
    // what brings it back in bounds, mirroring how every other
    // scrollable pane in this crate is exercised in its own tests.
    let app = app.handle_scroll_key(crate::app::InputKey::ScrollToBottom, 20);

    let mut actual_outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            actual_outcome = draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(
        text.contains("Glossary"),
        "Glossary should be visible after scrolling to the bottom"
    );
    assert!(
        text.contains("jumplist"),
        "the last glossary entry should be visible at the bottom"
    );
    // The scroll actually applied must be reported back (`DrawOutcome`'s
    // own doc comment on why `crate::run_app` needs this to fold the
    // overshot request back down) rather than staying at the
    // unclamped `usize::MAX` sentinel `ScrollToBottom` set.
    assert!(actual_outcome.clamped_help_scroll.is_some());
    assert_ne!(Some(usize::MAX), actual_outcome.clamped_help_scroll);
    assert!(actual_outcome.help_scroll_viewport_height.is_some());
}

#[test]
fn should_report_none_clamped_help_scroll_and_none_viewport_height_when_help_overlay_is_closed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    let mut actual_outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            actual_outcome = draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    assert_eq!(None, actual_outcome.clamped_help_scroll);
    assert_eq!(None, actual_outcome.help_scroll_viewport_height);
}

#[test]
fn should_not_draw_help_overlay_when_help_is_closed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(!text.contains("Glossary"));
}
