use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_apply_added_background_tint_and_keyword_foreground_in_diff_pane() {
    let report = report_with_one_symbol();
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    // The added line's "fn" keyword: foreground colored by the keyword
    // palette entry, background tinted with `ADDED_BG` — both signals
    // present on the same cell, per ADR 0018's "fg is token color, bg is
    // diff signal" decision. Disambiguated against the row via
    // "+fn foo() {}" (the marker plus full added line): the left-hand
    // tree pane's cursor row also happens to render a truncated "fn
    // foo" label for this fixture's one symbol.
    let keyword_style = find_cell_style(&terminal, "+fn foo() {}", "fn");
    assert_eq!(Some(ADDED_BG), keyword_style.bg);
    assert_eq!(Some(Color::Magenta), keyword_style.fg);
}

#[test]
fn should_apply_removed_background_tint_in_diff_pane() {
    let report = report_with_one_symbol();
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,1 @@
 fn a() {}
-fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    let keyword_style = find_cell_style(&terminal, "-fn foo() {}", "fn");
    assert_eq!(Some(REMOVED_BG), keyword_style.bg);
    assert_eq!(Some(Color::Magenta), keyword_style.fg);
}

#[test]
fn should_keep_context_line_unstyled_background_in_diff_pane() {
    let report = report_with_one_symbol();
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    // Context line "fn a() {}" keeps its keyword token color but must
    // not carry either diff background tint (`Style::bg` reports an
    // unset background as `Some(Color::Reset)`, not `None` — ratatui's
    // own `Cell` defaults every cell's `bg` field to `Color::Reset`
    // rather than leaving it absent). Disambiguated the same way as
    // the added-line test above (a leading space marker rather than
    // `+`/`-`, matching `diff_line`'s context-line rendering).
    let context_style = find_cell_style(&terminal, " fn a() {}", "fn");
    assert_eq!(Some(Color::Reset), context_style.bg);
    assert_eq!(Some(Color::Magenta), context_style.fg);
}

#[test]
fn should_keep_hunk_header_dark_gray_when_diff_pane_is_highlighted() {
    let report = report_with_one_symbol();
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn foo() {}
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    let header_style = find_cell_style(&terminal, "@@ -1,1 +1,2 @@", "@@");
    assert_eq!(Some(Color::DarkGray), header_style.fg);
    // DarkGray alone gives sufficient contrast; stacking `Modifier::DIM`
    // on top of it double-dims the header to near-invisibility on many
    // terminal themes (especially light backgrounds).
    assert!(!header_style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn should_fall_back_to_plain_diff_style_when_file_extension_is_unrecognized() {
    // A symbol whose path has no known extension (mirrors an unbuilt
    // language, e.g. YAML): `App::selected_diff_target` reads the path
    // straight off the symbol/file row, so this only needs a report
    // whose file path is unrecognized by `highlight::config_for_path`,
    // not a real diff for an actual YAML grammar.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "config.yaml".to_string(),
            symbols: vec![symbol("config.yaml::foo", "foo")],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/config.yaml b/config.yaml
index e69de29..4b825dc 100644
--- a/config.yaml
+++ b/config.yaml
@@ -1,1 +1,2 @@
 a: 1
+b: 2
";
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::note_markers::NoteMarkers::default(),
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("+b: 2"));

    // Falls back to the pane's plain green foreground, now with the same
    // `ADDED_BG` tint a highlighted line carries — highlighting failing
    // (or, here, never applying) must not lose the diff background signal.
    let added_style = find_cell_style(&terminal, "+b: 2", "b");
    assert_eq!(Some(ADDED_BG), added_style.bg);
    assert_eq!(Some(Color::Green), added_style.fg);
}

#[test]
fn should_apply_added_background_tint_to_plain_diff_line_when_no_token_spans() {
    let line = crate::diff_view::DiffLine {
        kind: crate::diff_view::DiffLineKind::Added,
        content: "fn foo() {}".to_string(),
    };

    let actual = plain_diff_line(&line);

    assert_eq!(
        Line::styled(
            "+fn foo() {}".to_string(),
            Style::default().fg(Color::Green).bg(ADDED_BG),
        ),
        actual
    );
}

#[test]
fn should_apply_removed_background_tint_to_plain_diff_line_when_no_token_spans() {
    let line = crate::diff_view::DiffLine {
        kind: crate::diff_view::DiffLineKind::Removed,
        content: "fn foo() {}".to_string(),
    };

    let actual = plain_diff_line(&line);

    assert_eq!(
        Line::styled(
            "-fn foo() {}".to_string(),
            Style::default().fg(Color::Red).bg(REMOVED_BG),
        ),
        actual
    );
}

#[test]
fn should_keep_context_line_unstyled_in_plain_diff_line_when_no_token_spans() {
    let line = crate::diff_view::DiffLine {
        kind: crate::diff_view::DiffLineKind::Context,
        content: "fn foo() {}".to_string(),
    };

    let actual = plain_diff_line(&line);

    assert_eq!(Line::raw(" fn foo() {}".to_string()), actual);
}

#[test]
fn should_replace_the_section_title_with_a_tinted_old_new_pair_in_unified_view_when_signature_changed()
 {
    let section = crate::diff_shape::DiffSection {
        title: "fn foo(a: i32, b: i32)".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: Some(crate::diff_shape::ContractHeader {
            previous_signature: "fn foo(a: i32)".to_string(),
            signature: "fn foo(a: i32, b: i32)".to_string(),
        }),
        hunks: vec![],
    };

    let actual = diff_pane_lines(
        &[&section],
        true,
        None,
        &crate::note_markers::NoteMarkers::default(),
        "lib.rs",
    );

    assert_eq!(
        vec![
            Line::styled(
                "- fn foo(a: i32)".to_string(),
                Style::default()
                    .fg(Color::Red)
                    .bg(REMOVED_BG)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                "+ fn foo(a: i32, b: i32)".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .bg(ADDED_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        ],
        actual
    );
}

#[test]
fn should_keep_the_plain_bold_title_in_unified_view_when_signature_is_unchanged() {
    let section = crate::diff_shape::DiffSection {
        title: "fn foo()".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        contract_header: None,
        hunks: vec![],
    };

    let actual = diff_pane_lines(
        &[&section],
        true,
        None,
        &crate::note_markers::NoteMarkers::default(),
        "lib.rs",
    );

    assert_eq!(
        vec![Line::styled(
            "fn foo()".to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )],
        actual
    );
}
