//! `detail_lines`'s `SignatureView` rendering (ADR 0060): a multi-line
//! signature must produce one `Line` per source line rather than a
//! single `Line` whose text contains a literal embedded `\n` — `Line`
//! itself never splits on `\n` (it renders as one row), so this behavior
//! has to be built by pushing multiple `Line`s, not left to ratatui.

use crate::detail::{DetailView, SignatureView};
use crate::ui::detail_pane::detail_lines;

use pretty_assertions::assert_eq;
use rinkaku_core::extract::SymbolKind;

fn detail_view(signature: SignatureView) -> DetailView {
    DetailView {
        id: "lib.rs::Point".to_string(),
        name: "Point".to_string(),
        kind: SymbolKind::Struct,
        path: "lib.rs".to_string(),
        container: None,
        signature,
        classification: None,
        used_by: vec![],
        callees: vec![],
        callers: vec![],
    }
}

/// Renders `detail_lines`'s output down to its text content, one `String`
/// per `Line` — styling (bold headings, red/green diff coloring) is
/// covered by the render code's own visual intent rather than pinned
/// here, since this suite's concern is which lines get pushed and in what
/// order.
fn rendered_text(detail: &DetailView) -> Vec<String> {
    detail_lines(detail)
        .iter()
        .map(|line| line.to_string())
        .collect()
}

#[test]
fn should_push_one_line_per_source_line_for_current_multiline_signature() {
    let detail = detail_view(SignatureView::Current(
        "struct Point {\n    x: i32,\n    y: i32,\n}".to_string(),
    ));

    let actual = rendered_text(&detail);

    let expected = vec![
        "Struct Point".to_string(),
        "lib.rs".to_string(),
        "".to_string(),
        "".to_string(),
        "struct Point {".to_string(),
        "    x: i32,".to_string(),
        "    y: i32,".to_string(),
        "}".to_string(),
        "".to_string(),
        "Used by (0)".to_string(),
        "".to_string(),
        "Callees (0)".to_string(),
    ];
    assert_eq!(expected, actual);
}

#[test]
fn should_push_one_line_per_source_line_for_each_side_of_a_changed_multiline_signature() {
    let detail = detail_view(SignatureView::Changed {
        previous: "struct Point {\n    x: i32,\n}".to_string(),
        current: "struct Point {\n    x: i32,\n    y: i32,\n}".to_string(),
    });

    let actual = rendered_text(&detail);

    let expected = vec![
        "Struct Point".to_string(),
        "lib.rs".to_string(),
        "".to_string(),
        "".to_string(),
        "- struct Point {".to_string(),
        "-     x: i32,".to_string(),
        "- }".to_string(),
        "+ struct Point {".to_string(),
        "+     x: i32,".to_string(),
        "+     y: i32,".to_string(),
        "+ }".to_string(),
        "".to_string(),
        "Used by (0)".to_string(),
        "".to_string(),
        "Callees (0)".to_string(),
    ];
    assert_eq!(expected, actual);
}
