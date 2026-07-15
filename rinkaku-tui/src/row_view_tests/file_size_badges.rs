use super::*;

#[test]
fn should_render_lines_count_unstyled_when_file_has_normal_band() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/small.rs".to_string(),
        badges: Badges {
            own_file_size_band: Some(FileSizeBand::Normal),
            own_file_line_count: Some(80),
            ..Badges::default()
        },
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    };
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "small.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  small.rs lines:80", line_text(&line));
    assert_eq!(None, fg_of_span_with_content(&line, "80"));
}

#[test]
fn should_render_lines_count_in_yellow_when_file_has_watch_band() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/mid.rs".to_string(),
        badges: Badges {
            own_file_size_band: Some(FileSizeBand::Watch),
            own_file_line_count: Some(700),
            ..Badges::default()
        },
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    };
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "mid.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  mid.rs lines:700", line_text(&line));
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "700"));
}

#[test]
fn should_render_lines_count_in_red_when_file_has_warn_band() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/big.rs".to_string(),
        badges: Badges {
            own_file_size_band: Some(FileSizeBand::Warn),
            own_file_line_count: Some(1734),
            ..Badges::default()
        },
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    };
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "big.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  big.rs lines:1734", line_text(&line));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "1734"));
    assert_eq!(None, fg_of_span_with_content(&line, "lines:"));
}

#[test]
fn should_render_lines_count_in_bold_red_when_file_has_split_band() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/huge.rs".to_string(),
        badges: Badges {
            own_file_size_band: Some(FileSizeBand::Split),
            own_file_line_count: Some(4837),
            ..Badges::default()
        },
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    };
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "huge.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  huge.rs lines:4837", line_text(&line));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "4837"));
    let span = line
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "4837")
        .expect("line_count span must be present");
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn should_render_warn_and_split_labels_side_by_side_on_dir_row() {
    let node = dir_node(
        "src",
        Badges {
            file_size_warn_count: 2,
            file_size_split_count: 1,
            ..Badges::default()
        },
        vec![file_node("src/a.rs", Badges::default())],
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: true,
    };

    let line = entry_row_line(
        &row,
        "src",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("v src warn:2 split:1", line_text(&line));
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "2"));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "1"));
    assert_eq!(None, fg_of_span_with_content(&line, "warn:"));
    assert_eq!(None, fg_of_span_with_content(&line, "split:"));
}

#[test]
fn should_not_render_file_size_badge_when_file_node_has_no_band() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "lib.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    let text = line_text(&line);
    assert!(!text.contains("lines:"));
    assert!(!text.contains("warn:"));
    assert!(!text.contains("split:"));
}

#[test]
fn should_render_only_warn_label_on_dir_row_when_no_split_files() {
    let node = dir_node(
        "src",
        Badges {
            file_size_warn_count: 3,
            file_size_split_count: 0,
            ..Badges::default()
        },
        vec![file_node("src/a.rs", Badges::default())],
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: true,
    };

    let line = entry_row_line(
        &row,
        "src",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("v src warn:3", line_text(&line));
}
