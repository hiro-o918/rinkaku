use super::*;

#[test]
fn should_render_plain_text_for_zero_badges_and_no_classification() {
    let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
    let row = Row {
        node: &node,
        depth: 2,
        expanded: false,
    };

    let line = entry_row_line(&row, "", &HashMap::new(), false);

    assert_eq!("        fn foo", line_text(&line));
}

#[test]
fn should_include_badge_labels_for_nonzero_badges_on_a_dir_row() {
    // ADR 0013 amendment (2026-07-13): the changed-symbol and fan-in
    // badges use `chg:` / `ref:` text labels instead of the original
    // `~` / `^` glyphs. `!{N}` (contract-change count) is
    // intentionally left as a compact glyph — see `push_badge_spans`'
    // doc comment for the scope split.
    let node = dir_node(
        "src",
        Badges {
            changed_symbols: 2,
            contract_changes: 1,
            fan_in: 3,
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

    assert_eq!("v src chg:2 !1 ref:3", line_text(&line));
}

#[test]
fn should_omit_zero_badges_entirely() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "assets/logo.png", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "assets/logo.png", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

    assert!(!line_text(&line).contains("skipped"));
}

#[test]
fn should_append_test_badge_with_plural_symbols_noun_for_a_whole_test_file_row() {
    let node = test_file_node("src/lib_test.go", 3);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(&row, "src/lib_test.go", &HashMap::new(), false);

    assert_eq!("  src/lib_test.go  [test] (3 symbols)", line_text(&line));
}

#[test]
fn should_append_test_badge_with_singular_symbol_noun_when_count_is_one() {
    let node = test_file_node("src/lib_test.go", 1);
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(&row, "src/lib_test.go", &HashMap::new(), false);

    assert_eq!("  src/lib_test.go  [test] (1 symbol)", line_text(&line));
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

    let line = entry_row_line(&row, "src", &HashMap::new(), false);

    assert_eq!("> src ", line_text(&line));
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

    let line = entry_row_line(&row, "src", &ranks, false);

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

    let line = entry_row_line(&row, "src", &ranks, false);

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

    let line = entry_row_line(&row, "", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "", &HashMap::new(), false);

    assert_eq!("  x fn gone_fn", line_text(&line));
}

#[test]
fn should_apply_reversed_modifier_when_row_is_selected() {
    let node = file_node("lib.rs", Badges::default());
    let row = Row {
        node: &node,
        depth: 0,
        expanded: false,
    };

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), true);

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

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

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

    let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

    assert_eq!("        lib.rs ", line_text(&line));
}
