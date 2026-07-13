// File-size warning badges (ADR 0028): a file row carrying a warning
// renders `lines:{N}` with the numeric N colored by severity (yellow
// for Warn, red for Split), and a directory row aggregates the
// per-severity counts as `warn:N split:N` with the numbers colored
// the same way. No emoji glyphs — the color already conveys severity
// and emoji rendering width is inconsistent across terminals.

use super::*;

#[test]
fn should_render_lines_count_in_yellow_when_file_has_warn_severity() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/big.rs".to_string(),
        badges: Badges {
            own_file_size_severity: Some(FileSizeSeverity::Warn),
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

    let line = entry_row_line(&row, "big.rs", &HashMap::new(), false);

    assert_eq!("  big.rs lines:1734", line_text(&line));
    // The numeric 1734 span carries the severity color; the leading
    // "lines:" label stays uncolored so the eye lands on the number.
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "1734"));
    assert_eq!(None, fg_of_span_with_content(&line, "lines:"));
}

#[test]
fn should_render_lines_count_in_red_when_file_has_split_severity() {
    let node = TreeNode {
        kind: NodeKind::File,
        path: "src/huge.rs".to_string(),
        badges: Badges {
            own_file_size_severity: Some(FileSizeSeverity::Split),
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

    let line = entry_row_line(&row, "huge.rs", &HashMap::new(), false);

    assert_eq!("  huge.rs lines:4837", line_text(&line));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "4837"));
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

    let line = entry_row_line(&row, "src", &HashMap::new(), false);

    assert_eq!("v src warn:2 split:1", line_text(&line));
    // The numeric part of each half picks up its own severity color;
    // the "warn:" / "split:" labels themselves stay uncolored so the
    // eye lands on the counts.
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "2"));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "1"));
    assert_eq!(None, fg_of_span_with_content(&line, "warn:"));
    assert_eq!(None, fg_of_span_with_content(&line, "split:"));
}

#[test]
fn should_not_render_file_size_badge_when_file_node_has_no_warning() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

    let text = line_text(&line);
    assert!(!text.contains("lines:"));
    assert!(!text.contains("warn:"));
    assert!(!text.contains("split:"));
}

#[test]
fn should_render_only_warn_label_on_dir_row_when_no_split_files() {
    // When only one severity is present under a directory, only that
    // half of the badge shows — the other half is omitted rather
    // than rendered as "warn:0" or "split:0".
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

    let line = entry_row_line(&row, "src", &HashMap::new(), false);

    assert_eq!("v src warn:3", line_text(&line));
}
