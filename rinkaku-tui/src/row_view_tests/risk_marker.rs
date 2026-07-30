use super::*;

// Visual-encoding prototype: a `!` risk co-occurrence marker prefixes a
// row's label when its badges show both a contract change and a fan-in
// clearing `HIGH_FAN_IN_THRESHOLD` — the combination that makes a change
// both hard to miss and wide-reaching.

#[test]
fn should_prepend_risk_marker_for_a_high_risk_dir_row() {
    let node = dir_node(
        "src",
        Badges {
            contract_changes: 1,
            fan_in: 2,
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

    assert_eq!("v ! src api:1 fan-in:2", line_text(&line));
    assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "!"));
}

#[test]
fn should_omit_risk_marker_when_contract_changes_is_zero() {
    let node = dir_node(
        "src",
        Badges {
            contract_changes: 0,
            fan_in: 5,
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

    assert_eq!("v src fan-in:5", line_text(&line));
}

#[test]
fn should_omit_risk_marker_when_fan_in_is_below_threshold() {
    let node = dir_node(
        "src",
        Badges {
            contract_changes: 1,
            fan_in: 1,
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

    assert_eq!("v src api:1 fan-in:1", line_text(&line));
}

#[test]
fn should_prepend_risk_marker_for_a_high_risk_file_row() {
    let node = file_node(
        "lib.rs",
        Badges {
            contract_changes: 1,
            fan_in: 2,
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
        "lib.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  ! lib.rs api:1 fan-in:2", line_text(&line));
}

#[test]
fn should_prepend_risk_marker_for_a_high_risk_signature_changed_symbol() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::SignatureChanged),
        ..plain_symbol("risky_fn")
    };
    let node = symbol_node(
        "lib.rs",
        symbol_ref,
        Badges {
            fan_in: 2,
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

    assert_eq!("  ~ fn ! risky_fn", line_text(&line));
}

#[test]
fn should_omit_risk_marker_for_a_signature_changed_symbol_below_fan_in_threshold() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::SignatureChanged),
        ..plain_symbol("changed_fn")
    };
    let node = symbol_node(
        "lib.rs",
        symbol_ref,
        Badges {
            fan_in: 1,
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

    assert_eq!("  ~ fn changed_fn", line_text(&line));
}

#[test]
fn should_omit_risk_marker_for_a_signature_changed_symbol_with_zero_test_count_and_low_fan_in() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::SignatureChanged),
        ..plain_symbol("changed_fn")
    };
    let node = symbol_node(
        "lib.rs",
        symbol_ref,
        Badges {
            fan_in: 1,
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

    assert_eq!("  ~ fn changed_fn tests:0", line_text(&line));
}
