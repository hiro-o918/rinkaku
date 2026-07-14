//! Positive-case coverage for the ADR 0048 note-marker column: every other
//! `diff_pane_lines`/`diff_pane_split_rows` test in this module exercises
//! an empty `NoteMarkers`, which pins only the "no marker drawn" default —
//! these tests populate `line_ranges` so the `*`-marker/space-alignment
//! branch itself is actually exercised.

use super::*;
use crate::diff_shape::{AttributedHunk, DiffSection};

const DIFF_TEXT: &str = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn foo() {}
 fn b() {}
";

fn section_for(diff_text: &str) -> DiffSection {
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let hunk = diff_files[0].hunks[0].clone();
    DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![AttributedHunk {
            source_index: 0,
            hunk,
        }],
    }
}

#[test]
fn should_prefix_note_marker_only_on_the_line_inside_the_notes_range_in_unified_view() {
    let section = section_for(DIFF_TEXT);
    let mut note_markers = crate::note_markers::NoteMarkers::default();
    // New-side line 2 is "+fn foo() {}" (line 1 is context "fn a() {}",
    // unchanged; line 3 is context "fn b() {}").
    note_markers
        .line_ranges
        .insert("lib.rs".to_string(), vec![(2, 2)]);

    let lines = diff_pane_lines(&[&section], true, None, &note_markers, "lib.rs");

    let rendered: Vec<String> = lines.iter().map(line_text).collect();
    let marked = rendered
        .iter()
        .find(|line| line.contains("fn foo() {}"))
        .expect("added line present");
    assert!(marked.starts_with("*"));
    let unmarked_context = rendered
        .iter()
        .find(|line| line.contains("fn b() {}"))
        .expect("context line present");
    assert!(unmarked_context.starts_with(" "));
}

#[test]
fn should_prefix_note_marker_only_on_the_new_side_in_split_view() {
    let section = section_for(DIFF_TEXT);
    let mut note_markers = crate::note_markers::NoteMarkers::default();
    note_markers
        .line_ranges
        .insert("lib.rs".to_string(), vec![(2, 2)]);

    let (left, right) = diff_pane_split_rows(&[&section], true, None, &note_markers, "lib.rs");

    let left_rendered: Vec<String> = left.iter().map(line_text).collect();
    let right_rendered: Vec<String> = right.iter().map(line_text).collect();
    let marked_right = right_rendered
        .iter()
        .find(|line| line.contains("fn foo() {}"))
        .expect("added line present on the new side");
    assert!(marked_right.starts_with("*"));
    // The old side never carries the marker column at all (ADR 0048:
    // `NoteLocation`'s anchoring is new-side only) — every left-side line
    // stays exactly as it would with an empty `NoteMarkers`.
    assert!(left_rendered.iter().all(|line| !line.starts_with("*")));
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
