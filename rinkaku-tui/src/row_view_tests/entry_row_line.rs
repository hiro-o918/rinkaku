use super::*;

#[test]
fn should_render_plain_text_for_zero_badges_and_no_classification() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let row = Row {
        node: &node,
        depth: 2,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("        fn foo", line_text(&line));
}

#[test]
fn should_include_badge_labels_for_nonzero_badges_on_a_dir_row() {
    // ADR 0013 amendments (2026-07-13): all three badges use text labels
    // (`chg:` / `api:` / `fan-in:`) instead of glyphs — the first
    // amendment relabeled changed-symbol/fan-in (as `ref:`, later
    // relabeled `fan-in:` per ADR 0034), the second (feat/label-contract-
    // changes-badge) relabeled contract-change from a bare `!` after
    // user testing showed it read as generic "warning" with no hint of
    // what changed. `fan_in: 1` here (rather than a higher, still-nonzero
    // value) deliberately stays below `HIGH_FAN_IN_THRESHOLD` so this
    // fixture does not also trigger the `!` risk marker tested separately
    // in `should_prepend_risk_marker_for_a_high_risk_dir_row` — this test's
    // only concern is the three badges' own label wording.
    let node = dir_node(
        "src",
        Badges {
            changed_symbols: 2,
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

    assert_eq!("v src chg:2 api:1 fan-in:1", line_text(&line));
}

#[test]
fn should_omit_zero_badges_entirely() {
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

    assert_eq!("  lib.rs ", line_text(&line));
}

#[test]
fn should_append_skip_reason_for_a_skipped_file_row() {
    let node = skipped_file_node("assets/logo.png", rinkaku_core::render::SkipReason::Binary);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "assets/logo.png",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  assets/logo.png  (skipped: binary)", line_text(&line));
}

#[test]
fn should_dim_label_for_a_skipped_file_row() {
    let node = skipped_file_node("assets/logo.png", rinkaku_core::render::SkipReason::Binary);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "assets/logo.png",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    // The label span is the third span: indent, expand marker, label.
    assert_eq!(Some(Color::DarkGray), line.spans[2].style.fg);
}

#[test]
fn should_not_append_skip_reason_for_an_ordinary_file_row() {
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

    assert!(!line_text(&line).contains("skipped"));
}

#[test]
fn should_prepend_test_badge_with_plural_symbols_noun_before_a_whole_test_file_label() {
    // The test badge sits before the file label (left of the name),
    // not after the trailing badges — a badge trailing a long label
    // gets clipped first when the row overflows the pane width, but a
    // reviewer still needs "this is a test file" to be visible at a
    // glance, so it must survive truncation.
    let node = test_file_node("src/lib_test.go", 3);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "src/lib_test.go",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  [test] (3 symbols) src/lib_test.go ", line_text(&line));
}

#[test]
fn should_prepend_test_badge_with_singular_symbol_noun_when_count_is_one() {
    let node = test_file_node("src/lib_test.go", 1);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "src/lib_test.go",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  [test] (1 symbol) src/lib_test.go ", line_text(&line));
}

#[test]
fn should_show_collapse_marker_when_dir_is_not_expanded() {
    let node = dir_node(
        "src",
        Badges::default(),
        vec![file_node("src/a.rs", Badges::default())],
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "src",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("> src ", line_text(&line));
}

// ADR 0035 Phase B: a `NodeKind::Section` row renders like a `Dir` row
// (expand marker, bold label, aggregated badges) but with its fixed
// `SectionKind::label()` instead of a path-derived label, and never
// shows the `(cycle)` marker — `ranks` never carries an entry for a
// section's synthetic path (`crate::order::rank_directories` only ever
// produces entries for real file-tree directories).

#[test]
fn should_render_section_row_with_its_fixed_label_and_badges() {
    let node = section_node(
        SectionKind::Tests,
        Badges {
            changed_symbols: 5,
            ..Badges::default()
        },
        vec![file_node("a_test.go", Badges::default())],
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: true,
    };

    let line = entry_row_line(
        &row,
        "ignored-label",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("v Tests chg:5", line_text(&line));
}

#[test]
fn should_show_collapse_marker_for_a_collapsed_section_row() {
    let node = section_node(
        SectionKind::Tests,
        Badges::default(),
        vec![file_node("a_test.go", Badges::default())],
    );
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "ignored-label",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("> Tests ", line_text(&line));
}

#[test]
fn should_never_show_cycle_marker_on_a_section_row_even_if_ranks_has_a_stray_entry() {
    // Defensive: even if `ranks` somehow carried an entry keyed by the
    // section's synthetic path, a Section row must never render
    // `(cycle)` — cycle detection is a production-directory concept
    // ADR 0035 Phase A already excludes test code from.
    let node = section_node(SectionKind::Tests, Badges::default(), vec![]);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        crate::tree::TESTS_SECTION_PATH.to_string(),
        DirRank {
            rank: 0,
            in_cycle: true,
        },
    );

    let line = entry_row_line(
        &row,
        "ignored-label",
        &ranks,
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  Tests ", line_text(&line));
}

#[test]
fn should_append_cycle_marker_when_dir_path_is_in_cycle() {
    let node = dir_node("src", Badges::default(), vec![]);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        "src".to_string(),
        DirRank {
            rank: 0,
            in_cycle: true,
        },
    );

    let line = entry_row_line(
        &row,
        "src",
        &ranks,
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  src  (cycle)", line_text(&line));
}

#[test]
fn should_not_append_cycle_marker_when_dir_path_is_not_in_cycle() {
    let node = dir_node("src", Badges::default(), vec![]);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut ranks = HashMap::new();
    ranks.insert(
        "src".to_string(),
        DirRank {
            rank: 0,
            in_cycle: false,
        },
    );

    let line = entry_row_line(
        &row,
        "src",
        &ranks,
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("  src ", line_text(&line));
}

#[test]
fn should_mark_added_symbol_with_plus() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::Added),
        ..plain_symbol("new_fn")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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

    assert_eq!("  + fn new_fn", line_text(&line));
}

#[test]
fn should_mark_signature_changed_symbol_with_tilde() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::SignatureChanged),
        ..plain_symbol("changed_fn")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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
fn should_mark_removed_symbol_with_x() {
    let symbol_ref = SymbolRef {
        removed: true,
        ..plain_symbol("gone_fn")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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

    assert_eq!("  x fn gone_fn", line_text(&line));
}

// Visual-encoding prototype: a test symbol left in the production tree
// (only possible for a *mixed* file, nested under a synthetic
// `TestGroup` — see `crate::tree::NodeKind::TestGroup`'s doc comment) no
// longer carries its own trailing `test` badge — group membership
// already conveys that — but its name still renders dimmed (DarkGray),
// same as a body-only symbol.

#[test]
fn should_dim_name_and_omit_test_badge_for_a_test_symbol_in_a_mixed_file() {
    let symbol_ref = SymbolRef {
        is_test: true,
        ..plain_symbol("test_it")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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

    assert_eq!("    fn test_it", line_text(&line));
    assert_eq!(
        Some(Color::DarkGray),
        fg_of_span_with_content(&line, "test_it")
    );
}

#[test]
fn should_not_append_test_badge_for_an_ordinary_non_test_symbol() {
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

#[test]
fn should_apply_reversed_modifier_when_row_is_selected() {
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
        true,
    );

    assert!(line.style.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn should_not_apply_reversed_modifier_when_row_is_not_selected() {
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

    assert!(!line.style.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn should_indent_by_depth_times_indent_width() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 3,
        expanded: false,
    };

    let line = entry_row_line(
        &row,
        "lib.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("        lib.rs ", line_text(&line));
}

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

// Visual-encoding prototype: a body-only (or unclassified) symbol's name
// dims to DarkGray — it carries less review weight than an added/
// signature-changed/removed symbol, since its signature didn't change.

#[test]
fn should_dim_name_for_a_body_only_symbol() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::BodyOnly),
        ..plain_symbol("touched_fn")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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

    assert_eq!(
        Some(Color::DarkGray),
        fg_of_span_with_content(&line, "touched_fn")
    );
}

#[test]
fn should_not_dim_name_for_an_added_symbol() {
    let symbol_ref = SymbolRef {
        classification: Some(Classification::Added),
        ..plain_symbol("new_fn")
    };
    let node = symbol_node("lib.rs", symbol_ref, Badges::default());
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

    assert_eq!(None, fg_of_span_with_content(&line, "new_fn"));
}

// Visual-encoding prototype: a `TestGroup` row renders a plain `N tests`
// (or `1 test` singular) label in DarkGray, no expand-marker badges.

#[test]
fn should_render_test_group_row_with_plural_count() {
    let node = TreeNode {
        kind: NodeKind::TestGroup { count: 3 },
        path: "lib.rs::tests".to_string(),
        badges: Badges::default(),
        children: vec![symbol_node(
            "lib.rs",
            plain_symbol("test_it"),
            Badges::default(),
        )],
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
        "ignored-label",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("> 3 tests", line_text(&line));
    assert_eq!(
        Some(Color::DarkGray),
        fg_of_span_with_content(&line, "3 tests")
    );
}

#[test]
fn should_render_test_group_row_with_singular_count() {
    let node = TreeNode {
        kind: NodeKind::TestGroup { count: 1 },
        path: "lib.rs::tests".to_string(),
        badges: Badges::default(),
        children: vec![symbol_node(
            "lib.rs",
            plain_symbol("test_it"),
            Badges::default(),
        )],
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
        "ignored-label",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        false,
    );

    assert_eq!("> 1 test", line_text(&line));
}

#[test]
fn should_show_annotation_badge_on_a_symbol_row_with_a_matching_annotation_count() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    annotation_markers
        .symbol_counts
        .insert("lib.rs::foo".to_string(), 2);

    let line = entry_row_line(&row, "", &HashMap::new(), &annotation_markers, false);

    assert_eq!("    fn foo ann:2", line_text(&line));
}

#[test]
fn should_omit_annotation_badge_on_a_symbol_row_with_no_matching_annotation_count() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    annotation_markers
        .symbol_counts
        .insert("lib.rs::bar".to_string(), 1);

    let line = entry_row_line(&row, "", &HashMap::new(), &annotation_markers, false);

    assert_eq!("    fn foo", line_text(&line));
}

#[test]
fn should_show_annotation_badge_on_a_file_row_with_a_matching_annotation_count() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };
    let mut annotation_markers = crate::annotation_markers::AnnotationMarkers::default();
    annotation_markers
        .file_counts
        .insert("lib.rs".to_string(), 3);

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), &annotation_markers, false);

    assert_eq!("  lib.rs  ann:3", line_text(&line));
}
