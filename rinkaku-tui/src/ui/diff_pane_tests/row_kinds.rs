use super::*;

#[test]
fn should_draw_diff_pane_with_hunk_lines_when_toggled_on_a_symbol_row() {
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Diff"));
    assert!(text.contains("+fn foo() {}"));
}

// Dynamic-verification note (see CLAUDE.md's reviewing-changes
// section): this pins that a skipped file's diff pane still resolves
// real hunks from the raw diff text — `App::selected_diff_target`
// scopes a `NodeKind::File` row to `DiffTarget::File { path }`
// regardless of `skip_reason` (see the `app.rs` unit test
// `should_return_file_diff_target_when_cursor_is_on_a_skipped_file_row`),
// and `draw_diff_pane` looks hunks up by that path alone — so a
// skipped file (which has no `FileReport`/symbols to key off of) must
// not silently fall back to the "no diff hunks found" placeholder just
// because rinkaku didn't extract symbols from it.
#[test]
fn should_draw_diff_pane_with_hunk_lines_when_toggled_on_a_skipped_file_row() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![rinkaku_core::render::SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
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
        files: vec![],
    };
    // Row 0 is the collapsing "assets" dir; row 1 is the skipped file.
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // A binary file has no hunks at all in the diff text itself (git
    // reports "Binary files ... differ" instead of `@@` hunks), so the
    // correct, honest behavior is the pane's own "no diff hunks found"
    // placeholder — this test's real assertion is that it names the
    // right path, not a stale/mismatched one, confirming the lookup
    // reached this row's `path` at all. Checked as two substrings
    // rather than the whole phrase since it wraps across rendered
    // lines at this terminal's pane width.
    assert!(text.contains("no diff hunks found for"));
    assert!(text.contains("assets/logo.png"));
}

/// Sibling of the binary-skip test above, using an unsupported-language
/// skip (a real text file with real hunks in the raw diff) to confirm
/// the diff pane actually renders content — not just the placeholder —
/// for a skipped-but-textual file.
#[test]
fn should_draw_diff_pane_with_hunk_lines_for_an_unsupported_language_skipped_file() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![rinkaku_core::render::SkippedFile {
            path: "vendor/lib.zig".to_string(),
            reason: rinkaku_core::render::SkipReason::UnsupportedLanguage,
        }],
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
        files: vec![],
    };
    // Row 0 is the collapsing "vendor" dir; row 1 is the skipped file.
    // ADR 0020 defaults the right pane to Diff already, so no
    // `ToggleDiff` press is needed to reach it here.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/vendor/lib.zig b/vendor/lib.zig
index e69de29..4b825dc 100644
--- a/vendor/lib.zig
+++ b/vendor/lib.zig
@@ -1,1 +1,2 @@
 const a = 1;
+const b = 2;
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("Diff"));
    assert!(text.contains("+const b = 2;"));
}

#[test]
fn should_draw_per_symbol_section_headers_when_diff_pane_shows_a_file_selection() {
    // Cursor stays on row 0, the "lib.rs" file row itself — a file
    // selection (ADR 0020) groups hunks under each symbol's own
    // signature as a section header, unlike a symbol selection (the
    // sibling test above), which shows no header at all.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![
                symbol("lib.rs::foo", "foo"),
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 10 },
                    ..symbol("lib.rs::bar", "bar")
                },
            ],
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
    let app = App::new(&report);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn a() {}
+fn foo() {}
@@ -9,1 +10,1 @@
-fn old_bar() {}
+fn bar() {}
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("fn foo()"));
    assert!(text.contains("fn bar()"));
    assert!(text.contains("+fn foo() {}"));
    assert!(text.contains("+fn bar() {}"));
}

#[test]
fn should_draw_contract_header_before_hunks_when_symbol_signature_changed() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                classification: Some(Classification::SignatureChanged),
                previous_signature: Some("fn foo(a: i32)".to_string()),
                signature: "fn foo(a: i32, b: i32)".to_string(),
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
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };
    // Row 0 is the "lib.rs" file row, row 1 is the "foo" symbol.
    let app = App::new(&report).handle_key(crate::app::InputKey::Down);
    let diff_text = "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,1 +1,1 @@
-fn foo(a: i32) {}
+fn foo(a: i32, b: i32) {}
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
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // The 2-line old/new signature pair stands in for the section title and
    // precedes the hunk body itself (ADR 0020's outline-before-
    // implementation disclosure order).
    assert!(text.contains("- fn foo(a: i32)"));
    assert!(text.contains("+ fn foo(a: i32, b: i32)"));
    assert!(text.contains("-fn foo(a: i32) {}"));
    assert!(text.contains("+fn foo(a: i32, b: i32) {}"));
}
