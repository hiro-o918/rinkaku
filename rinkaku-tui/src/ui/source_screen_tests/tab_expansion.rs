use super::*;
use crate::ui::style::TAB_WIDTH;
use pretty_assertions::assert_eq;
use rinkaku_core::extract::ExtractedSymbol;

fn draw_source(path: &str, contents: &str) -> Terminal<TestBackend> {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join(path), contents).expect("write file");

    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: path.to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange { start: 1, end: 1 },
                ..symbol(&format!("{path}::run"), "run")
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
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        &format!("{path}::run"),
        dir.path(),
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    terminal
}

/// The rendered column at which `token` starts, measured from the first
/// column after the `"{line_number:>5} | "` gutter.
fn column_after_gutter(terminal: &Terminal<TestBackend>, line_number: usize, token: &str) -> usize {
    let gutter = format!("{line_number:>5} | ");
    let row = buffer_text(terminal)
        .lines()
        .find(|row| row.contains(&gutter))
        .unwrap_or_else(|| panic!("expected a rendered row containing {gutter:?}"))
        .to_string();
    let gutter_end = row.find(&gutter).expect("gutter offset") + gutter.len();
    let token_offset = row[gutter_end..]
        .find(token)
        .unwrap_or_else(|| panic!("expected {token:?} after the gutter in {row:?}"));
    row[gutter_end..gutter_end + token_offset].chars().count()
}

#[test]
fn should_render_tab_indented_go_lines_at_tab_stop_columns_in_source_screen() {
    let terminal = draw_source("main.go", "func run() {\n\tif ok {\n\t\tdeep()\n\t}\n}\n");
    let buffer = buffer_text(&terminal);

    assert_eq!(
        (TAB_WIDTH, TAB_WIDTH * 2),
        (
            column_after_gutter(&terminal, 2, "if"),
            column_after_gutter(&terminal, 3, "deep"),
        ),
        "rendered buffer:\n{buffer}"
    );
}

#[test]
fn should_expand_tabs_on_the_unhighlighted_path_when_extension_is_unknown_in_source_screen() {
    // A `Makefile` has no bundled grammar, so no token spans exist and the
    // line renders through `gap_span` rather than `styled_content_spans`.
    let terminal = draw_source("Makefile", "build:\n\tcargo build\n");

    assert_eq!(TAB_WIDTH, column_after_gutter(&terminal, 2, "cargo"));
}

#[test]
fn should_not_leave_tab_characters_in_the_rendered_buffer_when_source_is_tab_indented() {
    let terminal = draw_source("main.go", "func run() {\n\t\tdeep()\n}\n");

    let actual: Vec<char> = buffer_text(&terminal)
        .chars()
        .filter(|c| *c == '\t')
        .collect();

    assert_eq!(Vec::<char>::new(), actual);
}
