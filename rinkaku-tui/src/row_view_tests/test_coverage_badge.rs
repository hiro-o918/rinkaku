use super::*;

#[test]
fn should_append_tests_badge_when_symbol_has_zero_test_count() {
    let node = symbol_node(
        "lib.rs",
        plain_symbol("foo"),
        Badges {
            test_count: Some(0),
            ..Badges::default()
        },
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("    fn foo tests:0", line_text(&line));
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "0"));
}

#[test]
fn should_omit_tests_badge_when_symbol_has_nonzero_test_count() {
    let node = symbol_node(
        "lib.rs",
        plain_symbol("foo"),
        Badges {
            test_count: Some(2),
            ..Badges::default()
        },
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("    fn foo", line_text(&line));
}

#[test]
fn should_omit_tests_badge_when_symbol_has_no_test_coverage_entry() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("    fn foo", line_text(&line));
}
