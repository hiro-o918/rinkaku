//! ADR 0061 tab expansion for the detail pane's signature lines. A
//! `gofmt`-formatted multi-line signature (ADR 0060) carries real tab
//! characters on its continuation lines, and a terminal draws those as
//! zero cells — so the pane must expand them just as the Diff and Source
//! screens do.

use super::*;
use crate::ui::style::TAB_WIDTH;
use pretty_assertions::assert_eq;

fn draw_detail_for_signature(signature: &str, previous_signature: Option<&str>) -> String {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "main.go".to_string(),
            symbols: vec![ExtractedSymbol {
                signature: signature.to_string(),
                previous_signature: previous_signature.map(str::to_string),
                classification: previous_signature
                    .map(|_| rinkaku_core::extract::Classification::SignatureChanged),
                ..symbol("main.go::Run", "Run")
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
        .handle_key(crate::app::InputKey::ToggleDiff);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    buffer_text(&terminal)
}

/// The rendered indentation of the detail pane row carrying `token`,
/// measured in columns from the pane's own left border rather than from
/// the buffer's left edge (the entry pane occupies the columns to the
/// left of it), and after `marker` when the row carries a diff marker.
fn indent_in_detail_pane(buffer: &str, marker: &str, token: &str) -> usize {
    let row = buffer
        .lines()
        .find(|row| row.contains(token))
        .unwrap_or_else(|| panic!("expected a rendered row containing {token:?}"))
        .to_string();
    let token_offset = row.find(token).expect("token offset");
    let content_start = row[..token_offset]
        .rfind('│')
        .map(|border| border + '│'.len_utf8() + marker.len())
        .expect("detail pane border left of the token");
    assert_eq!(
        marker,
        &row[content_start - marker.len()..content_start],
        "rendered buffer:\n{buffer}"
    );
    row[content_start..token_offset].chars().count()
}

#[test]
fn should_render_tab_indented_signature_lines_at_tab_stop_columns_in_detail_pane() {
    let buffer = draw_detail_for_signature("func Run(\n\tctx context.Context,\n) error", None);

    assert_eq!(
        TAB_WIDTH,
        indent_in_detail_pane(&buffer, "", "ctx context.Context,"),
        "rendered buffer:\n{buffer}"
    );
}

#[test]
fn should_render_tab_indented_changed_signature_lines_at_tab_stop_columns_in_detail_pane() {
    let buffer = draw_detail_for_signature(
        "func Run(\n\tctx context.Context,\n\tcfg Config,\n) error",
        Some("func Run(\n\tctx context.Context,\n) error"),
    );

    assert_eq!(
        (TAB_WIDTH, TAB_WIDTH),
        (
            indent_in_detail_pane(&buffer, "- ", "ctx context.Context,"),
            indent_in_detail_pane(&buffer, "+ ", "cfg Config,"),
        ),
        "rendered buffer:\n{buffer}"
    );
}

#[test]
fn should_not_leave_tab_characters_in_the_rendered_buffer_when_signature_is_tab_indented() {
    let buffer = draw_detail_for_signature("func Run(\n\tctx context.Context,\n) error", None);

    let actual: Vec<char> = buffer.chars().filter(|c| *c == '\t').collect();

    assert_eq!(Vec::<char>::new(), actual);
}
