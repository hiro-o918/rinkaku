use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_list_keymap_groups_in_tree_right_source_review_global_order() {
    let content = help_content(Locale::English);
    let titles: Vec<&str> = content
        .keymap_groups
        .iter()
        .map(|group| group.title.as_str())
        .collect();

    assert_eq!(
        vec![
            "Tree focus",
            "Right focus",
            "Source view",
            "Review",
            "Global"
        ],
        titles
    );
}

#[test]
fn should_document_source_view_scroll_bindings_in_the_source_view_group() {
    // ADR 0026: the source view has its own scroll bindings (j/k,
    // Ctrl-d/Ctrl-u, gg/G) plus esc/q to return to the entry view.
    // Pinned so a future rename/typo/omission of any of them is
    // caught, and so the group's own presence is not silently
    // dropped by a keymap refactor.
    let content = help_content(Locale::English);
    let source_view = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "Source view")
        .expect("Source view group present");

    let keys: Vec<&str> = source_view
        .bindings
        .iter()
        .map(|binding| binding.keys)
        .collect();

    assert!(keys.contains(&"j / k / ↓ / ↑"));
    assert!(keys.contains(&"ctrl-d / ctrl-u"));
    assert!(keys.contains(&"gg / G"));
    assert!(keys.contains(&"esc / q"));
}

#[test]
fn should_have_no_empty_keymap_group() {
    let content = help_content(Locale::English);
    for group in &content.keymap_groups {
        assert!(
            !group.bindings.is_empty(),
            "group {:?} has no bindings",
            group.title
        );
    }
}

#[test]
fn should_order_marker_legend_added_changed_removed_then_aggregates() {
    let content = help_content(Locale::English);
    let swatches: Vec<&str> = content.markers.iter().map(|entry| entry.swatch).collect();

    assert_eq!(
        vec![
            "v / >",
            "fn struct enum trait class iface type",
            "+",
            "~",
            "(dimmed name)",
            "x",
            "(dimmed + struck-through name)",
            "(cycle)",
            "!",
            "[test] (N symbols)",
            "N tests",
            "(skipped: ...)",
            "chg:N",
            "api:N",
            "fan-in:N",
            "lines:N",
            "warn:N",
            "split:N",
        ],
        swatches
    );
}

#[test]
fn should_describe_api_badge_as_signature_changed_plus_removed_symbols() {
    let content = help_content(Locale::English);
    let entry = content
        .markers
        .iter()
        .find(|entry| entry.swatch == "api:N")
        .expect("api:N entry present");

    assert!(entry.explanation.contains("removed"));
    assert!(entry.explanation.contains("signature-changed"));
}

#[test]
fn should_describe_fan_in_badge_as_a_sum_over_high_fan_in_symbols() {
    let content = help_content(Locale::English);
    let entry = content
        .markers
        .iter()
        .find(|entry| entry.swatch == "fan-in:N")
        .expect("fan-in:N entry present");

    assert!(entry.explanation.contains("Sum"));
    assert!(entry.explanation.contains("high-fan-in"));
}

#[test]
fn should_include_a_glossary_entry_for_blast_radius_and_cycle() {
    let content = help_content(Locale::English);
    let terms: Vec<&str> = content.glossary.iter().map(|entry| entry.term).collect();

    assert!(terms.contains(&"blast radius"));
    assert!(terms.contains(&"cycle"));
}

#[test]
fn should_include_a_glossary_entry_for_jumplist() {
    let content = help_content(Locale::English);
    let terms: Vec<&str> = content.glossary.iter().map(|entry| entry.term).collect();

    assert!(terms.contains(&"jumplist"));
}

#[test]
fn should_document_gd_gr_and_jumplist_bindings_in_the_global_group() {
    let content = help_content(Locale::English);
    let global = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "Global")
        .expect("Global group present");

    let keys: Vec<&str> = global.bindings.iter().map(|binding| binding.keys).collect();

    assert!(keys.contains(&"gd"));
    assert!(keys.contains(&"gr"));
    assert!(keys.contains(&"ctrl-o"));
    assert!(keys.contains(&"ctrl-i"));
}

#[test]
fn should_document_review_notes_bindings_in_a_review_group() {
    let content = help_content(Locale::English);
    let review = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "Review")
        .expect("Review group present");

    let keys: Vec<&str> = review.bindings.iter().map(|binding| binding.keys).collect();

    assert!(keys.contains(&"n"));
    assert!(keys.contains(&"N"));
    assert!(keys.contains(&"j/k, Enter, Esc, d"));
}

#[test]
fn should_document_open_pr_in_browser_binding_in_the_global_group() {
    let content = help_content(Locale::English);
    let global = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "Global")
        .expect("Global group present");

    let keys: Vec<&str> = global.bindings.iter().map(|binding| binding.keys).collect();

    assert!(keys.contains(&"w"));
}

#[test]
fn should_document_h_and_esc_as_the_return_to_tree_binding_in_right_focus_group() {
    let content = help_content(Locale::English);
    let right_focus = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "Right focus")
        .expect("Right focus group present");

    let has_return_binding = right_focus
        .bindings
        .iter()
        .any(|binding| binding.keys == "h / esc");

    assert!(has_return_binding);
}

fn source_screen() -> Screen {
    Screen::Source {
        symbol_id: "lib.rs::foo".to_string(),
        scroll_top: 0,
    }
}

#[test]
fn should_show_tree_focus_review_and_global_groups_when_tree_focused_on_entry_screen() {
    let groups = applicable_help_groups(Locale::English, &Screen::Entry, Focus::Tree);
    let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();

    assert_eq!(vec!["Tree focus", "Review", "Global"], titles);
}

#[test]
fn should_show_right_focus_review_and_global_groups_when_right_focused_on_entry_screen() {
    let groups = applicable_help_groups(Locale::English, &Screen::Entry, Focus::Right);
    let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();

    assert_eq!(vec!["Right focus", "Review", "Global"], titles);
}

#[test]
fn should_show_only_source_view_and_global_groups_on_source_screen_regardless_of_focus() {
    let groups = applicable_help_groups(Locale::English, &source_screen(), Focus::Tree);
    let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();

    assert_eq!(vec!["Source view", "Global"], titles);
}

#[test]
fn should_translate_move_cursor_description_to_japanese_when_locale_is_japanese() {
    let content = help_content(Locale::Japanese);
    let tree_focus = content
        .keymap_groups
        .iter()
        .find(|group| group.title == "ツリーフォーカス")
        .expect("Tree focus group present");

    let binding = tree_focus
        .bindings
        .iter()
        .find(|binding| binding.keys == "j / k / ↓ / ↑")
        .expect("move-cursor binding present");

    assert_eq!("カーソルを移動", binding.description);
}
