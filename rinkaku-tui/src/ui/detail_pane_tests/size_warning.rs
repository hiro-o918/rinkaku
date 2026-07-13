// ADR 0028: a `FileDetail` carrying a `size_warning` renders one
// extra line above the "Symbols" listing, with the severity named as
// a text label (`Warn:` / `Split:`) rather than an emoji glyph —
// terminal emoji width is inconsistent enough to distort the pane
// layout — plus the crossed threshold named in the trailing hint.
// Severity color (yellow / red) is applied at the caller as a
// whole-line style, not baked into this string.

use super::*;

#[test]
fn should_render_size_warning_line_when_file_detail_has_size_warning() {
    let detail = FileDetail {
        path: "src/big.rs".to_string(),
        symbols: vec![],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: Some(rinkaku_core::file_size::FileSizeWarning {
            path: "src/big.rs".to_string(),
            line_count: 1734,
            severity: rinkaku_core::file_size::FileSizeSeverity::Warn,
        }),
    };

    let lines = file_detail_lines(&detail);

    // NOTE: partial assert — the "File src/big.rs" header, the blank
    // line, and the "Symbols (0)" listing come from unrelated arms
    // of `file_detail_lines`; this test only pins the warning-line
    // portion (a whole-vec compare would double-book coverage the
    // sibling tests already have).
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    assert!(
        rendered
            .iter()
            .any(|line| line == "Warn: 1734 lines \u{2014} consider splitting (>1500 watch)"),
        "expected watch-severity warning line among: {rendered:?}",
    );
}

// Companion to the Warn test above: the `Split` variant swaps the
// label to `Split:` and names the split threshold in the trailing
// hint. Color (red vs yellow) is applied at the caller.
#[test]
fn should_render_split_severity_label_when_file_detail_size_warning_is_split() {
    let detail = FileDetail {
        path: "src/huge.rs".to_string(),
        symbols: vec![],
        skip_reason: None,
        test_symbol_count: None,
        size_warning: Some(rinkaku_core::file_size::FileSizeWarning {
            path: "src/huge.rs".to_string(),
            line_count: 4837,
            severity: rinkaku_core::file_size::FileSizeSeverity::Split,
        }),
    };

    let lines = file_detail_lines(&detail);

    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    assert!(
        rendered
            .iter()
            .any(|line| line == "Split: 4837 lines \u{2014} over the 2000-line split threshold"),
        "expected split-severity warning line among: {rendered:?}",
    );
}
