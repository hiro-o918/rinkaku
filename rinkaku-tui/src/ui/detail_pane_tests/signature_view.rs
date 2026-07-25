//! `detail_lines`'s `SignatureView` rendering (ADR 0060): a multi-line
//! signature must produce one `Line` per source line rather than a
//! single `Line` whose text contains a literal embedded `\n` — `Line`
//! itself never splits on `\n` (it renders as one row), so this behavior
//! has to be built by pushing multiple `Line`s, not left to ratatui.

use crate::detail::{DetailView, SignatureView};
use crate::ui::detail_pane::detail_lines;

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

#[test]
fn should_push_one_line_per_source_line_for_current_multiline_signature() {
    let detail = detail_view(SignatureView::Current(
        "struct Point {\n    x: i32,\n    y: i32,\n}".to_string(),
    ));

    let lines = detail_lines(&detail);
    let rendered: Vec<String> = lines.iter().map(|line| line.to_string()).collect();

    assert!(rendered.contains(&"struct Point {".to_string()));
    assert!(rendered.contains(&"    x: i32,".to_string()));
    assert!(rendered.contains(&"    y: i32,".to_string()));
    assert!(rendered.contains(&"}".to_string()));
    // No line should carry an embedded newline — each source line is its
    // own `Line`, not one `Line` with `\n` baked into its text.
    assert!(rendered.iter().all(|line| !line.contains('\n')));
}

#[test]
fn should_push_one_line_per_source_line_for_each_side_of_a_changed_multiline_signature() {
    let detail = detail_view(SignatureView::Changed {
        previous: "struct Point {\n    x: i32,\n}".to_string(),
        current: "struct Point {\n    x: i32,\n    y: i32,\n}".to_string(),
    });

    let lines = detail_lines(&detail);
    let rendered: Vec<String> = lines.iter().map(|line| line.to_string()).collect();

    assert!(rendered.contains(&"- struct Point {".to_string()));
    assert!(rendered.contains(&"-     x: i32,".to_string()));
    assert!(rendered.contains(&"- }".to_string()));
    assert!(rendered.contains(&"+ struct Point {".to_string()));
    assert!(rendered.contains(&"+     x: i32,".to_string()));
    assert!(rendered.contains(&"+     y: i32,".to_string()));
    assert!(rendered.contains(&"+ }".to_string()));
    assert!(rendered.iter().all(|line| !line.contains('\n')));
}
