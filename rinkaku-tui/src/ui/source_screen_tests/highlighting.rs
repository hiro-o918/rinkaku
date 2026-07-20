use super::*;
use rinkaku_core::extract::ExtractedSymbol;

#[test]
fn should_apply_keyword_foreground_and_symbol_range_background_in_source_screen() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("lib.rs"), "fn a() {}\nfn foo() {}\n").expect("write file");

    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange { start: 2, end: 2 },
                ..symbol("lib.rs::foo", "foo")
            }],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        "lib.rs::foo",
        dir.path(),
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    // Line 2 ("fn foo() {}") is the symbol's own highlighted range: its
    // "fn" keyword must carry both signals composited together — the
    // token's own foreground color (ADR 0018's palette, extended to
    // this screen) *and* the symbol-range background tint — mirroring
    // how the diff pane composites a token's foreground with its
    // added/removed background rather than one replacing the other.
    let keyword_style = find_cell_style(&terminal, "2 | fn foo() {}", "fn");
    assert_eq!(Some(Color::Magenta), keyword_style.fg);
    assert_eq!(Some(SOURCE_HIGHLIGHT_BG), keyword_style.bg);

    // Line 1 ("fn a() {}") is outside the symbol's range: its own "fn"
    // keyword must still be colored (highlighting applies to every
    // line, not just the highlighted range) but without the background
    // tint, since only the drilled-into symbol's own lines get it.
    // `Style::bg` reports an unset background as `Some(Color::Reset)`,
    // not `None` (ratatui's own `Cell` default — see
    // `should_keep_context_line_unstyled_background_in_diff_pane`'s own
    // doc comment for this same convention on the diff pane).
    let outside_range_style = find_cell_style(&terminal, "1 | fn a() {}", "fn");
    assert_eq!(Some(Color::Magenta), outside_range_style.fg);
    assert_eq!(Some(Color::Reset), outside_range_style.bg);
}

#[test]
fn should_fall_back_to_plain_source_style_when_file_extension_is_unrecognized() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("config.yaml"), "key: value\n").expect("write file");

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
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source);
    let source_content = Some(crate::source::load_highlighted_symbol_source(
        &report,
        "config.yaml::foo",
        dir.path(),
        &crate::source::WorkingTreeSourceReader,
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                source_content.as_ref(),
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    // No highlight configuration exists for `.yaml`
    // (`highlight::config_for_path`), so the line still renders — with
    // the symbol-range background tint (highlighting failing must never
    // regress the pane below its pre-ADR-0018 plain style) but no
    // per-token foreground coloring (`Style::fg` reports an unset
    // foreground as `Some(Color::Reset)`, not `None` — same ratatui
    // `Cell` default convention as `bg`, see the test above).
    let text = buffer_text(&terminal);
    assert!(text.contains("key: value"));
    let style = find_cell_style(&terminal, "1 | key: value", "key");
    assert_eq!(Some(SOURCE_HIGHLIGHT_BG), style.bg);
    assert_eq!(Some(Color::Reset), style.fg);
}
