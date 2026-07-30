use super::*;
use pretty_assertions::assert_eq;
use rstest::rstest;

fn skipped_detail(path: &str, reason: rinkaku_core::render::SkipReason) -> FileDetail {
    FileDetail {
        path: path.to_string(),
        symbols: vec![],
        skip_reason: Some(reason),
        test_symbol_count: None,
        size_warning: None,
    }
}

fn rendered(
    lines: &[ratatui::text::Line<'static>],
) -> Vec<(String, Option<ratatui::style::Color>)> {
    lines
        .iter()
        .map(|line| {
            let text = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            (text, line.style.fg)
        })
        .collect()
}

#[rstest]
#[case::binary(rinkaku_core::render::SkipReason::Binary, "binary")]
#[case::deleted(rinkaku_core::render::SkipReason::Deleted, "deleted")]
#[case::generated(rinkaku_core::render::SkipReason::Generated, "generated")]
fn should_say_no_symbols_were_extracted_when_the_file_has_no_readable_content(
    #[case] reason: rinkaku_core::render::SkipReason,
    #[case] label: &str,
) {
    let lines = file_detail_lines(&skipped_detail("assets/logo.png", reason));

    assert_eq!(
        vec![
            ("File assets/logo.png".to_string(), None),
            (String::new(), None),
            (
                format!("Skipped: {label}"),
                Some(ratatui::style::Color::DarkGray)
            ),
            (
                "rinkaku did not extract symbols from this file.".to_string(),
                None
            ),
        ],
        rendered(&lines)
    );
}

#[test]
fn should_point_the_reviewer_at_the_diff_when_the_language_is_unsupported() {
    let lines = file_detail_lines(&skipped_detail(
        "deploy/values.yaml",
        rinkaku_core::render::SkipReason::UnsupportedLanguage,
    ));

    assert_eq!(
        vec![
            ("File deploy/values.yaml".to_string(), None),
            (String::new(), None),
            (
                "Skipped: unsupported language".to_string(),
                Some(ratatui::style::Color::DarkGray)
            ),
            (
                "rinkaku has no parser for this file type, so review its diff directly."
                    .to_string(),
                None
            ),
        ],
        rendered(&lines)
    );
}
