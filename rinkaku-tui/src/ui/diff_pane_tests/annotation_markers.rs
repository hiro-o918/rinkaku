//! Positive-case coverage for the ADR 0048 annotation-marker column: every
//! other `diff_pane_lines`/`diff_pane_split_rows` test in this module
//! exercises an empty `AnnotationMarkers`, which pins only the "no marker
//! drawn" default — these tests populate `line_ranges` so the
//! `*`-marker/space-alignment branch itself is actually exercised.

use super::*;
use crate::diff_shape::AttributedHunk;

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

fn hunks_for(diff_text: &str) -> Vec<AttributedHunk> {
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let hunk = diff_files[0].hunks[0].clone();
    vec![AttributedHunk {
        source_index: 0,
        hunk,
    }]
}

#[test]
fn should_prefix_annotation_marker_only_on_the_line_inside_the_annotation_range_in_unified_view() {
    let hunks = hunks_for(DIFF_TEXT);
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    // New-side line 2 is the added "+fn foo() {}" line in DIFF_TEXT above.
    annotation_markers
        .line_ranges
        .insert("lib.rs".to_string(), vec![(2, 2)]);

    let lines = diff_pane_lines(&hunks, None, &annotation_markers, "lib.rs");

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
fn should_prefix_annotation_marker_only_on_the_new_side_in_split_view() {
    let hunks = hunks_for(DIFF_TEXT);
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    annotation_markers
        .line_ranges
        .insert("lib.rs".to_string(), vec![(2, 2)]);

    let (left, right) = diff_pane_split_rows(&hunks, None, &annotation_markers, "lib.rs");

    let left_rendered: Vec<String> = left.iter().map(line_text).collect();
    let right_rendered: Vec<String> = right.iter().map(line_text).collect();
    let marked_right = right_rendered
        .iter()
        .find(|line| line.contains("fn foo() {}"))
        .expect("added line present on the new side");
    assert!(marked_right.starts_with("*"));
    // The old side never carries the marker column at all (ADR 0048:
    // `AnnotationLocation`'s anchoring is new-side only) — every left-side line
    // stays exactly as it would with an empty `AnnotationMarkers`.
    assert!(left_rendered.iter().all(|line| !line.starts_with("*")));
}

/// A replaced line: the `-`/`+` pair share one new-side coordinate
/// (`crate::diff_view::new_side_positions`), which is what lets
/// `crate::diff_shape` scroll to the `-` half of a changed signature.
const REPLACEMENT_DIFF_TEXT: &str = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,2 @@
 fn a() {}
-fn foo(x: u32) {}
+fn foo(x: u64) {}
";

#[test]
fn should_not_prefix_annotation_marker_on_the_removed_half_of_a_replaced_line() {
    // An annotation anchors to a line that exists in the *new* file
    // (`crate::review::AnnotationLocation`), so the removed half of a
    // replacement must stay unmarked even though it shares the added
    // half's new-side coordinate.
    let hunks = hunks_for(REPLACEMENT_DIFF_TEXT);
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    annotation_markers
        .line_ranges
        .insert("lib.rs".to_string(), vec![(2, 2)]);

    let lines = diff_pane_lines(&hunks, None, &annotation_markers, "lib.rs");

    let rendered: Vec<String> = lines.iter().map(line_text).collect();
    let removed = rendered
        .iter()
        .find(|line| line.contains("fn foo(x: u32) {}"))
        .expect("removed line present");
    let added = rendered
        .iter()
        .find(|line| line.contains("fn foo(x: u64) {}"))
        .expect("added line present");
    assert_eq!((" ", "*"), (&removed[..1], &added[..1]));
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
